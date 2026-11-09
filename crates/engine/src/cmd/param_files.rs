//! Shared parameter files (`solidcraft_doc::param_files`): link a design to a parameter table in
//! its own file, read it again when it changes, write a change made in the design back to it,
//! share a design's parameters through a new file, and stop reading one.
//!
//! A file is kept relative to the design's folder once the design is saved, as linked designs
//! are (`design_link`), and is read and written through the same checks as other files a design
//! names: a network location the design may not reach is refused without touching it.

use std::path::Path;

use serde_json::Value;
use solidcraft_doc::{DesignLink, ParamFile, SharedRow};

use super::CommandSpec;
use super::design_link::{absolute, resolve, stored};
use crate::params::{bad, bool_, str_};
use crate::schema::parameters::{
    FailedFeature, ParamFileInfo, ParamFileLinkParams, ParamFileLinked, ParamFileUnlinkParams, ParamFileUnlinked, ParamFilesParams, ParamFilesResult,
    ParamFilesUpdateParams, ParamFilesUpdated, ParamShareParams, ParamShared,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const LINK_CONTRACT: TypedContract<ParamFileLinkParams> = TypedContract::new();
const UPDATE_CONTRACT: TypedContract<ParamFilesUpdateParams> = TypedContract::new();
const UNLINK_CONTRACT: TypedContract<ParamFileUnlinkParams> = TypedContract::new();
const SHARE_CONTRACT: TypedContract<ParamShareParams> = TypedContract::new();
const FILES_CONTRACT: TypedContract<ParamFilesParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("parameters.link_file", "Link Shared Parameters", crate::checked!("parameters.link_file", link_file))
        .icon("chain")
        .contract(&LINK_CONTRACT)
        .output(output_schema_for::<ParamFileLinked>)
        .params(
            "path: a parameter table (JSON or CSV, as parameters.export writes) → path, file, names, dropped, errors; its parameters \
             become user parameters kept in step with the file, and a change to one is written back to it; refused when a name is taken",
        ),
    CommandSpec::new("parameters.update_files", "Update Shared Parameters", crate::checked!("parameters.update_files", update_files))
        .icon("update")
        .contract(&UPDATE_CONTRACT)
        .output(output_schema_for::<ParamFilesUpdated>)
        .params("path?: one file (default: every file that changed since it was read) → updated [{path, file, names, dropped, errors}], unchanged"),
    CommandSpec::new("parameters.unlink_file", "Unlink Shared Parameters", crate::checked!("parameters.unlink_file", unlink_file))
        .icon("break")
        .contract(&UNLINK_CONTRACT)
        .output(output_schema_for::<ParamFileUnlinked>)
        .params("path, keep?: bool (default true: its parameters stay as the design's own; false deletes them) → path, names, kept"),
    CommandSpec::new("parameters.share", "Share Parameters", crate::checked!("parameters.share", share))
        .icon("export")
        .contract(&SHARE_CONTRACT)
        .output(output_schema_for::<ParamShared>)
        .params("path: a new JSON file, names?: user parameters (default: every one not shared yet) → path, file, names"),
    CommandSpec::new("parameters.files", "List Shared Parameter Files", crate::checked!("parameters.files", files))
        .icon("list")
        .noundo()
        .doc_only()
        .contract(&FILES_CONTRACT)
        .output(output_schema_for::<ParamFilesResult>)
        .params("→ files [{path, file, names, found, changed}]"),
];

/// Largest shared parameter file read.
const MAX_FILE_BYTES: usize = 16 << 20;

fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

fn io_error(path: &str, e: impl std::fmt::Display) -> EngineError {
    let e = e.to_string();
    EngineError::Other(if e.starts_with(path) { e } else { format!("{path}: {e}") })
}

/// The file a kept `path` names for this session's design.
fn file_of(s: &Session, path: &str) -> String {
    resolve(s.path.as_deref(), path).to_string_lossy().into_owned()
}

/// The text of the file at `file` and the digest of its bytes.
fn read(s: &Session, file: &str) -> Result<(String, String)> {
    let bytes = solidcraft_io::vfs::read_design_path(file, s.path.as_deref(), MAX_FILE_BYTES).map_err(|e| io_error(file, e))?;
    let digest = DesignLink::file_digest(&bytes);
    let text = String::from_utf8(bytes).map_err(|_| EngineError::Other(format!("{file}: the shared parameter file is not UTF-8 text")))?;
    Ok((text, digest))
}

