//! The Machining window: the design's cutters, its setups and their operations, as
//! `cam.generate` reports them, with forms that add each (`cam.tool`, `cam.setup`,
//! `cam.operation`), a Delete per row (`cam.delete`) and Export G-code… (`cam.export`). An
//! operation's faces and edges are the model's selection when Add Operation is clicked: a pocket's
//! floor or a contour's face, a drill's round edges. The ribbon's Generate Toolpaths (`ui.start` of
//! `cam.generate`) opens it.

use egui::RichText;
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_theme::Tokens;
use solidcraft_ui_text::{tr, tr_label};

use crate::SolveApp;

const KINDS: [&str; 4] = ["face", "pocket", "contour", "drill"];
const KIND_LABELS: [&str; 4] = ["Face", "Pocket", "Contour", "Drill"];
const SIDES: [&str; 3] = ["outside", "inside", "on"];
const SIDE_LABELS: [&str; 3] = ["Outside", "Inside", "On"];
const MATERIALS: [&str; 3] = ["wood", "acrylic", "aluminum"];
const MATERIAL_LABELS: [&str; 3] = ["Wood", "Acrylic", "Aluminum"];
const ORIGINS: [&str; 2] = ["top_center", "top_corner"];
const ORIGIN_LABELS: [&str; 2] = ["Top center", "Top front left"];
const TOOL_KINDS: [&str; 2] = ["flat_endmill", "drill"];
const TOOL_KIND_LABELS: [&str; 2] = ["Flat end mill", "Drill"];

/// The window's state; the lists come from the engine.
pub struct CamPanel {
    /// `cam.generate`'s reply for a design revision.
    cache: Option<(u64, Value)>,
    /// The setup new operations go to and Export writes (default: the first).
    pub setup: Option<u64>,
    // The tool form.
    pub tool_number: String,
    pub tool_diameter: String,
    pub tool_kind: usize,
    /// Empty: four times the diameter.
    pub tool_flute: String,
    // The setup form.
    pub material: usize,
    pub origin: usize,
    pub flipped: bool,
    pub margin_side: String,
    pub margin_top: String,
    pub margin_bottom: String,
    // The operation form.
    pub kind: usize,
    /// Index into the tool library.
    pub tool: usize,
    pub side: usize,
    /// The operation the form changes (`None`: the form adds one).
    pub editing: Option<u64>,
    /// Empty: the operation's default depth.
    pub depth: String,
    // Cutting data; empty: the material's starting values (or the operation's own, when changing one).
    pub rpm: String,
    pub feed: String,
    pub plunge: String,
    pub stepdown: String,
    /// Percent of the tool's diameter.
    pub stepover: String,
    pub tabs: String,
    pub tab_width: String,
    pub tab_height: String,
    pub peck: String,
    /// The last refusal.
    pub error: Option<String>,
    /// What the last export wrote.
    pub notice: Option<String>,
    /// Draw the picked setup's toolpaths over the model.
    pub show_paths: bool,
    /// `cam.toolpaths` for a design revision and setup, as lines to draw.
    paths: Option<(u64, u64, PathLines)>,
    /// Counts the times `paths` was worked out, so the model scene draws the new lines.
    generation: u64,
    /// The stock being cut along the toolpaths, while a simulation plays.
    pub sim: Option<crate::cam_sim::Sim>,
}

/// A setup's toolpaths in model coordinates: cutting runs and rapid moves.
#[derive(Clone, Default)]
pub struct PathLines {
    pub cuts: Vec<Vec<Vec3>>,
    pub rapids: Vec<[Vec3; 2]>,
    /// The stock's corners (lowest, highest).
    pub stock: Option<[Vec3; 2]>,
}

impl Default for CamPanel {
    fn default() -> Self {
        Self {
            cache: None,
            setup: None,
            tool_number: "1".into(),
            tool_diameter: "3.175".into(),
            tool_kind: 0,
            tool_flute: String::new(),
            material: 0,
            origin: 0,
            flipped: false,
            margin_side: "3".into(),
            margin_top: "1".into(),
            margin_bottom: "0".into(),
            kind: 2,
            tool: 0,
            side: 0,
            editing: None,
            depth: String::new(),
            rpm: String::new(),
            feed: String::new(),
            plunge: String::new(),
            stepdown: String::new(),
            stepover: String::new(),
            tabs: "4".into(),
            tab_width: "6".into(),
            tab_height: "2".into(),
            peck: "0".into(),
            error: None,
            notice: None,
            show_paths: true,
            paths: None,
            generation: 0,
            sim: None,
        }
    }
}

/// What a click asked for, done after the window is drawn.
enum Action {
    AddTool,
    NewSetup,
    Delete(u64),
    DeleteTool(u32),
    AddOperation,
    /// Load an operation into the form to change it.
    Edit(Value),
    /// Leave the form's change of an operation.
    StopEditing,
    /// The operation being changed back to the material's starting cutting data.
    ResetCutting,
    Move(u64, i32),
    Export,
    /// Start (or start again) playing the setup's toolpaths against its stock.
    Simulate,
    PlayPause,
    ToEnd,
    StopSimulation,
}

