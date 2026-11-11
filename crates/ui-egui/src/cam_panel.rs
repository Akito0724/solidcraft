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
    /// The side dialog open, and its tab (the operation dialog's).
    pub dialog: Option<CamDialog>,
    pub tab: usize,
    /// The operation picked in the browser.
    pub selected_op: Option<u64>,
    /// The operation dialog's toolpath as it stands, worked out without changing the design.
    preview: Option<Preview>,
    /// Starting values was pressed: the operation drops its own cutting data.
    pub reset_cutting: bool,
    /// The tool dialog shows one that is in the library (it can be deleted).
    tool_known: bool,
    /// A button inside the dialog's body, done after it is drawn.
    pending: Option<Action>,
}

/// `cam.preview`'s answer for the operation dialog's parameters.
struct Preview {
    /// The parameters it answers (as JSON text) and the design revision: either changing works
    /// it out again.
    key: String,
    revision: u64,
    error: Option<String>,
    /// The cutting data the operation runs with, and its time.
    cutting: Value,
    minutes: f64,
    lines: PathLines,
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
            dialog: None,
            tab: 0,
            selected_op: None,
            preview: None,
            reset_cutting: false,
            tool_known: false,
            pending: None,
        }
    }
}

/// What a click asked for, done after the panels are drawn.
enum Action {
    Delete(u64),
    DeleteTool(u32),
    /// Open an operation in the side dialog to change it.
    Edit(Value),
    /// The side dialog for a new operation of a kind (an index into `KINDS`).
    OpenOperation(usize),
    /// The side dialog for a setup: a new one, or this one (`cam.generate`'s summary).
    OpenSetup(Option<Value>),
    /// The side dialog for a tool: a new one, or this one from the library.
    OpenTool(Option<Value>),
    CloseDialog,
    /// The side dialog's OK.
    Commit,
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

    /// Work the operation dialog's toolpath out again when its parameters changed.
    fn refresh_preview(&mut self, app: &mut SolveApp, tools: &[Value], selection: &[Sel]) {
        let tool = tools.get(self.tool).and_then(|t| t["number"].as_u64()).and_then(|n| u32::try_from(n).ok());
        let params = match (self.setup, tool) {
            (Some(setup), Some(tool)) => self.operation_params(setup, tool, selection),
            _ => Err(tr("cam.panel.add_a_tool").to_string()),
        };
        let key = match &params {
            Ok(p) => p.to_string(),
            Err(e) => format!("error: {e}"),
        };
        let revision = app.session.revision;
        if self.preview.as_ref().is_some_and(|p| p.key == key && p.revision == revision) {
            return;
        }
        let mut preview = Preview { key, revision, error: None, cutting: Value::Null, minutes: 0.0, lines: PathLines::default() };
        match params.and_then(|p| app.session.execute("cam.preview", &p).map_err(|e| e.to_string())) {
            Ok(v) => {
                preview.error = v["operation"]["error"].as_str().map(str::to_string);
                preview.cutting = v["operation"]["cutting"].clone();
                preview.minutes = v["operation"]["minutes"].as_f64().unwrap_or(0.0);
                for run in v["cuts"].as_array().into_iter().flatten() {
                    let pts: Vec<Vec3> = run.as_array().into_iter().flatten().filter_map(point).collect();
                    if pts.len() > 1 {
                        preview.lines.cuts.push(pts);
                    }
                }
                for r in v["rapids"].as_array().into_iter().flatten() {
                    if let (Some(a), Some(b)) = (r.get(0).and_then(point), r.get(1).and_then(point)) {
                        preview.lines.rapids.push([a, b]);
                    }
                }
            }
            Err(e) => preview.error = Some(e),
        }
        self.preview = Some(preview);
    }

