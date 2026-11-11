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
    CamCuttingInfo, CamDeleteParams, CamDeleted, CamExportParams, CamExported, CamGenerateParams, CamGenerated, CamMoveParams, CamMoved,
    CamOpSummary, CamOperationParams, CamOperationResult, CamPathLines, CamPickParam, CamProgram, CamSetupParams, CamSetupResult, CamSetupSummary,
    CamSimSteps, CamSimulation, CamSimulationParams, CamTabsInfo, CamToolInfo, CamToolParams, CamToolpaths, CamToolpathsParams, CamToolsResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const TOOL_CONTRACT: TypedContract<CamToolParams> = TypedContract::new();
const SETUP_CONTRACT: TypedContract<CamSetupParams> = TypedContract::new();
const OPERATION_CONTRACT: TypedContract<CamOperationParams> = TypedContract::new();
const DELETE_CONTRACT: TypedContract<CamDeleteParams> = TypedContract::new();
const GENERATE_CONTRACT: TypedContract<CamGenerateParams> = TypedContract::new();
const EXPORT_CONTRACT: TypedContract<CamExportParams> = TypedContract::new();
const TOOLPATHS_CONTRACT: TypedContract<CamToolpathsParams> = TypedContract::new();
const MOVE_CONTRACT: TypedContract<CamMoveParams> = TypedContract::new();
const SIMULATION_CONTRACT: TypedContract<CamSimulationParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cam.tool", "Machining Tool", crate::checked!("cam.tool", tool_cmd))
        .icon("library")
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
    CommandSpec::new("cam.generate", "Machining", crate::checked!("cam.generate", generate_cmd))
        .icon("list")
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
    CommandSpec::new("cam.toolpaths", "Show Toolpaths", crate::checked!("cam.toolpaths", toolpaths_cmd))
        .icon("inspect")
        .noundo()
        .contract(&TOOLPATHS_CONTRACT)
        .output(output_schema_for::<CamToolpaths>)
        .params("setup? (default the only one), step? (mm, default 1) → setup, stock [min, max], paths [{operation, name, tool, cuts [[[x,y,z]…]…], rapids [[from, to]…], error?}], truncated?; model coordinates"),

    CommandSpec::new("cam.move", "Reorder Machining", crate::checked!("cam.move", move_cmd))
        .icon("reorder")
        .contract(&MOVE_CONTRACT)
        .output(output_schema_for::<CamMoved>)
        .params("id: an operation, by: places to move (negative: earlier) among its setup's operations → setup, operations (ids in order)"),
    CommandSpec::new("cam.simulation", "Simulate Machining", crate::checked!("cam.simulation", simulation_cmd))
        .icon("play")
        .noundo()
        .contract(&SIMULATION_CONTRACT)
        .output(output_schema_for::<CamSimulation>)
        .params(
            "setup? (default the only one), step? (mm, default 0.5) → setup, origin, flipped, stock_min, stock_max (machine), operations \
             [{operation, name, tool, radius, points [[x,y,z,rapid]…]}], truncated?; machine coordinates, to cut a height map along",
        ),
    CommandSpec::new("cam.face", "Face Milling", crate::checked!("cam.face", face_cmd))
        .icon("offset_face")
        .contract(&OPERATION_CONTRACT)
        .output(output_schema_for::<CamOperationResult>)
        .params("cam.operation with type face: setup?, tool, depth?, name?, cutting data? → operation, setup"),
    CommandSpec::new("cam.pocket", "Pocket", crate::checked!("cam.pocket", pocket_cmd))
        .icon("cut")
        .contract(&OPERATION_CONTRACT)
        .output(output_schema_for::<CamOperationResult>)
        .params("cam.operation with type pocket: setup?, tool, face: {at, body?} (the floor), depth?, name?, cutting data? → operation, setup"),
    CommandSpec::new("cam.contour", "Contour", crate::checked!("cam.contour", contour_cmd))
        .icon("trace")
        .contract(&OPERATION_CONTRACT)
        .output(output_schema_for::<CamOperationResult>)
        .params("cam.operation with type contour: setup?, tool, face: {at, body?}, side?, depth?, tabs?, name?, cutting data? → operation, setup"),
    CommandSpec::new("cam.drill", "Drilling", crate::checked!("cam.drill", drill_cmd))
        .icon("hole")
        .contract(&OPERATION_CONTRACT)
        .output(output_schema_for::<CamOperationResult>)
        .params("cam.operation with type drill: setup?, tool, edges: [{at, body?}], depth?, peck?, name?, cutting data? → operation, setup"),
];