/// A number typed in a field: `None` when it is empty or not a finite number.
fn number(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

impl CamPanel {
    /// `cam.generate` for the design as it is now (worked out again only after a change).
    fn generated(&mut self, app: &mut SolveApp) -> Value {
        let revision = app.session.revision;
        if let Some((r, v)) = &self.cache
            && *r == revision
        {
            return v.clone();
        }
        let v = match app.session.execute("cam.generate", &json!({})) {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e.to_string());
                json!({"setups": [], "tools": []})
            }
        };
        self.cache = Some((revision, v.clone()));
        v
    }

    /// The picked setup's toolpaths (worked out again only after a change).
    fn lines(&mut self, app: &mut SolveApp) -> Option<PathLines> {
        let setup = self.setup?;
        let revision = app.session.revision;
        if let Some((r, s, lines)) = &self.paths
            && *r == revision
            && *s == setup
        {
            return Some(lines.clone());
        }
        let lines = app.session.execute("cam.toolpaths", &json!({"setup": setup})).map(|v| path_lines(&v)).unwrap_or_default();
        self.paths = Some((revision, setup, lines.clone()));
        self.generation = self.generation.wrapping_add(1);
        Some(lines)
    }

    /// Fill the form from an operation (`cam.generate`'s summary of it) to change it.
    fn load(&mut self, op: &Value, tools: &[Value]) {
        let num = |v: &Value| v.as_f64().map(|x| format!("{}", (x * 1000.0).round() / 1000.0)).unwrap_or_default();
        self.editing = op["id"].as_u64();
        self.kind = KINDS.iter().position(|k| Some(*k) == op["kind"].as_str()).unwrap_or(self.kind);
        self.tool = tools.iter().position(|t| t["number"] == op["tool"]).unwrap_or(self.tool);
        self.depth = num(&op["depth"]);
        self.side = SIDES.iter().position(|k| Some(*k) == op["side"].as_str()).unwrap_or(0);
        if op["tabs"].is_object() {
            self.tabs = text(&op["tabs"]["count"]);
            self.tab_width = num(&op["tabs"]["width"]);
            self.tab_height = num(&op["tabs"]["height"]);
        } else if op["kind"] == "contour" {
            self.tabs = "0".into();
        }
        self.peck = num(&op["peck"]);
        let c = &op["cutting"];
        self.rpm = num(&c["spindle_rpm"]);
        self.feed = num(&c["feed"]);
        self.plunge = num(&c["plunge_feed"]);
        self.stepdown = num(&c["stepdown"]);
        self.stepover = c["stepover"].as_f64().map(|x| format!("{}", (x * 1000.0).round() / 10.0)).unwrap_or_default();
    }

    /// Back to adding operations, with the cutting data left to the material.
    fn stop_editing(&mut self) {
        self.editing = None;
        for f in [&mut self.rpm, &mut self.feed, &mut self.plunge, &mut self.stepdown, &mut self.stepover, &mut self.depth] {
            f.clear();
        }
    }

    fn tool_params(&self) -> Result<Value, String> {
        let n = self.tool_number.trim().parse::<u32>().map_err(|_| tr("cam.panel.bad_tool_number").to_string())?;
        let d = number(&self.tool_diameter).ok_or_else(|| tr("cam.panel.bad_diameter").to_string())?;
        let kind = TOOL_KINDS.get(self.tool_kind).copied().unwrap_or("flat_endmill");
        let mut p = json!({"number": n, "diameter": d, "kind": kind});
        if !self.tool_flute.trim().is_empty() {
            p["flute_length"] = json!(number(&self.tool_flute).ok_or_else(|| tr("cam.panel.bad_flute").to_string())?);
        }
        Ok(p)
    }

    fn setup_params(&self, body: Option<String>) -> Value {
        let mut p = json!({
            "material": MATERIALS.get(self.material).copied().unwrap_or("wood"),
            "origin": ORIGINS.get(self.origin).copied().unwrap_or("top_center"),
            "flipped": self.flipped,
        });
        for (key, text) in [("margin_side", &self.margin_side), ("margin_top", &self.margin_top), ("margin_bottom", &self.margin_bottom)] {
            if let Some(v) = number(text) {
                p[key] = json!(v);
            }
        }
        if let Some(b) = body {
            p["body"] = json!(b);
        }
        p
    }

    /// `cam.operation`'s parameters from the form and the model's selection.
    fn operation_params(&self, setup: u64, tool: u32, selection: &[Sel]) -> Result<Value, String> {
        let kind = KINDS.get(self.kind).copied().unwrap_or("contour");
        let mut p = json!({"setup": setup, "type": kind, "tool": tool});
        // Changing an operation keeps its faces and edges unless others are selected.
        let changing = self.editing.is_some();
        if let Some(id) = self.editing {
            p["id"] = json!(id);
        }
        if let Some(d) = number(&self.depth) {
            p["depth"] = json!(d);
        }
        for (key, text, scale) in [
            ("spindle_rpm", &self.rpm, 1.0),
            ("feed", &self.feed, 1.0),
            ("plunge_feed", &self.plunge, 1.0),
            ("stepdown", &self.stepdown, 1.0),
            ("stepover", &self.stepover, 0.01),
        ] {
            if let Some(v) = number(text) {
                p[key] = json!(v * scale);
            }
        }
        let face = selection.iter().find_map(|s| match s {
            Sel::Face { body, point, .. } => Some(json!({"at": [point.x, point.y, point.z], "body": body})),
            _ => None,
        });
        match kind {
            "pocket" | "contour" => match face {
                Some(face) => p["face"] = face,
                None if changing => {}
                None => return Err(tr(if kind == "pocket" { "cam.panel.pick_floor" } else { "cam.panel.pick_face" }).to_string()),
            },
            "drill" => {
                let edges: Vec<Value> = selection
                    .iter()
                    .filter_map(|s| match s {
                        Sel::Edge { body, point, .. } => Some(json!({"at": [point.x, point.y, point.z], "body": body})),
                        _ => None,
                    })
                    .collect();
                if !edges.is_empty() {
                    p["edges"] = json!(edges);
                } else if !changing {
                    return Err(tr("cam.panel.pick_edges").to_string());
                }
                if let Some(v) = number(&self.peck) {
                    p["peck"] = json!(v);
                }
            }
            _ => {}
        }
        if kind == "contour" {
            p["side"] = json!(SIDES.get(self.side).copied().unwrap_or("outside"));
            let count = self.tabs.trim().parse::<u32>().unwrap_or(0);
            p["tabs"] = json!({"count": count, "width": number(&self.tab_width).unwrap_or(6.0), "height": number(&self.tab_height).unwrap_or(2.0)});
        }
        Ok(p)
    }
}

fn point(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    Some(Vec3::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?))
}

/// `cam.toolpaths`'s reply as lines.
fn path_lines(v: &Value) -> PathLines {
    let mut out = PathLines::default();
    for p in v["paths"].as_array().into_iter().flatten() {
        for run in p["cuts"].as_array().into_iter().flatten() {
            let pts: Vec<Vec3> = run.as_array().into_iter().flatten().filter_map(point).collect();
            if pts.len() > 1 {
                out.cuts.push(pts);
            }
        }
        for r in p["rapids"].as_array().into_iter().flatten() {
            if let (Some(a), Some(b)) = (r.get(0).and_then(point), r.get(1).and_then(point)) {
                out.rapids.push([a, b]);
            }
        }
    }
    if let (Some(a), Some(b)) = (point(&v["stock"][0]), point(&v["stock"][1])) {
        out.stock = Some([a, b]);
    }
    out
}

/// The twelve edges of the box between `a` and `b`.
fn box_edges(a: Vec3, b: Vec3) -> Vec<[Vec3; 2]> {
    let c = |i: usize| Vec3::new(if i & 1 == 0 { a.x } else { b.x }, if i & 2 == 0 { a.y } else { b.y }, if i & 4 == 0 { a.z } else { b.z });
    let mut out = Vec::with_capacity(12);
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                out.push([c(i), c(i | bit)]);
            }
        }
    }
    out
}

/// The lines the model scene draws with the model hiding them (the cuts and the stock's
/// edges), while the window shows toolpaths. [`paint_toolpaths`] draws them all faintly on top,
/// so what the model hides still shows through, dimmed.
pub(crate) fn scene_lines(sc: &mut crate::gpu::GpuScene, app: &SolveApp) {
    let Some((_, _, lines)) = app.machining.as_ref().filter(|d| d.show_paths && d.sim.is_none()).and_then(|d| d.paths.as_ref()) else {
        return;
    };
    let t = Tokens::get();
    let up = [[0.0f32, 0.0, 1.0]; 2];
    let cut = sc.color(crate::viewport::c4(t.accent));
    for run in &lines.cuts {
        for w in run.windows(2) {
            if let [a, b] = w {
                sc.lifted_line(a.to_f32(), b.to_f32(), cut, 1.8, up);
            }
        }
    }
    if let Some([a, b]) = lines.stock {
        let edge = sc.color(crate::viewport::c4(t.text_dim));
        for [p, q] in box_edges(a, b) {
            sc.line(p.to_f32(), q.to_f32(), edge, 1.0, false);
        }
    }
}

/// What the model scene's lines depend on: `None` while no toolpaths are shown.
pub(crate) fn scene_key(app: &SolveApp) -> Option<(u64, u64)> {
    app.machining.as_ref().filter(|d| d.show_paths && d.paths.is_some() && d.sim.is_none()).map(|d| (d.generation, d.setup.unwrap_or(0)))
}

