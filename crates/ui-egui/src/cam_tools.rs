//! Cutting tools in the Manufacture workspace: the form a tool is edited in (the design's or the
//! shared library's), the tool drawn from the side with its sizes, and the shared tool library's
//! window (`cam.library*`), where tools and holders are kept for every design.

use egui::{Pos2, Rect, RichText, Stroke, Vec2};
use serde_json::{Value, json};
use solidcraft_theme::Tokens;
use solidcraft_ui_text::{tr, tr_label};

use crate::SolveApp;

/// The kinds of tool: the first two are the ones shown; the others behind "Other".
pub const KINDS: [&str; 5] = ["flat_endmill", "ball_endmill", "bull_nose", "drill", "v_bit"];
const MAIN_KINDS: [&str; 3] = ["Flat end mill", "Ball end mill", "Other"];
const OTHER_KINDS: [&str; 3] = ["Bull nose", "Drill", "V-bit"];
const MATERIALS: [&str; 3] = ["wood", "acrylic", "aluminum"];
const MATERIAL_LABELS: [&str; 3] = ["Wood", "Acrylic", "Aluminum"];
const PRESET_KEYS: [&str; 5] = ["spindle_rpm", "feed", "plunge_feed", "stepdown", "stepover"];
const PRESET_LABELS: [&str; 5] = ["Spindle (rpm)", "Feed (mm/min)", "Plunge (mm/min)", "Depth per pass", "Stepover (%)"];

fn num(v: &Value) -> String {
    v.as_f64().map(|x| format!("{}", (x * 1000.0).round() / 1000.0)).unwrap_or_default()
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn number(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

/// What a drawing needs of a tool (mm).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    /// An index into [`KINDS`].
    pub kind: usize,
    pub diameter: f64,
    pub flute: f64,
    pub shank: f64,
    pub stickout: f64,
    pub overall: Option<f64>,
    pub corner: f64,
    pub tip_angle: f64,
    pub tip_diameter: f64,
    /// The holder's sections from its face up: diameter, length.
    pub holder: Vec<(f64, f64)>,
}

impl Shape {
    /// From a tool as the engine reports it (`CamToolInfo`).
    pub fn from_info(v: &Value) -> Option<Shape> {
        let diameter = v["diameter"].as_f64().filter(|d| *d > 0.0)?;
        let flute = v["flute_length"].as_f64().unwrap_or(diameter * 4.0);
        Some(Shape {
            kind: KINDS.iter().position(|k| Some(*k) == v["kind"].as_str()).unwrap_or(0),
            diameter,
            flute,
            shank: v["shank_diameter"].as_f64().unwrap_or(diameter),
            stickout: v["stickout"].as_f64().unwrap_or(flute + 5.0),
            overall: v["overall_length"].as_f64(),
            corner: v["corner_radius"].as_f64().unwrap_or(0.0),
            tip_angle: v["tip_angle"].as_f64().unwrap_or(118.0),
            tip_diameter: v["tip_diameter"].as_f64().unwrap_or(0.0),
            holder: v["holder"]["sections"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| Some((s.get(0)?.as_f64()?, s.get(1)?.as_f64()?)))
                .filter(|(d, l)| *d > 0.0 && *l > 0.0)
                .collect(),
        })
    }

    /// How far from the axis the cutting end reaches `z` above the tip, below the full radius.
    fn end(&self, z: f64) -> f64 {
        let r = self.diameter / 2.0;
        match self.kind {
            1 => (r * r - (r - z).powi(2)).max(0.0).sqrt(),
            2 => {
                let c = self.corner.clamp(0.0, r);
                if z >= c { r } else { (r - c) + (c * c - (c - z).powi(2)).max(0.0).sqrt() }
            }
            3 | 4 => {
                let tip = (self.tip_diameter / 2.0).clamp(0.0, r);
                let len = self.point();
                if len <= 0.0 { r } else { tip + (r - tip) * (z / len).min(1.0) }
            }
            _ => r,
        }
    }

    /// The height of the end's shape: up to where it is the full width.
    fn point(&self) -> f64 {
        let r = self.diameter / 2.0;
        match self.kind {
            1 => r,
            2 => self.corner.clamp(0.0, r),
            3 | 4 => {
                let half = (self.tip_angle / 2.0).clamp(1.0, 89.0).to_radians();
                (r - (self.tip_diameter / 2.0).clamp(0.0, r)) / half.tan()
            }
            _ => 0.0,
        }
    }
}

/// Draw `s` from the side in `rect`: the flutes in the accent colour, the shank, the holder
/// above its face; with `dims`, its diameter, flute length, stickout and overall length.
pub fn draw(painter: &egui::Painter, rect: Rect, s: &Shape, dims: bool) {
    let t = Tokens::get();
    if !(s.diameter.is_finite() && s.diameter > 0.0 && rect.is_positive()) {
        return;
    }
    let flute = s.flute.max(s.point()).max(0.1);
    let holder_len: f64 = s.holder.iter().map(|h| h.1).sum();
    let shank_top = s.overall.unwrap_or_else(|| if holder_len > 0.0 { s.stickout + holder_len * 0.5 } else { flute * 1.7 }).max(flute);
    let top = shank_top.max(if holder_len > 0.0 { s.stickout + holder_len } else { 0.0 });
    let width = s.holder.iter().map(|h| h.0).fold(s.diameter.max(s.shank), f64::max);
    let (side, below) = if dims { (46.0, 26.0) } else { (2.0, 2.0) };
    let scale = ((rect.width() as f64 - 2.0 * side) / width).min((rect.height() as f64 - below - 4.0) / top).max(0.01);
    let cx = rect.center().x as f64;
    let base = rect.bottom() as f64 - below;
    let at = |x: f64, z: f64| Pos2::new((cx + x * scale) as f32, (base - z * scale) as f32);
    let painter = painter.with_clip_rect(rect);
    let outline = Stroke::new(1.0, t.text_dim);
    // The holder, its sections stacked from its face.
    let mut z = s.stickout;
    for (d, l) in &s.holder {
        let r = Rect::from_two_pos(at(-d / 2.0, z), at(d / 2.0, z + l));
        painter.rect(r, 1.0, t.text_dim.gamma_multiply(0.35), outline, egui::StrokeKind::Inside);
        z += l;
    }
    // The shank, then the flutes over it with the end's shape.
    let shank = Rect::from_two_pos(at(-s.shank / 2.0, flute), at(s.shank / 2.0, shank_top));
    painter.rect(shank, 0.0, t.text_dim.gamma_multiply(0.6), outline, egui::StrokeKind::Inside);
    let steps = 24;
    let point = s.point().min(flute);
    let mut right: Vec<Pos2> = (0..=steps).map(|k| point * f64::from(k) / f64::from(steps)).map(|z| at(s.end(z), z)).collect();
    right.push(at(s.diameter / 2.0, flute));
    let left: Vec<Pos2> = right.iter().rev().map(|p| Pos2::new((2.0 * cx) as f32 - p.x, p.y)).collect();
    let mut body = right.clone();
    body.extend(left);
    painter.add(egui::Shape::convex_polygon(body, t.accent.gamma_multiply(0.75), Stroke::new(1.0, t.accent)));
    // The flutes' helix, as slanted lines across them.
    let r = s.diameter / 2.0;
    let pitch = (s.diameter * 0.9).max(0.5);
    let mut zz = point + pitch * 0.5;
    while zz < flute - pitch * 0.3 {
        let (a, b) = (at(-r * 0.85, zz), at(r * 0.85, (zz + pitch * 0.45).min(flute)));
        painter.line_segment([a, b], Stroke::new(1.0, t.surface_fill.gamma_multiply(0.7)));
        zz += pitch;
    }
    if !dims {
        return;
    }
    let small = egui::FontId::proportional(11.0);
    let dim = |a: Pos2, b: Pos2, label: String, color: egui::Color32, anchor: egui::Align2, at_text: Pos2| {
        let stroke = Stroke::new(1.0, color);
        painter.line_segment([a, b], stroke);
        let d = (b - a).normalized() * 4.0;
        let n = Vec2::new(-d.y, d.x);
        for (p, dir) in [(a, d), (b, -d)] {
            painter.line_segment([p, p + dir + n * 0.6], stroke);
            painter.line_segment([p, p + dir - n * 0.6], stroke);
        }
        painter.text(at_text, anchor, label, small.clone(), color);
    };
    let short = |v: f64| format!("{}", (v * 100.0).round() / 100.0);
    // Diameter under the tip.
    let y = (base + 12.0) as f32;
    let (a, b) = (Pos2::new(at(-r, 0.0).x, y), Pos2::new(at(r, 0.0).x, y));
    dim(a, b, format!("⌀{}", short(s.diameter)), t.accent, egui::Align2::CENTER_TOP, Pos2::new(rect.center().x, y + 2.0));
    // Flute length on the right, the overall length beyond it.
    let x = at(width / 2.0, 0.0).x + 10.0;
    dim(
        Pos2::new(x, at(0.0, 0.0).y),
        Pos2::new(x, at(0.0, flute).y),
        short(s.flute),
        t.accent,
        egui::Align2::LEFT_CENTER,
        Pos2::new(x + 4.0, at(0.0, flute / 2.0).y),
    );
    if let Some(l) = s.overall {
        let x = x + 24.0;
        dim(
            Pos2::new(x, at(0.0, 0.0).y),
            Pos2::new(x, at(0.0, l).y),
            short(l),
            t.text,
            egui::Align2::LEFT_CENTER,
            Pos2::new(x + 4.0, at(0.0, l * 0.75).y),
        );
    }
    // Stickout on the left, where a holder sits.
    if !s.holder.is_empty() {
        let x = at(-width / 2.0, 0.0).x - 10.0;
        dim(
            Pos2::new(x, at(0.0, 0.0).y),
            Pos2::new(x, at(0.0, s.stickout).y),
            short(s.stickout),
            t.text_dim,
            egui::Align2::RIGHT_CENTER,
            Pos2::new(x - 4.0, at(0.0, s.stickout / 2.0).y),
        );
    }
}

/// A small picture of the tool for lists.
pub fn thumbnail(ui: &mut egui::Ui, s: Option<&Shape>, size: Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    if let Some(s) = s {
        draw(ui.painter(), rect, s, false);
    }
}

/// A tool being edited: each field as typed, empty for its default.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolForm {
    /// An index into [`KINDS`].
    pub kind: usize,
    pub name: String,
    pub diameter: String,
    pub flutes: String,
    pub flute_length: String,
    pub shank: String,
    pub overall: String,
    pub stickout: String,
    pub corner: String,
    pub tip_angle: String,
    pub tip_diameter: String,
    pub maker: String,
    pub model: String,
    pub notes: String,
    pub holder: bool,
    pub holder_name: String,
    /// Diameter and length of each section, from the holder's face up.
    pub sections: Vec<[String; 2]>,
    /// For each of [`MATERIALS`]: the values of [`PRESET_KEYS`].
    pub presets: [[String; 5]; 3],
}

