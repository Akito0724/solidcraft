//! The shared tool library: a file of this computer's (`<data>/cam/tools.json`) holding tools
//! and holders every design can take from. A design keeps a copy of each tool it uses, with the
//! library entry and the revision it was copied at, so it opens anywhere with its tools whole;
//! comparing revisions shows where the copy and the library have parted.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcraft_cam::{Holder, LibraryLink, Tool};

use super::CommandSpec;
use super::cam::{apply_tool_fields, holder_of, holder_param, tool_info, tool_revision};
use crate::params::bad;
use crate::schema::cam::{
    CamLibrary, CamLibraryDeleteParams, CamLibraryFileParams, CamLibraryHolder, CamLibraryHolderParams, CamLibraryParams, CamLibraryStoreParams,
    CamLibraryTool, CamLibraryToolParams, CamLibraryUseParams, CamToolsResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const LIBRARY_CONTRACT: TypedContract<CamLibraryParams> = TypedContract::new();
const TOOL_CONTRACT: TypedContract<CamLibraryToolParams> = TypedContract::new();
const HOLDER_CONTRACT: TypedContract<CamLibraryHolderParams> = TypedContract::new();
const DELETE_CONTRACT: TypedContract<CamLibraryDeleteParams> = TypedContract::new();
const USE_CONTRACT: TypedContract<CamLibraryUseParams> = TypedContract::new();
const STORE_CONTRACT: TypedContract<CamLibraryStoreParams> = TypedContract::new();
const FILE_CONTRACT: TypedContract<CamLibraryFileParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cam.library", "Tool Library", crate::checked!("cam.library", library_cmd))
        .icon("library")
        .noundo()
        .contract(&LIBRARY_CONTRACT)
        .output(output_schema_for::<CamLibrary>)
        .params("path? (default the user's library) → path, tools [{id, revision, tool}], holders [{id, name, sections}]; the design is not changed"),
    CommandSpec::new("cam.library.tool", "Library Tool", crate::checked!("cam.library.tool", tool_cmd))
        .icon("library")
        .noundo()
        .contract(&TOOL_CONTRACT)
        .output(output_schema_for::<CamLibrary>)
        .params("path?, id? (change; none adds), number?, and cam.tool's fields (diameter required for a new one) → the library"),
    CommandSpec::new("cam.library.holder", "Library Holder", crate::checked!("cam.library.holder", holder_cmd))
        .icon("library")
        .noundo()
        .contract(&HOLDER_CONTRACT)
        .output(output_schema_for::<CamLibrary>)
        .params("path?, id? (change; none adds), name, sections [[diameter, length]…] from the holder's face up → the library"),
    CommandSpec::new("cam.library.delete", "Remove from Tool Library", crate::checked!("cam.library.delete", delete_cmd))
        .icon("delete")
        .noundo()
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<CamLibrary>)
        .params("path?, id: a tool's or a holder's → the library; designs keep their copies"),
    CommandSpec::new("cam.library.use", "Use Library Tool", crate::checked!("cam.library.use", use_cmd))
        .icon("import")
        .contract(&USE_CONTRACT)
        .output(output_schema_for::<CamToolsResult>)
        .params("path?, id, number? → the design's tools; copies the tool in (or brings the design's copy up to date)"),
    CommandSpec::new("cam.library.store", "Keep Tool in Library", crate::checked!("cam.library.store", store_cmd))
        .icon("export")
        .contract(&STORE_CONTRACT)
        .output(output_schema_for::<CamLibrary>)
        .params("path?, number: a tool of the design → the library; a new entry, or over the one it was copied from"),
    CommandSpec::new("cam.library.file", "Tool Library File", crate::checked!("cam.library.file", file_cmd))
        .icon("export")
        .noundo()
        .contract(&FILE_CONTRACT)
        .output(output_schema_for::<CamLibrary>)
        .params("path?, to? (write the library there) or from? (add that file's tools and holders) → the library, added"),
];

/// What the library file holds.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Library {
    format: String,
    #[serde(default)]
    tools: Vec<Entry>,
    #[serde(default)]
    holders: Vec<HolderEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    id: String,
    tool: Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HolderEntry {
    id: String,
    holder: Holder,
}

const FORMAT: &str = "solidcraft-tools/1";
/// Most tools and holders a library keeps, and the largest file read (bytes).
const MAX_ENTRIES: usize = 2000;
const MAX_BYTES: u64 = 16 << 20;