/// Draw the open window's toolpaths over the model: faintly, so the parts the model hides show
/// through (the model scene draws the visible cuts solid, [`scene_lines`]), and rapids dashed.
pub fn paint_toolpaths(app: &mut SolveApp, painter: &egui::Painter, proj: &crate::viewport::Proj) {
    let Some(mut d) = app.machining.take() else { return };
    // A simulation playing shows the stock being cut instead of the lines; a change to the
    // design or another setup picked ends it.
    if d.sim.as_ref().is_some_and(|s| s.revision != app.session.revision || Some(s.setup) != d.setup) {
        d.sim = None;
    }
    if let Some(sim) = d.sim.as_mut() {
        crate::cam_sim::paint(sim, painter, proj);
        app.machining = Some(d);
        return;
    }
    let lines = if d.show_paths { d.lines(app) } else { None };
    app.machining = Some(d);
    let Some(lines) = lines else { return };
    let t = Tokens::get();
    // Without the GPU (tests, no adapter) nothing else draws them, so draw them solid.
    let faint = if app.viewport.gpu.is_some() { 0.3 } else { 1.0 };
    let cut = egui::Stroke::new(1.2, t.accent.gamma_multiply(faint));
    for run in &lines.cuts {
        let pts: Vec<egui::Pos2> = run.iter().filter_map(|p| proj.to_screen(*p)).collect();
        if pts.len() == run.len() {
            painter.add(egui::Shape::line(pts, cut));
        }
    }
    let rapid = egui::Stroke::new(1.0, t.text_dim.gamma_multiply(0.8));
    for [a, b] in &lines.rapids {
        if let (Some(sa), Some(sb)) = (proj.to_screen(*a), proj.to_screen(*b)) {
            painter.extend(crate::ui_kit::dashed_line(&[sa, sb], painter.clip_rect(), rapid, 4.0, 3.0));
        }
    }
}

/// The name of operation `id` of a setup (`cam.generate`'s summary), in the interface language.
fn s_ops_name(setup: Option<&Value>, id: u64) -> String {
    setup
        .and_then(|s| s["operations"].as_array())
        .and_then(|ops| ops.iter().find(|o| o["id"].as_u64() == Some(id)))
        .map(|o| op_name(&text(&o["name"])))
        .unwrap_or_default()
}

/// An operation's generated name (`contour 2`) in the interface language; others as they are.
fn op_name(name: &str) -> String {
    if let Some((kind, n)) = name.split_once(' ')
        && !n.is_empty()
        && n.chars().all(|c| c.is_ascii_digit())
        && let Some(i) = KINDS.iter().position(|k| *k == kind)
        && let Some(label) = KIND_LABELS.get(i)
    {
        return format!("{} {n}", tr_label(label));
    }
    name.to_string()
}

/// A setup's generated name (`Setup 1`) in the interface language; others as they are.
fn setup_name(name: &str) -> String {
    match name.strip_prefix("Setup ") {
        Some(n) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => solidcraft_ui_text::format("cam.setup_name", &[("n", n)]),
        _ => name.to_string(),
    }
}

/// A tool's generated name (`3.175 mm 2-flute end mill`, `3 mm drill`) in the interface
/// language; others as they are.
fn tool_name(name: &str) -> String {
    for (pattern, id) in [("{d} mm {f}-flute end mill", "cam.tool_name.endmill"), ("{d} mm drill", "cam.tool_name.drill")] {
        if let Some(args) = fill(pattern, name) {
            let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
            return solidcraft_ui_text::format(id, &args);
        }
    }
    name.to_string()
}

/// Any generated name in the interface language.
fn any_name(name: &str) -> String {
    let op = op_name(name);
    if op != name {
        return op;
    }
    let setup = setup_name(name);
    if setup != name { setup } else { tool_name(name) }
}

/// The machining commands' messages, as the engine words them, and their message ids. A
/// `{…}` stands for a value; a value that is a generated name is translated too.
const MESSAGES: &[(&str, &str)] = &[
    ("{depth} mm deep is more than {tool}'s flute length ({flute} mm)", "cam.error.too_deep"),
    ("the bottom must be below the top", "cam.error.bottom_below_top"),
    ("the safe height must be above the top of the material and the retract", "cam.error.safe_height"),
    ("too many passes: make the depth per pass larger", "cam.error.too_many_passes"),
    ("the area to face is empty", "cam.error.face_empty"),
    ("too many facing lines: make the stepover larger", "cam.error.too_many_facing_lines"),
    ("too many pocket rings: make the stepover larger", "cam.error.too_many_rings"),
    ("the pocket has no boundary", "cam.error.pocket_no_boundary"),
    ("{tool} doesn't fit in the pocket", "cam.error.tool_too_big_pocket"),
    ("the contour has no profile", "cam.error.no_profile"),
    ("{tool} doesn't fit inside the profile", "cam.error.tool_too_big_profile"),
    ("tabs need a width and a height", "cam.error.tabs_size"),
    ("there are no holes to drill", "cam.error.no_holes"),
    ("a hole's position is not a number", "cam.error.hole_position"),
    ("the peck depth must be more than 0 mm", "cam.error.peck"),
    ("a drill can only drill", "cam.error.drill_only"),
    ("the diameter must be between 0 and 100 mm", "cam.error.diameter"),
    ("the flute length must be more than 0 mm", "cam.error.flute_length"),
    ("a tool needs at least one flute", "cam.error.flutes"),
    ("there is no body `{body}`", "cam.error.no_body"),
    ("the stock margins must be between 0 and 1000 mm", "cam.error.margins"),
    ("there is no tool T{n} in the library", "cam.error.no_tool"),
    ("there is no tool T{n} in the library (cam.tool adds one)", "cam.error.no_tool"),
    ("the depth must be more than 0 mm", "cam.error.depth"),
    ("there is nothing to face: the stock has no margin above the part", "cam.error.nothing_to_face"),
    ("pick the pocket's floor, a face that faces up in this setup", "cam.error.pocket_floor"),
    ("pick the holes' circular edges", "cam.error.hole_edges"),
    ("the pick is not on a face", "cam.error.not_on_face"),
    ("the face is not flat", "cam.error.not_flat"),
    ("the face is not level: it must face straight up or down", "cam.error.not_level"),
    ("the face has no boundary", "cam.error.no_boundary"),
    ("the edge is not a circle seen from above: pick a hole's round edge", "cam.error.not_circle"),
    ("the face has a free-form edge, which can't be machined yet: only lines and arcs", "cam.error.free_form"),
    ("the face's boundary doesn't close", "cam.error.open_boundary"),
    ("the model has no body to machine", "cam.error.no_body_to_machine"),
    ("the model has several bodies: name the one to machine with `body`", "cam.error.several_bodies"),
    ("T{n} is used by {name}: change or delete that operation first", "cam.error.tool_in_use"),
    ("{name} has no operations to write", "cam.error.nothing_to_write"),
    ("make a setup first (cam.setup)", "cam.panel.no_setup"),
    ("the tool library is full", "cam.error.library_full"),
    ("there are too many setups", "cam.error.too_many_setups"),
    ("there are too many operations", "cam.error.too_many_operations"),
];

