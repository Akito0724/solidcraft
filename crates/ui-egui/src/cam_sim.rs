//! Machining simulation for the Manufacture workspace: the setup's stock as a height map
//! (`machining::Stock`), cut down along its toolpaths (`cam.simulation`) as they play back
//! (`machining::Run`), and drawn in the model scene as a lit block, coloured by how it compares
//! with the part. The tool and any crash are drawn over the view.

use egui::{Color32, Pos2};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::machining::{Crash, Cutter, Frame, P3, Run, RunOp, Shade, Stock, surface};
use solidcraft_theme::Tokens;

use crate::SolveApp;

/// Playback speeds: cutting travel per second (mm/s). Real feeds are 5–20 mm/s.
pub const SPEEDS: [f64; 4] = [25.0, 100.0, 400.0, 2000.0];
pub const SPEED_LABELS: [&str; 4] = ["×1", "×4", "×16", "×80"];
/// The width of the stock's columns (mm): finer when the stock is small enough.
const CELL: f64 = 0.2;
/// How often the stock is drawn again while it plays (s).
const REDRAW: f64 = 0.2;
/// Within this of the part's top a cut counts as down to it (mm).
pub const TOLERANCE: f64 = 0.05;
/// How much of the shank is drawn above the flutes (mm).
const SHANK: f64 = 25.0;

pub struct Sim {
    pub setup: u64,
    /// The design revision it was made for; a change ends it.
    pub revision: u64,
    /// The body the setup machines: hidden while the stock plays.
    pub body: String,
    frame: Frame,
    pub run: Run,
    /// The part's top over each column (from its display mesh), for the colours.
    target: Option<Vec<f64>>,
    /// Colour the stock by how it compares with the part.
    pub compare: bool,
    pub playing: bool,
    pub speed: usize,
    /// The stock's version the model scene draws, and when it last changed.
    pub shown: u64,
    shown_at: f64,
    /// The crash (an index into `run.events`) that stopped the playback.
    pub halted: Option<usize>,
}

fn arr3(v: &Value) -> Option<[f64; 3]> {
    Some([v.get(0)?.as_f64()?, v.get(1)?.as_f64()?, v.get(2)?.as_f64()?])
}

impl Sim {
    /// A playback from `cam.simulation`'s reply, the stock uncut and the tool at its first
    /// point; `part` is the part's display mesh in model coordinates, to compare with.
    pub fn from_reply(v: &Value, revision: u64, part: Option<&solidcraft_engine::geom::Mesh>) -> Option<Sim> {
        let f = &v["frame"];
        let frame = Frame { origin: arr3(&f["origin"])?, x: arr3(&f["x"])?, y: arr3(&f["y"])?, z: arr3(&f["z"])? };
        let stock = Stock::new(arr3(&v["stock_min"])?, arr3(&v["stock_max"])?, CELL)?;
        let ops: Vec<RunOp> = v["operations"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|op| {
                let points = op["points"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| Some((P3::new(p.get(0)?.as_f64()?, p.get(1)?.as_f64()?, p.get(2)?.as_f64()?), p.get(3)?.as_f64()? > 0.5)))
                    .collect();
                let radius = op["radius"].as_f64().unwrap_or(1.5);
                let flutes = op["flute_length"].as_f64().unwrap_or(f64::INFINITY);
                let cutter = if op["cutter"] == "drill" { Cutter::drill(radius, flutes) } else { Cutter::flat(radius, flutes) };
                RunOp { name: op["name"].as_str().unwrap_or_default().to_string(), cutter, points }
            })
            .filter(|op: &RunOp| op.points.len() > 1)
            .collect();
        if ops.is_empty() {
            return None;
        }
        let target = part.map(|mesh| {
            let machine = |i: u32| mesh.positions.get(i as usize).map(|p| frame.to_machine([p.x, p.y, p.z]));
            stock.target(mesh.triangles.iter().filter_map(|t| Some([machine(t[0])?, machine(t[1])?, machine(t[2])?])))
        });
        Some(Sim {
            setup: v["setup"].as_u64().unwrap_or(0),
            revision,
            body: v["body"].as_str().unwrap_or_default().to_string(),
            frame,
            run: Run::new(stock, ops),
            compare: target.is_some(),
            target,
            playing: true,
            speed: 1,
            shown: 0,
            shown_at: f64::NEG_INFINITY,
            halted: None,
        })
    }