/// The library file `given`, or the user's.
fn file(cmd: &str, given: &Option<String>) -> Result<PathBuf> {
    match given.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => Ok(PathBuf::from(p)),
        None => crate::recovery::data_dir()
            .map(|d| d.join("cam").join("tools.json"))
            .ok_or_else(|| bad(cmd, "this computer has no folder for a tool library: name a file with `path`")),
    }
}

/// The library in `path`; an empty one when there is no file yet.
fn load(cmd: &str, path: &Path) -> Result<Library> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Library { format: FORMAT.into(), ..Library::default() }),
        Err(e) => return Err(bad(cmd, format!("can't read {}: {e}", path.display()))),
    };
    if meta.len() > MAX_BYTES {
        return Err(bad(cmd, format!("{} is too large for a tool library", path.display())));
    }
    let text = std::fs::read_to_string(path).map_err(|e| bad(cmd, format!("can't read {}: {e}", path.display())))?;
    let mut lib: Library = serde_json::from_str(&text).map_err(|e| bad(cmd, format!("{} is not a tool library: {e}", path.display())))?;
    if !lib.format.is_empty() && lib.format != FORMAT {
        return Err(bad(cmd, format!("{} is a {} file, not a tool library", path.display(), lib.format)));
    }
    lib.format = FORMAT.into();
    // What can't be used is left out rather than refusing the whole library.
    lib.tools.retain(|e| e.tool.problem().is_none() && !e.id.is_empty());
    lib.holders.retain(|e| e.holder.problem().is_none() && !e.id.is_empty());
    lib.tools.truncate(MAX_ENTRIES);
    lib.holders.truncate(MAX_ENTRIES);
    Ok(lib)
}

/// Write the library to `path`: to a file beside it first, then over it.
fn save(cmd: &str, path: &Path, lib: &Library) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| bad(cmd, format!("can't make {}: {e}", dir.display())))?;
    }
    let text = serde_json::to_string_pretty(lib).map_err(|e| bad(cmd, e.to_string()))?;
    let temp = path.with_extension("json.part");
    std::fs::write(&temp, text).map_err(|e| bad(cmd, format!("can't write {}: {e}", temp.display())))?;
    std::fs::rename(&temp, path).map_err(|e| bad(cmd, format!("can't write {}: {e}", path.display())))
}

/// A new id: `t<n>` for tools, `h<n>` for holders, past every one in use.
fn next_id(lib: &Library, prefix: char) -> String {
    let n = lib
        .tools
        .iter()
        .map(|e| e.id.as_str())
        .chain(lib.holders.iter().map(|e| e.id.as_str()))
        .filter_map(|id| id.strip_prefix(prefix)?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("{prefix}{}", n.saturating_add(1))
}

fn view(path: &Path, lib: &Library, added: Option<usize>) -> CamLibrary {
    CamLibrary {
        path: path.display().to_string(),
        tools: lib.tools.iter().map(|e| CamLibraryTool { id: e.id.clone(), revision: tool_revision(&e.tool), tool: tool_info(&e.tool) }).collect(),
        holders: lib
            .holders
            .iter()
            .map(|e| CamLibraryHolder { id: e.id.clone(), name: e.holder.name.clone(), sections: holder_param(&e.holder).sections })
            .collect(),
        added,
        param_warnings: Vec::new(),
    }
}

fn reply(cmd: &str, v: impl Serialize) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| bad(cmd, e.to_string()))
}

fn parse<T: serde::de::DeserializeOwned>(cmd: &str, p: &Value) -> Result<T> {
    serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))
}

fn library_cmd(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.library";
    let q: CamLibraryParams = parse(cmd, p)?;
    let path = file(cmd, &q.path)?;
    let lib = load(cmd, &path)?;
    reply(cmd, view(&path, &lib, None))
}

fn tool_cmd(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.library.tool";
    let q: CamLibraryToolParams = parse(cmd, p)?;
    let path = file(cmd, &q.path)?;
    let mut lib = load(cmd, &path)?;
    if let Some(n) = q.number
        && !(1..=99).contains(&n)
    {
        return Err(bad(cmd, "`number` must be 1 to 99"));
    }
    let at = match &q.id {
        Some(id) => Some(lib.tools.iter().position(|e| &e.id == id).ok_or_else(|| bad(cmd, format!("the library has no tool {id}")))?),
        None => None,
    };
    let mut tool = match at.and_then(|i| lib.tools.get(i)) {
        Some(e) => e.tool.clone(),
        None => {
            let d = q.fields.diameter.ok_or_else(|| bad(cmd, "`diameter` is required for a new tool"))?;
            let mut t = Tool::endmill(1, d, 2, d * 4.0);
            t.name.clear();
            t
        }
    };
    if let Some(n) = q.number {
        tool.number = n;
    }
    apply_tool_fields(cmd, &mut tool, &q.fields)?;
    tool.details.library = None;
    match at.and_then(|i| lib.tools.get_mut(i)) {
        Some(e) => e.tool = tool,
        None => {
            if lib.tools.len() >= MAX_ENTRIES {
                return Err(bad(cmd, "the tool library is full"));
            }
            let id = next_id(&lib, 't');
            lib.tools.push(Entry { id, tool });
        }
    }
    save(cmd, &path, &lib)?;
    reply(cmd, view(&path, &lib, None))
}

