//! Machining (`solidcraft_doc::cam`): the tool library, setups and operations kept in the design,
//! their toolpaths (`solidcraft_cam`) and GRBL programs written one per tool.

use std::path::Path;

use serde_json::Value;
use solidcraft_cam::{Cutting, Material, Side, Tabs, Tool, ToolKind, Toolpath};
use solidcraft_doc::cam::{CamOperation, CamSetup, GeoPick, OpSpec, StockMargin, WorkOrigin};

use super::CommandSpec;
use super::design_link::absolute;
use crate::params::bad;
use crate::schema::cam::{
    CamDeleteParams, CamDeleted, CamExportParams, CamExported, CamGenerateParams, CamGenerated, CamOpSummary, CamOperationParams, CamOperationResult,
    CamPickParam, CamProgram, CamSetupParams, CamSetupResult, CamSetupSummary, CamToolInfo, CamToolParams, CamToolsResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const TOOL_CONTRACT: TypedContract<CamToolParams> = TypedContract::new();
const SETUP_CONTRACT: TypedContract<CamSetupParams> = TypedContract::new();
const OPERATION_CONTRACT: TypedContract<CamOperationParams> = TypedContract::new();
const DELETE_CONTRACT: TypedContract<CamDeleteParams> = TypedContract::new();
const GENERATE_CONTRACT: TypedContract<CamGenerateParams> = TypedContract::new();
const EXPORT_CONTRACT: TypedContract<CamExportParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cam.tool", "Machining Tool", crate::checked!("cam.tool", tool_cmd))
        .icon("hole")
        .contract(&TOOL_CONTRACT)
        .output(output_schema_for::<CamToolsResult>)
        .params("number: 1..99, diameter, flutes?, flute_length?, kind?: flat_endmill|drill, name? → tools; replaces the tool with that number"),
    CommandSpec::new("cam.setup", "Machining Setup", crate::checked!("cam.setup", setup_cmd))
        .icon("box")
        .contract(&SETUP_CONTRACT)
        .output(output_schema_for::<CamSetupResult>)
        .params(
            "id? (change), name?, body? (default the only body), material?: wood|acrylic|aluminum, origin?: top_center|top_corner, flipped?, \
             margin_side?, margin_top?, margin_bottom?, safe_z?, retract? → id, name, body, stock [x,y,z], part_top",
        ),
    CommandSpec::new("cam.operation", "Machining Operation", crate::checked!("cam.operation", operation_cmd))
        .icon("extrude")
        .contract(&OPERATION_CONTRACT)
        .output(output_schema_for::<CamOperationResult>)
        .params(
            "id? (change), setup?, type: face|pocket|contour|drill, tool: number, name?, face?: {at:[x,y,z], body?} (pocket floor / contour \
             face), edges?: [{at, body?}] (drill), side?: outside|inside|on, depth?, tabs?: {count, width, height}, peck?, spindle_rpm?, feed?, \
             plunge_feed?, stepdown?, stepover? → operation {id, name, kind, tool, cut_mm, rapid_mm, minutes}, setup; refused when its toolpath \
             can't be made",
        ),
    CommandSpec::new("cam.delete", "Delete Machining", crate::checked!("cam.delete", delete_cmd))
        .icon("delete")
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<CamDeleted>)
        .params("id?: a setup (with its operations) or an operation, tool?: a tool no operation uses → ids, tool"),
    CommandSpec::new("cam.generate", "Generate Toolpaths", crate::checked!("cam.generate", generate_cmd))
        .icon("update")
        .noundo()
        .contract(&GENERATE_CONTRACT)
        .output(output_schema_for::<CamGenerated>)
        .params("setup? (default every setup) → setups [{id, name, body, material, flipped, stock, operations [{id, name, kind, tool, cut_mm, rapid_mm, minutes, error?}], error?}], tools"),
    CommandSpec::new("cam.export", "Export G-code", crate::checked!("cam.export", export_cmd))
        .icon("export")
        .noundo()
        .contract(&EXPORT_CONTRACT)
        .output(output_schema_for::<CamExported>)
        .params("path, setup? (default the only one), arcs?: bool (default true) → files [{path, tool, operations, minutes}]; one GRBL program per tool"),
];