/// The digest of the file at `file` now; `None` when it isn't there, can't be read or may not
/// be looked for.
fn digest_now(s: &Session, file: &str) -> Option<String> {
    solidcraft_io::vfs::design_path_is_file(file, s.path.as_deref())
        .filter(|found| *found)
        .and_then(|_| solidcraft_io::vfs::read_design_path(file, s.path.as_deref(), MAX_FILE_BYTES).ok())
        .map(|bytes| DesignLink::file_digest(&bytes))
}

/// Write `rows` to the file at `file`, returning the digest of the bytes written.
fn write(s: &Session, file: &str, rows: &[SharedRow]) -> Result<String> {
    solidcraft_io::vfs::check_design_path(file, s.path.as_deref()).map_err(|e| io_error(file, e))?;
    let text = solidcraft_doc::param_files::table_json(rows);
    solidcraft_io::vfs::write(file, text.as_bytes()).map_err(|e| io_error(file, e))?;
    Ok(DesignLink::file_digest(text.as_bytes()))
}

/// The kept path of the design's shared file that `path` names (as kept, or as a file).
fn kept_path(s: &Session, path: &str) -> Option<String> {
    let wanted = absolute(&resolve(s.path.as_deref(), path));
    s.doc.param_files.iter().find(|f| f.path == path || absolute(&resolve(s.path.as_deref(), &f.path)) == wanted).map(|f| f.path.clone())
}

fn failing(s: &Session) -> Vec<FailedFeature> {
    FailedFeature::of(&s.model.results)
}

/// Read the file kept as `path` (or a new file there) and apply its table.
fn apply(s: &mut Session, cmd: &str, path: &str, file: &str) -> Result<ParamFileLinked> {
    let (text, digest) = read(s, file)?;
    let solved = s.solved_sketches();
    let applied = s.doc_mut().apply_shared_table(path, &text, digest).map_err(|e| bad(cmd, e.to_string()))?;
    s.settle_value_edit(&solved)?;
    Ok(ParamFileLinked {
        path: path.to_string(),
        file: file.to_string(),
        names: applied.names,
        dropped: applied.dropped,
        errors: failing(s),
        param_warnings: Vec::new(),
    })
}

fn link_file(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "parameters.link_file";
    let path = str_(p, "path").map(str::trim).filter(|x| !x.is_empty()).ok_or_else(|| bad(cmd, "`path` is required"))?;
    if let Some(kept) = kept_path(s, path) {
        return Err(bad(cmd, format!("{kept} is linked already: Update Shared Parameters reads it again")));
    }
    let file = absolute(Path::new(path)).to_string_lossy().into_owned();
    // Kept whole until the design is saved, then relative to it (as linked designs are).
    let kept = stored(s.path.as_deref(), Path::new(&file));
    let mut linked = apply(s, cmd, &kept, &file)?;
    linked.path = kept;
    reply(cmd, linked)
}

fn update_files(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "parameters.update_files";
    let only = match str_(p, "path").map(str::trim).filter(|x| !x.is_empty()) {
        Some(path) => Some(kept_path(s, path).ok_or_else(|| bad(cmd, format!("{path} is not a shared parameter file of this design")))?),
        None => None,
    };
    let kept: Vec<ParamFile> = s.doc.param_files.iter().filter(|f| only.as_ref().is_none_or(|o| *o == f.path)).cloned().collect();
    let (mut updated, mut unchanged) = (Vec::new(), Vec::new());
    for f in kept {
        let file = file_of(s, &f.path);
        // A file asked for by name is read again whatever its digest says.
        if only.is_none() && digest_now(s, &file).is_none_or(|now| f.file_sha256.as_deref() == Some(now.as_str())) {
            unchanged.push(f.path);
            continue;
        }
        updated.push(apply(s, cmd, &f.path, &file)?);
    }
    reply(cmd, ParamFilesUpdated { updated, unchanged, param_warnings: Vec::new() })
}