    /// Back to adding operations, with the cutting data left to the material.
    fn stop_editing(&mut self) {
        self.editing = None;
        self.reset_cutting = false;
        self.preview = None;
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
        if self.reset_cutting {
            p["reset_cutting"] = json!(true);
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
    // The operation dialog's toolpath as it stands, bright over the rest.
    let preview = d.preview.as_ref().filter(|_| d.dialog == Some(CamDialog::Operation)).map(|p| p.lines.clone());
    app.machining = Some(d);
    let t = Tokens::get();
    if let Some(p) = &preview {
        let cut = egui::Stroke::new(2.0, t.accent);
        for run in &p.cuts {
            let pts: Vec<egui::Pos2> = run.iter().filter_map(|q| proj.to_screen(*q)).collect();
            if pts.len() == run.len() {
                painter.add(egui::Shape::line(pts, cut));
            }
        }
        let rapid = egui::Stroke::new(1.0, t.text_dim);
        for [a, b] in &p.rapids {
            if let (Some(sa), Some(sb)) = (proj.to_screen(*a), proj.to_screen(*b)) {
                painter.extend(crate::ui_kit::dashed_line(&[sa, sb], painter.clip_rect(), rapid, 4.0, 3.0));
            }
        }
    }
    let Some(lines) = lines else { return };
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

/// The Manufacture tab's buttons (`ui.start` of a machining command): open the side dialog
/// ready for what the button names, or do it. `false` for any other command.
pub fn start(app: &mut SolveApp, id: &str) -> bool {
    let known = matches!(
        id,
        "cam.face"
            | "cam.pocket"
            | "cam.contour"
            | "cam.drill"
            | "cam.generate"
            | "cam.setup"
            | "cam.tool"
            | "cam.toolpaths"
            | "cam.export"
            | "cam.operation"
            | "cam.simulation"
    );
    if !known {
        return false;
    }
    open(app);
    let Some(mut d) = app.machining.take() else { return true };
    let generated = d.generated(app);
    let tools: Vec<Value> = generated["tools"].as_array().cloned().unwrap_or_default();
    if d.setup.is_none() {
        d.setup = generated["setups"].as_array().and_then(|s| s.first()).and_then(|s| s["id"].as_u64());
    }
    let action = match id {
        "cam.face" => Some(Action::OpenOperation(0)),
        "cam.pocket" => Some(Action::OpenOperation(1)),
        "cam.contour" | "cam.operation" => Some(Action::OpenOperation(2)),
        "cam.drill" => Some(Action::OpenOperation(3)),
        "cam.setup" => Some(Action::OpenSetup(None)),
        "cam.tool" => Some(Action::OpenTool(None)),
        "cam.simulation" => Some(Action::Simulate),
        "cam.export" => Some(Action::Export),
        "cam.toolpaths" => {
            d.show_paths = !d.show_paths;
            None
        }
        _ => None,
    };
    if let Some(a) = action {
        run(app, &mut d, a, &tools, &[]);
        if let Some(e) = d.error.take()
            && d.dialog.is_none()
        {
            app.set_status(cam_text(&e), true);
        }
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

/// The setup, operation or tool the side dialog makes or changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CamDialog {
    /// An operation of the form's `kind` (`editing` is the one changed).
    Operation,
    /// A setup: a new one, or this one changed.
    Setup(Option<u64>),
    /// A tool for the library (the same number replaces it).
    Tool,
}

/// The operation dialog's tabs.
const TABS: [&str; 4] = ["Tool", "Geometry", "Heights", "Passes"];

/// The icon of an operation's row and dialog.
fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "face" => "offset_face",
        "pocket" => "cut",
        "drill" => "hole",
        _ => "trace",
    }
}

/// A labelled field; `hint` shows while it is empty (the value used then).
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    crate::ui_kit::row(ui, label, |ui| {
        crate::ui_kit::hint_field(ui, value, hint);
    });
}

/// A value of the preview's summary of the operation, as a field's hint.
fn hint(v: &Value, scale: f64) -> String {
    v.as_f64().map(|x| format!("{}", (x * scale * 1000.0).round() / 1000.0)).unwrap_or_default()
}

/// The Manufacture workspace's part of the browser: the setups with their operations, and the
/// tool library. A click picks a row (the setup new operations go to, or an operation for the
/// buttons under the tree); a double click opens it in the side dialog.
pub fn browser_tree(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut d) = app.machining.take() else {
        open(app);
        return;
    };
    let t = Tokens::get();
    let generated = d.generated(app);
    let tools: Vec<Value> = generated["tools"].as_array().cloned().unwrap_or_default();
    let setups: Vec<Value> = generated["setups"].as_array().cloned().unwrap_or_default();
    if !setups.iter().any(|s| s["id"].as_u64() == d.setup) {
        d.setup = setups.first().and_then(|s| s["id"].as_u64());
    }
    let mut action = None;
    if setups.is_empty() {
        crate::ui_kit::text(ui, tr("cam.panel.start_hint"), crate::ui_kit::Text::Hint("tool"));
    }
    for s in &setups {
        let id = s["id"].as_u64().unwrap_or(0);
        let material = MATERIALS
            .iter()
            .position(|m| *m == text(&s["material"]))
            .and_then(|i| MATERIAL_LABELS.get(i))
            .map_or(String::new(), |l| tr_label(l).to_string());
        let stock: Vec<String> = s["stock"].as_array().into_iter().flatten().map(|v| format!("{:.0}", v.as_f64().unwrap_or(0.0))).collect();
        let secondary = format!("{material} · {}", stock.join("×"));
        let name = setup_name(&text(&s["name"]));
        let error = s["error"].as_str().map(cam_text);
        let row = crate::ui_kit::Row {
            depth: 0,
            fold: Some(true),
            icon: "box",
            label: &name,
            name: true,
            selected: d.setup == Some(id) && d.selected_op.is_none(),
            secondary: &secondary,
            status: error.as_ref().map(|_| crate::ui_kit::Status::Error),
            tip: error.as_deref(),
            ..Default::default()
        };
        let r = crate::ui_kit::tree_row(ui, ui.make_persistent_id(("cam_setup", id)), &row);
        if r.double {
            action = Some(Action::OpenSetup(Some(s.clone())));
        } else if r.clicked {
            d.setup = Some(id);
            d.selected_op = None;
        }
        for op in s["operations"].as_array().into_iter().flatten() {
            let op_id = op["id"].as_u64().unwrap_or(0);
            let name = op_name(&text(&op["name"]));
            let error = op["error"].as_str().map(cam_text);
            let secondary = format!("T{} · {:.1} {}", text(&op["tool"]), op["minutes"].as_f64().unwrap_or(0.0), tr_label("min"));
            let row = crate::ui_kit::Row {
                depth: 1,
                icon: kind_icon(op["kind"].as_str().unwrap_or("")),
                label: &name,
                name: true,
                selected: d.selected_op == Some(op_id),
                secondary: if error.is_some() { "" } else { &secondary },
                status: error.as_ref().map(|_| crate::ui_kit::Status::Error),
                tip: error.as_deref(),
                ..Default::default()
            };
            let r = crate::ui_kit::tree_row(ui, ui.make_persistent_id(("cam_op", op_id)), &row);
            if r.double {
                d.setup = Some(id);
                d.selected_op = Some(op_id);
                action = Some(Action::Edit(op.clone()));
            } else if r.clicked {
                d.setup = Some(id);
                d.selected_op = if d.selected_op == Some(op_id) { None } else { Some(op_id) };
            }
        }
    }
    // The picked operation's buttons.
    if let Some(op_id) = d.selected_op {
        let op = setups.iter().flat_map(|s| s["operations"].as_array().into_iter().flatten()).find(|o| o["id"].as_u64() == Some(op_id)).cloned();
        match op {
            None => d.selected_op = None,
            Some(op) => {
                ui.horizontal_wrapped(|ui| {
                    if crate::ui_kit::button(ui, tr_label("Edit"), crate::ui_kit::Button::Small).clicked() {
                        action = Some(Action::Edit(op.clone()));
                    }
                    if crate::ui_kit::button(ui, tr_label("Move Up"), crate::ui_kit::Button::Small).clicked() {
                        action = Some(Action::Move(op_id, -1));
                    }
                    if crate::ui_kit::button(ui, tr_label("Move Down"), crate::ui_kit::Button::Small).clicked() {
                        action = Some(Action::Move(op_id, 1));
                    }
                    if crate::ui_kit::button(ui, tr_label("Delete"), crate::ui_kit::Button::Small).clicked() {
                        action = Some(Action::Delete(op_id));
                    }
                });
            }
        }
    }
    // The tool library.
    ui.add_space(t.space_4);
    let count = tools.len().to_string();
    let row = crate::ui_kit::Row { depth: 0, fold: Some(true), icon: "library", label: tr_label("Tools"), count: &count, ..Default::default() };
    let r = crate::ui_kit::tree_row(ui, ui.make_persistent_id("cam_tools"), &row);
    if r.double {
        action = Some(Action::OpenTool(None));
    }
    for tool in &tools {
        let n = tool["number"].as_u64().unwrap_or(0);
        let label = format!("T{n} {}", tool_name(&text(&tool["name"])));
        let secondary = format!("{} {} mm", tr_label("Flute length"), text(&tool["flute_length"]));
        let row = crate::ui_kit::Row { depth: 1, icon: "hole", label: &label, name: true, secondary: &secondary, ..Default::default() };
        let r = crate::ui_kit::tree_row(ui, ui.make_persistent_id(("cam_tool", n)), &row);
        if r.double || r.clicked {
            action = Some(Action::OpenTool(Some(tool.clone())));
        }
    }
    if let Some(e) = &d.error
        && d.dialog.is_none()
    {
        ui.label(RichText::new(cam_text(e)).small().color(t.error));
    }
    if let Some(a) = action {
        run(app, &mut d, a, &tools, &[]);
    }
    app.machining = Some(d);
}