    pub fn done(&self) -> bool {
        self.run.done()
    }

    /// The operation playing and how far through the whole playback it is (0–1).
    pub fn progress(&self) -> (String, f64) {
        let name = self.run.current().map_or_else(String::new, |o| o.name.clone());
        let total = self.run.total();
        (name, if total > 0.0 { (self.run.travelled() / total).clamp(0.0, 1.0) } else { 1.0 })
    }

    /// Play `budget` mm of cutting travel without stopping at crashes (`f64::INFINITY`: to the
    /// end).
    pub fn advance(&mut self, budget: f64) {
        self.run.advance(budget, false);
        self.halted = None;
        if self.run.done() {
            self.playing = false;
        }
    }

    /// Go to `travel` into the playback (paused).
    pub fn seek(&mut self, travel: f64) {
        self.run.seek(travel);
        self.halted = None;
        self.playing = false;
    }

    /// Back to the uncut stock, playing.
    pub fn restart(&mut self) {
        self.run.restart();
        self.halted = None;
        self.playing = true;
    }

    /// Move the playback on by `dt` seconds of play, halting at a new crash.
    fn tick(&mut self, dt: f64) {
        if !self.playing || self.run.done() {
            self.playing = false;
            return;
        }
        if self.run.advance(SPEEDS.get(self.speed).copied().unwrap_or(100.0) * dt, true) {
            self.playing = false;
            self.halted = self.run.events.len().checked_sub(1);
        }
        if self.run.done() {
            self.playing = false;
        }
    }

    /// Pass the stock as it is to the model scene: at once when paused, every [`REDRAW`] while
    /// it plays.
    fn publish(&mut self, now: f64) {
        if self.shown != self.run.version && (!self.playing || now - self.shown_at >= REDRAW) {
            self.shown = self.run.version;
            self.shown_at = now;
        }
    }

    fn model(&self, p: P3) -> Vec3 {
        let m = self.frame.to_model([p.x, p.y, p.z]);
        Vec3::new(m[0], m[1], m[2])
    }
}

/// Start a playback of the setup's toolpaths.
pub fn start(app: &mut SolveApp, setup: u64) -> Result<Sim, String> {
    let v = app.session.execute("cam.simulation", &json!({"setup": setup})).map_err(|e| e.to_string())?;
    let world = crate::open_meshing::session_world(&app.session);
    let part = v["body"].as_str().and_then(|name| world.bodies.iter().find(|b| b.name == name)).map(|b| b.mesh());
    Sim::from_reply(&v, app.session.revision, part.as_deref()).ok_or_else(|| solidcraft_ui_text::tr("cam.panel.nothing_to_simulate").to_string())
}

/// The colour of a shade of the stock.
pub fn shade_color(shade: Shade) -> Color32 {
    let t = Tokens::get();
    match shade {
        Shade::Uncut => t.surface_fill,
        Shade::Cut => t.surface_fill.lerp_to_gamma(t.accent, 0.35),
        Shade::OnPart => t.agent_ok,
        Shade::Left(k) => t.warning.lerp_to_gamma(t.error, f32::from(k.min(3)) / 3.0),
        Shade::Gouge => t.interference,
    }
}