fn unlink_file(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "parameters.unlink_file";
    let path = str_(p, "path").map(str::trim).filter(|x| !x.is_empty()).ok_or_else(|| bad(cmd, "`path` is required"))?;
    let kept_as = kept_path(s, path).ok_or_else(|| bad(cmd, format!("{path} is not a shared parameter file of this design")))?;
    let keep = bool_(p, "keep").unwrap_or(true);
    let solved = s.solved_sketches();
    let names = s.doc_mut().unlink_param_file(&kept_as, keep).map_err(|e| bad(cmd, e.to_string()))?;
    s.settle_value_edit(&solved)?;
    reply(cmd, ParamFileUnlinked { path: kept_as, names, kept: keep, param_warnings: Vec::new() })
}

fn share(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "parameters.share";
    let path = str_(p, "path").map(str::trim).filter(|x| !x.is_empty()).ok_or_else(|| bad(cmd, "`path` is required"))?;
    let file = absolute(Path::new(path)).to_string_lossy().into_owned();
    solidcraft_io::vfs::check_design_path(&file, s.path.as_deref()).map_err(|e| io_error(&file, e))?;
    if solidcraft_io::vfs::exists(&file) {
        return Err(bad(cmd, format!("{file} exists already: link it with Link Shared Parameters, or choose a new file")));
    }
    let names: Vec<String> = match p.get("names") {
        Some(Value::Array(list)) => {
            list.iter().map(|n| n.as_str().map(str::to_string).ok_or_else(|| bad(cmd, "`names` must be a list of names"))).collect::<Result<_>>()?
        }
        Some(Value::Null) | None => {
            s.doc.params.iter().filter(|p| !p.model && s.doc.shared_file_of(&p.name).is_none()).map(|p| p.name.clone()).collect()
        }
        Some(_) => return Err(bad(cmd, "`names` must be a list of names")),
    };
    let kept = stored(s.path.as_deref(), Path::new(&file));
    let mut next = (*s.doc).clone();
    let rows = next.share_params(&kept, &names).map_err(|e| bad(cmd, e.to_string()))?;
    let digest = write(s, &file, &rows)?;
    next.shared_written(&kept, rows.clone(), digest);
    *s.doc_mut() = next;
    reply(cmd, ParamShared { path: kept, file, names: rows.into_iter().map(|r| r.name).collect(), param_warnings: Vec::new() })
}

fn files(s: &mut Session, _p: &Value) -> Result<Value> {
    let files = s
        .doc
        .param_files
        .iter()
        .map(|f| {
            let file = file_of(s, &f.path);
            let found = solidcraft_io::vfs::design_path_is_file(&file, s.path.as_deref());
            let changed = match found {
                Some(true) => digest_now(s, &file).map(|now| f.file_sha256.as_deref() != Some(now.as_str())),
                _ => None,
            };
            ParamFileInfo { path: f.path.clone(), names: f.rows.iter().map(|r| r.name.clone()).collect(), file, found, changed }
        })
        .collect();
    reply("parameters.files", ParamFilesResult { files, param_warnings: Vec::new() })
}

/// Refuse an edit of the parameters `names` when one of them comes from a shared file that
/// changed elsewhere since this design read it: writing the edit back would undo that change.
/// `None` checks every shared file (an import may touch any parameter).
pub(super) fn guard(s: &Session, cmd: &str, names: Option<&[String]>) -> Result<()> {
    for f in &s.doc.param_files {
        if let Some(names) = names
            && !f.rows.iter().any(|r| names.contains(&r.name))
        {
            continue;
        }
        let file = file_of(s, &f.path);
        if let Some(now) = digest_now(s, &file)
            && f.file_sha256.as_deref() != Some(now.as_str())
        {
            return Err(bad(
                cmd,
                format!("the shared parameter file {} changed since this design read it: Update Shared Parameters first, then edit", f.path),
            ));
        }
    }
    Ok(())
}

/// Write each shared file whose parameters this design now holds differently back to the file,
/// noting what was written. Called after a parameter edit has gone in.
pub(super) fn write_back(s: &mut Session) -> Result<()> {
    let pending: Vec<(String, Vec<SharedRow>)> = s
        .doc
        .param_files
        .iter()
        .filter_map(|f| {
            let now = s.doc.shared_rows_now(f);
            (now != f.rows).then(|| (f.path.clone(), now))
        })
        .collect();
    for (path, rows) in pending {
        let file = file_of(s, &path);
        let digest = write(s, &file, &rows)?;
        s.doc_mut().shared_written(&path, rows, digest);
    }
    Ok(())
}