fn holder_cmd(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.library.holder";
    let q: CamLibraryHolderParams = parse(cmd, p)?;
    let path = file(cmd, &q.path)?;
    let mut lib = load(cmd, &path)?;
    let holder = holder_of(cmd, &crate::schema::cam::CamHolderParam { name: q.name, sections: q.sections })?;
    match &q.id {
        Some(id) => {
            let e = lib.holders.iter_mut().find(|e| &e.id == id).ok_or_else(|| bad(cmd, format!("the library has no holder {id}")))?;
            e.holder = holder;
        }
        None => {
            if lib.holders.len() >= MAX_ENTRIES {
                return Err(bad(cmd, "the tool library is full"));
            }
            let id = next_id(&lib, 'h');
            lib.holders.push(HolderEntry { id, holder });
        }
    }
    save(cmd, &path, &lib)?;
    reply(cmd, view(&path, &lib, None))
}

fn delete_cmd(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.library.delete";
    let q: CamLibraryDeleteParams = parse(cmd, p)?;
    let path = file(cmd, &q.path)?;
    let mut lib = load(cmd, &path)?;
    let before = lib.tools.len() + lib.holders.len();
    lib.tools.retain(|e| e.id != q.id);
    lib.holders.retain(|e| e.id != q.id);
    if lib.tools.len() + lib.holders.len() == before {
        return Err(bad(cmd, format!("the library has nothing called {}", q.id)));
    }
    save(cmd, &path, &lib)?;
    reply(cmd, view(&path, &lib, None))
}

fn use_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.library.use";
    let q: CamLibraryUseParams = parse(cmd, p)?;
    let path = file(cmd, &q.path)?;
    let lib = load(cmd, &path)?;
    let entry = lib.tools.iter().find(|e| e.id == q.id).ok_or_else(|| bad(cmd, format!("the library has no tool {}", q.id)))?;
    let tools = &s.doc.cam.tools;
    // The design's copy of it, if it has one: brought up to date where it is.
    let mine = tools.iter().position(|t| t.details.library.as_ref().is_some_and(|l| l.id == q.id));
    let free = |n: u32| !tools.iter().any(|t| t.number == n);
    let number = match (q.number, mine.and_then(|i| tools.get(i))) {
        (Some(n), _) if !(1..=99).contains(&n) => return Err(bad(cmd, "`number` must be 1 to 99")),
        (Some(n), Some(t)) if n == t.number => n,
        (Some(n), _) if free(n) => n,
        (Some(n), _) => return Err(bad(cmd, format!("the design already has a T{n}"))),
        (None, Some(t)) => t.number,
        (None, None) if free(entry.tool.number) => entry.tool.number,
        (None, None) => (1..=99).find(|n| free(*n)).ok_or_else(|| bad(cmd, "the design's tool library is full"))?,
    };
    if mine.is_none() && tools.len() >= super::cam::MAX_ITEMS {
        return Err(bad(cmd, "the design's tool library is full"));
    }
    let mut tool = entry.tool.clone();
    tool.number = number;
    tool.details.library = Some(LibraryLink { id: entry.id.clone(), revision: tool_revision(&entry.tool) });
    let cam = &mut s.doc_mut().cam;
    match mine.and_then(|i| cam.tools.get_mut(i)) {
        Some(slot) => *slot = tool,
        None => cam.tools.push(tool),
    }
    cam.tools.sort_by_key(|t| t.number);
    reply(cmd, CamToolsResult { tools: s.doc.cam.tools.iter().map(tool_info).collect(), param_warnings: Vec::new() })
}