/// Rapid speed assumed for time estimates (mm/min).
const RAPID_SPEED: f64 = 3000.0;
/// Most tools, setups and operations a design keeps.
const MAX_ITEMS: usize = 500;

fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

fn parse<T: serde::de::DeserializeOwned>(cmd: &str, p: &Value) -> Result<T> {
    serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))
}

fn round(v: f64, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    (v * k).round() / k
}

fn material_name(m: Material) -> &'static str {
    match m {
        Material::Wood => "wood",
        Material::Acrylic => "acrylic",
        Material::Aluminum => "aluminum",
    }
}

fn tool_info(t: &Tool) -> CamToolInfo {
    CamToolInfo {
        number: t.number,
        name: t.name.clone(),
        kind: match t.kind {
            ToolKind::FlatEndmill => "flat_endmill",
            ToolKind::Drill => "drill",
        }
        .into(),
        diameter: t.diameter,
        flutes: t.flutes,
        flute_length: t.flute_length,
    }
}

fn kind_name(spec: &OpSpec) -> &'static str {
    match spec {
        OpSpec::Face { .. } => "face",
        OpSpec::Pocket { .. } => "pocket",
        OpSpec::Contour { .. } => "contour",
        OpSpec::Drill { .. } => "drill",
    }
}

fn tool_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.tool";
    let q: CamToolParams = parse(cmd, p)?;
    if !(1..=99).contains(&q.number) {
        return Err(bad(cmd, "`number` must be 1 to 99"));
    }
    let kind = match q.kind.as_deref() {
        None | Some("flat_endmill") => ToolKind::FlatEndmill,
        Some("drill") => ToolKind::Drill,
        Some(k) => return Err(bad(cmd, format!("`kind` must be flat_endmill or drill, not {k}"))),
    };
    let flutes = q.flutes.unwrap_or(2);
    let flute_length = q.flute_length.unwrap_or(q.diameter * 4.0);
    let mut tool = Tool::endmill(q.number, q.diameter, flutes, flute_length);
    tool.kind = kind;
    if let Some(name) = q.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
        tool.name = name.chars().take(80).collect();
    } else if kind == ToolKind::Drill {
        tool.name = format!("{} mm drill", q.diameter);
    }
    if let Some(problem) = tool.problem() {
        return Err(bad(cmd, problem));
    }
    let cam = &mut s.doc_mut().cam;
    match cam.tools.iter_mut().find(|t| t.number == q.number) {
        Some(slot) => *slot = tool,
        None if cam.tools.len() >= MAX_ITEMS => return Err(bad(cmd, "the tool library is full")),
        None => cam.tools.push(tool),
    }
    cam.tools.sort_by_key(|t| t.number);
    reply(cmd, CamToolsResult { tools: s.doc.cam.tools.iter().map(tool_info).collect(), param_warnings: Vec::new() })
}

fn positive_or_zero(cmd: &str, what: &str, v: Option<f64>, default: f64) -> Result<f64> {
    match v {
        None => Ok(default),
        Some(v) if v.is_finite() && (0.0..=1000.0).contains(&v) => Ok(v),
        Some(_) => Err(bad(cmd, format!("`{what}` must be between 0 and 1000 mm"))),
    }
}

/// The only body of the model, for a setup that doesn't name one.
fn only_body(s: &Session, cmd: &str) -> Result<String> {
    let st = s.model.state();
    match st.bodies.as_slice() {
        [one] => Ok(one.name.clone()),
        [] => Err(bad(cmd, "the model has no body to machine")),
        _ => Err(bad(cmd, "the model has several bodies: name the one to machine with `body`")),
    }
}