impl Default for ToolForm {
    fn default() -> Self {
        ToolForm {
            kind: 0,
            name: String::new(),
            diameter: String::new(),
            flutes: "2".into(),
            flute_length: String::new(),
            shank: String::new(),
            overall: String::new(),
            stickout: String::new(),
            corner: String::new(),
            tip_angle: String::new(),
            tip_diameter: String::new(),
            maker: String::new(),
            model: String::new(),
            notes: String::new(),
            holder: false,
            holder_name: String::new(),
            sections: vec![[String::new(), String::new()]],
            presets: Default::default(),
        }
    }
}

impl ToolForm {
    /// The form of a tool as the engine reports it.
    pub fn from_info(v: &Value) -> ToolForm {
        let mut f = ToolForm {
            kind: KINDS.iter().position(|k| Some(*k) == v["kind"].as_str()).unwrap_or(0),
            name: text(&v["name"]),
            diameter: num(&v["diameter"]),
            flutes: text(&v["flutes"]),
            flute_length: num(&v["flute_length"]),
            overall: num(&v["overall_length"]),
            corner: num(&v["corner_radius"]),
            tip_angle: num(&v["tip_angle"]),
            tip_diameter: num(&v["tip_diameter"]),
            maker: text(&v["maker"]),
            model: text(&v["model"]),
            notes: text(&v["notes"]),
            ..ToolForm::default()
        };
        // The shank and stickout only when they aren't what they'd be anyway.
        if v["shank_diameter"] != v["diameter"] {
            f.shank = num(&v["shank_diameter"]);
        }
        if v["holder"].is_object() {
            f.holder = true;
            f.holder_name = text(&v["holder"]["name"]);
            f.sections = v["holder"]["sections"].as_array().into_iter().flatten().map(|s| [num(&s[0]), num(&s[1])]).collect();
            f.stickout = num(&v["stickout"]);
        }
        for p in v["presets"].as_array().into_iter().flatten() {
            if let Some(i) = MATERIALS.iter().position(|m| Some(*m) == p["material"].as_str())
                && let Some(row) = f.presets.get_mut(i)
            {
                for (k, key) in PRESET_KEYS.iter().enumerate() {
                    if let Some(cell) = row.get_mut(k) {
                        *cell = if *key == "stepover" {
                            p[*key].as_f64().map(|v| format!("{}", (v * 1000.0).round() / 10.0)).unwrap_or_default()
                        } else {
                            num(&p[*key])
                        };
                    }
                }
            }
        }
        f
    }