/// The shared files kept for a design saved at `to` instead of at `from`.
pub(super) fn rebased(s: &Session, to: &str) -> Vec<ParamFile> {
    s.doc.param_files.iter().map(|f| ParamFile { path: stored(Some(to), &resolve(s.path.as_deref(), &f.path)), ..f.clone() }).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use serde_json::json;

    use super::*;

    fn temp(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("solidcraft-shared-params-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("machine.json").to_string_lossy().into_owned()
    }

    #[test]
    fn a_shared_file_links_updates_writes_back_and_unlinks() {
        let file = temp("cycle");
        std::fs::write(&file, r#"[{"name":"rail_pitch","expression":"300 mm","unit":"mm"},{"name":"screw","expression":"3","unit":""}]"#).unwrap();
        let mut s = Session::default();
        let linked = s.execute("parameters.link_file", &json!({"path": file})).unwrap();
        assert_eq!(linked["names"], json!(["rail_pitch", "screw"]));
        assert!(s.doc.param("rail_pitch").is_some());
        assert_eq!(s.doc.shared_file_of("rail_pitch").map(|f| f.rows.len()), Some(2));
        // Linking twice is refused; renaming or deleting a shared parameter is too.
        assert!(s.execute("parameters.link_file", &json!({"path": file})).is_err());
        assert!(s.execute("parameters.rename", &json!({"name": "rail_pitch", "new_name": "pitch"})).is_err());
        assert!(s.execute("parameters.delete", &json!({"name": "screw"})).is_err());
        // A change made in the design goes back to the file.
        s.execute("parameters.change", &json!({"name": "rail_pitch", "expression": "320 mm"})).unwrap();
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(written.contains("320 mm"), "{written}");
        let listed = s.execute("parameters.files", &json!({})).unwrap();
        assert_eq!(listed["files"][0]["changed"], json!(false));
        // A change made elsewhere is seen, refuses an edit until it is read, and is read.
        std::fs::write(&file, r#"[{"name":"rail_pitch","expression":"340 mm","unit":"mm"},{"name":"screw","expression":"4","unit":""}]"#).unwrap();
        assert_eq!(s.execute("parameters.files", &json!({})).unwrap()["files"][0]["changed"], json!(true));
        assert!(s.execute("parameters.change", &json!({"name": "rail_pitch", "expression": "1 mm"})).is_err());
        let updated = s.execute("parameters.update_files", &json!({})).unwrap();
        assert_eq!(updated["updated"].as_array().map(Vec::len), Some(1));
        assert_eq!(s.doc.param("rail_pitch").map(|p| p.expr.as_str()), Some("340 mm"));
        // Unlinking keeps the parameters as the design's own.
        let unlinked = s.execute("parameters.unlink_file", &json!({"path": file})).unwrap();
        assert_eq!(unlinked["kept"], json!(true));
        assert!(s.doc.param_files.is_empty());
        assert!(s.execute("parameters.rename", &json!({"name": "rail_pitch", "new_name": "pitch"})).is_ok());
    }

    #[test]
    fn a_taken_name_refuses_the_file_and_sharing_writes_a_new_one() {
        let file = temp("taken");
        let _ = std::fs::remove_file(&file);
        let mut s = Session::default();
        s.execute("parameters.add", &json!({"name": "width", "expression": "40 mm"})).unwrap();
        let shared = s.execute("parameters.share", &json!({"path": file})).unwrap();
        assert_eq!(shared["names"], json!(["width"]));
        assert!(std::fs::read_to_string(&file).unwrap().contains("width"));
        assert!(s.execute("parameters.share", &json!({"path": file})).is_err(), "the file exists now");
        // Another design that has its own `width` can't link the file.
        let mut other = Session::default();
        other.execute("parameters.add", &json!({"name": "width", "expression": "10 mm"})).unwrap();
        assert!(other.execute("parameters.link_file", &json!({"path": file})).is_err());
        assert!(other.doc.param_files.is_empty());
    }
}