fn setup_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.setup";
    let q: CamSetupParams = parse(cmd, p)?;
    let mut setup = match q.id {
        Some(id) => s.doc.cam.setup(id).cloned().ok_or_else(|| bad(cmd, format!("there is no setup {id}")))?,
        None => {
            if s.doc.cam.setups.len() >= MAX_ITEMS {
                return Err(bad(cmd, "there are too many setups"));
            }
            let id = s.doc.cam.next_id();
            CamSetup {
                id,
                name: format!("Setup {}", s.doc.cam.setups.len() + 1),
                body: String::new(),
                material: Material::Wood,
                origin: WorkOrigin::TopCenter,
                flipped: false,
                margin: StockMargin::default(),
                safe_z: 10.0,
                retract: 1.0,
            }
        }
    };
    if let Some(name) = q.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
        setup.name = name.chars().take(80).collect();
    }
    match q.body {
        Some(b) => {
            if s.model.state().body(&b).is_none() {
                return Err(bad(cmd, format!("there is no body `{b}`")));
            }
            setup.body = b;
        }
        None if setup.body.is_empty() => setup.body = only_body(s, cmd)?,
        None => {}
    }
    if let Some(m) = q.material.as_deref() {
        setup.material = match m {
            "wood" => Material::Wood,
            "acrylic" => Material::Acrylic,
            "aluminum" | "aluminium" => Material::Aluminum,
            _ => return Err(bad(cmd, format!("`material` must be wood, acrylic or aluminum, not {m}"))),
        };
    }
    if let Some(o) = q.origin.as_deref() {
        setup.origin = match o {
            "top_center" => WorkOrigin::TopCenter,
            "top_corner" => WorkOrigin::TopCorner,
            _ => return Err(bad(cmd, format!("`origin` must be top_center or top_corner, not {o}"))),
        };
    }
    if let Some(f) = q.flipped {
        setup.flipped = f;
    }
    setup.margin = StockMargin {
        side: positive_or_zero(cmd, "margin_side", q.margin_side, setup.margin.side)?,
        top: positive_or_zero(cmd, "margin_top", q.margin_top, setup.margin.top)?,
        bottom: positive_or_zero(cmd, "margin_bottom", q.margin_bottom, setup.margin.bottom)?,
    };
    setup.retract = positive_or_zero(cmd, "retract", q.retract, setup.retract)?;
    if let Some(z) = q.safe_z {
        if !(z.is_finite() && z > setup.retract && z <= 500.0) {
            return Err(bad(cmd, "`safe_z` must be above the retract and at most 500 mm"));
        }
        setup.safe_z = z;
    }
    let id = setup.id;
    let mut next = (*s.doc).clone();
    match next.cam.setups.iter_mut().find(|x| x.id == id) {
        Some(slot) => *slot = setup.clone(),
        None => next.cam.setups.push(setup.clone()),
    }
    let st = s.model.state();
    let plan = next.cam_plan(&st, id).map_err(|e| bad(cmd, e.to_string()))?;
    *s.doc_mut() = next;
    let size = [0, 1, 2].map(|i| round(plan.stock_max[i] - plan.stock_min[i], 3));
    reply(cmd, CamSetupResult { id, name: setup.name, body: setup.body, stock: size, part_top: round(plan.part_top, 3), param_warnings: Vec::new() })
}

/// The setup an operation names, or the only one.
fn which_setup(s: &Session, cmd: &str, id: Option<u64>) -> Result<u64> {
    match (id, s.doc.cam.setups.as_slice()) {
        (Some(id), _) if s.doc.cam.setup(id).is_some() => Ok(id),
        (Some(id), _) => Err(bad(cmd, format!("there is no setup {id}"))),
        (None, [one]) => Ok(one.id),
        (None, []) => Err(bad(cmd, "make a setup first (cam.setup)")),
        (None, _) => Err(bad(cmd, "there are several setups: name one with `setup`")),
    }
}

fn pick(p: &CamPickParam, body: &str) -> GeoPick {
    GeoPick { body: p.body.clone().unwrap_or_else(|| body.to_string()), at: p.at }
}

/// The toolpath of operation `id` (in its setup), or why there is none.
fn toolpath(s: &Session, id: u64) -> std::result::Result<Toolpath, String> {
    let op = s.doc.cam.operations.iter().find(|o| o.id == id).ok_or_else(|| format!("there is no operation {id}"))?;
    let st = s.model.state();
    let plan = s.doc.cam_plan(&st, op.setup).map_err(|e| e.to_string())?;
    let job = plan.jobs.into_iter().find(|(o, _)| *o == id).map(|(_, j)| j).ok_or_else(|| format!("there is no operation {id}"))??;
    solidcraft_cam::generate(&job, &plan.clearance).map_err(|e| format!("{}: {e}", op.name))
}