/// The stock in the model scene: its surface as lit triangles, coloured by [`shade_color`].
pub(crate) fn scene(sc: &mut crate::gpu::GpuScene, sim: &Sim) {
    let s = &sim.run.stock;
    let target = if sim.compare { sim.target.as_deref() } else { None };
    let shades: Vec<Shade> = (0..s.ny).flat_map(|j| (0..s.nx).map(move |i| (i, j))).map(|(i, j)| s.shade(target, i, j, TOLERANCE)).collect();
    let quads = surface(s, |i, j| shades.get(j * s.nx + i).copied().unwrap_or(Shade::Uncut));
    let mut colors: Vec<(Shade, crate::gpu::PaletteIndex)> = Vec::new();
    for q in quads {
        let c = match colors.iter().find(|(s, _)| *s == q.shade) {
            Some((_, c)) => *c,
            None => {
                let c = sc.color(crate::viewport::c4(shade_color(q.shade)));
                colors.push((q.shade, c));
                c
            }
        };
        let n = sim.frame.dir_to_model(q.normal).map(|v| v as f32);
        let p = q.corners.map(|c| sim.model(c).to_f32());
        let [a, b, c2, d] = p;
        for v in [a, b, c2, a, c2, d] {
            sc.tri(v, n, c);
        }
    }
}

/// The convex hull of `points`, counter-clockwise (Andrew's monotone chain).
fn hull(mut points: Vec<Pos2>) -> Vec<Pos2> {
    points.retain(|p| p.x.is_finite() && p.y.is_finite());
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let turn = |o: Pos2, a: Pos2, b: Pos2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut out: Vec<Pos2> = Vec::with_capacity(points.len() * 2);
    for pass in 0..2 {
        let start = out.len();
        let it: Box<dyn Iterator<Item = &Pos2>> = if pass == 0 { Box::new(points.iter()) } else { Box::new(points.iter().rev()) };
        for &p in it {
            while out.len() >= start + 2 {
                let (Some(&a), Some(&b)) = (out.get(out.len() - 2), out.last()) else { break };
                if turn(a, b, p) <= 0.0 {
                    out.pop();
                } else {
                    break;
                }
            }
            out.push(p);
        }
        out.pop();
    }
    out
}

/// Move the playback on by the frame's time, and draw over the view the tool (its flutes and
/// shank) and where it crashed.
pub fn paint(sim: &mut Sim, painter: &egui::Painter, proj: &crate::viewport::Proj) {
    let (dt, now) = painter.ctx().input(|i| (f64::from(i.stable_dt).clamp(0.0, 0.1), i.time));
    sim.tick(dt);
    sim.publish(now);
    if sim.playing || sim.shown != sim.run.version {
        painter.ctx().request_repaint();
    }
    let t = Tokens::get();
    let Some(op) = sim.run.current() else { return };
    let c = op.cutter;
    let tip = sim.run.tip();
    let ring = |z: f64| -> Vec<Pos2> {
        (0..24)
            .filter_map(|k| {
                let a = f64::from(k) * std::f64::consts::TAU / 24.0;
                proj.to_screen(sim.model(P3::new(tip.x + c.radius * a.cos(), tip.y + c.radius * a.sin(), tip.z + z)))
            })
            .collect()
    };
    let point = c.point_length();
    let flutes = if c.flute_length.is_finite() { c.flute_length.max(point) } else { 20.0 };
    let shank = hull([ring(flutes), ring(flutes + SHANK)].concat());
    let mut cutting = [ring(point), ring(flutes)].concat();
    if point > 0.0
        && let Some(apex) = proj.to_screen(sim.model(tip))
    {
        cutting.push(apex);
    }
    let cutting = hull(cutting);
    if shank.len() >= 3 {
        painter.add(egui::Shape::convex_polygon(shank, t.text_dim.gamma_multiply(0.8), egui::Stroke::new(1.0, t.text_dim)));
    }
    if cutting.len() >= 3 {
        painter.add(egui::Shape::convex_polygon(cutting, t.accent.gamma_multiply(0.85), egui::Stroke::new(1.0, t.accent)));
    }
    // Where it crashed: a ring, the one that stopped the playback filled.
    for (k, e) in sim.run.events.iter().enumerate() {
        if let Some(at) = proj.to_screen(sim.model(e.at)) {
            if sim.halted == Some(k) {
                painter.circle_filled(at, 6.0, t.error.gamma_multiply(0.6));
            }
            painter.circle_stroke(at, 8.0, egui::Stroke::new(2.0, t.error));
        }
    }
}

