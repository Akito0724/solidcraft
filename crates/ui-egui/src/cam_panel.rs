//! The Machining window: the design's cutters, its setups and their operations, as
//! `cam.generate` reports them, with forms that add each (`cam.tool`, `cam.setup`,
//! `cam.operation`), a Delete per row (`cam.delete`) and Export G-code… (`cam.export`). An
//! operation's faces and edges are the model's selection when Add Operation is clicked: a pocket's
//! floor or a contour's face, a drill's round edges. The ribbon's Generate Toolpaths (`ui.start` of
//! `cam.generate`) opens it.

use egui::RichText;
use serde_json::{Value, json};
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
    /// Empty: the operation's default depth.
    pub depth: String,
    pub tabs: String,
    pub tab_width: String,
    pub tab_height: String,
    pub peck: String,
    /// The last refusal.
    pub error: Option<String>,
    /// What the last export wrote.
    pub notice: Option<String>,
}

impl Default for CamPanel {
    fn default() -> Self {
        Self {
            cache: None,
            setup: None,
            tool_number: "1".into(),
            tool_diameter: "3.175".into(),
            tool_kind: 0,
            material: 0,
            origin: 0,
            flipped: false,
            margin_side: "3".into(),
            margin_top: "1".into(),
            margin_bottom: "0".into(),
            kind: 2,
            tool: 0,
            side: 0,
            depth: String::new(),
            tabs: "4".into(),
            tab_width: "6".into(),
            tab_height: "2".into(),
            peck: "0".into(),
            error: None,
            notice: None,
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
    Export,
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

    fn tool_params(&self) -> Result<Value, String> {
        let n = self.tool_number.trim().parse::<u32>().map_err(|_| tr("cam.panel.bad_tool_number").to_string())?;
        let d = number(&self.tool_diameter).ok_or_else(|| tr("cam.panel.bad_diameter").to_string())?;
        let kind = TOOL_KINDS.get(self.tool_kind).copied().unwrap_or("flat_endmill");
        Ok(json!({"number": n, "diameter": d, "kind": kind}))
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
        if let Some(d) = number(&self.depth) {
            p["depth"] = json!(d);
        }
        let face = selection.iter().find_map(|s| match s {
            Sel::Face { body, point, .. } => Some(json!({"at": [point.x, point.y, point.z], "body": body})),
            _ => None,
        });
        match kind {
            "pocket" | "contour" => {
                let Some(face) = face else {
                    return Err(tr(if kind == "pocket" { "cam.panel.pick_floor" } else { "cam.panel.pick_face" }).to_string());
                };
                p["face"] = face;
            }
            "drill" => {
                let edges: Vec<Value> = selection
                    .iter()
                    .filter_map(|s| match s {
                        Sel::Edge { body, point, .. } => Some(json!({"at": [point.x, point.y, point.z], "body": body})),
                        _ => None,
                    })
                    .collect();
                if edges.is_empty() {
                    return Err(tr("cam.panel.pick_edges").to_string());
                }
                p["edges"] = json!(edges);
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
    let selection = app.session.selection.clone();
    let faces = selection.iter().filter(|s| matches!(s, Sel::Face { .. })).count();
    let edges = selection.iter().filter(|s| matches!(s, Sel::Edge { .. })).count();
    let mut action = None;
    let closed = crate::ui_kit::Window::new(egui::Id::unique("sc_cam"), "update", tr_label("Machining")).cards(3.0).show_body(ctx, |ui| {
        {
            // Tools.
            heading(ui, "Tools");
            if tools.is_empty() {
                ui.label(RichText::new(tr_label("No tools yet")).color(t.text_dim));
            }
            egui::Grid::new("sc_cam_tools").num_columns(3).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                for tool in &tools {
                    let n = tool["number"].as_u64().unwrap_or(0);
                    ui.label(format!("T{n}"));
                    ui.label(format!("{} ({} mm)", text(&tool["name"]), text(&tool["diameter"])));
                    if crate::ui_kit::button(ui, format!("{} T{n}", tr_label("Delete")), crate::ui_kit::Button::Small).clicked() {
                        action = Some(Action::DeleteTool(u32::try_from(n).unwrap_or(0)));
                    }
                    ui.end_row();
                }
            });
            ui.horizontal_wrapped(|ui| {
                field(ui, "Number", &mut d.tool_number, 48.0);
                field(ui, "Diameter", &mut d.tool_diameter, 64.0);
                crate::ui_kit::row(ui, "Type", |ui| crate::ui_kit::dropdown(ui, "cam_tool_kind", &mut d.tool_kind, &TOOL_KIND_LABELS));
                if crate::ui_kit::button(ui, "Add Tool", crate::ui_kit::Button::Normal).clicked() {
                    action = Some(Action::AddTool);
                }
            });
            ui.separator();

            // Setups.
            heading(ui, "Setups");
            if setups.is_empty() {
                ui.label(RichText::new(tr_label("No setups yet")).color(t.text_dim));
            }
            for s in &setups {
                let id = s["id"].as_u64().unwrap_or(0);
                let stock: Vec<String> = s["stock"].as_array().into_iter().flatten().map(|v| format!("{:.1}", v.as_f64().unwrap_or(0.0))).collect();
                ui.horizontal(|ui| {
                    let label = format!("{} — {} · {} · {} mm", text(&s["name"]), text(&s["body"]), text(&s["material"]), stock.join(" × "));
                    if crate::ui_kit::list_picker(ui, &format!("cam_setup_{id}"), &label, d.setup == Some(id)).clicked() {
                        d.setup = Some(id);
                    }
                    if crate::ui_kit::button(ui, tr_label("Delete"), crate::ui_kit::Button::Small).clicked() {
                        action = Some(Action::Delete(id));
                    }
                });
                if let Some(e) = s["error"].as_str() {
                    ui.label(RichText::new(e).color(t.error));
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
            ui.separator();

            // Operations of the picked setup.
            heading(ui, "Operations");
            let picked = setups.iter().find(|s| s["id"].as_u64() == d.setup);
            let mut total = 0.0;
            if let Some(s) = picked {
                egui::Grid::new("sc_cam_ops").num_columns(4).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                    for op in s["operations"].as_array().into_iter().flatten() {
                        let id = op["id"].as_u64().unwrap_or(0);
                        let minutes = op["minutes"].as_f64().unwrap_or(0.0);
                        total += minutes;
                        ui.label(text(&op["name"]));
                        ui.label(format!("T{}", text(&op["tool"])));
                        match op["error"].as_str() {
                            Some(e) => ui.label(RichText::new(e).color(t.error)),
                            None => ui.label(RichText::new(format!("{minutes:.1} {}", tr_label("min"))).color(t.text_dim)),
                        };
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
                let tool_labels: Vec<String> = tools.iter().map(|tool| format!("T{} {}", text(&tool["number"]), text(&tool["name"]))).collect();
                let tool_refs: Vec<&str> = tool_labels.iter().map(String::as_str).collect();
                if d.tool >= tool_refs.len() {
                    d.tool = 0;
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
                if crate::ui_kit::button(ui, "Add Operation", crate::ui_kit::Button::Primary).clicked() {
                    action = Some(Action::AddOperation);
                }
            } else if picked.is_some() {
                ui.label(RichText::new(tr("cam.panel.add_a_tool")).color(t.text_dim));
            }
            if let Some(e) = &d.error {
                ui.label(RichText::new(e).color(t.error));
            }
            if let Some(n) = &d.notice {
                ui.label(RichText::new(n).color(t.text_dim));
            }
            ui.separator();
            if crate::ui_kit::button(ui, "Export G-code…", crate::ui_kit::Button::Normal).clicked() {
                action = Some(Action::Export);
            }
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
        Action::Delete(id) => app.run("cam.delete", json!({"id": id})),
        Action::DeleteTool(n) => app.run("cam.delete", json!({"tool": n})),
        Action::AddOperation => {
            let tool = tools.get(d.tool).and_then(|t| t["number"].as_u64()).and_then(|n| u32::try_from(n).ok());
            match (d.setup, tool) {
                (Some(setup), Some(tool)) => d.operation_params(setup, tool, selection).and_then(|p| app.run("cam.operation", p)),
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
}

#[cfg(test)]
mod tests {
    use serde_json::json;
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