/// The side dialog (an operation, a setup or a tool) and the simulation's player. Called once
/// per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = app.machining.take() else { return };
    let generated = d.generated(app);
    let tools: Vec<Value> = generated["tools"].as_array().cloned().unwrap_or_default();
    let setups: Vec<Value> = generated["setups"].as_array().cloned().unwrap_or_default();
    if !setups.iter().any(|s| s["id"].as_u64() == d.setup) {
        d.setup = setups.first().and_then(|s| s["id"].as_u64());
    }
    // A simulation of an older design, or of another setup, ends.
    if d.sim.as_ref().is_some_and(|s| s.revision != app.session.revision || Some(s.setup) != d.setup) {
        d.sim = None;
    }
    // The operation being changed is gone (undone, deleted elsewhere).
    if let Some(id) = d.editing
        && !setups.iter().flat_map(|s| s["operations"].as_array().into_iter().flatten()).any(|o| o["id"].as_u64() == Some(id))
    {
        d.stop_editing();
        if d.dialog == Some(CamDialog::Operation) {
            d.dialog = None;
        }
    }
    let selection = app.session.selection.clone();
    if d.dialog == Some(CamDialog::Operation) {
        d.refresh_preview(app, &tools, &selection);
    }
    let mut action = None;
    if let Some(dialog) = d.dialog {
        action = dialog_card(app, ctx, &mut d, dialog, &tools, &selection);
    }
    if action.is_none() {
        action = d.pending.take();
    }
    if d.sim.is_some() {
        player(ctx, &mut d, &mut action);
    }
    if let Some(a) = action {
        run(app, &mut d, a, &tools, &selection);
    }
    app.machining = Some(d);
}