fn store_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.library.store";
    let q: CamLibraryStoreParams = parse(cmd, p)?;
    let path = file(cmd, &q.path)?;
    let mut lib = load(cmd, &path)?;
    let at = s.doc.cam.tools.iter().position(|t| t.number == q.number).ok_or_else(|| bad(cmd, format!("the design has no T{}", q.number)))?;
    let Some(mine) = s.doc.cam.tools.get(at).cloned() else { return Err(bad(cmd, format!("the design has no T{}", q.number))) };
    let mut kept = mine.clone();
    kept.details.library = None;
    let linked = mine.details.library.as_ref().and_then(|l| lib.tools.iter().position(|e| e.id == l.id));
    let id = match linked.and_then(|i| lib.tools.get_mut(i)) {
        Some(e) => {
            e.tool = kept.clone();
            e.id.clone()
        }
        None => {
            if lib.tools.len() >= MAX_ENTRIES {
                return Err(bad(cmd, "the tool library is full"));
            }
            let id = next_id(&lib, 't');
            lib.tools.push(Entry { id: id.clone(), tool: kept.clone() });
            id
        }
    };
    save(cmd, &path, &lib)?;
    let link = LibraryLink { id, revision: tool_revision(&kept) };
    if mine.details.library.as_ref() != Some(&link)
        && let Some(t) = s.doc_mut().cam.tools.get_mut(at)
    {
        t.details.library = Some(link);
    }
    reply(cmd, view(&path, &lib, None))
}