    /// The tool's fields for `cam.tool` or `cam.library.tool` (without its number or id).
    pub fn params(&self) -> Result<Value, String> {
        let kind = KINDS.get(self.kind).copied().unwrap_or("flat_endmill");
        let diameter = number(&self.diameter).ok_or_else(|| tr("cam.panel.bad_diameter").to_string())?;
        let mut p = json!({"kind": kind, "diameter": diameter, "name": self.name.trim()});
        if let Ok(n) = self.flutes.trim().parse::<u32>() {
            p["flutes"] = json!(n);
        }
        if !self.flute_length.trim().is_empty() {
            p["flute_length"] = json!(number(&self.flute_length).ok_or_else(|| tr("cam.panel.bad_flute").to_string())?);
        }
        // Empty clears (0), so a field emptied goes back to its default.
        let or_zero = |s: &str| number(s).unwrap_or(0.0);
        p["shank_diameter"] = json!(or_zero(&self.shank));
        p["overall_length"] = json!(or_zero(&self.overall));
        p["corner_radius"] = json!(if kind == "bull_nose" { or_zero(&self.corner) } else { 0.0 });
        p["tip_angle"] = json!(if kind == "drill" || kind == "v_bit" { or_zero(&self.tip_angle) } else { 0.0 });
        p["tip_diameter"] = json!(if kind == "v_bit" { or_zero(&self.tip_diameter) } else { 0.0 });
        p["maker"] = json!(self.maker.trim());
        p["model"] = json!(self.model.trim());
        p["notes"] = json!(self.notes.trim());
        if self.holder {
            let sections: Vec<[f64; 2]> = self.sections.iter().filter_map(|[d, l]| Some([number(d)?, number(l)?])).collect();
            if sections.is_empty() {
                return Err(tr("cam.lib.holder_sections").to_string());
            }
            p["holder"] = json!({"name": self.holder_name.trim(), "sections": sections});
            p["stickout"] = json!(or_zero(&self.stickout));
        } else {
            p["holder"] = json!({"sections": []});
            p["stickout"] = json!(0.0);
        }
        let presets: Vec<Value> = MATERIALS
            .iter()
            .zip(&self.presets)
            .filter_map(|(m, row)| {
                let mut o = json!({"material": m});
                let mut any = false;
                for (key, cell) in PRESET_KEYS.iter().zip(row) {
                    if let Some(v) = number(cell) {
                        o[*key] = json!(if *key == "stepover" { v / 100.0 } else { v });
                        any = true;
                    }
                }
                any.then_some(o)
            })
            .collect();
        p["presets"] = json!(presets);
        Ok(p)
    }

    /// The drawing of the tool as typed so far (`None` until it has a diameter).
    pub fn shape(&self) -> Option<Shape> {
        let diameter = number(&self.diameter).filter(|d| *d > 0.0)?;
        let flute = number(&self.flute_length).filter(|l| *l > 0.0).unwrap_or(diameter * 4.0);
        let holder: Vec<(f64, f64)> = if self.holder {
            self.sections.iter().filter_map(|[d, l]| Some((number(d)?, number(l)?))).filter(|(d, l)| *d > 0.0 && *l > 0.0).collect()
        } else {
            Vec::new()
        };
        Some(Shape {
            kind: self.kind,
            diameter,
            flute,
            shank: number(&self.shank).filter(|v| *v > 0.0).unwrap_or(diameter),
            stickout: number(&self.stickout).filter(|v| *v > 0.0).unwrap_or(flute + 5.0),
            overall: number(&self.overall).filter(|v| *v > 0.0),
            corner: number(&self.corner).unwrap_or(0.0),
            tip_angle: number(&self.tip_angle).filter(|v| *v > 0.0).unwrap_or(if self.kind == 4 { 60.0 } else { 118.0 }),
            tip_diameter: number(&self.tip_diameter).unwrap_or(0.0),
            holder,
        })
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    crate::ui_kit::row(ui, label, |ui| {
        crate::ui_kit::hint_field(ui, value, hint);
    });
}

fn heading(ui: &mut egui::Ui, label: &str) {
    let t = Tokens::get();
    ui.add_space(t.space_4);
    ui.label(RichText::new(tr_label(label)).small().strong().color(t.text_dim));
}

/// The form's fields, with the tool drawn above them; `holders` are the library's, to pick
/// one from.
pub fn form(ui: &mut egui::Ui, f: &mut ToolForm, holders: &[Value]) {
    let width = ui.available_width().clamp(160.0, 420.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 170.0), egui::Sense::hover());
    match f.shape() {
        Some(s) => draw(ui.painter(), rect, &s, true),
        None => {
            let t = Tokens::get();
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, tr("cam.lib.draw_hint"), egui::FontId::proportional(12.0), t.text_dim);
        }
    }
    // Flat and ball shown; the other kinds behind "Other".
    let mut main = f.kind.min(2);
    crate::ui_kit::segmented(ui, &mut main, &MAIN_KINDS, crate::ui_kit::Segments::default());
    if main < 2 {
        f.kind = main;
    } else {
        let mut other = f.kind.saturating_sub(2).min(2);
        crate::ui_kit::row(ui, "Kind", |ui| crate::ui_kit::dropdown(ui, "cam_tool_other_kind", &mut other, &OTHER_KINDS));
        f.kind = other + 2;
    }
    heading(ui, "Shape");
    field(ui, "Diameter", &mut f.diameter, "6");
    if f.kind != 3 {
        field(ui, "Flutes", &mut f.flutes, "2");
    }
    field(ui, "Flute length", &mut f.flute_length, tr_label("4 × diameter"));
    field(ui, "Shank diameter", &mut f.shank, tr_label("as the cutter"));
    field(ui, "Overall length", &mut f.overall, "");
    match f.kind {
        2 => field(ui, "Corner radius", &mut f.corner, "0.5"),
        3 => field(ui, "Point angle (°)", &mut f.tip_angle, "118"),
        4 => {
            field(ui, "Point angle (°)", &mut f.tip_angle, "60");
            field(ui, "Tip diameter", &mut f.tip_diameter, "0");
        }
        _ => {}
    }
    heading(ui, "Holder");
    crate::ui_kit::checkbox(ui, tr_label("Use a holder"), &mut f.holder);
    if f.holder {
        if !holders.is_empty() {
            let names: Vec<String> = holders.iter().map(|h| text(&h["name"])).collect();
            let mut labels: Vec<&str> = vec![tr_label("Pick from the library")];
            labels.extend(names.iter().map(String::as_str));
            let mut picked = 0;
            crate::ui_kit::row(ui, "Holder", |ui| crate::ui_kit::dropdown(ui, "cam_tool_holder_pick", &mut picked, &labels));
            if let Some(h) = picked.checked_sub(1).and_then(|i| holders.get(i)) {
                f.holder_name = text(&h["name"]);
                f.sections = h["sections"].as_array().into_iter().flatten().map(|s| [num(&s[0]), num(&s[1])]).collect();
            }
        }
        field(ui, "Name", &mut f.holder_name, "ER11");
        field(ui, "Stickout", &mut f.stickout, tr_label("flutes + 5"));
        sections(ui, &mut f.sections);
    }
    heading(ui, "Cutting data");
    crate::ui_kit::text(ui, tr("cam.lib.preset_hint"), crate::ui_kit::Text::Hint("tool"));
    let mut material = ui.data(|d| d.get_temp::<usize>(egui::Id::unique("sc_cam_tool_material"))).unwrap_or(0);
    crate::ui_kit::segmented(ui, &mut material, &MATERIAL_LABELS, crate::ui_kit::Segments::default());
    ui.data_mut(|d| d.insert_temp(egui::Id::unique("sc_cam_tool_material"), material));
    if let Some(row) = f.presets.get_mut(material) {
        for (cell, label) in row.iter_mut().zip(PRESET_LABELS) {
            field(ui, label, cell, tr_label("starting value"));
        }
    }
    heading(ui, "Notes");
    field(ui, "Name", &mut f.name, tr_label("from its size"));
    field(ui, "Maker", &mut f.maker, "");
    field(ui, "Model", &mut f.model, "");
    field(ui, "Notes", &mut f.notes, "");
}