/// The docked side dialog; returns what its buttons asked for.
fn dialog_card(app: &mut SolveApp, ctx: &egui::Context, d: &mut CamPanel, dialog: CamDialog, tools: &[Value], selection: &[Sel]) -> Option<Action> {
    let t = Tokens::get();
    let viewport = crate::agent_chat::tool_space(app, crate::shell::content_rect(app, app.viewport.rect.unwrap_or_else(|| ctx.content_rect())));
    if !viewport.is_finite() || !viewport.is_positive() {
        return None;
    }
    let side = crate::frame::Side::Right;
    let top = crate::shell::panel_top(app, viewport, viewport.bottom(), side);
    let docked = egui::pos2(side.anchor_x(viewport), top);
    let moved = crate::frame::dock_offset(ctx, "sc_cam_dialog");
    let drag_bounds = crate::shell::tool_drag_rect(app, viewport);
    let anchor = crate::frame::offset_anchor(docked, moved, drag_bounds, side);
    let frame = crate::ui_kit::Panel::feature_frame(ctx, app.ui.layout, side);
    let kind = KINDS.get(d.kind).copied().unwrap_or("contour");
    let (icon, title) = match dialog {
        CamDialog::Operation => {
            let label = tr_label(KIND_LABELS.get(d.kind).copied().unwrap_or("Contour")).to_string();
            let title = match d.editing {
                Some(_) => solidcraft_ui_text::format("dialog.edit_title", &[("title", &label)]),
                None => label,
            };
            (kind_icon(kind), title)
        }
        CamDialog::Setup(Some(_)) => ("box", solidcraft_ui_text::format("dialog.edit_title", &[("title", tr_label("Setup"))])),
        CamDialog::Setup(None) => ("box", tr_label("New Setup").to_string()),
        CamDialog::Tool => ("library", tr_label("Tool").to_string()),
    };
    let panel = crate::ui_kit::Panel::new(&title, egui::Id::unique("sc_cam_dialog"), anchor, drag_bounds, side, frame);
    let mut action = None;
    let valid = match dialog {
        CamDialog::Operation => d.preview.as_ref().is_some_and(|p| p.error.is_none()),
        _ => true,
    };
    let response = panel.window(ctx).show(ctx, |ui| {
        let (closed, done) = panel.feature(
            ui,
            crate::ui_kit::FeatureOptions { icon, title: &title, drag_key: "sc_cam_dialog", moved, docked, apply_label: None },
            false,
            valid,
            |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                match dialog {
                    CamDialog::Operation => operation_body(ui, d, kind, tools, selection),
                    CamDialog::Setup(_) => setup_body(ui, d),
                    CamDialog::Tool => tool_body(ui, d),
                }
                let error = match dialog {
                    CamDialog::Operation => d.preview.as_ref().and_then(|p| p.error.clone()).or_else(|| d.error.clone()),
                    _ => d.error.clone(),
                };
                if let Some(e) = error {
                    ui.label(RichText::new(cam_text(&e)).color(t.error));
                }
                crate::ui_kit::ComputeStatus { text: None, busy: false, applying: false }
            },
        );
        if closed || done == crate::ui_kit::Action::Cancel {
            action = Some(Action::CloseDialog);
        } else if done == crate::ui_kit::Action::Ok {
            action = Some(Action::Commit);
        }
    });
    if let Some(r) = response {
        crate::scenario::publish_rect("cam:dialog", r.response.rect);
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = Some(Action::CloseDialog);
        app.esc_handled = true;
    }
    action
}