fn summary(op: &CamOperation, path: std::result::Result<&Toolpath, &String>) -> CamOpSummary {
    let (cut, rapid, minutes, error) = match path {
        Ok(t) => {
            let (rapid, cut) = t.lengths();
            (cut, rapid, t.estimate_seconds(RAPID_SPEED) / 60.0, None)
        }
        Err(e) => (0.0, 0.0, 0.0, Some(e.clone())),
    };
    CamOpSummary {
        id: op.id,
        name: op.name.clone(),
        kind: kind_name(&op.spec).into(),
        tool: op.tool,
        cut_mm: round(cut, 1),
        rapid_mm: round(rapid, 1),
        minutes: round(minutes, 1),
        error,
    }
}

fn operation_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.operation";
    let q: CamOperationParams = parse(cmd, p)?;
    let existing = match q.id {
        Some(id) => Some(s.doc.cam.operations.iter().find(|o| o.id == id).cloned().ok_or_else(|| bad(cmd, format!("there is no operation {id}")))?),
        None => None,
    };
    let setup_id = which_setup(s, cmd, q.setup.or(existing.as_ref().map(|o| o.setup)))?;
    let body = s.doc.cam.setup(setup_id).map(|x| x.body.clone()).unwrap_or_default();
    let tool = q.tool.or(existing.as_ref().map(|o| o.tool)).ok_or_else(|| bad(cmd, "`tool` is required: a tool number from the library"))?;
    if s.doc.cam.tool(tool).is_none() {
        return Err(bad(cmd, format!("there is no tool T{tool} in the library (cam.tool adds one)")));
    }
    let kind = q.kind.as_deref().or(existing.as_ref().map(|o| kind_name(&o.spec)));
    let old = existing.as_ref().map(|o| &o.spec);
    let face = || -> Result<GeoPick> {
        match (&q.face, old) {
            (Some(f), _) => Ok(pick(f, &body)),
            (None, Some(OpSpec::Pocket { face } | OpSpec::Contour { face, .. })) => Ok(face.clone()),
            _ => Err(bad(cmd, "`face` is required: a point on the face")),
        }
    };
    let depth = q.depth.or(match old {
        Some(OpSpec::Face { depth } | OpSpec::Contour { depth, .. } | OpSpec::Drill { depth, .. }) => *depth,
        _ => None,
    });
    let spec = match kind {
        Some("face") => OpSpec::Face { depth },
        Some("pocket") => OpSpec::Pocket { face: face()? },
        Some("contour") => {
            let side = match q.side.as_deref() {
                Some("outside") => Side::Outside,
                Some("inside") => Side::Inside,
                Some("on") => Side::On,
                Some(x) => return Err(bad(cmd, format!("`side` must be outside, inside or on, not {x}"))),
                None => match old {
                    Some(OpSpec::Contour { side, .. }) => *side,
                    _ => Side::Outside,
                },
            };
            let tabs = match (q.tabs, old) {
                (Some(t), _) if t.count == 0 => None,
                (Some(t), _) => Some(Tabs { count: t.count.min(64), width: t.width, height: t.height }),
                (None, Some(OpSpec::Contour { tabs, .. })) => *tabs,
                (None, _) => None,
            };
            OpSpec::Contour { face: face()?, side, depth, tabs }
        }
        Some("drill") => {
            let edges = match (&q.edges, old) {
                (Some(list), _) => list.iter().take(MAX_ITEMS).map(|e| pick(e, &body)).collect(),
                (None, Some(OpSpec::Drill { edges, .. })) => edges.clone(),
                _ => return Err(bad(cmd, "`edges` is required: points on the holes' round edges")),
            };
            let peck = match (q.peck, old) {
                (Some(x), _) if x <= 0.0 => None,
                (Some(x), _) => Some(x),
                (None, Some(OpSpec::Drill { peck, .. })) => *peck,
                (None, _) => None,
            };
            OpSpec::Drill { edges, depth, peck }
        }
        Some(k) => return Err(bad(cmd, format!("`type` must be face, pocket, contour or drill, not {k}"))),
        None => return Err(bad(cmd, "`type` is required for a new operation")),
    };
    let overrides = [q.spindle_rpm, q.feed, q.plunge_feed, q.stepdown, q.stepover];
    let cutting = if overrides.iter().any(Option::is_some) {
        let material = s.doc.cam.setup(setup_id).map_or(Material::Wood, |x| x.material);
        let base = existing.as_ref().and_then(|o| o.cutting).unwrap_or_else(|| {
            s.doc.cam.tool(tool).map_or(Cutting { spindle_rpm: 10_000.0, feed: 500.0, plunge_feed: 150.0, stepdown: 1.0, stepover: 0.4 }, |t| {
                Cutting::starting(material, t)
            })
        });
        Some(Cutting {
            spindle_rpm: q.spindle_rpm.unwrap_or(base.spindle_rpm),
            feed: q.feed.unwrap_or(base.feed),
            plunge_feed: q.plunge_feed.unwrap_or(base.plunge_feed),
            stepdown: q.stepdown.unwrap_or(base.stepdown),
            stepover: q.stepover.unwrap_or(base.stepover),
        })
    } else {
        existing.as_ref().and_then(|o| o.cutting)
    };
    let id = existing.as_ref().map_or_else(|| s.doc.cam.next_id(), |o| o.id);
    let name = match q.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
        Some(n) => n.chars().take(80).collect(),
        None => existing.as_ref().map_or_else(|| format!("{} {}", kind_name(&spec), s.doc.cam.operations.len() + 1), |o| o.name.clone()),
    };
    let op = CamOperation { id, name, setup: setup_id, tool, spec, cutting };
    let before = (*s.doc).clone();
    {
        let cam = &mut s.doc_mut().cam;
        match cam.operations.iter_mut().find(|o| o.id == id) {
            Some(slot) => *slot = op.clone(),
            None if cam.operations.len() >= MAX_ITEMS => return Err(bad(cmd, "there are too many operations")),
            None => cam.operations.push(op.clone()),
        }
    }
    let path = toolpath(s, id);
    if let Err(why) = &path {
        *s.doc_mut() = before;
        return Err(bad(cmd, why.clone()));
    }
    reply(cmd, CamOperationResult { operation: summary(&op, path.as_ref()), setup: setup_id, param_warnings: Vec::new() })
}

