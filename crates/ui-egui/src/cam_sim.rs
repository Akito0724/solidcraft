//! Machining simulation for the Machining window: the setup's stock as a height map
//! (`machining::Stock`), cut down along its toolpaths (`cam.simulation`) as they play back, and
//! drawn over the model as a shaded block.

use egui::{Color32, Pos2};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::machining::{Frame, P3, Playback, Stock};
use solidcraft_theme::Tokens;

use crate::SolveApp;

/// Playback speeds: cutting travel per second (mm/s). Real feeds are 5–20 mm/s.
pub const SPEEDS: [f64; 4] = [25.0, 100.0, 400.0, 2000.0];
pub const SPEED_LABELS: [&str; 4] = ["×1", "×4", "×16", "×80"];
/// Rapids play this many times faster than cuts.
const RAPID_FACTOR: f64 = 10.0;
/// The finest the stock is drawn: at most this many columns a side.
const DRAWN_SIDE: usize = 160;

/// A playback of a setup's toolpaths against its stock.
pub struct Sim {
    pub setup: u64,
    /// The design revision it was made for; a change ends it.
    pub revision: u64,
    frame: Frame,
    pub stock: Stock,
    ops: Vec<(String, Playback)>,
    /// The operation playing, the next point it moves to, and the tool's tip.
    op: usize,
    next: usize,
    tip: P3,
    pub playing: bool,
    pub speed: usize,
}

impl Sim {
    /// A playback from `cam.simulation`'s reply, the stock uncut and the tool at its first point.
    pub fn from_reply(v: &Value, revision: u64) -> Option<Sim> {
        let arr3 = |v: &Value| -> Option<[f64; 3]> { Some([v.get(0)?.as_f64()?, v.get(1)?.as_f64()?, v.get(2)?.as_f64()?]) };
        let frame = Frame { origin: arr3(&v["origin"])?, flipped: v["flipped"].as_bool().unwrap_or(false) };
        let stock = Stock::new(arr3(&v["stock_min"])?, arr3(&v["stock_max"])?, 0.25)?;
        let ops: Vec<(String, Playback)> = v["operations"]
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
                (op["name"].as_str().unwrap_or_default().to_string(), Playback { radius: op["radius"].as_f64().unwrap_or(1.5), points })
            })
            .filter(|(_, p)| !p.points.is_empty())
            .collect();
        let tip = ops.first().and_then(|(_, p)| p.points.first()).map_or(P3::new(0.0, 0.0, stock.top + 10.0), |(p, _)| *p);
        Some(Sim { setup: v["setup"].as_u64().unwrap_or(0), revision, frame, stock, ops, op: 0, next: 1, tip, playing: true, speed: 1 })
    }

    pub fn done(&self) -> bool {
        self.op >= self.ops.len()
    }

    /// The operation playing and how far through the whole playback it is (0–1).
    pub fn progress(&self) -> (String, f64) {
        let total: usize = self.ops.iter().map(|(_, p)| p.points.len()).sum::<usize>().max(1);
        let before: usize = self.ops.iter().take(self.op).map(|(_, p)| p.points.len()).sum();
        let name = self.ops.get(self.op).map_or_else(String::new, |(n, _)| n.clone());
        (name, ((before + self.next.min(total)) as f64 / total as f64).min(1.0))
    }

    /// Play `budget` mm of cutting travel (rapids cost a tenth of theirs). `f64::INFINITY` plays
    /// to the end.
    pub fn advance(&mut self, mut budget: f64) {
        while budget > 0.0 {
            let Some((_, play)) = self.ops.get(self.op) else {
                self.playing = false;
                return;
            };
            let Some(&(to, rapid)) = play.points.get(self.next) else {
                // The next operation starts at its first point.
                self.op += 1;
                self.next = 1;
                if let Some((p, _)) = self.ops.get(self.op).and_then(|(_, p)| p.points.first()) {
                    self.tip = *p;
                }
                continue;
            };
            let radius = play.radius;
            let d = ((to.x - self.tip.x).powi(2) + (to.y - self.tip.y).powi(2) + (to.z - self.tip.z).powi(2)).sqrt();
            let cost = if rapid { d / RAPID_FACTOR } else { d };
            let reach = if cost <= budget {
                self.next += 1;
                budget -= cost;
                to
            } else {
                let t = budget / cost;
                budget = 0.0;
                P3::new(self.tip.x + (to.x - self.tip.x) * t, self.tip.y + (to.y - self.tip.y) * t, self.tip.z + (to.z - self.tip.z) * t)
            };
            if !rapid {
                self.stock.sweep(self.tip, reach, radius);
            }
            self.tip = reach;
        }
    }

    /// Back to the uncut stock.
    pub fn restart(&mut self) {
        self.stock.heights.iter_mut().for_each(|h| *h = self.stock.top);
        self.op = 0;
        self.next = 1;
        if let Some((p, _)) = self.ops.first().and_then(|(_, p)| p.points.first()) {
            self.tip = *p;
        }
        self.playing = true;
    }

    fn model(&self, p: P3) -> Vec3 {
        let m = self.frame.to_model([p.x, p.y, p.z]);
        Vec3::new(m[0], m[1], m[2])
    }

    fn radius(&self) -> f64 {
        self.ops.get(self.op).map_or(1.5, |(_, p)| p.radius)
    }
}