/// The values of `pattern`'s `{…}` holes when `text` is the pattern filled in.
fn fill<'p>(pattern: &'p str, text: &str) -> Option<Vec<(&'p str, String)>> {
    let mut args = Vec::new();
    let (mut p, mut t) = (pattern, text);
    loop {
        match p.find('{') {
            None => return (p == t).then_some(args),
            Some(open) => {
                t = t.strip_prefix(p.get(..open)?)?;
                let rest = p.get(open + 1..)?;
                let close = rest.find('}')?;
                let name = rest.get(..close)?;
                p = rest.get(close + 1..)?;
                // The value runs to the next literal piece (or the end).
                let next = p.find('{').map_or(p, |i| p.get(..i).unwrap_or(p));
                let end = if next.is_empty() { t.len() } else { t.find(next)? };
                let value = t.get(..end)?;
                // A value never holds `: `: that separates a name from its message.
                if value.is_empty() || value.contains(": ") {
                    return None;
                }
                args.push((name, value.to_string()));
                t = t.get(end..)?;
            }
        }
    }
}

/// A machining message from the engine in the interface language: the command's
/// `invalid parameters for …:` dropped, names translated, known wordings translated; anything
/// else as it came.
fn cam_text(error: &str) -> String {
    let text = error.strip_prefix("invalid parameters for `").and_then(|rest| rest.split_once("`: ")).map_or(error, |(_, text)| text);
    translate(text)
}

fn translate(text: &str) -> String {
    for (pattern, id) in MESSAGES {
        if let Some(args) = fill(pattern, text) {
            let values: Vec<(&str, String)> = args.iter().map(|(k, v)| (*k, any_name(v))).collect();
            let values: Vec<(&str, &str)> = values.iter().map(|(k, v)| (*k, v.as_str())).collect();
            return solidcraft_ui_text::format(id, &values);
        }
    }
    // `name: message`: the name of the operation, setup or tool the message is about.
    if let Some((name, rest)) = text.split_once(": ") {
        return format!("{}: {}", any_name(name), translate(rest));
    }
    crate::i18n::tr_engine_error(text)
}

/// The Manufacture workspace is open (and Render is not).
pub fn workspace_active(app: &SolveApp) -> bool {
    app.ui.manufacture && !crate::render_workspace::active(app)
}

/// Switch to the `design`, `render` or `manufacture` workspace. Manufacture shows only its own
/// ribbon tab and opens the Machining window; leaving it closes the window.
pub fn switch_workspace(app: &mut SolveApp, workspace: &str) -> Result<(), String> {
    match workspace {
        "manufacture" => {
            if workspace_active(app) {
                open(app);
                return Ok(());
            }
            if app.session.active_sketch.is_some() || app.tool.is_some() || app.dialog.is_some() || app.file_load.busy() {
                return Err(tr("cam.panel.finish_first").to_string());
            }
            if crate::render_workspace::active(app) {
                crate::render_workspace::enter(app, false)?;
            }
            app.ui.manufacture = true;
            app.ui.tab = "MANUFACTURE".into();
            app.ui.ribbon_open = false;
            open(app);
            Ok(())
        }
        "render" | "design" => {
            let leaving = app.ui.manufacture;
            app.ui.manufacture = false;
            if workspace == "render" {
                if let Err(e) = crate::render_workspace::enter(app, true) {
                    app.ui.manufacture = leaving;
                    return Err(e);
                }
            } else {
                crate::render_workspace::enter(app, false)?;
            }
            if leaving {
                app.machining = None;
                if app.ui.tab == "MANUFACTURE" {
                    app.ui.tab = "MODEL".into();
                }
                crate::workspace::sync_context(app);
            }
            Ok(())
        }
        other => Err(format!("there is no workspace `{other}`")),
    }
}

/// The Manufacture tab's buttons (`ui.start` of a machining command): open the window ready for
/// what the button names. `false` for any other command.
pub fn start(app: &mut SolveApp, id: &str) -> bool {
    let kind = match id {
        "cam.face" => Some(0),
        "cam.pocket" => Some(1),
        "cam.contour" => Some(2),
        "cam.drill" => Some(3),
        "cam.generate" | "cam.setup" | "cam.tool" | "cam.toolpaths" | "cam.export" | "cam.operation" | "cam.simulation" => None,
        _ => return false,
    };
    let was_open = app.machining.is_some();
    open(app);
    let Some(mut d) = app.machining.take() else { return true };
    if let Some(k) = kind {
        d.kind = k;
    }
    match id {
        // Show Toolpaths turns them on, or off when the window already shows them.
        "cam.toolpaths" => d.show_paths = !(was_open && d.show_paths),
        "cam.simulation" => {
            if d.setup.is_none() {
                let generated = d.generated(app);
                d.setup = generated["setups"].as_array().and_then(|s| s.first()).and_then(|s| s["id"].as_u64());
            }
            run(app, &mut d, Action::Simulate, &[], &[]);
        }
        "cam.export" => {
            if d.setup.is_none() {
                let generated = d.generated(app);
                d.setup = generated["setups"].as_array().and_then(|s| s.first()).and_then(|s| s["id"].as_u64());
            }
            run(app, &mut d, Action::Export, &[], &[]);
        }
        _ => {}
    }
    app.machining = Some(d);
    true
}

/// Open the window.
pub fn open(app: &mut SolveApp) {
    if app.machining.is_none() {
        app.machining = Some(CamPanel::default());
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::String(t) => t.clone(),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

/// The body a new setup machines: the selection's, else the engine's default (the only body).
fn selected_body(selection: &[Sel]) -> Option<String> {
    selection.iter().find_map(|s| match s {
        Sel::Body { name } => Some(name.clone()),
        Sel::Face { body, .. } | Sel::Edge { body, .. } if !body.is_empty() => Some(body.clone()),
        _ => None,
    })
}

/// A labelled text field, `width` wide.
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, width: f32) {
    crate::ui_kit::row(ui, label, |ui| {
        ui.allocate_ui(egui::vec2(width, crate::ui_kit::row_height()), |ui| {
            crate::ui_kit::cell_editor(ui, ui.make_persistent_id(("sc_cam_field", label)), value, tr_label(label), false);
        });
    });
}

fn heading(ui: &mut egui::Ui, label: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(tr_label(label)).strong());
}