/// A holder's sections, one row each, with a button to add one and one to take each away.
fn sections(ui: &mut egui::Ui, rows: &mut Vec<[String; 2]>) {
    let mut remove = None;
    for (i, [d, l]) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}", i + 1)).small());
            ui.add_sized([60.0, 20.0], egui::TextEdit::singleline(d).hint_text("⌀"));
            ui.add_sized([60.0, 20.0], egui::TextEdit::singleline(l).hint_text(tr_label("length")));
            if crate::ui_kit::button(ui, tr_label("Remove"), crate::ui_kit::Button::Small).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove
        && rows.len() > 1
    {
        rows.remove(i);
    }
    if rows.len() < 8 && crate::ui_kit::button(ui, tr_label("Add Section"), crate::ui_kit::Button::Small).clicked() {
        rows.push([String::new(), String::new()]);
    }
}

/// Which list the library's window shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    /// The design's own tools.
    Design,
    /// The shared library's tools.
    #[default]
    Library,
    /// The shared library's holders.
    Holders,
}

/// What the editor window is changing: a tool of the design (`None`: a new one), of the
/// library, or a holder.
#[derive(Clone, Debug, PartialEq)]
pub enum Editing {
    DesignTool(Option<u64>),
    LibraryTool(Option<String>),
    Holder(Option<String>),
}

/// The tool library's window: the lists on the left, the tools in the middle with their cutting
/// data under them, filters and the picked tool's drawing on the right; and the editor.
#[derive(Clone, Debug, Default)]
pub struct Library {
    pub open: bool,
    /// `cam.library`'s last answer.
    pub data: Option<Value>,
    /// Another library file than the user's (tests).
    pub path: Option<String>,
    pub source: Source,
    /// The row picked: `T<n>` for a design tool, else its library id.
    pub selected: Option<String>,
    /// The column sorted by (an index into [`COLUMNS`]) and whether it runs down.
    pub sort: (usize, bool),
    pub search: String,
    /// Which kinds show (all when none is ticked).
    pub kinds: [bool; 5],
    pub min_diameter: String,
    pub max_diameter: String,
    /// The right side's tab: 0 filters, 1 the picked tool.
    pub side: usize,
    pub editing: Option<Editing>,
    pub form: ToolForm,
    /// The design tool's number being edited.
    pub number: String,
    pub holder_name: String,
    pub holder_sections: Vec<[String; 2]>,
    pub note: Option<String>,
    pub error: Option<String>,
}

/// What the window asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Do {
    Source(Source),
    Select(String),
    New,
    Edit,
    Duplicate,
    Delete,
    /// Copy the picked library tool into the design.
    CopyToDesign,
    /// Keep the picked design tool in the library.
    KeepInLibrary,
    SaveEdit,
    CancelEdit,
    ClearFilters,
    Export,
    Import,
    Close,
}

/// The tool table's columns: picture, number, name, diameter, flute length, overall length,
/// kind; and their widths.
const COLUMNS: [&str; 7] = ["", "No.", "Name", "Diameter", "Flute length", "Overall length", "Kind"];
const WIDTHS: [f32; 7] = [24.0, 34.0, 210.0, 62.0, 62.0, 62.0, 110.0];

fn kind_label(info: &Value) -> &'static str {
    match KINDS.iter().position(|k| Some(*k) == info["kind"].as_str()).unwrap_or(0) {
        0 => "Flat end mill",
        1 => "Ball end mill",
        k => OTHER_KINDS.get(k - 2).copied().unwrap_or(""),
    }
}

impl Library {
    /// `params` with the library file when it isn't the user's.
    fn with_path(&self, mut params: Value) -> Value {
        if let Some(p) = &self.path {
            params["path"] = json!(p);
        }
        params
    }

    /// Read the library again.
    pub fn refresh(&mut self, app: &mut SolveApp) {
        match app.session.execute("cam.library", &self.with_path(json!({}))) {
            Ok(v) => {
                self.data = Some(v);
                self.error = None;
            }
            Err(e) => {
                // Read once: an unreadable library isn't read again every frame.
                self.data = Some(json!({}));
                self.error = Some(e.to_string());
            }
        }
    }

    pub fn tools(&self) -> Vec<Value> {
        self.data.as_ref().and_then(|d| d["tools"].as_array()).cloned().unwrap_or_default()
    }

    pub fn holders(&self) -> Vec<Value> {
        self.data.as_ref().and_then(|d| d["holders"].as_array()).cloned().unwrap_or_default()
    }

    /// The library entry's revision, for a design tool copied from it.
    pub fn revision(&self, id: &str) -> Option<String> {
        self.tools().into_iter().find(|e| e["id"] == id).map(|e| text(&e["revision"]))
    }

    /// Run a library command and take the library it answers with.
    fn run(&mut self, app: &mut SolveApp, cmd: &str, params: Value) -> Result<Value, String> {
        let v = app.run(cmd, self.with_path(params))?;
        if v["tools"].is_array() && v["holders"].is_array() {
            self.data = Some(v.clone());
        }
        Ok(v)
    }

    /// Copy library tool `id` into the design.
    pub fn use_tool(&mut self, app: &mut SolveApp, id: &str) -> Result<Value, String> {
        self.run(app, "cam.library.use", json!({"id": id}))
    }

    /// Put the design's tool `number` into the library.
    pub fn store(&mut self, app: &mut SolveApp, number: u64) -> Result<Value, String> {
        self.run(app, "cam.library.store", json!({"number": number}))
    }

    /// The rows the table shows: the source's tools (as `CamToolInfo`) with their keys, filtered
    /// and sorted.
    pub fn rows(&self, design: &[Value]) -> Vec<(String, Value)> {
        let all: Vec<(String, Value)> = match self.source {
            Source::Design => design.iter().map(|t| (format!("T{}", text(&t["number"])), t.clone())).collect(),
            Source::Library => self.tools().into_iter().map(|e| (text(&e["id"]), e["tool"].clone())).collect(),
            Source::Holders => Vec::new(),
        };
        let needle = self.search.trim().to_lowercase();
        let any_kind = !self.kinds.iter().any(|k| *k);
        let (lo, hi) = (number(&self.min_diameter), number(&self.max_diameter));
        let mut rows: Vec<(String, Value)> = all
            .into_iter()
            .filter(|(_, t)| {
                let kind = KINDS.iter().position(|k| Some(*k) == t["kind"].as_str()).unwrap_or(0);
                let d = t["diameter"].as_f64().unwrap_or(0.0);
                let name = text(&t["name"]);
                (any_kind || self.kinds.get(kind).copied().unwrap_or(false))
                    && lo.is_none_or(|lo| d >= lo - 1e-9)
                    && hi.is_none_or(|hi| d <= hi + 1e-9)
                    && (needle.is_empty()
                        || name.to_lowercase().contains(&needle)
                        || crate::cam_panel::tool_name(&name).to_lowercase().contains(&needle)
                        || text(&t["maker"]).to_lowercase().contains(&needle)
                        || text(&t["model"]).to_lowercase().contains(&needle))
            })
            .collect();
        let key = |t: &Value| -> (f64, String) {
            match self.sort.0 {
                1 => (t["number"].as_f64().unwrap_or(0.0), String::new()),
                2 => (0.0, crate::cam_panel::tool_name(&text(&t["name"]))),
                4 => (t["flute_length"].as_f64().unwrap_or(0.0), String::new()),
                5 => (t["overall_length"].as_f64().unwrap_or(0.0), String::new()),
                6 => (0.0, tr_label(kind_label(t)).to_string()),
                _ => (t["diameter"].as_f64().unwrap_or(0.0), String::new()),
            }
        };
        rows.sort_by(|a, b| {
            let (ka, kb) = (key(&a.1), key(&b.1));
            let o = ka.0.total_cmp(&kb.0).then_with(|| ka.1.cmp(&kb.1));
            if self.sort.1 { o.reverse() } else { o }
        });
        rows
    }