/// Most points `cam.toolpaths` returns.
const MAX_POINTS: usize = 400_000;

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
    let at = s.doc.cam.tools.iter().position(|t| t.number == q.number);
    if at.is_none() && s.doc.cam.tools.len() >= MAX_ITEMS {
        return Err(bad(cmd, "the tool library is full"));
    }
    let cam = &mut s.doc_mut().cam;
    match at.and_then(|i| cam.tools.get_mut(i)) {
        Some(slot) => *slot = tool,
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
    solidcraft_cam::generate(&job, &plan.clearance).map_err(|e| named(&op.name, &e))
}

fn summary(op: &CamOperation, path: std::result::Result<&Toolpath, &String>) -> CamOpSummary {
    let (cut, rapid, minutes, error) = match path {
        Ok(t) => {
            let (rapid, cut) = t.lengths();
            (cut, rapid, t.estimate_seconds(RAPID_SPEED) / 60.0, None)
        }
        Err(e) => (0.0, 0.0, 0.0, Some(e.clone())),
    };
    let cutting = path.ok().map(|t| CamCuttingInfo {
        spindle_rpm: round(t.cutting.spindle_rpm, 1),
        feed: round(t.cutting.feed, 1),
        plunge_feed: round(t.cutting.plunge_feed, 1),
        stepdown: round(t.cutting.stepdown, 3),
        stepover: round(t.cutting.stepover, 3),
    });
    let (depth, side, tabs, peck) = match &op.spec {
        OpSpec::Face { depth } => (*depth, None, None, None),
        OpSpec::Pocket { .. } => (None, None, None, None),
        OpSpec::Contour { side, depth, tabs, .. } => {
            (*depth, Some(side_name(*side).to_string()), tabs.map(|t| CamTabsInfo { count: t.count, width: t.width, height: t.height }), None)
        }
        OpSpec::Drill { depth, peck, .. } => (*depth, None, None, *peck),
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
        cutting,
        custom_cutting: op.cutting.is_some(),
        depth,
        side,
        tabs,
        peck,
    }
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Outside => "outside",
        Side::Inside => "inside",
        Side::On => "on",
    }
}

fn move_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.move";
    let q: CamMoveParams = parse(cmd, p)?;
    let Some(op) = s.doc.cam.operations.iter().find(|o| o.id == q.id).cloned() else {
        return Err(bad(cmd, format!("there is no operation {}", q.id)));
    };
    // The places of the setup's operations in the list, and where this one is among them.
    let places: Vec<usize> = s.doc.cam.operations.iter().enumerate().filter(|(_, o)| o.setup == op.setup).map(|(i, _)| i).collect();
    let from = places.iter().position(|&i| s.doc.cam.operations.get(i).is_some_and(|o| o.id == q.id)).unwrap_or(0);
    let last = places.len().saturating_sub(1);
    let to = (i64::try_from(from).unwrap_or(0) + i64::from(q.by)).clamp(0, i64::try_from(last).unwrap_or(0));
    let to = usize::try_from(to).unwrap_or(from);
    if to != from {
        let mut ops: Vec<CamOperation> = places.iter().filter_map(|&i| s.doc.cam.operations.get(i).cloned()).collect();
        if from < ops.len() && to < ops.len() {
            let moved = ops.remove(from);
            ops.insert(to, moved);
        }
        let cam = &mut s.doc_mut().cam;
        for (&i, o) in places.iter().zip(ops) {
            if let Some(slot) = cam.operations.get_mut(i) {
                *slot = o;
            }
        }
    }
    let operations = s.doc.cam.operations.iter().filter(|o| o.setup == op.setup).map(|o| o.id).collect();
    reply(cmd, CamMoved { setup: op.setup, operations, param_warnings: Vec::new() })
}

/// `cam.operation` of one type: `type` is set (and must agree when given).
fn typed_operation(s: &mut Session, p: &Value, cmd: &str, kind: &str) -> Result<Value> {
    let mut p = match p {
        Value::Null => serde_json::json!({}),
        p => p.clone(),
    };
    let Some(o) = p.as_object_mut() else { return Err(bad(cmd, "the parameters are an object")) };
    match o.get("type").and_then(Value::as_str) {
        Some(t) if t != kind => return Err(bad(cmd, format!("{cmd} makes a {kind} operation, not {t}"))),
        _ => {}
    }
    o.insert("type".into(), Value::from(kind));
    operation_cmd(s, &p)
}