/// The operation dialog's tabs: the tool and its cutting data, the faces or edges it machines,
/// how deep it goes, and its passes.
fn operation_body(ui: &mut egui::Ui, d: &mut CamPanel, kind: &str, tools: &[Value], selection: &[Sel]) {
    let t = Tokens::get();
    crate::ui_kit::segmented(ui, &mut d.tab, &TABS, crate::ui_kit::Segments::default());
    ui.add_space(t.space_4);
    let used = d.preview.as_ref().map(|p| p.cutting.clone()).unwrap_or(Value::Null);
    match d.tab {
        0 => {
            if tools.is_empty() {
                ui.label(RichText::new(tr("cam.panel.add_a_tool")).color(t.text_dim));
            } else {
                let labels: Vec<String> =
                    tools.iter().map(|tool| format!("T{} {}", text(&tool["number"]), tool_name(&text(&tool["name"])))).collect();
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                if d.tool >= refs.len() {
                    d.tool = 0;
                }
                crate::ui_kit::row(ui, "Tool", |ui| crate::ui_kit::dropdown(ui, "cam_op_tool", &mut d.tool, &refs));
            }
            field(ui, "Spindle (rpm)", &mut d.rpm, &hint(&used["spindle_rpm"], 1.0));
            field(ui, "Feed (mm/min)", &mut d.feed, &hint(&used["feed"], 1.0));
            field(ui, "Plunge (mm/min)", &mut d.plunge, &hint(&used["plunge_feed"], 1.0));
        }
        1 => {
            let faces = selection.iter().filter(|s| matches!(s, Sel::Face { .. })).count();
            let edges = selection.iter().filter(|s| matches!(s, Sel::Edge { .. })).count();
            let (label, picked, what) = match kind {
                "pocket" => ("Floor", faces > 0, "cam.panel.pick_floor"),
                "contour" => ("Face", faces > 0, "cam.panel.pick_face"),
                "drill" => ("Holes", edges > 0, "cam.panel.pick_edges"),
                _ => ("", true, "cam.panel.face_hint"),
            };
            if !label.is_empty() {
                let shown = if picked {
                    let n = if kind == "drill" { edges } else { 1 };
                    solidcraft_ui_text::format("cam.panel.picked", &[("n", &n.to_string())])
                } else if d.editing.is_some() {
                    tr("cam.panel.kept").to_string()
                } else {
                    tr("dialog.select").to_string()
                };
                crate::ui_kit::row(ui, label, |ui| {
                    crate::ui_kit::selection_field(ui, &shown, crate::ui_kit::Selection { active: !picked, filled: picked || d.editing.is_some() });
                });
            }
            if kind == "contour" {
                crate::ui_kit::row(ui, "Side", |ui| crate::ui_kit::dropdown(ui, "cam_op_side", &mut d.side, &SIDE_LABELS));
            }
            crate::ui_kit::text(ui, tr(what), crate::ui_kit::Text::Hint("tool"));
        }
        2 => {
            let through = if kind == "face" { tr_label("Part top") } else { tr_label("Through") };
            field(ui, "Depth", &mut d.depth, through);
            if kind == "contour" {
                field(ui, "Tabs", &mut d.tabs, "0");
                field(ui, "Tab width", &mut d.tab_width, "6");
                field(ui, "Tab height", &mut d.tab_height, "2");
            }
            if kind == "drill" {
                field(ui, "Peck", &mut d.peck, "0");
            }
        }
        _ => {
            field(ui, "Depth per pass", &mut d.stepdown, &hint(&used["stepdown"], 1.0));
            if kind != "drill" {
                field(ui, "Stepover (%)", &mut d.stepover, &hint(&used["stepover"], 100.0));
            }
            crate::ui_kit::text(ui, tr("cam.panel.cutting_hint"), crate::ui_kit::Text::Hint("tool"));
            if crate::ui_kit::button(ui, tr_label("Starting values"), crate::ui_kit::Button::Normal).clicked() {
                for f in [&mut d.rpm, &mut d.feed, &mut d.plunge, &mut d.stepdown, &mut d.stepover] {
                    f.clear();
                }
                d.reset_cutting = true;
            }
        }
    }
    if let Some(p) = &d.preview
        && p.error.is_none()
    {
        ui.add_space(t.space_4);
        ui.label(RichText::new(format!("≈ {:.1} {}", p.minutes, tr_label("min"))).small().color(t.text_dim));
    }
}

fn setup_body(ui: &mut egui::Ui, d: &mut CamPanel) {
    crate::ui_kit::row(ui, "Material", |ui| crate::ui_kit::dropdown(ui, "cam_material", &mut d.material, &MATERIAL_LABELS));
    crate::ui_kit::row(ui, "Origin", |ui| crate::ui_kit::dropdown(ui, "cam_origin", &mut d.origin, &ORIGIN_LABELS));
    crate::ui_kit::checkbox(ui, tr_label("Turn over"), &mut d.flipped);
    field(ui, "Side margin", &mut d.margin_side, "3");
    field(ui, "Top margin", &mut d.margin_top, "1");
    field(ui, "Bottom margin", &mut d.margin_bottom, "0");
    if let CamDialog::Setup(Some(id)) = d.dialog.unwrap_or(CamDialog::Tool)
        && crate::ui_kit::button(ui, tr_label("Delete"), crate::ui_kit::Button::Normal).clicked()
    {
        d.pending = Some(Action::Delete(id));
    }
}

fn tool_body(ui: &mut egui::Ui, d: &mut CamPanel) {
    field(ui, "Number", &mut d.tool_number, "1");
    crate::ui_kit::row(ui, "Type", |ui| crate::ui_kit::dropdown(ui, "cam_tool_kind", &mut d.tool_kind, &TOOL_KIND_LABELS));
    field(ui, "Diameter", &mut d.tool_diameter, "3.175");
    field(ui, "Flute length", &mut d.tool_flute, tr_label("4 × diameter"));
    if let Ok(n) = d.tool_number.trim().parse::<u32>()
        && d.tool_known
        && crate::ui_kit::button(ui, tr_label("Delete"), crate::ui_kit::Button::Normal).clicked()
    {
        d.pending = Some(Action::DeleteTool(n));
    }
}