    /// The picked row's tool.
    pub fn picked(&self, design: &[Value]) -> Option<Value> {
        let key = self.selected.as_ref()?;
        match self.source {
            Source::Design => design.iter().find(|t| format!("T{}", text(&t["number"])) == *key).cloned(),
            Source::Library => self.tools().into_iter().find(|e| text(&e["id"]) == *key).map(|e| e["tool"].clone()),
            Source::Holders => None,
        }
    }

    fn picked_holder(&self) -> Option<Value> {
        let key = self.selected.as_ref()?;
        self.holders().into_iter().find(|h| text(&h["id"]) == *key)
    }

    /// Carry out what the window asked for; `design` are the design's tools.
    pub fn act(&mut self, app: &mut SolveApp, design: &[Value], what: Do) {
        let saved = || Some(tr("cam.lib.saved").to_string());
        let result: Result<Option<String>, String> = match what {
            Do::Source(s) => {
                self.source = s;
                self.selected = None;
                Ok(None)
            }
            Do::Select(key) => {
                self.selected = Some(key);
                self.side = 1;
                Ok(None)
            }
            Do::New => {
                match self.source {
                    Source::Holders => {
                        self.holder_name.clear();
                        self.holder_sections = vec![[String::new(), String::new()]];
                        self.editing = Some(Editing::Holder(None));
                    }
                    Source::Design => {
                        self.form = ToolForm::default();
                        let next = design.iter().filter_map(|t| t["number"].as_u64()).max().unwrap_or(0) + 1;
                        self.number = next.to_string();
                        self.editing = Some(Editing::DesignTool(None));
                    }
                    Source::Library => {
                        self.form = ToolForm::default();
                        self.editing = Some(Editing::LibraryTool(None));
                    }
                }
                Ok(None)
            }
            Do::Edit | Do::Duplicate => {
                let copy = what == Do::Duplicate;
                match self.source {
                    Source::Holders => match self.picked_holder() {
                        Some(h) => {
                            self.holder_name = text(&h["name"]);
                            self.holder_sections = h["sections"].as_array().into_iter().flatten().map(|s| [num(&s[0]), num(&s[1])]).collect();
                            self.editing = Some(Editing::Holder(if copy { None } else { Some(text(&h["id"])) }));
                            Ok(None)
                        }
                        None => Ok(None),
                    },
                    _ => match (self.picked(design), self.selected.clone()) {
                        (Some(t), Some(key)) => {
                            self.form = ToolForm::from_info(&t);
                            if copy {
                                self.form.name.clear();
                            }
                            self.editing = Some(match self.source {
                                Source::Design => {
                                    let n = t["number"].as_u64();
                                    let next = design.iter().filter_map(|t| t["number"].as_u64()).max().unwrap_or(0) + 1;
                                    self.number = if copy { next.to_string() } else { n.map(|n| n.to_string()).unwrap_or_default() };
                                    Editing::DesignTool(if copy { None } else { n })
                                }
                                _ => Editing::LibraryTool(if copy { None } else { Some(key) }),
                            });
                            Ok(None)
                        }
                        _ => Ok(None),
                    },
                }
            }
            Do::Delete => match (self.source, self.selected.clone()) {
                (Source::Design, Some(key)) => match key.trim_start_matches('T').parse::<u64>() {
                    Ok(n) => app.run("cam.delete", json!({"tool": n})).map(|_| {
                        self.selected = None;
                        None
                    }),
                    Err(_) => Ok(None),
                },
                (_, Some(id)) => self.run(app, "cam.library.delete", json!({"id": id})).map(|_| {
                    self.selected = None;
                    None
                }),
                _ => Ok(None),
            },
            Do::CopyToDesign => match (self.source, self.selected.clone()) {
                (Source::Library, Some(id)) => self.use_tool(app, &id).map(|_| Some(tr("cam.lib.used").to_string())),
                _ => Ok(None),
            },
            Do::KeepInLibrary => match (self.source, self.picked(design).and_then(|t| t["number"].as_u64())) {
                (Source::Design, Some(n)) => self.store(app, n).map(|_| Some(tr("cam.lib.stored").to_string())),
                _ => Ok(None),
            },
            Do::SaveEdit => match self.editing.clone() {
                Some(Editing::DesignTool(old)) => {
                    let n = self.number.trim().parse::<u64>().map_err(|_| tr("cam.panel.bad_tool_number").to_string());
                    match n.and_then(|n| self.form.params().map(|p| (n, p))) {
                        Ok((n, mut p)) => {
                            p["number"] = json!(n);
                            // A new number on an old tool: the old one goes.
                            let moved = old.filter(|o| *o != n);
                            app.run("cam.tool", p)
                                .and_then(|_| match moved {
                                    Some(o) => app.run("cam.delete", json!({"tool": o})).map(|_| ()),
                                    None => Ok(()),
                                })
                                .map(|_| {
                                    self.editing = None;
                                    self.selected = Some(format!("T{n}"));
                                    saved()
                                })
                        }
                        Err(e) => Err(e),
                    }
                }
                Some(Editing::LibraryTool(id)) => self.form.params().and_then(|mut p| {
                    if let Some(id) = &id {
                        p["id"] = json!(id);
                    }
                    let v = self.run(app, "cam.library.tool", p)?;
                    self.selected = id.or_else(|| v["tools"].as_array().and_then(|t| t.last()).map(|e| text(&e["id"])));
                    self.editing = None;
                    Ok(saved())
                }),
                Some(Editing::Holder(id)) => {
                    let sections: Vec<[f64; 2]> = self.holder_sections.iter().filter_map(|[d, l]| Some([number(d)?, number(l)?])).collect();
                    let mut p = json!({"name": self.holder_name.trim(), "sections": sections});
                    if let Some(id) = &id {
                        p["id"] = json!(id);
                    }
                    self.run(app, "cam.library.holder", p).map(|v| {
                        self.selected = id.or_else(|| v["holders"].as_array().and_then(|h| h.last()).map(|e| text(&e["id"])));
                        self.editing = None;
                        saved()
                    })
                }
                None => Ok(None),
            },
            Do::CancelEdit => {
                self.editing = None;
                Ok(None)
            }
            Do::ClearFilters => {
                self.search.clear();
                self.kinds = [false; 5];
                self.min_diameter.clear();
                self.max_diameter.clear();
                Ok(None)
            }
            Do::Export => {
                let path = self.path.clone();
                app.pick_save("tools.solidcraft-tools.json", &["json"], move |app, to| {
                    let Some(to) = to else { return };
                    let mut p = json!({"to": to});
                    if let Some(path) = path {
                        p["path"] = json!(path);
                    }
                    match app.run("cam.library.file", p) {
                        Ok(_) => app.set_status(tr("cam.lib.exported").to_string(), false),
                        Err(e) => app.set_status(e, true),
                    }
                });
                Ok(None)
            }
            Do::Import => {
                let path = self.path.clone();
                app.pick_open_for(&["json"], move |app, from| {
                    let Some(from) = from else { return };
                    let mut p = json!({"from": from});
                    if let Some(path) = path {
                        p["path"] = json!(path);
                    }
                    match app.run("cam.library.file", p) {
                        Ok(v) => {
                            let n = text(&v["added"]);
                            app.set_status(solidcraft_ui_text::format("cam.lib.imported", &[("n", &n)]), false);
                            if let Some(d) = app.machining.as_mut() {
                                d.library.data = Some(v);
                            }
                        }
                        Err(e) => app.set_status(e, true),
                    }
                });
                Ok(None)
            }
            Do::Close => {
                self.open = false;
                self.editing = None;
                Ok(None)
            }
        };
        match result {
            Ok(note) => {
                self.note = note;
                self.error = None;
            }
            Err(e) => {
                self.error = Some(e);
                self.note = None;
            }
        }
    }
}