fn file_cmd(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.library.file";
    let q: CamLibraryFileParams = parse(cmd, p)?;
    let path = file(cmd, &q.path)?;
    let mut lib = load(cmd, &path)?;
    match (q.to.as_deref().map(str::trim).filter(|t| !t.is_empty()), q.from.as_deref().map(str::trim).filter(|t| !t.is_empty())) {
        (Some(to), None) => {
            save(cmd, Path::new(to), &lib)?;
            reply(cmd, view(&path, &lib, None))
        }
        (None, Some(from)) => {
            let other = load(cmd, Path::new(from))?;
            let mut added = 0;
            for e in other.tools {
                let rev = tool_revision(&e.tool);
                if lib.tools.iter().any(|m| tool_revision(&m.tool) == rev) || lib.tools.len() >= MAX_ENTRIES {
                    continue;
                }
                let id = next_id(&lib, 't');
                let mut tool = e.tool;
                tool.details.library = None;
                lib.tools.push(Entry { id, tool });
                added += 1;
            }
            for e in other.holders {
                if lib.holders.iter().any(|m| m.holder == e.holder) || lib.holders.len() >= MAX_ENTRIES {
                    continue;
                }
                let id = next_id(&lib, 'h');
                lib.holders.push(HolderEntry { id, holder: e.holder });
                added += 1;
            }
            save(cmd, &path, &lib)?;
            reply(cmd, view(&path, &lib, Some(added)))
        }
        _ => Err(bad(cmd, "give either `to` (write the library there) or `from` (add a file's tools)")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn temp(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("solidcraft-cam-library-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("tools.json").display().to_string()
    }

    #[test]
    fn the_library_keeps_tools_and_holders_that_designs_copy_and_compare() {
        let path = temp("keeps");
        let mut s = Session::default();
        let empty = s.execute("cam.library", &json!({"path": path})).unwrap();
        assert_eq!(empty["tools"].as_array().unwrap().len(), 0, "a new library is empty");
        // A 6 mm flat end mill in an ER11 holder, and a ball end mill.
        let er11 = s.execute("cam.library.holder", &json!({"path": path, "name": "ER11", "sections": [[16, 12], [22, 25]]})).unwrap();
        assert_eq!(er11["holders"][0]["id"], "h1");
        let lib = s
            .execute(
                "cam.library.tool",
                &json!({"path": path, "diameter": 6, "flute_length": 20, "stickout": 25, "holder": {"name": "ER11", "sections": [[16, 12], [22, 25]]},
                        "presets": [{"material": "wood", "feed": 900}]}),
            )
            .unwrap();
        let flat = &lib["tools"][0];
        assert_eq!(flat["id"], "t1");
        assert_eq!(flat["tool"]["name"], "6 mm 2-flute end mill");
        assert_eq!(flat["tool"]["holder"]["name"], "ER11");
        let lib = s.execute("cam.library.tool", &json!({"path": path, "kind": "ball_endmill", "diameter": 4, "number": 2})).unwrap();
        assert_eq!(lib["tools"][1]["tool"]["name"], "4 mm 2-flute ball end mill");
        // It is in the file: a fresh session reads it back.
        let mut other = Session::default();
        assert_eq!(other.execute("cam.library", &json!({"path": path})).unwrap()["tools"].as_array().unwrap().len(), 2);
        // A design takes the flat one: a copy, linked, alike.
        let tools = s.execute("cam.library.use", &json!({"path": path, "id": "t1"})).unwrap();
        let copy = &tools["tools"][0];
        assert_eq!((copy["number"].as_u64(), copy["library"].as_str()), (Some(1), Some("t1")));
        assert_eq!(copy["revision"], flat["revision"]);
        assert_eq!(s.doc.cam.tools[0].cutting(solidcraft_cam::Material::Wood).feed, 900.0, "with its cutting data for wood");
        // Changed in the library, the copy differs until it is brought up to date.
        let lib = s.execute("cam.library.tool", &json!({"path": path, "id": "t1", "flute_length": 22})).unwrap();
        let now = lib["tools"][0]["revision"].clone();
        let mine = crate::cmd::cam::tool_revision(&s.doc.cam.tools[0]);
        assert_ne!(json!(mine), now);
        let tools = s.execute("cam.library.use", &json!({"path": path, "id": "t1"})).unwrap();
        assert_eq!(tools["tools"].as_array().unwrap().len(), 1, "the same copy, updated");
        assert_eq!(tools["tools"][0]["revision"], now);
        // The design's own tool goes into the library, and back over its entry once linked.
        s.execute("cam.tool", &json!({"number": 5, "diameter": 2, "kind": "drill"})).unwrap();
        let lib = s.execute("cam.library.store", &json!({"path": path, "number": 5})).unwrap();
        assert_eq!(lib["tools"].as_array().unwrap().len(), 3);
        s.execute("cam.tool", &json!({"number": 5, "notes": "for M2 taps"})).unwrap();
        let lib = s.execute("cam.library.store", &json!({"path": path, "number": 5})).unwrap();
        assert_eq!(lib["tools"].as_array().unwrap().len(), 3, "over its entry");
        assert_eq!(lib["tools"][2]["tool"]["notes"], "for M2 taps");
        // Out to a file and into another library: only what it lacks is added.
        let out = temp("out");
        s.execute("cam.library.file", &json!({"path": path, "to": out})).unwrap();
        let second = temp("second");
        let merged = s.execute("cam.library.file", &json!({"path": second, "from": out})).unwrap();
        assert_eq!(merged["added"], 4);
        let again = s.execute("cam.library.file", &json!({"path": second, "from": out})).unwrap();
        assert_eq!(again["added"], 0);
        // Removing an entry leaves the design's copy.
        s.execute("cam.library.delete", &json!({"path": path, "id": "t1"})).unwrap();
        assert_eq!(s.doc.cam.tools.len(), 2);
        assert!(s.execute("cam.library.delete", &json!({"path": path, "id": "t1"})).is_err());
        // Refused: a number taken, a missing entry, a bad holder, a file that isn't a library.
        assert!(s.execute("cam.library.use", &json!({"path": path, "id": "t3", "number": 1})).is_err());
        assert!(s.execute("cam.library.use", &json!({"path": path, "id": "t99"})).is_err());
        assert!(s.execute("cam.library.holder", &json!({"path": path, "name": "x", "sections": [[0, 10]]})).is_err());
        let junk = temp("junk");
        std::fs::create_dir_all(std::path::Path::new(&junk).parent().unwrap()).unwrap();
        std::fs::write(&junk, "{\"format\": \"something-else/1\"}").unwrap();
        assert!(s.execute("cam.library", &json!({"path": junk})).is_err());
    }

    #[test]
    fn a_tool_keeps_its_shape_and_holder_in_the_design() {
        let mut s = Session::default();
        let t = s
            .execute("cam.tool", &json!({"number": 1, "diameter": 6, "kind": "bull_nose", "corner_radius": 1, "shank_diameter": 6, "overall_length": 50, "maker": "Acme"}))
            .unwrap();
        let info = &t["tools"][0];
        assert_eq!(info["kind"], "bull_nose");
        assert_eq!(info["corner_radius"], 1.0);
        assert_eq!(info["name"], "6 mm R1 bull nose");
        assert_eq!(info["maker"], "Acme");
        // Changing one thing keeps the rest.
        let t = s.execute("cam.tool", &json!({"number": 1, "flutes": 4})).unwrap();
        assert_eq!(t["tools"][0]["overall_length"], 50.0);
        assert!(s.execute("cam.tool", &json!({"number": 2, "diameter": 6, "kind": "bull_nose"})).is_err(), "a bull nose needs its corner");
        assert!(s.execute("cam.tool", &json!({"number": 2, "diameter": 6, "kind": "spade"})).is_err());
        assert!(s.execute("cam.tool", &json!({"number": 2})).is_err(), "a new tool needs its diameter");
        assert!(s.execute("cam.tool", &json!({"number": 1, "overall_length": 10})).is_err(), "shorter than the flutes");
    }
}