/// A crash in words: the operation, what happened, and where (machine coordinates).
pub fn crash_text(sim: &Sim, k: usize) -> Option<String> {
    let e = sim.run.events.get(k)?;
    let name = sim.run.ops.get(e.op).map_or("", |o| o.name.as_str());
    let id = match e.crash {
        Crash::Rapid => "cam.sim.rapid_crash",
        Crash::Shank => "cam.sim.shank_crash",
    };
    let (x, y, z) = (format!("{:.1}", e.at.x), format!("{:.1}", e.at.y), format!("{:.1}", e.at.z));
    let text = solidcraft_ui_text::format(id, &[("x", &x), ("y", &y), ("z", &z)]);
    Some(format!("{}: {text}", crate::cam_panel::op_name(name)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcraft_engine::machining::Shade;

    use super::Sim;
    use crate::scenario::Harness;

    #[test]
    fn a_playback_cuts_the_stock_step_by_step_and_restarts_uncut() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 8})).unwrap();
        h.app.run("cam.tool", json!({"number": 1, "diameter": 3.175})).unwrap();
        h.app.run("cam.setup", json!({})).unwrap();
        h.app.run("cam.face", json!({"tool": 1})).unwrap();
        let setup = h.app.session.doc.cam.setups.first().map(|s| s.id).unwrap();
        let mut sim = super::start(&mut h.app, setup).unwrap();
        assert!(!sim.done() && sim.run.stock.removed() == 0.0);
        assert!(sim.compare, "the part is there to compare with");
        sim.advance(50.0);
        let some = sim.run.stock.removed();
        assert!(some > 0.0, "50 mm of facing removes material");
        assert!(sim.progress().1 > 0.0 && sim.progress().1 < 1.0);
        // Before facing is done, the top over the part still has its 1 mm margin on it.
        let s = &sim.run.stock;
        let (i, j) = (s.nx - 2, s.ny / 2);
        assert!(matches!(s.shade(sim.target.as_deref(), s.nx / 2, s.ny / 2, super::TOLERANCE), Shade::Left(_) | Shade::OnPart));
        assert_ne!(s.shade(sim.target.as_deref(), i, j, super::TOLERANCE), Shade::Gouge);
        sim.advance(f64::INFINITY);
        assert!(sim.done() && !sim.playing);
        assert!(sim.run.events.is_empty(), "facing crashes nowhere: {:?}", sim.run.events);
        // Facing takes the 1 mm margin off the whole top: about 66 × 46 × 1 mm, down to the part.
        let all = sim.run.stock.removed();
        assert!(all > some && (all - 66.0 * 46.0).abs() < 66.0 * 46.0 * 0.2, "{all}");
        let s = &sim.run.stock;
        assert_eq!(s.shade(sim.target.as_deref(), s.nx / 2, s.ny / 2, super::TOLERANCE), Shade::OnPart);
        // Its surface goes to the model scene as triangles.
        let mut sc = crate::gpu::GpuScene::default();
        super::scene(&mut sc, &sim);
        assert!(!sc.tris.is_empty());
        let mid = sim.run.total() / 2.0;
        sim.seek(mid);
        assert!(!sim.playing && (sim.run.travelled() - mid).abs() < 1e-6);
        sim.restart();
        assert_eq!(sim.run.stock.removed(), 0.0);
        assert!(Sim::from_reply(&json!({}), 0, None).is_none(), "a reply without a stock plays nothing");
    }

    #[test]
    fn the_hull_of_points_is_their_outline() {
        let p = |x: f32, y: f32| egui::pos2(x, y);
        let h = super::hull(vec![p(0.0, 0.0), p(2.0, 0.0), p(1.0, 1.0), p(2.0, 2.0), p(0.0, 2.0), p(1.0, 0.0)]);
        assert_eq!(h.len(), 4, "{h:?}");
        assert!(!h.contains(&p(1.0, 1.0)));
        assert!(super::hull(vec![p(f32::NAN, 0.0), p(1.0, 1.0)]).len() <= 1);
    }
}