/// Start a playback of the setup's toolpaths.
pub fn start(app: &mut SolveApp, setup: u64) -> Result<Sim, String> {
    let v = app.session.execute("cam.simulation", &json!({"setup": setup})).map_err(|e| e.to_string())?;
    Sim::from_reply(&v, app.session.revision).ok_or_else(|| solidcraft_ui_text::tr("cam.panel.nothing_to_simulate").to_string())
}

/// Move the playback on by the frame's time, and draw it: the stock as a shaded block (its
/// faces sorted far to near), and the tool as a short upright bar at its tip.
pub fn paint(sim: &mut Sim, painter: &egui::Painter, proj: &crate::viewport::Proj) {
    if sim.playing && !sim.done() {
        let dt = painter.ctx().input(|i| f64::from(i.stable_dt)).clamp(0.0, 0.1);
        sim.advance(SPEEDS.get(sim.speed).copied().unwrap_or(100.0) * dt);
        painter.ctx().request_repaint();
    }
    let t = Tokens::get();
    let s = &sim.stock;
    let stride = s.nx.max(s.ny).div_ceil(DRAWN_SIDE).max(1);
    let xs: Vec<usize> = (0..s.nx).step_by(stride).chain(std::iter::once(s.nx - 1)).collect();
    let ys: Vec<usize> = (0..s.ny).step_by(stride).chain(std::iter::once(s.ny - 1)).collect();
    // The lowest column in each drawn cell, so a narrow slot still shows.
    let low = |i0: usize, j0: usize| -> f64 {
        let mut h = f64::MAX;
        for j in j0..(j0 + stride).min(s.ny) {
            for i in i0..(i0 + stride).min(s.nx) {
                h = h.min(s.heights.get(j * s.nx + i).copied().unwrap_or(s.top));
            }
        }
        h
    };
    let grid: Vec<Vec<P3>> =
        ys.iter().map(|&j| xs.iter().map(|&i| s.column(i, j).map_or(P3::new(0.0, 0.0, s.top), |c| P3::new(c.x, c.y, low(i, j)))).collect()).collect();
    let mut quads: Vec<(f64, [Pos2; 4], Color32)> = Vec::new();
    let uncut = t.surface_fill;
    let cut = uncut.lerp_to_gamma(t.accent, 0.45);
    let light = {
        let l: [f64; 3] = [-0.35, -0.45, 0.82];
        let n: f64 = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        [l[0] / n, l[1] / n, l[2] / n]
    };
    let mut add = |corners: [P3; 4], base: Color32| {
        let m = corners.map(|p| sim.model(p));
        let (Some(a), Some(b), Some(c), Some(d)) = (proj.to_screen(m[0]), proj.to_screen(m[1]), proj.to_screen(m[2]), proj.to_screen(m[3])) else {
            return;
        };
        let u = [m[1].x - m[0].x, m[1].y - m[0].y, m[1].z - m[0].z];
        let v = [m[3].x - m[0].x, m[3].y - m[0].y, m[3].z - m[0].z];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        let shade = if len > 1e-12 { ((n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / len).abs() } else { 1.0 };
        let depth = m.iter().map(|p| proj.vp.apply(*p)).map(|c| if c[3].abs() > 1e-12 { c[2] / c[3] } else { 0.0 }).sum::<f64>() / 4.0;
        quads.push((depth, [a, b, c, d], base.gamma_multiply((0.45 + 0.55 * shade) as f32).to_opaque()));
    };
    for (r, row) in grid.iter().enumerate().skip(1) {
        let Some(prev) = grid.get(r - 1) else { continue };
        for k in 1..row.len() {
            let (Some(p00), Some(p10), Some(p11), Some(p01)) = (prev.get(k - 1), prev.get(k), row.get(k), row.get(k - 1)) else { continue };
            let deepest = [p00.z, p10.z, p11.z, p01.z].into_iter().fold(f64::MAX, f64::min);
            add([*p00, *p10, *p11, *p01], if deepest < s.top - 1e-6 { cut } else { uncut });
        }
    }
    // The four sides, from the bottom up to the columns along each edge.
    let bottom = s.bottom;
    let sides: [Vec<P3>; 4] = [
        grid.first().cloned().unwrap_or_default(),
        grid.last().cloned().unwrap_or_default(),
        grid.iter().filter_map(|row| row.first().copied()).collect(),
        grid.iter().filter_map(|row| row.last().copied()).collect(),
    ];
    for edge in &sides {
        for w in edge.windows(2) {
            if let [a, b] = w {
                add([P3::new(a.x, a.y, bottom), P3::new(b.x, b.y, bottom), *b, *a], uncut);
            }
        }
    }
    // Far to near.
    quads.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut mesh = egui::Mesh::default();
    for (_, [a, b, c, d], color) in quads {
        let base = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
        for p in [a, b, c, d] {
            mesh.colored_vertex(p, color);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
    // The tool: an upright bar from its tip, as wide as it looks.
    let tip = sim.model(sim.tip);
    let top = sim.model(P3::new(sim.tip.x, sim.tip.y, sim.tip.z + 25.0));
    let side = sim.model(P3::new(sim.tip.x + sim.radius(), sim.tip.y, sim.tip.z));
    if let (Some(a), Some(b), Some(r)) = (proj.to_screen(tip), proj.to_screen(top), proj.to_screen(side)) {
        let width = (r - a).length().max(2.0) * 2.0;
        painter.line_segment([a, b], egui::Stroke::new(width, t.text_dim.gamma_multiply(0.85)));
        painter.circle_filled(a, width / 2.0, t.accent);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

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
        assert!(!sim.done() && sim.stock.removed() == 0.0);
        sim.advance(50.0);
        let some = sim.stock.removed();
        assert!(some > 0.0, "50 mm of facing removes material");
        assert!(sim.progress().1 > 0.0 && sim.progress().1 < 1.0);
        sim.advance(f64::INFINITY);
        assert!(sim.done() && !sim.playing);
        // Facing takes the 1 mm margin off the whole top: about 66 × 46 × 1 mm.
        let all = sim.stock.removed();
        assert!(all > some && (all - 66.0 * 46.0).abs() < 66.0 * 46.0 * 0.2, "{all}");
        sim.restart();
        assert_eq!(sim.stock.removed(), 0.0);
        assert!(Sim::from_reply(&json!({}), 0).is_none(), "a reply without a stock plays nothing");
    }
}