fn face_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    typed_operation(s, p, "cam.face", "face")
}

fn pocket_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    typed_operation(s, p, "cam.pocket", "pocket")
}

fn contour_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    typed_operation(s, p, "cam.contour", "contour")
}

fn drill_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    typed_operation(s, p, "cam.drill", "drill")
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
    let kept = if q.reset_cutting == Some(true) { None } else { existing.as_ref().and_then(|o| o.cutting) };
    let cutting = if overrides.iter().any(Option::is_some) {
        let material = s.doc.cam.setup(setup_id).map_or(Material::Wood, |x| x.material);
        let base = kept.unwrap_or_else(|| {
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
        kept
    };
    let id = existing.as_ref().map_or_else(|| s.doc.cam.next_id(), |o| o.id);
    let name = match q.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
        Some(n) => n.chars().take(80).collect(),
        None => existing.as_ref().map_or_else(|| format!("{} {}", kind_name(&spec), s.doc.cam.operations.len() + 1), |o| o.name.clone()),
    };
    let op = CamOperation { id, name, setup: setup_id, tool, spec, cutting };
    let at = s.doc.cam.operations.iter().position(|o| o.id == id);
    if at.is_none() && s.doc.cam.operations.len() >= MAX_ITEMS {
        return Err(bad(cmd, "there are too many operations"));
    }
    let before = (*s.doc).clone();
    {
        let cam = &mut s.doc_mut().cam;
        match at.and_then(|i| cam.operations.get_mut(i)) {
            Some(slot) => *slot = op.clone(),
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
            let path = job.and_then(|j| solidcraft_cam::generate(&j, &plan.clearance).map_err(|e| named(&op.name, &e)));
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

fn simulation_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.simulation";
    let q: CamSimulationParams = parse(cmd, p)?;
    let step = match q.step {
        Some(v) if !(v.is_finite() && (0.01..=100.0).contains(&v)) => return Err(bad(cmd, "step is between 0.01 and 100 mm")),
        Some(v) => v,
        None => 0.5,
    };
    let setup = which_setup(s, cmd, q.setup)?;
    let s: &Session = s;
    let plan = s.doc.cam_plan(&s.model.state(), setup).map_err(|e| bad(cmd, e.to_string()))?;
    let list = setup_paths(s, setup).map_err(|e| bad(cmd, e))?;
    let mut budget = MAX_POINTS;
    let mut truncated = false;
    let mut operations = Vec::new();
    for (op, path) in list {
        let Ok(t) = path else { continue };
        let mut points = Vec::new();
        for (p, rapid) in t.points(step) {
            if budget == 0 {
                truncated = true;
                break;
            }
            budget -= 1;
            points.push([round(p.x, 4), round(p.y, 4), round(p.z, 4), if rapid { 1.0 } else { 0.0 }]);
        }
        operations.push(CamSimSteps { operation: op.id, name: op.name.clone(), tool: op.tool, radius: round(t.tool.diameter / 2.0, 4), points });
    }
    reply(
        cmd,
        CamSimulation {
            setup,
            origin: plan.frame.origin.map(|v| round(v, 4)),
            flipped: plan.frame.flipped,
            stock_min: plan.stock_min.map(|v| round(v, 4)),
            stock_max: plan.stock_max.map(|v| round(v, 4)),
            operations,
            truncated,
            param_warnings: Vec::new(),
        },
    )
}

fn toolpaths_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.toolpaths";
    let q: CamToolpathsParams = parse(cmd, p)?;
    let step = match q.step {
        Some(v) if !(v.is_finite() && (0.01..=100.0).contains(&v)) => return Err(bad(cmd, "step is between 0.01 and 100 mm")),
        Some(v) => v,
        None => 1.0,
    };
    let setup = which_setup(s, cmd, q.setup)?;
    let s: &Session = s;
    let plan = s.doc.cam_plan(&s.model.state(), setup).map_err(|e| bad(cmd, e.to_string()))?;
    let frame = plan.frame;
    let corners = [frame.to_model(plan.stock_min), frame.to_model(plan.stock_max)];
    let stock = [[0, 1, 2].map(|i| round(corners[0][i].min(corners[1][i]), 4)), [0, 1, 2].map(|i| round(corners[0][i].max(corners[1][i]), 4))];
    let list = setup_paths(s, setup).map_err(|e| bad(cmd, e))?;
    let model = |p: solidcraft_cam::P3| frame.to_model([p.x, p.y, p.z]).map(|v| round(v, 4));
    let mut budget = MAX_POINTS;
    let mut truncated = false;
    let mut paths = Vec::new();
    for (op, path) in list {
        let mut lines = CamPathLines { operation: op.id, name: op.name.clone(), tool: op.tool, cuts: Vec::new(), rapids: Vec::new(), error: None };
        match path {
            Err(e) => lines.error = Some(e),
            Ok(t) => {
                let mut last: Option<[f64; 3]> = None;
                let mut run: Vec<[f64; 3]> = Vec::new();
                for (p, rapid) in t.points(step) {
                    if budget == 0 {
                        truncated = true;
                        break;
                    }
                    budget -= 1;
                    let p = model(p);
                    if rapid {
                        if run.len() > 1 {
                            lines.cuts.push(std::mem::take(&mut run));
                        }
                        run.clear();
                        if let Some(from) = last {
                            lines.rapids.push([from, p]);
                        }
                    } else if run.is_empty()
                        && let Some(from) = last
                    {
                        run.push(from);
                    }
                    if !rapid {
                        run.push(p);
                    }
                    last = Some(p);
                }
                if run.len() > 1 {
                    lines.cuts.push(run);
                }
            }
        }
        paths.push(lines);
    }
    reply(cmd, CamToolpaths { setup, stock, paths, truncated, param_warnings: Vec::new() })
}

/// An operation's error, with its name in front unless the toolpath's own message already has it.
fn named(name: &str, e: &dyn std::fmt::Display) -> String {
    let e = e.to_string();
    if e.starts_with(&format!("{name}:")) { e } else { format!("{name}: {e}") }
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
    fn toolpaths_are_drawn_where_the_model_is_and_follow_a_turned_over_part() {
        let mut s = block();
        s.execute("cam.tool", &json!({"number": 2, "diameter": 3.175, "flute_length": 20})).unwrap();
        assert!(s.execute("cam.toolpaths", &json!({})).is_err(), "no setup yet");
        let setup = s.execute("cam.setup", &json!({})).unwrap();
        s.execute("cam.operation", &json!({"type": "contour", "tool": 2, "face": {"at": [10, 10, 10]}})).unwrap();
        let shown = s.execute("cam.toolpaths", &json!({})).unwrap();
        let path = &shown["paths"][0];
        let cuts: Vec<[f64; 3]> = serde_json::from_value::<Vec<Vec<[f64; 3]>>>(path["cuts"].clone()).unwrap().concat();
        assert!(!cuts.is_empty() && !path["rapids"].as_array().unwrap().is_empty(), "{shown}");
        // Around the block, a tool radius outside it, down to just below its bottom.
        let r = 3.175 / 2.0;
        for p in &cuts {
            // How far the tool centre is from the block (around its corners too).
            let dx = (-p[0]).max(p[0] - 60.0).max(0.0);
            let dy = (-p[1]).max(p[1] - 40.0).max(0.0);
            assert!(dx.hypot(dy) >= r - 1e-3, "{p:?} cuts into the block");
        }
        let low = cuts.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
        assert!((low + 0.5).abs() < 1e-6, "{low}");
        // The stock: 3 mm around the block, 1 mm above it.
        assert_eq!(shown["stock"], json!([[-3.0, -3.0, 0.0], [63.0, 43.0, 11.0]]));
        // Turned over, the cut ends past the stock's far side, now above it (1 mm of stock on top).
        let id = setup["id"].as_u64().unwrap();
        s.execute("cam.setup", &json!({"id": id, "flipped": true})).unwrap();
        let flipped = s.execute("cam.toolpaths", &json!({"setup": id})).unwrap();
        let cuts: Vec<[f64; 3]> = serde_json::from_value::<Vec<Vec<[f64; 3]>>>(flipped["paths"][0]["cuts"].clone()).unwrap().concat();
        let high = cuts.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
        assert!((high - 11.5).abs() < 1e-6, "{high}");
        assert!(s.execute("cam.toolpaths", &json!({"step": 0})).is_err());
    }

    #[test]
    fn operations_report_their_settings_move_and_go_back_to_the_starting_cutting_data() {
        let mut s = block();
        s.execute("cam.tool", &json!({"number": 1, "diameter": 3.175, "flute_length": 20})).unwrap();
        s.execute("cam.setup", &json!({})).unwrap();
        let face = s.execute("cam.face", &json!({"tool": 1})).unwrap()["operation"]["id"].as_u64().unwrap();
        let contour = s
            .execute(
                "cam.contour",
                &json!({"tool": 1, "face": {"at": [10, 10, 10]}, "side": "outside", "tabs": {"count": 4, "width": 6, "height": 2}, "stepdown": 0.8}),
            )
            .unwrap()["operation"]
            .clone();
        assert_eq!(contour["side"], "outside");
        assert_eq!(contour["tabs"]["count"], 4);
        assert_eq!(contour["cutting"]["stepdown"], 0.8);
        assert_eq!(contour["custom_cutting"], true);
        let id = contour["id"].as_u64().unwrap();
        // Back to the starting values, keeping the rest.
        let reset = s.execute("cam.operation", &json!({"id": id, "reset_cutting": true})).unwrap()["operation"].clone();
        assert!(reset.get("custom_cutting").is_none(), "{reset}");
        assert_ne!(reset["cutting"]["stepdown"], 0.8);
        assert_eq!(reset["tabs"]["count"], 4);
        // The contour goes first, then back.
        let moved = s.execute("cam.move", &json!({"id": id, "by": -1})).unwrap();
        assert_eq!(moved["operations"], json!([id, face]));
        let moved = s.execute("cam.move", &json!({"id": id, "by": 5})).unwrap();
        assert_eq!(moved["operations"], json!([face, id]), "it stops at the end");
        assert!(s.execute("cam.move", &json!({"id": 999, "by": 1})).is_err());
        s.execute("edit.undo", &json!({})).unwrap();
        let order: Vec<u64> = s.doc.cam.operations.iter().map(|o| o.id).collect();
        assert_eq!(order, [id, face], "a move is one undo step");
    }

    #[test]
    fn a_simulation_cuts_the_stock_around_the_block_and_leaves_the_block() {
        let mut s = block();
        s.execute("cam.tool", &json!({"number": 1, "diameter": 3.175, "flute_length": 20})).unwrap();
        s.execute("cam.setup", &json!({})).unwrap();
        s.execute("cam.contour", &json!({"tool": 1, "face": {"at": [10, 10, 10]}})).unwrap();
        let sim = s.execute("cam.simulation", &json!({})).unwrap();
        assert_eq!(sim["stock_min"], json!([-33.0, -23.0, -11.0]), "{sim}");
        assert_eq!(sim["stock_max"], json!([33.0, 23.0, 0.0]));
        let op = &sim["operations"][0];
        let points: Vec<[f64; 4]> = serde_json::from_value(op["points"].clone()).unwrap();
        let radius = op["radius"].as_f64().unwrap();
        let mut stock = solidcraft_cam::Stock::new([-33.0, -23.0, -11.0], [33.0, 23.0, 0.0], 0.5).unwrap();
        let moves: Vec<(solidcraft_cam::P3, bool)> = points.iter().map(|p| (solidcraft_cam::P3::new(p[0], p[1], p[2]), p[3] > 0.5)).collect();
        solidcraft_cam::sim::cut(&mut stock, radius, &moves, 0, moves.len());
        let at = |x: f64, y: f64| {
            let i = ((x - stock.x0) / stock.dx).round() as usize;
            let j = ((y - stock.y0) / stock.dy).round() as usize;
            stock.column(i, j).unwrap().z
        };
        assert_eq!(at(0.0, 0.0), 0.0, "the stock's top over the block stays");
        assert_eq!(at(31.5, 0.0), -11.0, "the slot around the block goes through");
        assert!(s.execute("cam.simulation", &json!({"step": 1000})).is_err());
    }

    #[test]
    fn the_typed_operation_commands_set_their_type() {
        let mut s = block();
        s.execute("cam.tool", &json!({"number": 1, "diameter": 3.175, "flute_length": 20})).unwrap();
        s.execute("cam.setup", &json!({})).unwrap();
        let face = s.execute("cam.face", &json!({"tool": 1})).unwrap();
        assert_eq!(face["operation"]["kind"], "face");
        let contour = s.execute("cam.contour", &json!({"tool": 1, "face": {"at": [10, 10, 10]}})).unwrap();
        assert_eq!(contour["operation"]["kind"], "contour");
        assert!(s.execute("cam.drill", &json!({"tool": 1, "type": "pocket"})).is_err(), "a different type is refused");
        assert!(s.execute("cam.pocket", &json!({"tool": 1})).is_err(), "a pocket still needs its floor");
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