fn delete_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.delete";
    let q: CamDeleteParams = parse(cmd, p)?;
    if q.id.is_none() && q.tool.is_none() {
        return Err(bad(cmd, "name an `id` (a setup or an operation) or a `tool`"));
    }
    if let Some(t) = q.tool {
        if s.doc.cam.tool(t).is_none() {
            return Err(bad(cmd, format!("there is no tool T{t}")));
        }
        if let Some(user) = s.doc.cam.operations.iter().find(|o| o.tool == t) {
            return Err(bad(cmd, format!("T{t} is used by {}: change or delete that operation first", user.name)));
        }
    }
    let mut ids = Vec::new();
    if let Some(id) = q.id {
        if s.doc.cam.setup(id).is_some() {
            ids.push(id);
            ids.extend(s.doc.cam.operations.iter().filter(|o| o.setup == id).map(|o| o.id));
        } else if s.doc.cam.operations.iter().any(|o| o.id == id) {
            ids.push(id);
        } else {
            return Err(bad(cmd, format!("there is no setup or operation {id}")));
        }
    }
    let cam = &mut s.doc_mut().cam;
    cam.setups.retain(|x| !ids.contains(&x.id));
    cam.operations.retain(|o| !ids.contains(&o.id));
    if let Some(t) = q.tool {
        cam.tools.retain(|x| x.number != t);
    }
    reply(cmd, CamDeleted { ids, tool: q.tool, param_warnings: Vec::new() })
}

