//! Machining (`solidcraft_doc::cam`): the tool library, setups and operations kept in the design,
//! their toolpaths (`solidcraft_cam`) and GRBL programs written one per tool.

use std::path::Path;

use serde_json::Value;
use solidcraft_cam::{Approach, Cutting, Entry, Finish, Material, Side, Tabs, Tool, ToolKind, Toolpath};
use solidcraft_doc::cam::{CamOperation, CamSetup, FixedStock, GeoPick, OpSpec, SetupAxes, StockMargin, WorkOrigin, WorkZero, axis_along};

use super::CommandSpec;
use super::design_link::absolute;
use crate::params::bad;
use crate::schema::cam::{
    CamCuttingInfo, CamDeleteParams, CamDeleted, CamExportParams, CamExported, CamFrame, CamGenerateParams, CamGenerated, CamHolderParam,
    CamMoveParams, CamMoved, CamOpSummary, CamOperationParams, CamOperationResult, CamPathLines, CamPickParam, CamPresetParam, CamPreview,
    CamProgram, CamSetupParams, CamSetupResult, CamSetupSummary, CamSimSteps, CamSimulation, CamSimulationParams, CamTabsInfo, CamToolFields,
    CamToolInfo, CamToolParams, CamToolpaths, CamToolpathsParams, CamToolsResult,
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
        .params(
            "number: 1..99, diameter (new tool), flutes?, flute_length?, kind?: flat_endmill|ball_endmill|bull_nose|drill|v_bit, name?, \
             shank_diameter?, overall_length?, stickout?, corner_radius?, tip_angle?, tip_diameter?, maker?, model?, notes?, \
             holder? {name, sections [[d, l]…]}, presets? [{material, spindle_rpm?, feed?, plunge_feed?, stepdown?, stepover?}] → tools; \
             changes the tool with that number (what is left out stays)",
        ),
    CommandSpec::new("cam.setup", "Machining Setup", crate::checked!("cam.setup", setup_cmd))
        .icon("box")
        .contract(&SETUP_CONTRACT)
        .output(output_schema_for::<CamSetupResult>)
        .params(
            "id? (change), name?, body? (default the only body), material?: wood|acrylic|aluminum, table? {at} (the face on the table), x_axis?, \
             origin?: top_center|bottom_center|top_front_left…bottom_back_right, origin_point? [x,y,z], stock?: margin|fixed, stock_size?, stock_place?, \
             margin_side?, margin_top?, margin_bottom?, safe_z?, retract?, dry_run? → id, name, body, stock [x,y,z], part_top, frame, stock_min, stock_max",
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
    CommandSpec::new("cam.preview", "Preview Machining", crate::checked!("cam.preview", preview_cmd))
        .icon("inspect")
        .noundo()
        .contract(&OPERATION_CONTRACT)
        .output(output_schema_for::<CamPreview>)
        .params("cam.operation's parameters → operation, setup, cuts, rapids (model coordinates); the design is not changed"),
    CommandSpec::new("cam.simulation", "Simulate Machining", crate::checked!("cam.simulation", simulation_cmd))
        .icon("play")
        .noundo()
        .contract(&SIMULATION_CONTRACT)
        .output(output_schema_for::<CamSimulation>)
        .params(
            "setup? (default the only one), step? (mm, default 0.5) → setup, body, frame {origin, x, y, z}, stock_min, stock_max (machine), operations \
             [{operation, name, tool, radius, cutter: flat|drill, flute_length, points [[x,y,z,rapid]…]}], truncated?; machine coordinates, to cut a \
             height map along",
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
pub(super) const MAX_ITEMS: usize = 500;

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

pub(super) fn tool_kind_name(k: ToolKind) -> &'static str {
    match k {
        ToolKind::FlatEndmill => "flat_endmill",
        ToolKind::BallEndmill => "ball_endmill",
        ToolKind::BullNose => "bull_nose",
        ToolKind::Drill => "drill",
        ToolKind::VBit => "v_bit",
    }
}

fn material_of(name: &str) -> Option<Material> {
    Some(match name {
        "wood" => Material::Wood,
        "acrylic" => Material::Acrylic,
        "aluminum" => Material::Aluminum,
        _ => return None,
    })
}

/// A short fingerprint of what a tool is (not its number nor where it was copied from): alike
/// tools share it.
pub(super) fn tool_revision(t: &Tool) -> String {
    let mut plain = t.clone();
    plain.number = 0;
    plain.details.library = None;
    let text = serde_json::to_string(&plain).unwrap_or_default();
    // FNV-1a, 64 bits.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

pub(super) fn holder_param(h: &solidcraft_cam::Holder) -> CamHolderParam {
    CamHolderParam { name: h.name.clone(), sections: h.sections.iter().map(|s| [s.diameter, s.length]).collect() }
}

pub(super) fn tool_info(t: &Tool) -> CamToolInfo {
    let d = &t.details;
    let pointed = matches!(t.kind, ToolKind::Drill | ToolKind::VBit);
    CamToolInfo {
        number: t.number,
        name: t.name.clone(),
        kind: tool_kind_name(t.kind).into(),
        diameter: t.diameter,
        flutes: t.flutes,
        flute_length: t.flute_length,
        shank_diameter: t.shank(),
        stickout: t.stickout(),
        overall_length: d.overall_length,
        corner_radius: d.corner_radius.filter(|_| t.kind == ToolKind::BullNose),
        tip_angle: pointed.then(|| t.tip_angle()),
        tip_diameter: d.tip_diameter.filter(|_| t.kind == ToolKind::VBit),
        maker: d.maker.clone(),
        model: d.model.clone(),
        notes: d.notes.clone(),
        holder: d.holder.as_ref().map(holder_param),
        presets: d
            .presets
            .iter()
            .map(|p| CamPresetParam {
                material: material_name(p.material).into(),
                spindle_rpm: p.spindle_rpm,
                feed: p.feed,
                plunge_feed: p.plunge_feed,
                stepdown: p.stepdown,
                stepover: p.stepover,
            })
            .collect(),
        library: d.library.as_ref().map(|l| l.id.clone()),
        revision: tool_revision(t),
    }
}

/// Set on `tool` what `f` gives, checking each; the tool's name follows its size and kind unless
/// it has one of its own.
pub(super) fn apply_tool_fields(cmd: &str, tool: &mut Tool, f: &CamToolFields) -> Result<()> {
    let text =
        |v: &Option<String>, max: usize| v.as_ref().map(|t| t.trim().chars().filter(|c| !c.is_control() || *c == '\n').take(max).collect::<String>());
    let length = |what: &str, v: Option<f64>| -> Result<Option<Option<f64>>> {
        match v {
            None => Ok(None),
            Some(v) if v == 0.0 => Ok(Some(None)),
            Some(v) if v.is_finite() && v > 0.0 && v <= 500.0 => Ok(Some(Some(v))),
            Some(_) => Err(bad(cmd, format!("`{what}` must be from 0 to 500 mm"))),
        }
    };
    let generated = tool.name.is_empty() || tool.name == default_name(tool);
    if let Some(k) = f.kind.as_deref() {
        tool.kind = match k {
            "flat_endmill" => ToolKind::FlatEndmill,
            "ball_endmill" => ToolKind::BallEndmill,
            "bull_nose" => ToolKind::BullNose,
            "drill" => ToolKind::Drill,
            "v_bit" => ToolKind::VBit,
            k => return Err(bad(cmd, format!("`kind` must be flat_endmill, ball_endmill, bull_nose, drill or v_bit, not {k}"))),
        };
    }
    if let Some(d) = f.diameter {
        tool.diameter = d;
    }
    if let Some(n) = f.flutes {
        tool.flutes = n;
    }
    if let Some(l) = f.flute_length {
        tool.flute_length = l;
    }
    let d = &mut tool.details;
    if let Some(v) = length("shank_diameter", f.shank_diameter)? {
        d.shank_diameter = v;
    }
    if let Some(v) = length("overall_length", f.overall_length)? {
        d.overall_length = v;
    }
    if let Some(v) = length("stickout", f.stickout)? {
        d.stickout = v;
    }
    if let Some(v) = length("corner_radius", f.corner_radius)? {
        d.corner_radius = v;
    }
    if let Some(v) = f.tip_angle {
        d.tip_angle = if v == 0.0 { None } else { Some(v) };
    }
    if let Some(v) = f.tip_diameter {
        d.tip_diameter = if v == 0.0 { None } else { Some(v) };
    }
    if let Some(v) = text(&f.maker, 80) {
        d.maker = v;
    }
    if let Some(v) = text(&f.model, 80) {
        d.model = v;
    }
    if let Some(v) = text(&f.notes, 2000) {
        d.notes = v;
    }
    if let Some(h) = &f.holder {
        d.holder = if h.sections.is_empty() { None } else { Some(holder_of(cmd, h)?) };
    }
    if let Some(presets) = &f.presets {
        let mut out = Vec::new();
        for p in presets.iter().take(3) {
            let material = material_of(&p.material)
                .ok_or_else(|| bad(cmd, format!("a preset's `material` must be wood, acrylic or aluminum, not {}", p.material)))?;
            let ok = |v: Option<f64>| v.is_none_or(|v| v.is_finite() && v > 0.0 && v <= 100_000.0);
            if ![p.spindle_rpm, p.feed, p.plunge_feed, p.stepdown].into_iter().all(ok) || !p.stepover.is_none_or(|v| v > 0.0 && v <= 1.0) {
                return Err(bad(cmd, "a preset's values must be above 0 (its stepover a fraction of the diameter up to 1)"));
            }
            out.retain(|q: &solidcraft_cam::Preset| q.material != material);
            out.push(solidcraft_cam::Preset {
                material,
                spindle_rpm: p.spindle_rpm,
                feed: p.feed,
                plunge_feed: p.plunge_feed,
                stepdown: p.stepdown,
                stepover: p.stepover,
            });
        }
        d.presets = out;
    }
    match text(&f.name, 80).filter(|n| !n.is_empty()) {
        Some(name) => tool.name = name,
        None if generated || f.name.is_some() => tool.name = default_name(tool),
        None => {}
    }
    if let Some(problem) = tool.problem() {
        return Err(bad(cmd, problem));
    }
    Ok(())
}

/// A holder from its parameters, checked.
pub(super) fn holder_of(cmd: &str, h: &CamHolderParam) -> Result<solidcraft_cam::Holder> {
    let name: String = h.name.trim().chars().filter(|c| !c.is_control()).take(80).collect();
    let holder = solidcraft_cam::Holder {
        name: if name.is_empty() { "holder".into() } else { name },
        sections: h.sections.iter().map(|s| solidcraft_cam::HolderSection { diameter: s[0], length: s[1] }).collect(),
    };
    match holder.problem() {
        Some(p) => Err(bad(cmd, p)),
        None => Ok(holder),
    }
}

/// The name a tool goes by unless it is given one: its size, flutes and kind.
fn default_name(t: &Tool) -> String {
    let d = t.diameter;
    match t.kind {
        ToolKind::FlatEndmill => format!("{d} mm {}-flute end mill", t.flutes),
        ToolKind::BallEndmill => format!("{d} mm {}-flute ball end mill", t.flutes),
        ToolKind::BullNose => format!("{d} mm R{} bull nose", t.details.corner_radius.unwrap_or(0.0)),
        ToolKind::Drill => format!("{d} mm drill"),
        ToolKind::VBit => format!("{}° V-bit", t.tip_angle()),
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
    let at = s.doc.cam.tools.iter().position(|t| t.number == q.number);
    let mut tool = match at.and_then(|i| s.doc.cam.tools.get(i)) {
        Some(t) => t.clone(),
        None => {
            let d = q.fields.diameter.ok_or_else(|| bad(cmd, "`diameter` is required for a new tool"))?;
            let mut t = Tool::endmill(q.number, d, 2, d * 4.0);
            t.name.clear();
            t
        }
    };
    apply_tool_fields(cmd, &mut tool, &q.fields)?;
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
                table: None,
                axes: None,
                zero: None,
                fixed_stock: None,
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
    // An older setup's `flipped` and `origin` become the axes and origin they mean.
    setup.axes = Some(setup.axes());
    setup.zero = Some(setup.zero());
    setup.flipped = false;
    setup.origin = WorkOrigin::TopCenter;
    if let Some(f) = q.flipped {
        setup.axes = Some(SetupAxes { x: [1.0, 0.0, 0.0], z: if f { [0.0, 0.0, -1.0] } else { [0.0, 0.0, 1.0] } });
        setup.table = None;
    }
    if let Some(t) = &q.table {
        let face = pick(t, &setup.body);
        let st = s.model.state();
        let axes = s.doc.cam_table_axes(&st, &face).map_err(|e| bad(cmd, e.to_string()))?;
        setup.axes = Some(axes);
        setup.table = Some(face);
    }
    if let Some(x) = q.x_axis {
        let z = setup.axes().z;
        let x = axis_along(x).ok_or_else(|| bad(cmd, "`x_axis` must run along one of the model's axes"))?;
        if (x[0] * z[0] + x[1] * z[1] + x[2] * z[2]).abs() > 1e-6 {
            return Err(bad(cmd, "`x_axis` must be square to the machine's Z (the face on the table)"));
        }
        setup.axes = Some(SetupAxes { x, z });
    }
    if let Some(o) = q.origin.as_deref() {
        setup.zero = Some(WorkZero::Stock {
            at: origin_at(o)
                .ok_or_else(|| bad(cmd, format!("`origin` must be top_center, bottom_center or a corner like top_front_left, not {o}")))?,
        });
    }
    if let Some(at) = q.origin_point {
        if !at.iter().all(|v| v.is_finite() && v.abs() < 1e6) {
            return Err(bad(cmd, "`origin_point` must be a point"));
        }
        setup.zero = Some(WorkZero::Point { at });
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
    // The stock as it is before the change: a fixed stock's default size.
    let st = s.model.state();
    let before = next.cam_plan(&st, id).ok().map(|plan| [0, 1, 2].map(|i| round(plan.stock_max[i] - plan.stock_min[i], 3)));
    match q.stock.as_deref() {
        None => {}
        Some("margin") => setup.fixed_stock = None,
        Some("fixed") => {
            if setup.fixed_stock.is_none() {
                let size = before.ok_or_else(|| bad(cmd, "give the stock's size with `stock_size`"))?;
                setup.fixed_stock = Some(FixedStock { size, place: [0, 0] });
            }
        }
        Some(other) => return Err(bad(cmd, format!("`stock` must be margin or fixed, not {other}"))),
    }
    if q.stock_size.is_some() || q.stock_place.is_some() {
        let mut f = setup.fixed_stock.ok_or_else(|| bad(cmd, "`stock_size` and `stock_place` are for a fixed stock: set `stock` to fixed"))?;
        if let Some(size) = q.stock_size {
            if !size.iter().all(|v| v.is_finite() && *v > 0.0 && *v <= 10_000.0) {
                return Err(bad(cmd, "`stock_size` must be between 0 and 10000 mm along each axis"));
            }
            f.size = size;
        }
        if let Some(place) = q.stock_place.as_deref() {
            f.place = place_at(place).ok_or_else(|| {
                bad(
                    cmd,
                    format!("`stock_place` must be center, front_left, front_right, back_left, back_right, front, back, left or right, not {place}"),
                )
            })?;
        }
        setup.fixed_stock = Some(f);
    }
    match next.cam.setups.iter_mut().find(|x| x.id == id) {
        Some(slot) => *slot = setup.clone(),
        None => next.cam.setups.push(setup.clone()),
    }
    let plan = next.cam_plan(&st, id).map_err(|e| bad(cmd, e.to_string()))?;
    if !q.dry_run.unwrap_or(false) {
        *s.doc_mut() = next;
    }
    let size = [0, 1, 2].map(|i| round(plan.stock_max[i] - plan.stock_min[i], 3));
    reply(
        cmd,
        CamSetupResult {
            id,
            name: setup.name,
            body: setup.body,
            stock: size,
            part_top: round(plan.part_top, 3),
            frame: frame_info(&plan.frame),
            stock_min: plan.stock_min.map(|v| round(v, 4)),
            stock_max: plan.stock_max.map(|v| round(v, 4)),
            param_warnings: Vec::new(),
        },
    )
}

/// A setup's frame, rounded, for a reply.
fn frame_info(f: &solidcraft_cam::Frame) -> CamFrame {
    let r = |v: [f64; 3]| v.map(|c| round(c, 6));
    CamFrame { origin: r(f.origin), x: r(f.x), y: r(f.y), z: r(f.z) }
}

/// The stock-box point an origin name means: −1, 0 or 1 along machine X, Y and Z.
fn origin_at(name: &str) -> Option<[i8; 3]> {
    match name {
        "top_center" => return Some([0, 0, 1]),
        "bottom_center" => return Some([0, 0, -1]),
        "top_corner" => return Some([-1, -1, 1]),
        _ => {}
    }
    let mut parts = name.split('_');
    let z = match parts.next()? {
        "top" => 1,
        "bottom" => -1,
        _ => return None,
    };
    let y = match parts.next()? {
        "front" => -1,
        "back" => 1,
        _ => return None,
    };
    let x = match parts.next()? {
        "left" => -1,
        "right" => 1,
        _ => return None,
    };
    parts.next().is_none().then_some([x, y, z])
}

/// The name of a stock-box point: the inverse of [`origin_at`].
fn origin_name(at: [i8; 3]) -> String {
    let z = if at[2] < 0 { "bottom" } else { "top" };
    match (at[0], at[1]) {
        (0, 0) => format!("{z}_center"),
        (x, y) => format!("{z}_{}_{}", if y < 0 { "front" } else { "back" }, if x < 0 { "left" } else { "right" }),
    }
}

/// Where the part sits in a fixed stock: −1, 0 or 1 along machine X and Y.
fn place_at(name: &str) -> Option<[i8; 2]> {
    Some(match name {
        "center" => [0, 0],
        "front_left" => [-1, -1],
        "front_right" => [1, -1],
        "back_left" => [-1, 1],
        "back_right" => [1, 1],
        "front" => [0, -1],
        "back" => [0, 1],
        "left" => [-1, 0],
        "right" => [1, 0],
        _ => return None,
    })
}

fn place_name(place: [i8; 2]) -> &'static str {
    match (place[0].signum(), place[1].signum()) {
        (-1, -1) => "front_left",
        (1, -1) => "front_right",
        (-1, 1) => "back_left",
        (1, 1) => "back_right",
        (0, -1) => "front",
        (0, 1) => "back",
        (-1, 0) => "left",
        (1, 0) => "right",
        _ => "center",
    }
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
    super::cam_kit::generate(&job, &plan.clearance).map_err(|e| named(&op.name, &e))
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
    let (offset, side, tabs, peck) = match &op.spec {
        OpSpec::Face { offset } | OpSpec::Pocket { offset, .. } => (*offset, None, None, None),
        OpSpec::Contour { side, offset, tabs, .. } => {
            (*offset, Some(side_name(*side).to_string()), tabs.map(|t| CamTabsInfo { count: t.count, width: t.width, height: t.height }), None)
        }
        OpSpec::Drill { offset, peck, .. } => (*offset, None, None, *peck),
    };
    let lowest = path.ok().and_then(Toolpath::lowest_z).map(|z| round(z, 3));
    let approach = match &op.spec {
        OpSpec::Pocket { approach, .. } | OpSpec::Contour { approach, .. } => Some(*approach),
        OpSpec::Face { .. } | OpSpec::Drill { .. } => None,
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
        offset,
        lowest,
        side,
        tabs,
        peck,
        entry: approach.map(|a| entry_name(a.entry).to_string()),
        entry_angle: approach.and_then(|a| match a.entry {
            Some(Entry::Ramp { angle } | Entry::Helix { angle, .. }) => Some(angle),
            _ => None,
        }),
        helix_radius: approach.and_then(|a| match a.entry {
            Some(Entry::Helix { radius, .. }) => Some(radius),
            _ => None,
        }),
        lead: approach.and_then(|a| a.lead),
        finish_passes: approach.and_then(|a| a.finish).map(|f| f.passes),
        finish_stepover: approach.and_then(|a| a.finish).map(|f| f.stepover),
        spring_pass: approach.and_then(|a| a.finish).is_some_and(|f| f.spring),
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
    let (op, setup_id) = build_operation(s, cmd, q)?;
    let id = op.id;
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
    let path = toolpath(s, id).and_then(|t| match gouge(s, setup_id, &op, &t) {
        Some(why) => Err(why),
        None => Ok(t),
    });
    if let Err(why) = &path {
        *s.doc_mut() = before;
        return Err(bad(cmd, why.clone()));
    }
    reply(cmd, CamOperationResult { operation: summary(&op, path.as_ref()), setup: setup_id, param_warnings: Vec::new() })
}

/// The operation `cam.operation` would make, and its toolpath drawn in model coordinates: worked
/// out on a copy of the design, which stays as it was.
fn preview_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.preview";
    let q: CamOperationParams = parse(cmd, p)?;
    let s: &Session = s;
    let (op, setup) = build_operation(s, cmd, q)?;
    let mut doc = (*s.doc).clone();
    match doc.cam.operations.iter_mut().find(|o| o.id == op.id) {
        Some(slot) => *slot = op.clone(),
        None => doc.cam.operations.push(op.clone()),
    }
    let st = s.model.state();
    let mut plan = doc.cam_plan(&st, setup).map_err(|e| bad(cmd, e.to_string()))?;
    let frame = plan.frame;
    let jobs = std::mem::take(&mut plan.jobs);
    let path = jobs
        .into_iter()
        .find(|(o, _)| *o == op.id)
        .map(|(_, j)| j)
        .ok_or_else(|| format!("there is no operation {}", op.id))
        .and_then(|j| j)
        .and_then(|j| super::cam_kit::generate(&j, &plan.clearance).map_err(|e| named(&op.name, &e)))
        .and_then(|t| match gouge_in(&doc, &st, setup, &plan, &op, &t) {
            Some(why) => Err(why),
            None => Ok(t),
        });
    let model = |p: solidcraft_cam::P3| frame.to_model([p.x, p.y, p.z]).map(|v| round(v, 4));
    let (mut cuts, mut rapids) = (Vec::new(), Vec::new());
    if let Ok(t) = &path {
        let mut last: Option<[f64; 3]> = None;
        let mut run: Vec<[f64; 3]> = Vec::new();
        for (p, rapid) in t.points(1.0).into_iter().take(MAX_POINTS) {
            let p = model(p);
            if rapid {
                if run.len() > 1 {
                    cuts.push(std::mem::take(&mut run));
                }
                run.clear();
                if let Some(from) = last {
                    rapids.push([from, p]);
                }
            } else {
                if run.is_empty()
                    && let Some(from) = last
                {
                    run.push(from);
                }
                run.push(p);
            }
            last = Some(p);
        }
        if run.len() > 1 {
            cuts.push(run);
        }
    }
    reply(cmd, CamPreview { operation: summary(&op, path.as_ref()), setup, cuts, rapids, param_warnings: Vec::new() })
}

/// The operation `q` makes (a new one, or `q.id` changed), checked but not yet in the design.
fn build_operation(s: &Session, cmd: &str, q: CamOperationParams) -> Result<(CamOperation, u64)> {
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
            (None, Some(OpSpec::Pocket { face, .. } | OpSpec::Contour { face, .. })) => Ok(face.clone()),
            _ => Err(bad(cmd, "`face` is required: a point on the face")),
        }
    };
    let kept = match old {
        _ if q.default_offset == Some(true) => None,
        Some(OpSpec::Face { offset } | OpSpec::Pocket { offset, .. } | OpSpec::Contour { offset, .. } | OpSpec::Drill { offset, .. }) => *offset,
        None => None,
    };
    let offset = q.offset.or(kept);
    let spec = match kind {
        Some("face") => OpSpec::Face { offset },
        Some("pocket") => OpSpec::Pocket { face: face()?, offset, approach: approach(cmd, &q, old, false)? },
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
            OpSpec::Contour { face: face()?, side, offset, tabs, approach: approach(cmd, &q, old, true)? }
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
            OpSpec::Drill { edges, offset, peck }
        }
        Some(k) => return Err(bad(cmd, format!("`type` must be face, pocket, contour or drill, not {k}"))),
        None => return Err(bad(cmd, "`type` is required for a new operation")),
    };
    let overrides = [q.spindle_rpm, q.feed, q.plunge_feed, q.stepdown, q.stepover];
    let kept = if q.reset_cutting == Some(true) { None } else { existing.as_ref().and_then(|o| o.cutting) };
    let cutting = if overrides.iter().any(Option::is_some) {
        let material = s.doc.cam.setup(setup_id).map_or(Material::Wood, |x| x.material);
        let base = kept.unwrap_or_else(|| {
            s.doc
                .cam
                .tool(tool)
                .map_or(Cutting { spindle_rpm: 10_000.0, feed: 500.0, plunge_feed: 150.0, stepdown: 1.0, stepover: 0.4 }, |t| t.cutting(material))
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
    Ok((CamOperation { id, name, setup: setup_id, tool, spec, cutting }, setup_id))
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
            let path = job.and_then(|j| super::cam_kit::generate(&j, &plan.clearance).map_err(|e| named(&op.name, &e)));
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
                flipped: x.axes().z[2] < -0.5,
                table: x.table.as_ref().map(|t| t.at),
                x_axis: x.axes().x,
                z_axis: x.axes().z,
                origin: match x.zero() {
                    WorkZero::Stock { at } => origin_name(at),
                    WorkZero::Point { .. } => "point".into(),
                },
                origin_point: match x.zero() {
                    WorkZero::Point { at } => Some(at),
                    WorkZero::Stock { .. } => None,
                },
                margins: [x.margin.side, x.margin.top, x.margin.bottom],
                stock_mode: if x.fixed_stock.is_some() { "fixed" } else { "margin" }.into(),
                stock_size: x.fixed_stock.map(|f| f.size),
                stock_place: x.fixed_stock.map(|f| place_name(f.place).to_string()),
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
        let cutter = match t.tool.kind {
            ToolKind::Drill | ToolKind::VBit => "drill",
            ToolKind::BallEndmill => "ball",
            ToolKind::BullNose => "bull",
            ToolKind::FlatEndmill => "flat",
        };
        operations.push(CamSimSteps {
            operation: op.id,
            name: op.name.clone(),
            tool: op.tool,
            radius: round(t.tool.diameter / 2.0, 4),
            cutter: cutter.into(),
            flute_length: round(t.tool.flute_length, 4),
            tool_data: serde_json::to_value(&t.tool).unwrap_or(Value::Null),
            points,
        });
    }
    reply(
        cmd,
        CamSimulation {
            setup,
            body: s.doc.cam.setup(setup).map(|x| x.body.clone()).unwrap_or_default(),
            frame: frame_info(&plan.frame),
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

/// Why `path` (operation `op` of setup `setup`) can't be cut: where it would cut into the model.
/// A contour on its line cuts the part's edge on purpose and isn't checked.
fn gouge(s: &Session, setup: u64, op: &CamOperation, path: &Toolpath) -> Option<String> {
    let st = s.model.state();
    let plan = s.doc.cam_plan(&st, setup).ok()?;
    gouge_in(&s.doc, &st, setup, &plan, op, path)
}

fn gouge_in(
    doc: &solidcraft_doc::Document,
    st: &solidcraft_doc::ModelState,
    setup: u64,
    plan: &solidcraft_doc::cam::CamPlan,
    op: &CamOperation,
    path: &Toolpath,
) -> Option<String> {
    if matches!(op.spec, OpSpec::Contour { side: Side::On, .. }) {
        return None;
    }
    let at = doc.cam_gouge(st, setup, plan, path)?;
    Some(named(&op.name, &format!("the toolpath cuts into the model near ({:.1}, {:.1}, {:.1})", at[0], at[1], at[2])))
}

/// An operation's error, with its name in front unless the toolpath's own message already has it.
fn entry_name(e: Option<Entry>) -> &'static str {
    match e {
        None => "auto",
        Some(Entry::Plunge) => "plunge",
        Some(Entry::Ramp { .. }) => "ramp",
        Some(Entry::Helix { .. }) => "helix",
    }
}

/// The entry, leads and finishing passes `q` asks for, on top of the operation's (`old`);
/// leads and finishing passes only for a contour.
fn approach(cmd: &str, q: &CamOperationParams, old: Option<&OpSpec>, contour: bool) -> Result<Approach> {
    let mut a = match old {
        Some(OpSpec::Pocket { approach, .. } | OpSpec::Contour { approach, .. }) => *approach,
        _ => Approach::default(),
    };
    let angle_was = match a.entry {
        Some(Entry::Ramp { angle } | Entry::Helix { angle, .. }) => angle,
        _ => 3.0,
    };
    let angle = match q.entry_angle {
        Some(v) if v.is_finite() && v > 0.0 && v <= 85.0 => v,
        Some(_) => return Err(bad(cmd, "`entry_angle` must be above 0 and at most 85 degrees")),
        None => angle_was,
    };
    let radius_was = match a.entry {
        Some(Entry::Helix { radius, .. }) => Some(radius),
        _ => None,
    };
    let radius = match q.helix_radius {
        Some(v) if v.is_finite() && v > 0.0 && v <= 1000.0 => Some(v),
        Some(_) => return Err(bad(cmd, "`helix_radius` must be above 0 mm")),
        None => radius_was,
    };
    let kind = match q.entry.as_deref() {
        Some(k) => Some(k),
        None if q.entry_angle.is_some() || q.helix_radius.is_some() => Some(entry_name(a.entry)),
        None => None,
    };
    if let Some(kind) = kind {
        a.entry = match kind {
            "auto" => None,
            "plunge" => Some(Entry::Plunge),
            "ramp" => Some(Entry::Ramp { angle }),
            // The helix's radius depends on the tool: the planner's default when none is given.
            "helix" => Some(Entry::Helix { radius: radius.unwrap_or(0.0), angle }),
            other => return Err(bad(cmd, format!("`entry` must be auto, plunge, ramp or helix, not {other}"))),
        };
    }
    let asks_contour = q.lead.is_some() || q.finish_passes.is_some() || q.finish_stepover.is_some() || q.spring_pass.is_some();
    if asks_contour && !contour {
        return Err(bad(cmd, "leads and finishing passes are for a contour"));
    }
    if let Some(r) = q.lead {
        a.lead = match r {
            r if r == 0.0 => None,
            r if r.is_finite() && r > 0.0 && r <= 100.0 => Some(r),
            _ => return Err(bad(cmd, "`lead` must be from 0 to 100 mm")),
        };
    }
    if q.finish_passes.is_some() || q.finish_stepover.is_some() || q.spring_pass.is_some() {
        let was = a.finish.unwrap_or(Finish { passes: 0, stepover: 0.2, spring: false });
        let passes = q.finish_passes.unwrap_or(if a.finish.is_none() { 1 } else { was.passes }).min(20);
        let stepover = match q.finish_stepover {
            Some(v) if v.is_finite() && v > 0.0 && v <= 10.0 => v,
            Some(_) => return Err(bad(cmd, "`finish_stepover` must be above 0 and at most 10 mm")),
            None => was.stepover,
        };
        let spring = q.spring_pass.unwrap_or(was.spring);
        a.finish = (passes > 0).then_some(Finish { passes, stepover, spring });
    }
    Ok(a)
}

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
        // Turned over, the cut ends past the stock's far side: the model's top, now on the table
        // (the 1 mm of stock on top is on the spindle's side, the model's bottom).
        let id = setup["id"].as_u64().unwrap();
        s.execute("cam.setup", &json!({"id": id, "flipped": true})).unwrap();
        let flipped = s.execute("cam.toolpaths", &json!({"setup": id})).unwrap();
        let cuts: Vec<[f64; 3]> = serde_json::from_value::<Vec<Vec<[f64; 3]>>>(flipped["paths"][0]["cuts"].clone()).unwrap().concat();
        let high = cuts.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
        assert!((high - 10.5).abs() < 1e-6, "{high}");
        assert!(s.execute("cam.toolpaths", &json!({"step": 0})).is_err());
    }

    #[test]
    fn a_setup_takes_its_axes_from_the_face_on_the_table_and_its_origin_from_the_stock() {
        let mut s = block();
        s.execute("cam.tool", &json!({"number": 2, "diameter": 3.175, "flute_length": 50})).unwrap();
        // The block's front face (model −Y) on the table: machine Z is the model's +Y, X stays X.
        let setup = s.execute("cam.setup", &json!({"table": {"at": [30, 0, 5]}})).unwrap();
        let id = setup["id"].as_u64().unwrap();
        assert_eq!(setup["frame"]["z"], json!([0.0, 1.0, 0.0]), "{setup}");
        assert_eq!(setup["frame"]["x"], json!([1.0, 0.0, 0.0]));
        assert_eq!(setup["frame"]["y"], json!([0.0, 0.0, -1.0]));
        // 40 mm tall now, 1 mm of stock on top; the origin at the top's centre.
        assert_eq!(setup["stock"], json!([66.0, 16.0, 41.0]));
        assert_eq!(setup["part_top"], json!(-1.0));
        assert_eq!(setup["frame"]["origin"], json!([30.0, 41.0, 5.0]));
        // The back face faces up: its outline is cut out down through the part.
        let cut = s.execute("cam.contour", &json!({"tool": 2, "face": {"at": [30, 40, 5]}})).unwrap();
        assert!((cut["operation"]["lowest"].as_f64().unwrap() + 41.5).abs() < 1e-6, "{cut}");
        // The model's top is a wall in this setup, not a face to cut down to.
        assert!(s.execute("cam.pocket", &json!({"tool": 2, "face": {"at": [10, 10, 10]}})).is_err());
        // A corner of the stock's bottom as the origin, tried without a change.
        let tried = s.execute("cam.setup", &json!({"id": id, "origin": "bottom_front_left", "dry_run": true})).unwrap();
        assert_eq!(tried["frame"]["origin"], json!([-3.0, 0.0, 13.0]), "{tried}");
        assert_eq!(tried["stock_min"], json!([0.0, 0.0, 0.0]));
        let report = s.execute("cam.generate", &json!({"setup": id})).unwrap();
        assert_eq!(report["setups"][0]["origin"], "top_center", "a dry run changes nothing");
        // A vertex of the model as the origin.
        let at_vertex = s.execute("cam.setup", &json!({"id": id, "origin_point": [60, 40, 10], "dry_run": true})).unwrap();
        assert_eq!(at_vertex["frame"]["origin"], json!([60.0, 40.0, 10.0]));
        // Machine X along the model's Z instead; along the table's normal it can't be.
        let turned = s.execute("cam.setup", &json!({"id": id, "x_axis": [0, 0, 1], "dry_run": true})).unwrap();
        assert_eq!(turned["stock"], json!([16.0, 66.0, 41.0]), "{turned}");
        assert!(s.execute("cam.setup", &json!({"id": id, "x_axis": [0, 1, 0]})).is_err());
        assert!(s.execute("cam.setup", &json!({"id": id, "x_axis": [1, 1, 0]})).is_err());
        // A block of stock 70 × 20 × 45 with the part in its front left corner.
        let fixed = s.execute("cam.setup", &json!({"id": id, "stock": "fixed", "stock_size": [70, 20, 45], "stock_place": "front_left"})).unwrap();
        assert_eq!(fixed["stock"], json!([70.0, 20.0, 45.0]), "{fixed}");
        assert_eq!(fixed["stock_min"], json!([-35.0, -10.0, -45.0]));
        assert_eq!(fixed["part_top"], json!(-5.0));
        // The part against the stock's left and front: origin = stock top centre at (35, 0).
        assert_eq!(fixed["frame"]["origin"], json!([35.0, 45.0, 0.0]));
        let report = s.execute("cam.generate", &json!({"setup": id})).unwrap();
        let summary = &report["setups"][0];
        assert_eq!(summary["stock_mode"], "fixed");
        assert_eq!(summary["stock_place"], "front_left");
        assert_eq!(summary["z_axis"], json!([0.0, 1.0, 0.0]));
        assert!(s.execute("cam.setup", &json!({"id": id, "stock_size": [50, 20, 45]})).is_err(), "the part doesn't fit");
        assert!(s.execute("cam.setup", &json!({"id": id, "stock_place": "middle"})).is_err());
        assert!(s.execute("cam.setup", &json!({"id": id, "origin": "top_left"})).is_err());
        // Back to margins, and the setup is as it was.
        let back = s.execute("cam.setup", &json!({"id": id, "stock": "margin"})).unwrap();
        assert_eq!(back["stock"], json!([66.0, 16.0, 41.0]));
        // A face that doesn't face along a model axis can't sit on the table; a missing pick neither.
        assert!(s.execute("cam.setup", &json!({"id": id, "table": {"at": [500, 500, 500]}})).is_err());
    }

    #[test]
    fn contours_and_pockets_take_entries_leads_and_finishing_passes() {
        let mut s = block();
        s.execute("cam.tool", &json!({"number": 1, "diameter": 3.175, "flute_length": 20})).unwrap();
        s.execute("cam.setup", &json!({})).unwrap();
        // A contour ramping in at 5°, with leads and a finishing pass gone round twice.
        let cut = s
            .execute(
                "cam.contour",
                &json!({"tool": 1, "face": {"at": [10, 10, 10]}, "entry": "ramp", "entry_angle": 5, "lead": 2, "finish_passes": 1, "finish_stepover": 0.2, "spring_pass": true}),
            )
            .unwrap();
        let op = &cut["operation"];
        assert!(op["error"].is_null(), "{cut}");
        assert_eq!(op["entry"], "ramp");
        assert_eq!(op["entry_angle"], 5.0);
        assert_eq!(op["lead"], 2.0);
        assert_eq!(op["finish_passes"], 1);
        assert_eq!(op["spring_pass"], true);
        assert!((op["lowest"].as_f64().unwrap() + 11.5).abs() < 1e-6, "{op}");
        let id = op["id"].as_u64().unwrap();
        // Taken off again: no lead, no finishing pass, back to automatic.
        let plain = s.execute("cam.operation", &json!({"id": id, "lead": 0, "finish_passes": 0, "entry": "auto"})).unwrap();
        assert!(plain["operation"]["lead"].is_null() && plain["operation"]["finish_passes"].is_null(), "{plain}");
        assert_eq!(plain["operation"]["entry"], "auto");
        // A pocket over the top face, round the hole, goes down in a helix by itself.
        let pocket = s.execute("cam.pocket", &json!({"tool": 1, "face": {"at": [10, 10, 10]}})).unwrap();
        assert!(pocket["operation"]["error"].is_null(), "{pocket}");
        assert_eq!(pocket["operation"]["entry"], "auto");
        let lines = s.execute("cam.toolpaths", &json!({})).unwrap();
        assert!(lines.to_string().len() > 100);
        // Leads are for contours; unknown entries and steep angles are refused.
        assert!(s.execute("cam.pocket", &json!({"tool": 1, "face": {"at": [10, 10, 10]}, "lead": 2})).is_err());
        assert!(s.execute("cam.contour", &json!({"tool": 1, "face": {"at": [10, 10, 10]}, "entry": "spiral"})).is_err());
        assert!(s.execute("cam.contour", &json!({"tool": 1, "face": {"at": [10, 10, 10]}, "entry": "ramp", "entry_angle": 90})).is_err());
        // A helix wider than the tool can't go down: the toolkit says why.
        let wide = s
            .execute("cam.preview", &json!({"type": "pocket", "tool": 1, "face": {"at": [10, 10, 10]}, "entry": "helix", "helix_radius": 5}))
            .unwrap();
        assert!(wide["operation"]["error"].as_str().is_some_and(|e| e.contains("helix")), "{wide}");
    }

    #[test]
    fn an_older_setup_turned_over_reads_as_the_top_on_the_table() {
        let mut s = block();
        s.execute("cam.tool", &json!({"number": 2, "diameter": 3.175, "flute_length": 20})).unwrap();
        let id = s.execute("cam.setup", &json!({})).unwrap()["id"].as_u64().unwrap();
        {
            let setup = s.doc_mut().cam.setups.iter_mut().find(|x| x.id == id).unwrap();
            setup.axes = None;
            setup.zero = None;
            setup.flipped = true;
            setup.origin = WorkOrigin::TopCorner;
        }
        let report = s.execute("cam.generate", &json!({"setup": id})).unwrap();
        let summary = &report["setups"][0];
        assert_eq!(summary["z_axis"], json!([0.0, 0.0, -1.0]), "{report}");
        assert_eq!(summary["flipped"], true);
        assert_eq!(summary["origin"], "top_front_left");
        let sim = s.execute("cam.simulation", &json!({"setup": id})).unwrap();
        // Machine Y runs along the model's −Y; the front left of the turned-over stock is the
        // model's (−3, 43), its top 1 mm below the model's bottom.
        assert_eq!(sim["frame"]["y"], json!([0.0, -1.0, 0.0]), "{sim}");
        assert_eq!(sim["frame"]["origin"], json!([-3.0, 43.0, -1.0]));
        // Changing it saves the new form, meaning the same.
        let again = s.execute("cam.setup", &json!({"id": id, "margin_side": 3})).unwrap();
        assert_eq!(again["frame"]["origin"], json!([-3.0, 43.0, -1.0]));
        let setup = s.doc.cam.setup(id).unwrap();
        assert!(!setup.flipped && setup.axes.is_some() && setup.zero.is_some());
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
        assert_eq!(op["cutter"], "flat");
        assert_eq!(op["flute_length"], 20.0);
        assert_eq!(sim["body"], s.doc.cam.setups[0].body.as_str());
        let stock = solidcraft_cam::Stock::new([-33.0, -23.0, -11.0], [33.0, 23.0, 0.0], 0.5).unwrap();
        let moves: Vec<(solidcraft_cam::P3, bool)> = points.iter().map(|p| (solidcraft_cam::P3::new(p[0], p[1], p[2]), p[3] > 0.5)).collect();
        let cutter = solidcraft_cam::Cutter::flat(radius, 20.0);
        let mut run = solidcraft_cam::Run::new(stock, vec![solidcraft_cam::RunOp { name: "contour".into(), cutter, points: moves }]);
        run.advance(f64::INFINITY, false);
        assert!(run.events.is_empty(), "nothing crashes: {:?}", run.events);
        let stock = &run.stock;
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
    fn a_preview_shows_the_toolpath_and_leaves_the_design_alone() {
        let mut s = block();
        s.execute("cam.tool", &json!({"number": 1, "diameter": 3.175, "flute_length": 20})).unwrap();
        s.execute("cam.setup", &json!({})).unwrap();
        let count = s.doc.cam.operations.len();
        let undo = s.undo.len();
        let shown = s.execute("cam.preview", &json!({"type": "contour", "tool": 1, "face": {"at": [10, 10, 10]}, "stepdown": 2})).unwrap();
        assert!(!shown["cuts"].as_array().unwrap().is_empty(), "{shown}");
        assert_eq!(shown["operation"]["cutting"]["stepdown"], 2.0);
        assert_eq!(s.doc.cam.operations.len(), count, "nothing is added");
        assert_eq!(s.undo.len(), undo);
        // A toolpath that can't be made says why, still without a change.
        let deep = s.execute("cam.preview", &json!({"type": "contour", "tool": 1, "face": {"at": [10, 10, 10]}, "offset": -60})).unwrap();
        assert!(deep["operation"]["error"].is_string(), "{deep}");
        assert!(deep["cuts"].as_array().unwrap().is_empty());
        // Changing an operation shows the change, and the operation itself stays.
        let id = s.execute("cam.contour", &json!({"tool": 1, "face": {"at": [10, 10, 10]}})).unwrap()["operation"]["id"].as_u64().unwrap();
        let changed = s.execute("cam.preview", &json!({"id": id, "stepdown": 0.5})).unwrap();
        assert_eq!(changed["operation"]["cutting"]["stepdown"], 0.5);
        assert!(s.doc.cam.operations.iter().find(|o| o.id == id).unwrap().cutting.is_none());
    }

    #[test]
    fn bottoms_come_from_the_model_and_nothing_cuts_into_it() {
        let mut s = block();
        s.execute("cam.tool", &json!({"number": 1, "diameter": 3.175, "flute_length": 20})).unwrap();
        s.execute("cam.tool", &json!({"number": 2, "diameter": 8, "flute_length": 20})).unwrap();
        s.execute("cam.tool", &json!({"number": 3, "diameter": 5, "flute_length": 20})).unwrap();
        s.execute("cam.setup", &json!({})).unwrap();
        // Facing stops at the part's top (1 mm under the stock's), and can't go lower.
        let face = s.execute("cam.face", &json!({"tool": 1})).unwrap()["operation"].clone();
        assert!((face["lowest"].as_f64().unwrap() + 1.0).abs() < 1e-6, "{face}");
        assert!(s.execute("cam.face", &json!({"tool": 1, "offset": -0.5})).is_err(), "below the part's top");
        let higher = s.execute("cam.face", &json!({"tool": 1, "offset": 0.3})).unwrap()["operation"].clone();
        assert!((higher["lowest"].as_f64().unwrap() + 0.7).abs() < 1e-6, "{higher}");
        // Cutting the block out goes through: 0.5 mm below the stock by default, more by offset.
        let out = s.execute("cam.contour", &json!({"tool": 1, "face": {"at": [10, 10, 10]}})).unwrap()["operation"].clone();
        assert!((out["lowest"].as_f64().unwrap() + 11.5).abs() < 1e-6, "{out}");
        let deeper = s.execute("cam.preview", &json!({"type": "contour", "tool": 1, "face": {"at": [10, 10, 10]}, "offset": -1})).unwrap();
        assert!((deeper["operation"]["lowest"].as_f64().unwrap() + 12.0).abs() < 1e-6, "{deeper}");
        // The 6 mm hole goes through: drilled through with a 5 mm drill.
        let hole = s.execute("cam.drill", &json!({"tool": 3, "edges": [{"at": [48, 20, 10]}]})).unwrap()["operation"].clone();
        assert!((hole["lowest"].as_f64().unwrap() + 11.5).abs() < 1e-6, "{hole}");
        // An 8 mm drill would cut into the block around the 6 mm hole: refused, saying where.
        let fat = s.execute("cam.preview", &json!({"type": "drill", "tool": 2, "edges": [{"at": [48, 20, 10]}]})).unwrap();
        assert!(fat["operation"]["error"].as_str().unwrap_or("").contains("cuts into the model"), "{fat}");
        assert!(s.execute("cam.drill", &json!({"tool": 2, "edges": [{"at": [48, 20, 10]}]})).is_err());
        // A contour inside the top face meets the block right under it: it only takes the stock
        // above the part's top.
        let inside = s.execute("cam.preview", &json!({"type": "contour", "tool": 1, "face": {"at": [10, 10, 10]}, "side": "inside"})).unwrap();
        assert!((inside["operation"]["lowest"].as_f64().unwrap() + 1.0).abs() < 0.02, "{inside}");
        // Lower it can't go: a preview says why, and the operation is refused.
        let lower =
            s.execute("cam.preview", &json!({"type": "contour", "tool": 1, "face": {"at": [10, 10, 10]}, "side": "inside", "offset": -1})).unwrap();
        assert!(lower["operation"]["error"].as_str().unwrap_or("").contains("through the part"), "{lower}");
        assert!(s.execute("cam.contour", &json!({"tool": 1, "face": {"at": [10, 10, 10]}, "side": "inside", "offset": -1})).is_err());
        // Back to where the model puts the bottom.
        let id = higher["id"].as_u64().unwrap();
        let back = s.execute("cam.operation", &json!({"id": id, "default_offset": true})).unwrap()["operation"].clone();
        assert!(back.get("offset").is_none() && (back["lowest"].as_f64().unwrap() + 1.0).abs() < 1e-6, "{back}");
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