/// A sample of each kind, drawn small for the filters.
fn sample(kind: usize) -> Shape {
    Shape {
        kind,
        diameter: 6.0,
        flute: 14.0,
        shank: 6.0,
        stickout: 20.0,
        corner: 1.5,
        tip_angle: if kind == 4 { 60.0 } else { 118.0 },
        ..Shape::default()
    }
}

/// The cutting data a tool cuts each material with: its own where it keeps some, else the
/// starting values (marked).
fn cutting_rows(info: &Value) -> Vec<(usize, [Option<(f64, bool)>; 5])> {
    let tool = serde_json::from_value::<solidcraft_engine::machining::Tool>(json!({
        "number": info["number"].as_u64().unwrap_or(1).clamp(1, 99),
        "name": info["name"],
        "kind": info["kind"],
        "diameter": info["diameter"],
        "flute_length": info["flute_length"],
        "flutes": info["flutes"],
    }));
    let Ok(tool) = tool else { return Vec::new() };
    let materials = [
        solidcraft_engine::machining::Material::Wood,
        solidcraft_engine::machining::Material::Acrylic,
        solidcraft_engine::machining::Material::Aluminum,
    ];
    materials
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let start = solidcraft_engine::machining::Cutting::starting(*m, &tool);
            let own = info["presets"].as_array().into_iter().flatten().find(|p| p["material"] == MATERIALS[i]).cloned().unwrap_or(Value::Null);
            let starting = [start.spindle_rpm, start.feed, start.plunge_feed, start.stepdown, start.stepover * 100.0];
            let mut row = [None; 5];
            for (k, key) in PRESET_KEYS.iter().enumerate() {
                let mine = own[*key].as_f64().map(|v| if *key == "stepover" { v * 100.0 } else { v });
                if let Some(cell) = row.get_mut(k) {
                    *cell = match mine {
                        Some(v) => Some((v, true)),
                        None => starting.get(k).map(|v| (*v, false)),
                    };
                }
            }
            (i, row)
        })
        .collect()
}

fn cell(ui: &mut egui::Ui, width: f32, text: RichText) {
    ui.add_sized([width, 18.0], egui::Label::new(text).truncate());
}