/// Each operation of setup `id` with its toolpath, or the setup's error.
fn setup_paths(s: &Session, id: u64) -> std::result::Result<Vec<(CamOperation, std::result::Result<Toolpath, String>)>, String> {
    let st = s.model.state();
    let plan = s.doc.cam_plan(&st, id).map_err(|e| e.to_string())?;
    Ok(plan
        .jobs
        .into_iter()
        .filter_map(|(op_id, job)| {
            let op = s.doc.cam.operations.iter().find(|o| o.id == op_id)?.clone();
            let path = job.and_then(|j| solidcraft_cam::generate(&j, &plan.clearance).map_err(|e| format!("{}: {e}", op.name)));
            Some((op, path))
        })
        .collect())
}

fn generate_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.generate";
    let q: CamGenerateParams = parse(cmd, p)?;
    let s: &Session = s;
    if let Some(id) = q.setup
        && s.doc.cam.setup(id).is_none()
    {
        return Err(bad(cmd, format!("there is no setup {id}")));
    }
    let st = s.model.state();
    let setups = s
        .doc
        .cam
        .setups
        .iter()
        .filter(|x| q.setup.is_none_or(|id| id == x.id))
        .map(|x| {
            let stock = s.doc.cam_plan(&st, x.id).map(|plan| [0, 1, 2].map(|i| round(plan.stock_max[i] - plan.stock_min[i], 3))).unwrap_or([0.0; 3]);
            let (operations, error) = match setup_paths(s, x.id) {
                Ok(list) => (list.iter().map(|(op, path)| summary(op, path.as_ref())).collect(), None),
                Err(e) => (Vec::new(), Some(e)),
            };
            CamSetupSummary {
                id: x.id,
                name: x.name.clone(),
                body: x.body.clone(),
                material: material_name(x.material).into(),
                flipped: x.flipped,
                stock,
                operations,
                error,
            }
        })
        .collect();
    reply(cmd, CamGenerated { setups, tools: s.doc.cam.tools.iter().map(tool_info).collect(), param_warnings: Vec::new() })
}