/// Draw the window when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = app.machining.take() else { return };
    let t = Tokens::get();
    let generated = d.generated(app);
    let tools: Vec<Value> = generated["tools"].as_array().cloned().unwrap_or_default();
    let setups: Vec<Value> = generated["setups"].as_array().cloned().unwrap_or_default();
    // The picked setup, or the first while the picked one is gone.
    if !setups.iter().any(|s| s["id"].as_u64() == d.setup) {
        d.setup = setups.first().and_then(|s| s["id"].as_u64());
    }
    // A simulation of an older design, or of another setup, ends.
    if d.sim.as_ref().is_some_and(|s| s.revision != app.session.revision || Some(s.setup) != d.setup) {
        d.sim = None;
    }
    // The operation being changed is gone (undone, deleted elsewhere) or in another setup.
    if let Some(id) = d.editing {
        let here = setups.iter().filter(|s| s["id"].as_u64() == d.setup).flat_map(|s| s["operations"].as_array().into_iter().flatten());
        if !here.clone().any(|o| o["id"].as_u64() == Some(id)) {
            d.stop_editing();
        }
    }
    let selection = app.session.selection.clone();
    let faces = selection.iter().filter(|s| matches!(s, Sel::Face { .. })).count();
    let edges = selection.iter().filter(|s| matches!(s, Sel::Edge { .. })).count();
    let mut action = None;
    let closed = crate::ui_kit::Window::new(egui::Id::unique("sc_cam"), "update", tr_label("Machining")).cards(3.0).show_body(ctx, |ui| {
        {
            // What is used most comes first: export, the operations and their form; the setups
            // and tools fold away once they have something in them.
            ui.horizontal(|ui| {
                if crate::ui_kit::button(ui, "Export G-code…", crate::ui_kit::Button::Normal).clicked() {
                    action = Some(Action::Export);
                }
                crate::ui_kit::checkbox(ui, tr_label("Show toolpaths"), &mut d.show_paths);
            });
            // Simulation: play the toolpaths against the stock.
            ui.horizontal_wrapped(|ui| {
                if crate::ui_kit::button(ui, "Simulate", crate::ui_kit::Button::Normal).clicked() {
                    action = Some(Action::Simulate);
                }
                if let Some(sim) = d.sim.as_mut() {
                    let label = if sim.playing && !sim.done() { "Pause" } else { "Play" };
                    if crate::ui_kit::button(ui, label, crate::ui_kit::Button::Small).clicked() {
                        action = Some(Action::PlayPause);
                    }
                    if crate::ui_kit::button(ui, "To End", crate::ui_kit::Button::Small).clicked() {
                        action = Some(Action::ToEnd);
                    }
                    if crate::ui_kit::button(ui, "Stop Simulation", crate::ui_kit::Button::Small).clicked() {
                        action = Some(Action::StopSimulation);
                    }
                    crate::ui_kit::dropdown(ui, "cam_sim_speed", &mut sim.speed, &crate::cam_sim::SPEED_LABELS);
                    let (name, done) = sim.progress();
                    ui.label(RichText::new(format!("{} · {:.0}%", op_name(&name), done * 100.0)).small().color(t.text_dim));
                }
            });
            // Operations of the picked setup.
            heading(ui, "Operations");
            let picked = setups.iter().find(|s| s["id"].as_u64() == d.setup);
            let mut total = 0.0;
            if let Some(s) = picked {
                egui::Grid::new("sc_cam_ops").num_columns(6).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
                    for op in s["operations"].as_array().into_iter().flatten() {
                        let id = op["id"].as_u64().unwrap_or(0);
                        let minutes = op["minutes"].as_f64().unwrap_or(0.0);
                        total += minutes;
                        // A click loads it into the form below, to change it.
                        let name = op_name(&text(&op["name"]));
                        if crate::ui_kit::list_picker(ui, &format!("cam_op_{id}"), &name, d.editing == Some(id)).clicked() {
                            action = Some(if d.editing == Some(id) { Action::StopEditing } else { Action::Edit(op.clone()) });
                        }
                        ui.label(format!("T{}", text(&op["tool"])));
                        match op["error"].as_str() {
                            Some(e) => ui.label(RichText::new(cam_text(e)).color(t.error)),
                            None => ui.label(RichText::new(format!("{minutes:.1} {}", tr_label("min"))).color(t.text_dim)),
                        };
                        if crate::ui_kit::button(ui, format!("↑ #{id}"), crate::ui_kit::Button::Small).clicked() {
                            action = Some(Action::Move(id, -1));
                        }
                        if crate::ui_kit::button(ui, format!("↓ #{id}"), crate::ui_kit::Button::Small).clicked() {
                            action = Some(Action::Move(id, 1));
                        }
                        if crate::ui_kit::button(ui, format!("{} #{id}", tr_label("Delete")), crate::ui_kit::Button::Small).clicked() {
                            action = Some(Action::Delete(id));
                        }
                        ui.end_row();
                    }
                });
                if total > 0.0 {
                    ui.label(RichText::new(format!("≈ {total:.1} {}", tr_label("min"))).small().color(t.text_dim));
                }
            }
            if picked.is_some() && !tools.is_empty() {
                let tool_labels: Vec<String> =
                    tools.iter().map(|tool| format!("T{} {}", text(&tool["number"]), tool_name(&text(&tool["name"])))).collect();
                let tool_refs: Vec<&str> = tool_labels.iter().map(String::as_str).collect();
                if d.tool >= tool_refs.len() {
                    d.tool = 0;
                }
                if let Some(id) = d.editing {
                    let name = s_ops_name(picked, id);
                    ui.label(RichText::new(solidcraft_ui_text::format("cam.panel.editing", &[("name", &name)])).strong());
                }
                ui.horizontal_wrapped(|ui| {
                    crate::ui_kit::row(ui, "Type", |ui| crate::ui_kit::dropdown(ui, "cam_op_kind", &mut d.kind, &KIND_LABELS));
                    crate::ui_kit::row(ui, "Tool", |ui| crate::ui_kit::dropdown(ui, "cam_op_tool", &mut d.tool, &tool_refs));
                    field(ui, "Depth", &mut d.depth, 56.0);
                });
                let kind = KINDS.get(d.kind).copied().unwrap_or("contour");
                if kind == "contour" {
                    ui.horizontal_wrapped(|ui| {
                        crate::ui_kit::row(ui, "Side", |ui| crate::ui_kit::dropdown(ui, "cam_op_side", &mut d.side, &SIDE_LABELS));
                        field(ui, "Tabs", &mut d.tabs, 40.0);
                        field(ui, "Tab width", &mut d.tab_width, 48.0);
                        field(ui, "Tab height", &mut d.tab_height, 48.0);
                    });
                }
                if kind == "drill" {
                    ui.horizontal_wrapped(|ui| field(ui, "Peck", &mut d.peck, 48.0));
                }
                // Cutting data: empty fields take the material's starting values.
                crate::ui_kit::section(ui, "cam_cutting", "Cutting data", true, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        field(ui, "Spindle (rpm)", &mut d.rpm, 64.0);
                        field(ui, "Feed (mm/min)", &mut d.feed, 56.0);
                        field(ui, "Plunge (mm/min)", &mut d.plunge, 56.0);
                    });
                    ui.horizontal_wrapped(|ui| {
                        field(ui, "Depth per pass", &mut d.stepdown, 48.0);
                        if kind != "drill" {
                            field(ui, "Stepover (%)", &mut d.stepover, 48.0);
                        }
                    });
                    ui.label(RichText::new(tr("cam.panel.cutting_hint")).small().color(t.text_dim));
                });
                let hint = match kind {
                    "pocket" => tr("cam.panel.pick_floor"),
                    "contour" => tr("cam.panel.pick_face"),
                    "drill" => tr("cam.panel.pick_edges"),
                    _ => tr("cam.panel.face_hint"),
                };
                ui.label(RichText::new(hint).small().color(t.text_dim));
                ui.label(
                    RichText::new(format!("{}: {faces} · {}: {edges}", tr_label("Selected faces"), tr_label("Selected edges")))
                        .small()
                        .color(t.text_dim),
                );
                ui.horizontal(|ui| {
                    let label = if d.editing.is_some() { "Update Operation" } else { "Add Operation" };
                    if crate::ui_kit::button(ui, label, crate::ui_kit::Button::Primary).clicked() {
                        action = Some(Action::AddOperation);
                    }
                    if d.editing.is_some() {
                        if crate::ui_kit::button(ui, "Starting values", crate::ui_kit::Button::Normal).clicked() {
                            action = Some(Action::ResetCutting);
                        }
                        if crate::ui_kit::button(ui, "Cancel", crate::ui_kit::Button::Normal).clicked() {
                            action = Some(Action::StopEditing);
                        }
                    }
                });
            } else if picked.is_some() {
                ui.label(RichText::new(tr("cam.panel.add_a_tool")).color(t.text_dim));
            }
            if let Some(e) = &d.error {
                ui.label(RichText::new(cam_text(e)).color(t.error));
            }
            if let Some(n) = &d.notice {
                ui.label(RichText::new(n).color(t.text_dim));
            }
            ui.separator();
            crate::ui_kit::section_count(ui, "cam_setups", "Setups", setups.is_empty(), Some(setups.len()), |ui| {
                // Setups.
                if setups.is_empty() {
                    ui.label(RichText::new(tr_label("No setups yet")).color(t.text_dim));
                }
                for s in &setups {
                    let id = s["id"].as_u64().unwrap_or(0);
                    let stock: Vec<String> =
                        s["stock"].as_array().into_iter().flatten().map(|v| format!("{:.1}", v.as_f64().unwrap_or(0.0))).collect();
                    ui.horizontal(|ui| {
                        let material = MATERIALS
                            .iter()
                            .position(|m| *m == text(&s["material"]))
                            .and_then(|i| MATERIAL_LABELS.get(i))
                            .map_or(String::new(), |l| tr_label(l).to_string());
                        let label = format!("{} — {} · {} · {} mm", setup_name(&text(&s["name"])), text(&s["body"]), material, stock.join(" × "));
                        if crate::ui_kit::list_picker(ui, &format!("cam_setup_{id}"), &label, d.setup == Some(id)).clicked() {
                            d.setup = Some(id);
                        }
                        if crate::ui_kit::button(ui, tr_label("Delete"), crate::ui_kit::Button::Small).clicked() {
                            action = Some(Action::Delete(id));
                        }
                    });
                    if let Some(e) = s["error"].as_str() {
                        ui.label(RichText::new(cam_text(e)).color(t.error));
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    crate::ui_kit::row(ui, "Material", |ui| crate::ui_kit::dropdown(ui, "cam_material", &mut d.material, &MATERIAL_LABELS));
                    crate::ui_kit::row(ui, "Origin", |ui| crate::ui_kit::dropdown(ui, "cam_origin", &mut d.origin, &ORIGIN_LABELS));
                    crate::ui_kit::checkbox(ui, tr_label("Turn over"), &mut d.flipped);
                });
                ui.horizontal_wrapped(|ui| {
                    field(ui, "Side margin", &mut d.margin_side, 48.0);
                    field(ui, "Top margin", &mut d.margin_top, 48.0);
                    field(ui, "Bottom margin", &mut d.margin_bottom, 48.0);
                    if crate::ui_kit::button(ui, "New Setup", crate::ui_kit::Button::Normal).clicked() {
                        action = Some(Action::NewSetup);
                    }
                });
            });
            crate::ui_kit::section_count(ui, "cam_tools", "Tools", tools.is_empty(), Some(tools.len()), |ui| {
                // Tools.
                if tools.is_empty() {
                    ui.label(RichText::new(tr_label("No tools yet")).color(t.text_dim));
                }
                egui::Grid::new("sc_cam_tools").num_columns(3).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                    for tool in &tools {
                        let n = tool["number"].as_u64().unwrap_or(0);
                        ui.label(format!("T{n}"));
                        ui.label(format!("{} · {} {} mm", tool_name(&text(&tool["name"])), tr_label("Flute length"), text(&tool["flute_length"])));
                        if crate::ui_kit::button(ui, format!("{} T{n}", tr_label("Delete")), crate::ui_kit::Button::Small).clicked() {
                            action = Some(Action::DeleteTool(u32::try_from(n).unwrap_or(0)));
                        }
                        ui.end_row();
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    field(ui, "Number", &mut d.tool_number, 48.0);
                    field(ui, "Diameter", &mut d.tool_diameter, 64.0);
                    field(ui, "Flute length", &mut d.tool_flute, 56.0);
                    crate::ui_kit::row(ui, "Type", |ui| crate::ui_kit::dropdown(ui, "cam_tool_kind", &mut d.tool_kind, &TOOL_KIND_LABELS));
                    if crate::ui_kit::button(ui, "Add Tool", crate::ui_kit::Button::Normal).clicked() {
                        action = Some(Action::AddTool);
                    }
                });
            });
        }
    });
    if let Some(a) = action {
        run(app, &mut d, a, &tools, &selection);
    }
    if !closed {
        app.machining = Some(d);
    }
}