/// The library's window, and the editor when something is being changed. `design` are the
/// design's tools.
pub fn window(ctx: &egui::Context, lib: &mut Library, design: &[Value]) -> Option<Do> {
    let t = Tokens::get();
    let mut action = None;
    let holders = lib.holders();
    let rows = lib.rows(design);
    let picked = lib.picked(design);
    let closed =
        crate::ui_kit::Window::new(egui::Id::unique("sc_cam_library"), "library", tr_label("Tool Library")).cards(4.6).show_body(ctx, |ui| {
            ui.horizontal_top(|ui| {
                // Left: the lists.
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    crate::ui_kit::hint_field(ui, &mut lib.search, tr_label("Search"));
                    ui.add_space(t.space_4);
                    for (source, label, count) in [
                        (Source::Design, "This design", design.len()),
                        (Source::Library, "My library", lib.tools().len()),
                        (Source::Holders, "Holders", holders.len()),
                    ] {
                        let text = RichText::new(format!("{}  ({count})", tr_label(label)));
                        if ui.selectable_label(lib.source == source, text).clicked() {
                            action = Some(Do::Source(source));
                        }
                    }
                    ui.add_space(t.space_4);
                    if crate::ui_kit::button(ui, tr_label("Import…"), crate::ui_kit::Button::Small).clicked() {
                        action = Some(Do::Import);
                    }
                    if crate::ui_kit::button(ui, tr_label("Export…"), crate::ui_kit::Button::Small).clicked() {
                        action = Some(Do::Export);
                    }
                    if let Some(path) = lib.data.as_ref().map(|d| text(&d["path"])).filter(|p| !p.is_empty()) {
                        ui.label(RichText::new(solidcraft_ui_text::format("cam.lib.path", &[("path", &path)])).small().color(t.text_dim));
                    }
                });
                ui.separator();
                // Middle: the toolbar, the table, the cutting data.
                ui.vertical(|ui| {
                    ui.set_width(WIDTHS.iter().sum::<f32>() + 30.0);
                    ui.horizontal(|ui| {
                        let has = lib.selected.is_some();
                        for (label, what, enabled) in
                            [("New", Do::New, true), ("Edit", Do::Edit, has), ("Duplicate", Do::Duplicate, has), ("Delete", Do::Delete, has)]
                        {
                            if ui.add_enabled(enabled, egui::Button::new(RichText::new(tr_label(label)).small())).clicked() {
                                action = Some(what);
                            }
                        }
                        match lib.source {
                            Source::Library => {
                                if ui
                                    .add_enabled(lib.selected.is_some(), egui::Button::new(RichText::new(tr_label("Use in This Design")).small()))
                                    .clicked()
                                {
                                    action = Some(Do::CopyToDesign);
                                }
                            }
                            Source::Design => {
                                if ui
                                    .add_enabled(lib.selected.is_some(), egui::Button::new(RichText::new(tr_label("Keep in Library")).small()))
                                    .clicked()
                                {
                                    action = Some(Do::KeepInLibrary);
                                }
                            }
                            Source::Holders => {}
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::ui_kit::button(ui, tr_label("Clear Filters"), crate::ui_kit::Button::Small).clicked() {
                                action = Some(Do::ClearFilters);
                            }
                        });
                    });
                    if lib.source == Source::Holders {
                        egui::ScrollArea::vertical().id_salt("sc_cam_library_holders").max_height(300.0).min_scrolled_height(300.0).show(ui, |ui| {
                            if holders.is_empty() {
                                ui.label(RichText::new(tr("cam.lib.no_holders")).small().color(t.text_dim));
                            }
                            for h in &holders {
                                let id = text(&h["id"]);
                                let sections: Vec<String> =
                                    h["sections"].as_array().into_iter().flatten().map(|s| format!("⌀{}×{}", num(&s[0]), num(&s[1]))).collect();
                                let label = format!("{}   {}", text(&h["name"]), sections.join(" + "));
                                let r = ui.selectable_label(lib.selected.as_deref() == Some(id.as_str()), label);
                                if r.double_clicked() {
                                    action = Some(Do::Edit);
                                } else if r.clicked() {
                                    action = Some(Do::Select(id));
                                }
                            }
                        });
                    } else {
                        // The header: a click sorts by the column, again turns it round.
                        ui.horizontal(|ui| {
                            for (i, (name, w)) in COLUMNS.iter().zip(WIDTHS).enumerate() {
                                if i == 1 && lib.source != Source::Design {
                                    continue;
                                }
                                let arrow = if lib.sort.0 == i { if lib.sort.1 { " ▼" } else { " ▲" } } else { "" };
                                let label = RichText::new(format!("{}{arrow}", if name.is_empty() { "" } else { tr_label(name) })).small().strong();
                                if ui.add_sized([w, 18.0], egui::Button::new(label).frame(false)).clicked() && i > 0 {
                                    lib.sort = if lib.sort.0 == i { (i, !lib.sort.1) } else { (i, false) };
                                }
                            }
                        });
                        egui::ScrollArea::vertical().id_salt("sc_cam_library_table").max_height(260.0).min_scrolled_height(260.0).show(ui, |ui| {
                            if rows.is_empty() {
                                let hint = match lib.source {
                                    Source::Design => tr("cam.lib.design_empty"),
                                    _ if lib.tools().is_empty() => tr("cam.lib.empty"),
                                    _ => tr("cam.lib.none_shown"),
                                };
                                ui.label(RichText::new(hint).small().color(t.text_dim));
                            }
                            for (key, info) in &rows {
                                let selected = lib.selected.as_deref() == Some(key.as_str());
                                let inner = ui.horizontal(|ui| {
                                    let shape = Shape::from_info(info);
                                    thumbnail(ui, shape.as_ref(), Vec2::new(WIDTHS[0], 30.0));
                                    let color = if selected { t.accent } else { t.text };
                                    if lib.source == Source::Design {
                                        cell(ui, WIDTHS[1], RichText::new(format!("T{}", text(&info["number"]))).color(color));
                                    }
                                    cell(ui, WIDTHS[2], RichText::new(crate::cam_panel::tool_name(&text(&info["name"]))).color(color));
                                    cell(ui, WIDTHS[3], RichText::new(format!("{} mm", num(&info["diameter"]))).color(color));
                                    cell(ui, WIDTHS[4], RichText::new(format!("{} mm", num(&info["flute_length"]))).color(color));
                                    let overall = num(&info["overall_length"]);
                                    cell(
                                        ui,
                                        WIDTHS[5],
                                        RichText::new(if overall.is_empty() { "—".into() } else { format!("{overall} mm") }).color(color),
                                    );
                                    cell(ui, WIDTHS[6], RichText::new(tr_label(kind_label(info))).color(t.text_dim));
                                });
                                let rect = inner.response.rect;
                                let response = ui.interact(rect, egui::Id::unique(("sc_cam_lib_row", key)), egui::Sense::click());
                                if selected {
                                    ui.painter().rect_filled(rect, 2.0, t.accent.gamma_multiply(0.12));
                                } else if response.hovered() {
                                    ui.painter().rect_filled(rect, 2.0, t.text_dim.gamma_multiply(0.08));
                                }
                                crate::scenario::publish_handle(&format!("cam:lib:{key}"), rect.center());
                                if response.double_clicked() {
                                    action = Some(Do::Edit);
                                } else if response.clicked() {
                                    action = Some(Do::Select(key.clone()));
                                }
                            }
                        });
                        // The picked tool's cutting data for each material.
                        ui.add_space(t.space_4);
                        ui.label(RichText::new(tr_label("Cutting data")).small().strong().color(t.text_dim));
                        match &picked {
                            None => {
                                ui.label(RichText::new(tr("cam.lib.pick_a_tool")).small().color(t.text_dim));
                            }
                            Some(info) => {
                                egui::Grid::new("sc_cam_library_cutting").striped(true).show(ui, |ui| {
                                    ui.label("");
                                    for label in PRESET_LABELS {
                                        ui.label(RichText::new(tr_label(label)).small().strong());
                                    }
                                    ui.end_row();
                                    for (i, row) in cutting_rows(info) {
                                        ui.label(RichText::new(tr_label(MATERIAL_LABELS.get(i).copied().unwrap_or(""))).small());
                                        for v in row {
                                            let label = match v {
                                                Some((v, own)) => RichText::new(format!("{}", (v * 10.0).round() / 10.0)).small().color(if own {
                                                    t.text
                                                } else {
                                                    t.text_dim
                                                }),
                                                None => RichText::new("—").small(),
                                            };
                                            ui.label(label);
                                        }
                                        ui.end_row();
                                    }
                                });
                                ui.label(RichText::new(tr("cam.lib.starting_dim")).small().color(t.text_dim));
                            }
                        }
                    }
                });
                ui.separator();
                // Right: filters, or the picked tool.
                ui.vertical(|ui| {
                    ui.set_width(210.0);
                    crate::ui_kit::segmented(ui, &mut lib.side, &["Filters", "Information"], crate::ui_kit::Segments::default());
                    if lib.side == 0 {
                        ui.label(RichText::new(tr_label("Kind")).small().strong().color(t.text_dim));
                        let labels = ["Flat end mill", "Ball end mill", "Bull nose", "Drill", "V-bit"];
                        for (k, label) in labels.iter().enumerate() {
                            ui.horizontal(|ui| {
                                thumbnail(ui, Some(&sample(k)), Vec2::new(14.0, 26.0));
                                if let Some(on) = lib.kinds.get_mut(k) {
                                    crate::ui_kit::checkbox(ui, tr_label(label), on);
                                }
                            });
                        }
                        ui.add_space(t.space_4);
                        ui.label(RichText::new(tr_label("Diameter")).small().strong().color(t.text_dim));
                        field(ui, "From", &mut lib.min_diameter, "0");
                        field(ui, "To", &mut lib.max_diameter, "∞");
                    } else if let Some(info) = &picked {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(210.0, 200.0), egui::Sense::hover());
                        if let Some(s) = Shape::from_info(info) {
                            draw(ui.painter(), rect, &s, true);
                        }
                        ui.label(RichText::new(crate::cam_panel::tool_name(&text(&info["name"]))).strong());
                        let mut line = |label: &str, value: String| {
                            if !value.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(tr_label(label)).small().color(t.text_dim));
                                    ui.label(RichText::new(value).small());
                                });
                            }
                        };
                        line("Kind", tr_label(kind_label(info)).to_string());
                        line("Flutes", text(&info["flutes"]));
                        line("Shank diameter", num(&info["shank_diameter"]));
                        line("Corner radius", num(&info["corner_radius"]));
                        line("Point angle (°)", num(&info["tip_angle"]));
                        line("Holder", text(&info["holder"]["name"]));
                        if info["holder"].is_object() {
                            line("Stickout", num(&info["stickout"]));
                        }
                        line("Maker", text(&info["maker"]));
                        line("Model", text(&info["model"]));
                        line("Notes", text(&info["notes"]));
                        if lib.source == Source::Design
                            && let Some(id) = info["library"].as_str()
                        {
                            let same = lib.revision(id).is_some_and(|r| r == text(&info["revision"]));
                            let (msg, color) = match lib.revision(id) {
                                None => (tr("cam.lib.missing"), t.text_dim),
                                Some(_) if same => (tr("cam.lib.same"), t.agent_ok),
                                Some(_) => (tr("cam.lib.differs"), t.warning),
                            };
                            ui.label(RichText::new(msg).small().color(color));
                        }
                    } else if let Some(h) = (lib.source == Source::Holders).then(|| lib.picked_holder()).flatten() {
                        let shape = Shape {
                            holder: h["sections"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|s| Some((s.get(0)?.as_f64()?, s.get(1)?.as_f64()?)))
                                .collect(),
                            ..sample(0)
                        };
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(210.0, 200.0), egui::Sense::hover());
                        draw(ui.painter(), rect, &shape, true);
                        ui.label(RichText::new(text(&h["name"])).strong());
                    } else {
                        ui.label(RichText::new(tr("cam.lib.pick_a_tool")).small().color(t.text_dim));
                    }
                });
            });
            ui.horizontal(|ui| {
                if let Some(n) = &lib.note {
                    ui.label(RichText::new(n).small().color(t.agent_ok));
                }
                if let Some(e) = &lib.error {
                    ui.label(RichText::new(crate::cam_panel::cam_text(e)).small().color(t.error));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::ui_kit::button(ui, tr_label("Close"), crate::ui_kit::Button::Normal).clicked() {
                        action = Some(Do::Close);
                    }
                });
            });
        });
    if closed {
        action = Some(Do::Close);
    }
    // The editor, beside it.
    if let Some(editing) = lib.editing.clone() {
        let title = match editing {
            Editing::Holder(_) => tr_label("Holder"),
            _ => tr_label("Edit Tool"),
        };
        let closed = crate::ui_kit::Window::new(egui::Id::unique("sc_cam_library_editor"), "edit", title).cards(2.0).show_body(ctx, |ui| {
            match editing {
                Editing::Holder(_) => {
                    let shape = Shape {
                        holder: lib
                            .holder_sections
                            .iter()
                            .filter_map(|[d, l]| Some((number(d)?, number(l)?)))
                            .filter(|(d, l)| *d > 0.0 && *l > 0.0)
                            .collect(),
                        ..sample(0)
                    };
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(300.0, 170.0), egui::Sense::hover());
                    draw(ui.painter(), rect, &shape, true);
                    field(ui, "Name", &mut lib.holder_name, "ER11");
                    sections(ui, &mut lib.holder_sections);
                }
                Editing::DesignTool(_) => {
                    field(ui, "Number", &mut lib.number, "1");
                    form(ui, &mut lib.form, &holders);
                }
                Editing::LibraryTool(_) => form(ui, &mut lib.form, &holders),
            }
            ui.horizontal(|ui| {
                if crate::ui_kit::button(ui, tr_label("Save"), crate::ui_kit::Button::Primary).clicked() {
                    action = Some(Do::SaveEdit);
                }
                if crate::ui_kit::button(ui, tr_label("Cancel"), crate::ui_kit::Button::Normal).clicked() {
                    action = Some(Do::CancelEdit);
                }
            });
            if let Some(e) = &lib.error {
                ui.label(RichText::new(crate::cam_panel::cam_text(e)).small().color(t.error));
            }
        });
        if closed {
            action = Some(Do::CancelEdit);
        }
    }
    action
}