fn export_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.export";
    let q: CamExportParams = parse(cmd, p)?;
    let setup_id = which_setup(s, cmd, q.setup)?;
    let name = s.doc.cam.setup(setup_id).map(|x| x.name.clone()).unwrap_or_default();
    let paths = setup_paths(s, setup_id).map_err(|e| bad(cmd, e))?;
    if paths.is_empty() {
        return Err(bad(cmd, format!("{name} has no operations to write")));
    }
    let mut ok = Vec::new();
    for (_, path) in paths {
        ok.push(path.map_err(|e| bad(cmd, e))?);
    }
    let base = absolute(Path::new(q.path.trim()));
    let stem = base.file_stem().map(|x| x.to_string_lossy().into_owned()).filter(|x| !x.is_empty()).unwrap_or_else(|| "program".into());
    let ext = base.extension().map(|x| x.to_string_lossy().into_owned()).filter(|x| !x.is_empty()).unwrap_or_else(|| "nc".into());
    let dir = base.parent().map(Path::to_path_buf).unwrap_or_default();
    let options = solidcraft_cam::PostOptions { title: name.clone(), arcs: q.arcs.unwrap_or(true), ..solidcraft_cam::PostOptions::default() };
    let mut files = Vec::new();
    for (k, (tool, group)) in solidcraft_cam::by_tool(&ok).into_iter().enumerate() {
        let file = dir.join(format!("{stem}_{:02}_T{}.{ext}", k + 1, tool.number)).to_string_lossy().into_owned();
        solidcraft_io::vfs::check_design_path(&file, s.path.as_deref()).map_err(|e| EngineError::Other(format!("{file}: {e}")))?;
        let text = solidcraft_cam::grbl(&group, &options);
        solidcraft_io::vfs::write(&file, text.as_bytes()).map_err(|e| EngineError::Other(format!("{file}: {e}")))?;
        let minutes: f64 = group.iter().map(|t| t.estimate_seconds(RAPID_SPEED)).sum::<f64>() / 60.0;
        files.push(CamProgram {
            path: file,
            tool: tool.number,
            operations: group.iter().map(|t| t.name.clone()).collect(),
            minutes: round(minutes, 1),
        });
    }
    reply(cmd, CamExported { files, param_warnings: Vec::new() })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A 60 × 40 × 10 block from the origin with a 6 mm hole at (45, 20).
    fn block() -> Session {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 60, "width": 40, "height": 10, "corner": [0, 0, 0]})).unwrap();
        s.execute("solid.hole", &json!({"position": [45, 20, 10], "diameter": 6})).unwrap();
        s
    }

    #[test]
    fn a_block_is_faced_cut_out_and_drilled_into_one_program_per_tool() {
        let mut s = block();
        s.execute("cam.tool", &json!({"number": 1, "diameter": 6, "flute_length": 20})).unwrap();
        s.execute("cam.tool", &json!({"number": 2, "diameter": 3.175, "flute_length": 20})).unwrap();
        let setup = s.execute("cam.setup", &json!({})).unwrap();
        assert_eq!(setup["stock"], json!([66.0, 46.0, 11.0]));
        assert_eq!(setup["part_top"], json!(-1.0));
        s.execute("cam.operation", &json!({"type": "face", "tool": 1})).unwrap();
        let cut = s
            .execute(
                "cam.operation",
                &json!({"type": "contour", "tool": 2, "face": {"at": [10, 10, 10]}, "tabs": {"count": 4, "width": 5, "height": 2}}),
            )
            .unwrap();
        assert!(cut["operation"]["cut_mm"].as_f64().unwrap() > 200.0, "{cut}");
        s.execute("cam.operation", &json!({"type": "drill", "tool": 2, "edges": [{"at": [48, 20, 10]}]})).unwrap();
        // A pocket's floor must face up; the block's bottom doesn't.
        assert!(s.execute("cam.operation", &json!({"type": "pocket", "tool": 2, "face": {"at": [10, 10, 0]}})).is_err());
        assert_eq!(s.doc.cam.operations.len(), 3, "a refused operation is not kept");
        let report = s.execute("cam.generate", &json!({})).unwrap();
        let ops = report["setups"][0]["operations"].as_array().unwrap();
        assert!(ops.iter().all(|o| o.get("error").is_none()), "{report}");
        let dir = std::env::temp_dir().join(format!("solidcraft-cam-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let out = s.execute("cam.export", &json!({"path": dir.join("block.nc")})).unwrap();
        let files = out["files"].as_array().unwrap();
        assert_eq!(files.len(), 2, "{out}");
        let second = std::fs::read_to_string(files[1]["path"].as_str().unwrap()).unwrap();
        assert!(second.contains("M3 S") && second.contains("G2 ") && second.ends_with("M30\n"), "{second}");
        // Turned over, the same contour is cut from the other side.
        let id = setup["id"].as_u64().unwrap();
        s.execute("cam.setup", &json!({"id": id, "flipped": true})).unwrap();
        let again = s.execute("cam.generate", &json!({"setup": id})).unwrap();
        assert!(again["setups"][0]["operations"][1].get("error").is_none(), "{again}");
    }

    #[test]
    fn tools_and_setups_are_checked_and_kept() {
        let mut s = Session::default();
        let tools = s.execute("cam.tool", &json!({"number": 2, "diameter": 3.175})).unwrap();
        assert_eq!(tools["tools"][0]["flutes"], json!(2));
        assert!(s.execute("cam.tool", &json!({"number": 0, "diameter": 3.0})).is_err());
        assert!(s.execute("cam.tool", &json!({"number": 1, "diameter": -1.0})).is_err());
        assert!(s.execute("cam.tool", &json!({"number": 1, "diameter": 3.0, "kind": "laser"})).is_err());
        s.execute("cam.tool", &json!({"number": 2, "diameter": 6.0})).unwrap();
        assert_eq!(s.doc.cam.tools.len(), 1, "the same number replaces the tool");
        // No body yet: a setup needs one.
        assert!(s.execute("cam.setup", &json!({})).is_err());
        assert!(s.execute("cam.operation", &json!({"type": "face", "tool": 2})).is_err());
        assert!(s.execute("cam.delete", &json!({"tool": 9})).is_err());
        s.execute("cam.delete", &json!({"tool": 2})).unwrap();
        assert!(s.doc.cam.tools.is_empty());
    }
}