/// Carry out a click: one command each (one undo step), its refusal shown in the window.
fn run(app: &mut SolveApp, d: &mut CamPanel, action: Action, tools: &[Value], selection: &[Sel]) {
    d.notice = None;
    let result = match action {
        Action::AddTool => d.tool_params().and_then(|p| app.run("cam.tool", p)),
        Action::NewSetup => app.run("cam.setup", d.setup_params(selected_body(selection))).map(|v| {
            d.setup = v["id"].as_u64();
            v
        }),
        Action::Delete(id) => {
            if d.editing == Some(id) {
                d.stop_editing();
            }
            app.run("cam.delete", json!({"id": id}))
        }
        Action::Edit(op) => {
            d.load(&op, tools);
            Ok(Value::Null)
        }
        Action::StopEditing => {
            d.stop_editing();
            Ok(Value::Null)
        }
        Action::ResetCutting => match d.editing {
            Some(id) => app.run("cam.operation", json!({"id": id, "reset_cutting": true})).map(|v| {
                d.load(&v["operation"], tools);
                v
            }),
            None => Ok(Value::Null),
        },
        Action::Move(id, by) => app.run("cam.move", json!({"id": id, "by": by})),
        Action::Simulate => match d.setup {
            Some(setup) => crate::cam_sim::start(app, setup).map(|mut sim| {
                // A restart keeps the speed picked.
                if let Some(old) = &d.sim {
                    sim.speed = old.speed;
                }
                d.sim = Some(sim);
                Value::Null
            }),
            None => Err(tr("cam.panel.no_setup").to_string()),
        },
        Action::PlayPause => {
            if let Some(sim) = d.sim.as_mut() {
                if sim.done() {
                    sim.restart();
                } else {
                    sim.playing = !sim.playing;
                }
            }
            Ok(Value::Null)
        }
        Action::ToEnd => {
            if let Some(sim) = d.sim.as_mut() {
                sim.advance(f64::INFINITY);
            }
            Ok(Value::Null)
        }
        Action::StopSimulation => {
            d.sim = None;
            Ok(Value::Null)
        }
        Action::DeleteTool(n) => app.run("cam.delete", json!({"tool": n})),
        Action::AddOperation => {
            let tool = tools.get(d.tool).and_then(|t| t["number"].as_u64()).and_then(|n| u32::try_from(n).ok());
            match (d.setup, tool) {
                (Some(setup), Some(tool)) => d.operation_params(setup, tool, selection).and_then(|p| app.run("cam.operation", p)).map(|v| {
                    // A change stays loaded (with what the engine made of it); an addition clears.
                    if d.editing.is_some() {
                        d.load(&v["operation"], tools);
                    }
                    v
                }),
                _ => Err(tr("cam.panel.add_a_tool").to_string()),
            }
        }
        Action::Export => {
            let Some(setup) = d.setup else {
                d.error = Some(tr("cam.panel.no_setup").to_string());
                return;
            };
            app.pick_save("part.nc", &["nc", "gcode"], move |app, path| {
                let Some(path) = path else { return };
                let r = app.run("cam.export", json!({"path": path, "setup": setup}));
                if let Some(panel) = app.machining.as_mut() {
                    match r {
                        Ok(v) => {
                            let files: Vec<String> = v["files"].as_array().into_iter().flatten().map(|f| text(&f["path"])).collect();
                            panel.error = None;
                            panel.notice = Some(format!("{}: {}", tr_label("Programs written"), files.join(", ")));
                        }
                        Err(e) => panel.error = Some(e),
                    }
                }
            });
            Ok(Value::Null)
        }
    };
    match result {
        Ok(_) => d.error = None,
        Err(e) => d.error = Some(e),
    }
    // The lists follow the change at once, even when the revision didn't move.
    d.cache = None;
    d.paths = None;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{MESSAGES, cam_text, fill};
    use solidcraft_engine::Sel;

    use crate::scenario::Harness;

    #[test]
    fn the_window_adds_a_tool_a_setup_and_a_contour_from_the_selected_face() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 8})).unwrap();
        h.step(&json!({"start": "cam.generate"})).unwrap();
        assert!(h.app.machining.is_some(), "ui.start opens the window");
        h.frames(2);
        assert!(h.widget("Export G-code…").is_some());
        h.step(&json!({"widget": "Add Tool"})).unwrap();
        h.frames(2);
        h.step(&json!({"widget": "New Setup"})).unwrap();
        h.frames(2);
        let cam = h.app.session.execute("cam.generate", &json!({})).unwrap();
        assert_eq!(cam["tools"].as_array().map(Vec::len), Some(1), "{cam}");
        assert_eq!(cam["setups"].as_array().map(Vec::len), Some(1), "{cam}");
        // The box's top face, picked in the model.
        let state = crate::open_meshing::session_world(&h.app.session);
        let body = state.bodies.first().unwrap();
        let faces = body.body.faces(0.1).unwrap();
        let top = faces.iter().filter(|f| f.plane_normal.is_some()).max_by(|a, b| a.centroid.z.total_cmp(&b.centroid.z)).unwrap();
        h.app.session.selection = vec![Sel::Face { body: body.name.clone(), index: top.index, point: top.centroid }];
        h.app.machining.as_mut().unwrap().kind = 2;
        h.frames(2);
        h.step(&json!({"widget": "Add Operation"})).unwrap();
        h.frames(2);
        let cam = h.app.session.execute("cam.generate", &json!({})).unwrap();
        let ops = cam["setups"][0]["operations"].as_array().cloned().unwrap_or_default();
        assert_eq!(ops.len(), 1, "{cam} {:?}", h.app.machining.as_ref().unwrap().error);
        assert_eq!(ops[0]["kind"], "contour");
        // The window draws the new contour over the model, and stops when it is turned off.
        let mut d = h.app.machining.take().unwrap();
        let lines = d.lines(&mut h.app).unwrap();
        assert!(!lines.cuts.is_empty() && !lines.rapids.is_empty());
        d.show_paths = false;
        h.app.machining = Some(d);
        h.frames(2);
    }

    #[test]
    fn engine_messages_are_read_back_into_their_ids_and_translated() {
        crate::i18n::install();
        assert_eq!(fill("{d} mm drill", "3.5 mm drill"), Some(vec![("d", "3.5".to_string())]));
        assert_eq!(fill("{d} mm drill", "3.5 mm drill bit"), None);
        assert_eq!(fill("T{n} is used by {name}: x", "T2 is used by contour 1: x").map(|a| a.len()), Some(2));
        // In English every known wording comes back as it was.
        for (pattern, id) in MESSAGES {
            if !id.starts_with("cam.error.") || pattern.ends_with("(cam.tool adds one)") {
                continue;
            }
            let text = pattern.replace("{depth}", "15.50").replace("{flute}", "12.70").replace("{tool}", "T9").replace("{body}", "Body1");
            let text = text.replace("{n}", "3").replace("{name}", "Setup 9");
            let back = cam_text(&format!("invalid parameters for `cam.operation`: {text}"));
            assert_eq!(back.to_lowercase(), text.to_lowercase(), "{id}");
            // Every wording is translated.
            assert_ne!(solidcraft_ui_text::Locale::for_language("ja").text(id), *id, "{id}");
        }
        // Names and the message after them.
        let e = "invalid parameters for `cam.operation`: contour 1: 15.50 mm deep is more than 3.175 mm 2-flute end mill's flute length (12.70 mm)";
        assert_eq!(cam_text(e), "Contour 1: 15.50 mm deep is more than 3.175 mm 2-flute end mill's flute length (12.70 mm)");
        assert_eq!(cam_text("something new"), "something new", "an unknown message stays as it came");
        let ja = solidcraft_ui_text::Locale::for_language("ja");
        assert!(ja.format("cam.error.too_deep", &[("depth", "15.50"), ("tool", "T1"), ("flute", "12.70")]).contains("12.70"));
    }

    #[test]
    fn a_tool_s_flute_length_is_kept_and_a_deeper_cut_needs_it() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 14})).unwrap();
        crate::cam_panel::open(&mut h.app);
        h.app.machining.as_mut().unwrap().tool_flute = "20".into();
        h.frames(2);
        h.step(&json!({"widget": "Add Tool"})).unwrap();
        let tools = h.app.session.execute("cam.generate", &json!({})).unwrap();
        assert_eq!(tools["tools"][0]["flute_length"], json!(20.0), "{tools}");
        h.app.machining.as_mut().unwrap().tool_flute = "deep".into();
        h.frames(2);
        h.step(&json!({"widget": "Add Tool"})).unwrap();
        assert!(h.app.machining.as_ref().unwrap().error.is_some(), "a flute length that is not a number is refused");
    }

    #[test]
    fn the_stock_and_cuts_go_into_the_model_scene_while_toolpaths_are_shown() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 8})).unwrap();
        h.app.run("cam.tool", json!({"number": 1, "diameter": 3.175})).unwrap();
        h.app.run("cam.setup", json!({})).unwrap();
        h.app.run("cam.operation", json!({"type": "face", "tool": 1})).unwrap();
        crate::cam_panel::open(&mut h.app);
        assert_eq!(super::scene_key(&h.app), None, "nothing worked out yet");
        h.frames(3);
        let mut d = h.app.machining.take().unwrap();
        let lines = d.lines(&mut h.app).unwrap();
        h.app.machining = Some(d);
        assert!(lines.stock.is_some());
        assert!(super::scene_key(&h.app).is_some());
        let mut sc = crate::gpu::GpuScene::default();
        super::scene_lines(&mut sc, &h.app);
        assert!(!sc.lines.is_empty());
        h.app.machining.as_mut().unwrap().show_paths = false;
        assert_eq!(super::scene_key(&h.app), None);
        assert_eq!(super::box_edges(solidcore_record::Vec3::new(0.0, 0.0, 0.0), solidcore_record::Vec3::new(1.0, 2.0, 3.0)).len(), 12);
    }

    #[test]
    fn the_manufacture_tab_opens_the_window_ready_for_each_button() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 8})).unwrap();
        for (id, kind) in [("cam.face", 0), ("cam.pocket", 1), ("cam.contour", 2), ("cam.drill", 3)] {
            h.app.machining = None;
            h.step(&json!({"start": id})).unwrap();
            assert_eq!(h.app.machining.as_ref().map(|d| d.kind), Some(kind), "{id}");
        }
        h.step(&json!({"start": "cam.toolpaths"})).unwrap();
        assert!(!h.app.machining.as_ref().unwrap().show_paths, "pressed with the window showing them, it hides them");
        h.step(&json!({"start": "cam.toolpaths"})).unwrap();
        assert!(h.app.machining.as_ref().unwrap().show_paths);
        assert!(!super::start(&mut h.app, "solid.box"));
        assert!(!crate::workspace::visible_tabs(&h.app).contains(&"MANUFACTURE"), "Design has no Manufacture tab");
        assert_eq!(crate::workspace::tab_name("MANUFACTURE"), "Manufacture");
    }

    #[test]
    fn the_manufacture_workspace_has_its_own_ribbon_and_design_gets_its_tabs_back() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 8})).unwrap();
        h.app.ui.tab = "INSPECT".into();
        super::switch_workspace(&mut h.app, "manufacture").unwrap();
        h.frames(2);
        assert!(super::workspace_active(&h.app));
        assert_eq!(crate::workspace::visible_tabs(&h.app), ["MANUFACTURE"]);
        assert_eq!(h.app.ui.tab, "MANUFACTURE");
        assert!(h.app.machining.is_some(), "entering opens the Machining window");
        // Selecting things in the model keeps the Manufacture ribbon.
        crate::workspace::sync_context(&mut h.app);
        assert_eq!(h.app.ui.tab, "MANUFACTURE");
        // The control channel reports and switches it too.
        assert_eq!(h.call("ui.workspace", json!({}))["result"]["workspace"], "manufacture");
        super::switch_workspace(&mut h.app, "design").unwrap();
        h.frames(2);
        assert!(!super::workspace_active(&h.app));
        assert!(h.app.machining.is_none(), "leaving closes the window");
        assert!(crate::workspace::visible_tabs(&h.app).contains(&"MODEL"));
        assert_ne!(h.app.ui.tab, "MANUFACTURE");
        assert!(super::switch_workspace(&mut h.app, "drawing").is_err());
        // A sketch being edited has to be finished first.
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        assert!(super::switch_workspace(&mut h.app, "manufacture").is_err());
        assert!(!super::workspace_active(&h.app));
    }

    #[test]
    fn an_operation_is_changed_reordered_and_given_back_its_starting_cutting_data_from_the_window() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 8})).unwrap();
        h.app.run("cam.tool", json!({"number": 1, "diameter": 3.175})).unwrap();
        h.app.run("cam.setup", json!({})).unwrap();
        let face = h.app.run("cam.face", json!({"tool": 1})).unwrap()["operation"]["id"].as_u64().unwrap();
        let state = crate::open_meshing::session_world(&h.app.session);
        let body = state.bodies.first().unwrap();
        let faces = body.body.faces(0.1).unwrap();
        let top = faces.iter().filter(|f| f.plane_normal.is_some()).max_by(|a, b| a.centroid.z.total_cmp(&b.centroid.z)).unwrap();
        let at = [top.centroid.x, top.centroid.y, top.centroid.z];
        let contour = h.app.run("cam.contour", json!({"tool": 1, "face": {"at": at}})).unwrap()["operation"]["id"].as_u64().unwrap();
        h.step(&json!({"start": "cam.generate"})).unwrap();
        h.frames(2);
        // Pick the contour from the list: the form takes its values.
        h.step(&json!({"widget": "Contour 2"})).unwrap();
        h.frames(2);
        let d = h.app.machining.as_ref().unwrap();
        assert_eq!(d.editing, Some(contour));
        assert_eq!(d.kind, 2);
        assert!(!d.stepdown.is_empty(), "the cutting data it runs with is shown");
        // A shallower pass, with nothing selected: the face is kept.
        h.app.session.selection.clear();
        h.app.machining.as_mut().unwrap().stepdown = "0.5".into();
        h.frames(2);
        h.step(&json!({"widget": "Update Operation"})).unwrap();
        let cam = h.app.session.execute("cam.generate", &json!({})).unwrap();
        let ops = cam["setups"][0]["operations"].clone();
        assert_eq!(ops.as_array().map(Vec::len), Some(2), "{cam}");
        assert_eq!(ops[1]["cutting"]["stepdown"], 0.5, "{:?}", h.app.machining.as_ref().unwrap().error);
        assert_eq!(ops[1]["custom_cutting"], true);
        // Back to the starting values.
        h.frames(2);
        h.step(&json!({"widget": "Starting values"})).unwrap();
        let cam = h.app.session.execute("cam.generate", &json!({})).unwrap();
        assert!(cam["setups"][0]["operations"][1].get("custom_cutting").is_none(), "{cam}");
        // The contour first.
        h.frames(2);
        h.step(&json!({"widget": format!("↑ #{contour}")})).unwrap();
        let cam = h.app.session.execute("cam.generate", &json!({})).unwrap();
        let order: Vec<u64> = cam["setups"][0]["operations"].as_array().unwrap().iter().filter_map(|o| o["id"].as_u64()).collect();
        assert_eq!(order, [contour, face]);
        // Cancel goes back to adding.
        h.frames(2);
        h.step(&json!({"widget": "Cancel"})).unwrap();
        assert_eq!(h.app.machining.as_ref().unwrap().editing, None);
    }

    #[test]
    fn simulate_plays_the_setup_against_its_stock_and_stops_when_the_design_changes() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 8})).unwrap();
        h.app.run("cam.tool", json!({"number": 1, "diameter": 3.175})).unwrap();
        h.app.run("cam.setup", json!({})).unwrap();
        h.app.run("cam.face", json!({"tool": 1})).unwrap();
        h.step(&json!({"start": "cam.generate"})).unwrap();
        h.frames(2);
        h.step(&json!({"widget": "Simulate"})).unwrap();
        h.frames(2);
        assert!(h.app.machining.as_ref().unwrap().sim.is_some(), "{:?}", h.app.machining.as_ref().unwrap().error);
        h.step(&json!({"widget": "To End"})).unwrap();
        let sim = h.app.machining.as_ref().unwrap().sim.as_ref().unwrap();
        assert!(sim.done() && sim.stock.removed() > 0.0);
        assert_eq!(super::scene_key(&h.app), None, "the toolpath lines step aside while it plays");
        h.frames(2);
        // A change to the design ends it.
        h.app.run("cam.face", json!({"tool": 1, "depth": 0.5})).unwrap();
        h.frames(2);
        assert!(h.app.machining.as_ref().unwrap().sim.is_none());
    }

    #[test]
    fn a_pocket_without_a_selected_face_says_what_to_pick() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 12})).unwrap();
        h.app.run("cam.tool", json!({"number": 1, "diameter": 3})).unwrap();
        h.app.run("cam.setup", json!({})).unwrap();
        h.step(&json!({"start": "cam.generate"})).unwrap();
        h.app.session.selection.clear();
        h.app.machining.as_mut().unwrap().kind = 1;
        h.frames(2);
        let undo = h.app.session.undo.len();
        h.step(&json!({"widget": "Add Operation"})).unwrap();
        assert!(h.app.machining.as_ref().unwrap().error.is_some());
        assert_eq!(h.app.session.undo.len(), undo, "nothing added");
    }
}