/// The simulation's player: a small bar at the bottom of the view.
fn player(ctx: &egui::Context, d: &mut CamPanel, action: &mut Option<Action>) {
    let Some(sim) = d.sim.as_mut() else { return };
    let t = Tokens::get();
    let closed = crate::ui_kit::Window::new(egui::Id::unique("sc_cam_player"), "play", tr_label("Simulate")).show_body(ctx, |ui| {
        ui.horizontal(|ui| {
            let label = if sim.playing && !sim.done() { tr_label("Pause") } else { tr_label("Play") };
            if crate::ui_kit::button(ui, label, crate::ui_kit::Button::Small).clicked() {
                *action = Some(Action::PlayPause);
            }
            if crate::ui_kit::button(ui, tr_label("To End"), crate::ui_kit::Button::Small).clicked() {
                *action = Some(Action::ToEnd);
            }
            crate::ui_kit::dropdown(ui, "cam_sim_speed", &mut sim.speed, &crate::cam_sim::SPEED_LABELS);
        });
        let (name, done) = sim.progress();
        ui.label(RichText::new(format!("{} · {:.0}%", op_name(&name), done * 100.0)).small().color(t.text_dim));
    });
    if closed {
        *action = Some(Action::StopSimulation);
    }
}

/// Carry out a click: at most one command (one undo step), its refusal shown where it was made.
fn run(app: &mut SolveApp, d: &mut CamPanel, action: Action, tools: &[Value], selection: &[Sel]) {
    let num = |v: &Value| v.as_f64().map(|x| format!("{}", (x * 1000.0).round() / 1000.0)).unwrap_or_default();
    let result = match action {
        Action::Delete(id) => {
            if d.editing == Some(id) {
                d.stop_editing();
                d.dialog = None;
            }
            if d.dialog == Some(CamDialog::Setup(Some(id))) {
                d.dialog = None;
            }
            if d.selected_op == Some(id) {
                d.selected_op = None;
            }
            app.run("cam.delete", json!({"id": id}))
        }
        Action::DeleteTool(n) => app.run("cam.delete", json!({"tool": n})).map(|v| {
            d.dialog = None;
            v
        }),
        Action::Edit(op) => {
            d.load(&op, tools);
            d.reset_cutting = false;
            d.preview = None;
            d.tab = 0;
            d.dialog = Some(CamDialog::Operation);
            Ok(Value::Null)
        }
        Action::OpenOperation(kind) => {
            d.stop_editing();
            d.kind = kind;
            // Picking what it machines comes first, as the dialog opens.
            d.tab = if KINDS.get(kind).is_some_and(|k| *k != "face") { 1 } else { 0 };
            d.dialog = Some(CamDialog::Operation);
            Ok(Value::Null)
        }
        Action::OpenSetup(setup) => {
            match &setup {
                Some(s) => {
                    d.material = MATERIALS.iter().position(|m| *m == text(&s["material"])).unwrap_or(0);
                    d.origin = ORIGINS.iter().position(|o| *o == text(&s["origin"])).unwrap_or(0);
                    d.flipped = s["flipped"].as_bool().unwrap_or(false);
                    d.margin_side = num(&s["margins"][0]);
                    d.margin_top = num(&s["margins"][1]);
                    d.margin_bottom = num(&s["margins"][2]);
                }
                None => {
                    let fresh = CamPanel::default();
                    d.material = fresh.material;
                    d.origin = fresh.origin;
                    d.flipped = fresh.flipped;
                    d.margin_side = fresh.margin_side;
                    d.margin_top = fresh.margin_top;
                    d.margin_bottom = fresh.margin_bottom;
                }
            }
            d.dialog = Some(CamDialog::Setup(setup.and_then(|s| s["id"].as_u64())));
            Ok(Value::Null)
        }
        Action::OpenTool(tool) => {
            match &tool {
                Some(t) => {
                    d.tool_number = text(&t["number"]);
                    d.tool_diameter = num(&t["diameter"]);
                    d.tool_flute = num(&t["flute_length"]);
                    d.tool_kind = TOOL_KINDS.iter().position(|k| *k == text(&t["kind"])).unwrap_or(0);
                }
                None => {
                    let next = tools.iter().filter_map(|t| t["number"].as_u64()).max().unwrap_or(0) + 1;
                    d.tool_number = next.to_string();
                    d.tool_diameter = "3.175".into();
                    d.tool_flute.clear();
                    d.tool_kind = 0;
                }
            }
            d.tool_known = tool.is_some();
            d.dialog = Some(CamDialog::Tool);
            Ok(Value::Null)
        }
        Action::CloseDialog => {
            if d.dialog == Some(CamDialog::Operation) {
                d.stop_editing();
            }
            d.dialog = None;
            Ok(Value::Null)
        }
        Action::Commit => match d.dialog {
            Some(CamDialog::Operation) => {
                let tool = tools.get(d.tool).and_then(|t| t["number"].as_u64()).and_then(|n| u32::try_from(n).ok());
                match (d.setup, tool) {
                    (Some(setup), Some(tool)) => d.operation_params(setup, tool, selection).and_then(|p| app.run("cam.operation", p)).map(|v| {
                        d.selected_op = v["operation"]["id"].as_u64();
                        d.stop_editing();
                        d.dialog = None;
                        v
                    }),
                    _ => Err(tr("cam.panel.add_a_tool").to_string()),
                }
            }
            Some(CamDialog::Setup(id)) => {
                let mut p = d.setup_params(if id.is_none() { selected_body(selection) } else { None });
                if let Some(id) = id {
                    p["id"] = json!(id);
                }
                app.run("cam.setup", p).map(|v| {
                    d.setup = v["id"].as_u64();
                    d.dialog = None;
                    v
                })
            }
            Some(CamDialog::Tool) => d.tool_params().and_then(|p| app.run("cam.tool", p)).map(|v| {
                d.dialog = None;
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
        Action::Export => {
            let Some(setup) = d.setup else {
                app.set_status(tr("cam.panel.no_setup").to_string(), true);
                return;
            };
            app.pick_save("part.nc", &["nc", "gcode"], move |app, path| {
                let Some(path) = path else { return };
                match app.run("cam.export", json!({"path": path, "setup": setup})) {
                    Ok(v) => {
                        let files: Vec<String> = v["files"].as_array().into_iter().flatten().map(|f| text(&f["path"])).collect();
                        app.set_status(format!("{}: {}", tr_label("Programs written"), files.join(", ")), false);
                    }
                    Err(e) => app.set_status(cam_text(&e), true),
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

    use super::{Action, CamDialog, MESSAGES, cam_text, fill, run};
    use solidcraft_engine::Sel;

    use crate::scenario::Harness;

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
    fn the_manufacture_tab_opens_the_dialog_ready_for_each_button() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 8})).unwrap();
        for (id, kind) in [("cam.face", 0), ("cam.pocket", 1), ("cam.contour", 2), ("cam.drill", 3)] {
            h.app.machining = None;
            h.step(&json!({"start": id})).unwrap();
            assert_eq!(h.app.machining.as_ref().map(|d| d.kind), Some(kind), "{id}");
            assert_eq!(h.app.machining.as_ref().and_then(|d| d.dialog), Some(super::CamDialog::Operation));
        }
        h.step(&json!({"start": "cam.toolpaths"})).unwrap();
        assert!(!h.app.machining.as_ref().unwrap().show_paths, "pressed while they show, it hides them");
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

    /// A 60 × 40 × 8 mm box and its top face selected.
    fn boxed() -> Harness {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 8})).unwrap();
        h
    }

    fn select_top(h: &mut Harness) {
        let state = crate::open_meshing::session_world(&h.app.session);
        let body = state.bodies.first().unwrap();
        let faces = body.body.faces(0.1).unwrap();
        let top = faces.iter().filter(|f| f.plane_normal.is_some()).max_by(|a, b| a.centroid.z.total_cmp(&b.centroid.z)).unwrap();
        h.app.session.selection = vec![Sel::Face { body: body.name.clone(), index: top.index, point: top.centroid }];
    }

    fn ok(h: &mut Harness) {
        h.frames(3);
        h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
        h.frames(2);
    }

    fn generated(h: &mut Harness) -> serde_json::Value {
        h.app.session.execute("cam.generate", &json!({})).unwrap()
    }

    #[test]
    fn the_side_dialogs_add_a_tool_a_setup_and_a_contour_from_the_selected_face() {
        let mut h = boxed();
        h.step(&json!({"start": "cam.tool"})).unwrap();
        assert_eq!(h.app.machining.as_ref().and_then(|d| d.dialog), Some(CamDialog::Tool));
        h.app.machining.as_mut().unwrap().tool_flute = "20".into();
        ok(&mut h);
        h.step(&json!({"start": "cam.setup"})).unwrap();
        ok(&mut h);
        let cam = generated(&mut h);
        assert_eq!(cam["tools"][0]["flute_length"], json!(20.0), "{cam}");
        assert_eq!(cam["setups"].as_array().map(Vec::len), Some(1), "{cam}");
        // Contour: the dialog opens on what it machines; the top face is picked.
        h.step(&json!({"start": "cam.contour"})).unwrap();
        assert_eq!(h.app.machining.as_ref().unwrap().tab, 1);
        select_top(&mut h);
        h.frames(3);
        let d = h.app.machining.as_ref().unwrap();
        let preview = d.preview.as_ref().unwrap();
        assert!(preview.error.is_none(), "{:?}", preview.error);
        assert!(!preview.lines.cuts.is_empty(), "the toolpath shows before OK");
        assert!(generated(&mut h)["setups"][0]["operations"].as_array().unwrap().is_empty(), "nothing is added before OK");
        ok(&mut h);
        let cam = generated(&mut h);
        let ops = cam["setups"][0]["operations"].as_array().cloned().unwrap_or_default();
        assert_eq!(ops.len(), 1, "{cam} {:?}", h.app.machining.as_ref().unwrap().error);
        assert_eq!(ops[0]["kind"], "contour");
        let d = h.app.machining.as_ref().unwrap();
        assert_eq!(d.dialog, None, "OK closes it");
        assert_eq!(d.selected_op, ops[0]["id"].as_u64(), "the new operation is picked in the browser");
    }

    #[test]
    fn a_pocket_without_its_floor_says_what_to_pick_and_adds_nothing() {
        let mut h = boxed();
        h.app.run("cam.tool", json!({"number": 1, "diameter": 3})).unwrap();
        h.app.run("cam.setup", json!({})).unwrap();
        h.app.session.selection.clear();
        h.step(&json!({"start": "cam.pocket"})).unwrap();
        h.frames(3);
        assert!(h.app.machining.as_ref().unwrap().preview.as_ref().unwrap().error.is_some());
        let undo = h.app.session.undo.len();
        ok(&mut h);
        assert_eq!(h.app.session.undo.len(), undo, "OK does nothing while the toolpath can't be made");
        assert_eq!(h.app.machining.as_ref().unwrap().dialog, Some(CamDialog::Operation));
    }

    #[test]
    fn an_operation_is_changed_reordered_and_given_back_its_starting_cutting_data() {
        let mut h = boxed();
        h.app.run("cam.tool", json!({"number": 1, "diameter": 3.175})).unwrap();
        h.app.run("cam.setup", json!({})).unwrap();
        let face = h.app.run("cam.face", json!({"tool": 1})).unwrap()["operation"]["id"].as_u64().unwrap();
        select_top(&mut h);
        let at = match h.app.session.selection.first() {
            Some(Sel::Face { point, .. }) => [point.x, point.y, point.z],
            _ => unreachable!(),
        };
        let contour = h.app.run("cam.contour", json!({"tool": 1, "face": {"at": at}})).unwrap()["operation"]["id"].as_u64().unwrap();
        h.app.session.selection.clear();
        crate::cam_panel::open(&mut h.app);
        let cam = generated(&mut h);
        let tools = cam["tools"].as_array().cloned().unwrap();
        let op = cam["setups"][0]["operations"][1].clone();
        let mut d = h.app.machining.take().unwrap();
        run(&mut h.app, &mut d, Action::Edit(op), &tools, &[]);
        assert_eq!((d.dialog, d.editing, d.kind), (Some(CamDialog::Operation), Some(contour), 2));
        assert!(!d.stepdown.is_empty(), "the cutting data it runs with is shown");
        // A shallower pass, with nothing selected: the face is kept.
        d.stepdown = "0.5".into();
        h.app.machining = Some(d);
        ok(&mut h);
        let cam = generated(&mut h);
        let ops = cam["setups"][0]["operations"].clone();
        assert_eq!(ops.as_array().map(Vec::len), Some(2), "{cam}");
        assert_eq!(ops[1]["cutting"]["stepdown"], 0.5, "{:?}", h.app.machining.as_ref().unwrap().error);
        // Starting values.
        let mut d = h.app.machining.take().unwrap();
        run(&mut h.app, &mut d, Action::Edit(ops[1].clone()), &tools, &[]);
        d.tab = 3;
        h.app.machining = Some(d);
        h.frames(2);
        h.step(&json!({"widget": "Starting values"})).unwrap();
        ok(&mut h);
        assert!(generated(&mut h)["setups"][0]["operations"][1].get("custom_cutting").is_none());
        // The contour first.
        let mut d = h.app.machining.take().unwrap();
        run(&mut h.app, &mut d, Action::Move(contour, -1), &tools, &[]);
        h.app.machining = Some(d);
        let order: Vec<u64> = generated(&mut h)["setups"][0]["operations"].as_array().unwrap().iter().filter_map(|o| o["id"].as_u64()).collect();
        assert_eq!(order, [contour, face]);
    }

    #[test]
    fn simulate_plays_the_setup_against_its_stock_and_stops_when_the_design_changes() {
        let mut h = boxed();
        h.app.run("cam.tool", json!({"number": 1, "diameter": 3.175})).unwrap();
        h.app.run("cam.setup", json!({})).unwrap();
        h.app.run("cam.face", json!({"tool": 1})).unwrap();
        h.step(&json!({"start": "cam.simulation"})).unwrap();
        assert!(h.app.machining.as_ref().unwrap().sim.is_some(), "{:?}", h.app.machining.as_ref().unwrap().error);
        let mut d = h.app.machining.take().unwrap();
        run(&mut h.app, &mut d, Action::ToEnd, &[], &[]);
        h.app.machining = Some(d);
        let sim = h.app.machining.as_ref().unwrap().sim.as_ref().unwrap();
        assert!(sim.done() && sim.stock.removed() > 0.0);
        assert_eq!(super::scene_key(&h.app), None, "the toolpath lines step aside while it plays");
        h.frames(2);
        h.app.run("cam.face", json!({"tool": 1, "depth": 0.5})).unwrap();
        h.frames(2);
        assert!(h.app.machining.as_ref().unwrap().sim.is_none(), "a change to the design ends it");
    }

    #[test]
    fn the_browser_lists_setups_operations_and_tools_in_manufacture() {
        let mut h = boxed();
        h.app.run("cam.tool", json!({"number": 1, "diameter": 3.175})).unwrap();
        h.app.run("cam.setup", json!({})).unwrap();
        h.app.run("cam.face", json!({"tool": 1})).unwrap();
        super::switch_workspace(&mut h.app, "manufacture").unwrap();
        h.frames(3);
        assert!(crate::scenario::handle_at("indent:Setup 1").is_some(), "the setup's row");
        assert!(crate::scenario::handle_at("indent:Face 1").is_some(), "the operation's row");
        assert!(crate::scenario::handle_at("indent:T1 3.175 mm 2-flute end mill").is_some(), "the tool's row");
    }
}