/// One row of a tool list: its picture, name and size; clicked or not.
pub fn tool_row(ui: &mut egui::Ui, id: egui::Id, info: &Value, selected: bool, numbered: bool, note: &str) -> bool {
    let t = Tokens::get();
    let shape = Shape::from_info(info);
    let inner = ui.horizontal(|ui| {
        thumbnail(ui, shape.as_ref(), Vec2::new(22.0, 40.0));
        ui.vertical(|ui| {
            let name = crate::cam_panel::tool_name(&text(&info["name"]));
            let label = match info["number"].as_u64().filter(|_| numbered) {
                Some(n) => format!("T{n} {name}"),
                None => name,
            };
            ui.label(RichText::new(label).color(if selected { t.accent } else { t.text }));
            let kind = KINDS.iter().position(|k| Some(*k) == info["kind"].as_str()).unwrap_or(0);
            let kind = match kind {
                0 => "Flat end mill",
                1 => "Ball end mill",
                k => OTHER_KINDS.get(k - 2).copied().unwrap_or(""),
            };
            let mut line = format!("⌀{} · {} {} · {}", num(&info["diameter"]), tr_label("Flute length"), num(&info["flute_length"]), tr_label(kind));
            if !note.is_empty() {
                line.push_str(" · ");
                line.push_str(note);
            }
            ui.label(RichText::new(line).small().color(t.text_dim));
        });
    });
    let response = ui.interact(inner.response.rect, id, egui::Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_stroke(
            inner.response.rect.expand(2.0),
            3.0,
            Stroke::new(1.0, if selected { t.accent } else { t.text_dim }),
            egui::StrokeKind::Outside,
        );
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Shape, ToolForm};

    #[test]
    fn a_form_reads_a_tool_back_and_gives_its_fields() {
        let info = json!({"kind": "bull_nose", "name": "6 mm R1 bull nose", "diameter": 6.0, "flutes": 4, "flute_length": 18.0, "shank_diameter": 6.0,
                          "stickout": 25.0, "corner_radius": 1.0, "holder": {"name": "ER11", "sections": [[16.0, 12.0], [22.0, 25.0]]},
                          "presets": [{"material": "wood", "feed": 900.0, "stepover": 0.4}]});
        let f = ToolForm::from_info(&info);
        assert_eq!((f.kind, f.corner.as_str(), f.shank.as_str()), (2, "1", ""), "the shank as the cutter stays empty");
        assert!(f.holder && f.sections.len() == 2);
        assert_eq!(f.presets[0][1], "900");
        assert_eq!(f.presets[0][4], "40");
        let p = f.params().unwrap();
        assert_eq!(p["kind"], "bull_nose");
        assert_eq!(p["corner_radius"], 1.0);
        assert_eq!(p["holder"]["sections"][1][1], 25.0);
        assert_eq!(p["presets"][0]["stepover"], 0.4);
        // Without a diameter there's nothing to draw nor to keep.
        let empty = ToolForm::default();
        assert!(empty.shape().is_none() && empty.params().is_err());
        // The shape the drawing takes: a ball's end is round, a V-bit's a cone.
        let mut ball = ToolForm { diameter: "4".into(), kind: 1, ..ToolForm::default() };
        let s = ball.shape().unwrap();
        assert!((s.point() - 2.0).abs() < 1e-9 && s.end(0.0) == 0.0 && (s.end(2.0) - 2.0).abs() < 1e-9);
        ball.kind = 4;
        ball.tip_angle = "90".into();
        let v = ball.shape().unwrap();
        assert!((v.point() - 2.0).abs() < 1e-9 && (v.end(1.0) - 1.0).abs() < 1e-9);
        assert!(Shape::from_info(&json!({"diameter": 0})).is_none());
    }
}
