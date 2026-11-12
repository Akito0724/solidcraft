//! Stock removal: the stock as a height map (a grid of columns seen from above, each as high as
//! the material left in it), cut down wherever the cutter passes. For 2.5D work (every cut is
//! made from above) this is exact to the grid's cell size, and it is cheap enough to play a
//! toolpath back move by move.
//!
//! A [`Run`] plays a setup's toolpaths against its stock: it cuts, notes where the tool would
//! crash (a rapid into material, the shank rubbing above the flutes), and can go back and forth.
//! [`Stock::shade`] compares what is left with the part, and [`surface`] turns the stock into
//! flat quads (merged where they are alike) for drawing.

use std::collections::HashMap;

use crate::toolpath::{P3, Toolpath};

/// Most columns a stock is split into (about 1400 × 1400).
pub const MAX_CELLS: usize = 2_000_000;

/// Rapids cost this much less travel than cuts in a playback (they are that much quicker).
pub const RAPID_FACTOR: f64 = 10.0;

/// How much the cutter may touch before it counts as touching (mm): rounding, not a crash.
const TOUCH: f64 = 0.02;

/// The shape of a cutter's end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// A flat end.
    Flat,
    /// A hemispherical end (a ball end mill).
    Ball,
    /// A cone, as a drill's point: `tan` is the tangent of its half angle.
    Cone { tan: f64 },
}

/// A cutter as a playback sees it: its radius, its end's shape and how far up its flutes reach.
/// Above the flutes is the shank, as wide as the cutter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cutter {
    pub radius: f64,
    pub shape: Shape,
    pub flute_length: f64,
}

impl Cutter {
    pub fn flat(radius: f64, flute_length: f64) -> Cutter {
        Cutter { radius, shape: Shape::Flat, flute_length }
    }

    /// A twist drill: a 118° point.
    pub fn drill(radius: f64, flute_length: f64) -> Cutter {
        Cutter { radius, shape: Shape::Cone { tan: 59f64.to_radians().tan() }, flute_length }
    }

    pub fn ball(radius: f64, flute_length: f64) -> Cutter {
        Cutter { radius, shape: Shape::Ball, flute_length }
    }

    /// How far above its tip the cutter's end is at `d` from its axis; `None` beyond its radius.
    pub fn lift(&self, d: f64) -> Option<f64> {
        if !(d.is_finite() && d <= self.radius) {
            return None;
        }
        let d = d.max(0.0);
        Some(match self.shape {
            Shape::Flat => 0.0,
            Shape::Ball => self.radius - (self.radius * self.radius - d * d).max(0.0).sqrt(),
            Shape::Cone { tan } if tan > 1e-9 => d / tan,
            Shape::Cone { .. } => 0.0,
        })
    }

    /// The cutter's length from its tip to the top of its point (0 for a flat end).
    pub fn point_length(&self) -> f64 {
        self.lift(self.radius).unwrap_or(0.0)
    }

    fn usable(&self) -> bool {
        self.radius.is_finite() && self.radius > 0.0 && self.flute_length.is_finite()
    }
}

/// The stock as a height map, in machine coordinates.
#[derive(Clone, Debug)]
pub struct Stock {
    /// The centre of the first column.
    pub x0: f64,
    pub y0: f64,
    /// The distance between column centres along X and Y (mm).
    pub dx: f64,
    pub dy: f64,
    pub nx: usize,
    pub ny: usize,
    /// The stock's bottom and its uncut top.
    pub bottom: f64,
    pub top: f64,
    /// Each column's height, row by row (`x` fastest).
    pub heights: Vec<f64>,
}

/// The columns a cutter at `(x, y)` with radius `r` may reach: `i0..i1` × `j0..j1`.
type Window = (usize, usize, usize, usize);

impl Stock {
    /// A block from `min` to `max`, in columns about `cell` wide (wider when it would take more
    /// than [`MAX_CELLS`] of them). `None` for an empty or non-finite block.
    pub fn new(min: [f64; 3], max: [f64; 3], cell: f64) -> Option<Stock> {
        if !min.iter().chain(&max).all(|v| v.is_finite()) || max.iter().zip(&min).any(|(a, b)| a <= b) {
            return None;
        }
        let (w, d) = (max[0] - min[0], max[1] - min[1]);
        let fewest = (w * d / MAX_CELLS as f64).sqrt();
        let cell = if cell.is_finite() && cell > 0.0 { cell.max(fewest) } else { fewest.max(0.5) };
        let nx = ((w / cell).ceil() as usize).clamp(1, MAX_CELLS);
        let ny = ((d / cell).ceil() as usize).clamp(1, (MAX_CELLS / nx.max(1)).max(1));
        let (cx, cy) = (w / nx as f64, d / ny as f64);
        Some(Stock {
            x0: min[0] + cx / 2.0,
            y0: min[1] + cy / 2.0,
            dx: cx,
            dy: cy,
            nx,
            ny,
            bottom: min[2],
            top: max[2],
            heights: vec![max[2]; nx * ny],
        })
    }

    /// The column's centre and height.
    pub fn column(&self, i: usize, j: usize) -> Option<P3> {
        let h = *self.heights.get(j * self.nx + i)?;
        Some(P3::new(self.x0 + i as f64 * self.dx, self.y0 + j as f64 * self.dy, h))
    }

    /// The column's height.
    pub fn height(&self, i: usize, j: usize) -> Option<f64> {
        if i >= self.nx || j >= self.ny {
            return None;
        }
        self.heights.get(j * self.nx + i).copied()
    }

    /// The columns within `r` of the box from `(xa, ya)` to `(xb, yb)`.
    fn window(&self, xa: f64, ya: f64, xb: f64, yb: f64, r: f64) -> Window {
        let lo = |v: f64, o: f64, c: f64| ((v - o) / c).ceil().max(0.0) as usize;
        let hi = |v: f64, o: f64, c: f64, n: usize| ((((v - o) / c).floor().max(-1.0) + 1.0) as usize).min(n);
        (
            lo(xa.min(xb) - r, self.x0, self.dx),
            hi(xa.max(xb) + r, self.x0, self.dx, self.nx),
            lo(ya.min(yb) - r, self.y0, self.dy),
            hi(ya.max(yb) + r, self.y0, self.dy, self.ny),
        )
    }

    /// The radius a cutter is played with: at least half a column, so a fine cutter still cuts.
    fn reach(&self, c: &Cutter) -> Cutter {
        Cutter { radius: c.radius.max(self.dx.min(self.dy) / 2.0), ..*c }
    }

    /// Every column the cutter reaches with its tip at `a` and moving straight to `b` level
    /// (`a.z == b.z`), with how far above the tip its end is there: exact, as a level cutter's
    /// lowest point over a column is where it passes nearest.
    fn level_pass(&self, a: P3, b: P3, c: &Cutter, mut each: impl FnMut(usize, f64) -> bool) {
        let (i0, i1, j0, j1) = self.window(a.x, a.y, b.x, b.y, c.radius);
        let (ux, uy) = (b.x - a.x, b.y - a.y);
        let len2 = ux * ux + uy * uy;
        for j in j0..j1 {
            let y = self.y0 + j as f64 * self.dy;
            for i in i0..i1 {
                let x = self.x0 + i as f64 * self.dx;
                let t = if len2 > 1e-18 { (((x - a.x) * ux + (y - a.y) * uy) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let d = (x - a.x - ux * t).hypot(y - a.y - uy * t);
                if let Some(lift) = c.lift(d)
                    && !each(j * self.nx + i, lift)
                {
                    return;
                }
            }
        }
    }

    /// The tip's positions along a move from `a` to `b`: every half column (just `a` and `b` for
    /// a level move, handled exactly by [`Stock::level_pass`]).
    fn steps(&self, a: P3, b: P3) -> usize {
        let len = (b.x - a.x).hypot(b.y - a.y);
        ((len / (self.dx.min(self.dy) / 2.0)).ceil() as usize).clamp(1, 1_000_000)
    }

    /// Cut along a straight move of the tip from `a` to `b`.
    pub fn sweep(&mut self, a: P3, b: P3, cutter: &Cutter) {
        if !(finite(a) && finite(b) && cutter.usable()) || (a.z >= self.top && b.z >= self.top) {
            return;
        }
        let c = self.reach(cutter);
        let bottom = self.bottom;
        if (a.z - b.z).abs() < 1e-9 {
            let z = a.z;
            let mut heights = std::mem::take(&mut self.heights);
            self.level_pass(a, b, &c, |k, lift| {
                if let Some(h) = heights.get_mut(k) {
                    let to = (z + lift).max(bottom);
                    if *h > to {
                        *h = to;
                    }
                }
                true
            });
            self.heights = heights;
            return;
        }
        let n = self.steps(a, b);
        for k in 0..=n {
            let t = k as f64 / n as f64;
            let p = lerp(a, b, t);
            self.sweep(p, p, &c);
        }
    }

    /// The first place along the tip's move from `a` to `b` where material stands higher than
    /// `above(lift)` over the tip: inside the cutter (`above` = its end), or above its flutes.
    fn first_touch(&self, a: P3, b: P3, cutter: &Cutter, above: impl Fn(f64) -> f64) -> Option<P3> {
        if !(finite(a) && finite(b) && cutter.usable()) {
            return None;
        }
        let c = self.reach(cutter);
        let n = if (a.z - b.z).abs() < 1e-9 { 0 } else { self.steps(a, b) };
        let mut found = None;
        // A column cut down to the bottom holds nothing.
        let floor = self.bottom + 1e-9;
        if n == 0 {
            let z = a.z;
            self.level_pass(a, b, &c, |k, lift| {
                if self.heights.get(k).is_some_and(|h| *h > floor && *h > z + above(lift) + TOUCH) {
                    found = Some(k);
                    return false;
                }
                true
            });
            return found.map(|k| self.at_column(k, a.z));
        }
        for s in 0..=n {
            let p = lerp(a, b, s as f64 / n as f64);
            self.level_pass(p, p, &c, |k, lift| {
                if self.heights.get(k).is_some_and(|h| *h > floor && *h > p.z + above(lift) + TOUCH) {
                    found = Some(k);
                    return false;
                }
                true
            });
            if found.is_some() {
                return Some(p);
            }
        }
        None
    }

    fn at_column(&self, k: usize, z: f64) -> P3 {
        let (i, j) = (k % self.nx.max(1), k / self.nx.max(1));
        P3::new(self.x0 + i as f64 * self.dx, self.y0 + j as f64 * self.dy, z)
    }

    /// Where a move of the tip from `a` to `b` runs the cutter into material (a rapid must not).
    pub fn rapid_hit(&self, a: P3, b: P3, cutter: &Cutter) -> Option<P3> {
        self.first_touch(a, b, cutter, |lift| lift)
    }

    /// Where a cutting move from `a` to `b` meets material above the flutes (the shank rubs).
    pub fn shank_hit(&self, a: P3, b: P3, cutter: &Cutter) -> Option<P3> {
        let flutes = cutter.flute_length.max(cutter.point_length());
        self.first_touch(a, b, cutter, |_| flutes)
    }

    /// The material removed so far (mm³).
    pub fn removed(&self) -> f64 {
        self.heights.iter().map(|h| (self.top - h).max(0.0)).sum::<f64>() * self.dx * self.dy
    }

    /// Back to the uncut block.
    pub fn reset(&mut self) {
        let top = self.top;
        self.heights.iter_mut().for_each(|h| *h = top);
    }

    /// The part's top over every column (`NEG_INFINITY` where the part isn't): the highest of
    /// `triangles` (machine coordinates) over the column's centre.
    pub fn target<I: IntoIterator<Item = [[f64; 3]; 3]>>(&self, triangles: I) -> Vec<f64> {
        let mut out = vec![f64::NEG_INFINITY; self.nx * self.ny];
        for [a, b, c] in triangles {
            if !a.iter().chain(&b).chain(&c).all(|v| v.is_finite()) {
                continue;
            }
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if area.abs() < 1e-12 {
                continue;
            }
            let (i0, i1, j0, j1) =
                self.window(a[0].min(b[0]).min(c[0]), a[1].min(b[1]).min(c[1]), a[0].max(b[0]).max(c[0]), a[1].max(b[1]).max(c[1]), 0.0);
            for j in j0..j1 {
                let y = self.y0 + j as f64 * self.dy;
                for i in i0..i1 {
                    let x = self.x0 + i as f64 * self.dx;
                    // Barycentric weights of (x, y) in the triangle seen from above.
                    let wb = ((x - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (y - a[1])) / area;
                    let wc = ((b[0] - a[0]) * (y - a[1]) - (x - a[0]) * (b[1] - a[1])) / area;
                    let wa = 1.0 - wb - wc;
                    if wa < -1e-9 || wb < -1e-9 || wc < -1e-9 {
                        continue;
                    }
                    let z = wa * a[2] + wb * b[2] + wc * c[2];
                    if let Some(t) = out.get_mut(j * self.nx + i)
                        && z > *t
                    {
                        *t = z;
                    }
                }
            }
        }
        out
    }

    /// How column `(i, j)` compares with the part (`target`, from [`Stock::target`]) within
    /// `tol` (mm). Beside the part's edges a column is measured against its neighbours' part as
    /// well, so a column the grid splits across an edge isn't taken for a gouge or for material
    /// left.
    pub fn shade(&self, target: Option<&[f64]>, i: usize, j: usize, tol: f64) -> Shade {
        let Some(h) = self.height(i, j) else { return Shade::Uncut };
        let plain = if h >= self.top - 1e-9 { Shade::Uncut } else { Shade::Cut };
        let Some(target) = target else { return plain };
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for jj in j.saturating_sub(1)..=(j + 1).min(self.ny - 1) {
            for ii in i.saturating_sub(1)..=(i + 1).min(self.nx - 1) {
                let t = target.get(jj * self.nx + ii).copied().unwrap_or(f64::NEG_INFINITY);
                lo = lo.min(t);
                hi = hi.max(t);
            }
        }
        if hi == f64::NEG_INFINITY {
            return plain;
        }
        if h < lo - tol {
            return Shade::Gouge;
        }
        let left = h - hi;
        if left <= tol { Shade::OnPart } else { Shade::Left(LEFT_STEPS.iter().take_while(|s| left > **s).count().min(3) as u8) }
    }
}

/// Where the shades of material left change (mm above the part): up to 0.2, 0.5, 1, and more.
const LEFT_STEPS: [f64; 3] = [0.2, 0.5, 1.0];

/// What a column of the stock is, compared with the part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shade {
    /// Not cut yet, and no part to compare with.
    Uncut,
    /// Cut, with no part under it.
    Cut,
    /// Down to the part's top, within the tolerance.
    OnPart,
    /// Material left over the part: 0 (a little) to 3 (more than a millimetre).
    Left(u8),
    /// Cut below the part's top.
    Gouge,
}

fn finite(p: P3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}

fn lerp(a: P3, b: P3, t: f64) -> P3 {
    P3::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t)
}

/// A flat quad of the stock's surface, in machine coordinates: its corners counter-clockwise
/// seen from outside, its outward normal, and the shade of the material it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub corners: [P3; 4],
    pub normal: [f64; 3],
    pub shade: Shade,
}

/// The stock's surface as quads: each run of alike columns' tops as one, the walls between
/// columns of different heights, the block's sides and its bottom. `shade` says what each column
/// shows; a wall shows its higher column's shade.
pub fn surface(stock: &Stock, shade: impl Fn(usize, usize) -> Shade) -> Vec<Quad> {
    let (nx, ny) = (stock.nx, stock.ny);
    let mut out = Vec::new();
    let x_at = |i: usize| stock.x0 + (i as f64 - 0.5) * stock.dx;
    let y_at = |j: usize| stock.y0 + (j as f64 - 0.5) * stock.dy;
    let h = |i: usize, j: usize| stock.height(i, j).unwrap_or(stock.bottom);
    // Tops: runs along each row, carried on up while the next row has the same run.
    let mut open: HashMap<(usize, usize, u64, Shade), (usize, usize)> = HashMap::new();
    let close = |out: &mut Vec<Quad>, (i0, i1, hb, s): (usize, usize, u64, Shade), (j0, j1): (usize, usize)| {
        let z = f64::from_bits(hb);
        let (xa, xb, ya, yb) = (x_at(i0), x_at(i1), y_at(j0), y_at(j1));
        out.push(Quad {
            corners: [P3::new(xa, ya, z), P3::new(xb, ya, z), P3::new(xb, yb, z), P3::new(xa, yb, z)],
            normal: [0.0, 0.0, 1.0],
            shade: s,
        });
    };
    for j in 0..ny {
        let mut runs = Vec::new();
        let mut i = 0;
        while i < nx {
            let (z, s) = (h(i, j), shade(i, j));
            let mut k = i + 1;
            while k < nx && h(k, j) == z && shade(k, j) == s {
                k += 1;
            }
            runs.push((i, k, z.to_bits(), s));
            i = k;
        }
        let mut next: HashMap<(usize, usize, u64, Shade), (usize, usize)> = HashMap::with_capacity(runs.len());
        for run in runs {
            let span = match open.remove(&run) {
                Some((j0, _)) => (j0, j + 1),
                None => (j, j + 1),
            };
            next.insert(run, span);
        }
        for (run, span) in open.drain() {
            close(&mut out, run, span);
        }
        open = next;
    }
    for (run, span) in open.drain() {
        close(&mut out, run, span);
    }
    // Walls facing ±X: between columns i − 1 and i of each row (the block's sides at the ends),
    // carried on along Y while alike.
    let wall_x = |out: &mut Vec<Quad>, i: usize| {
        let mut j = 0;
        while j < ny {
            let pair = |j: usize| {
                let left = if i == 0 { stock.bottom } else { h(i - 1, j) };
                let right = if i == nx { stock.bottom } else { h(i, j) };
                let (hi_col, lo, hi) = if left > right { (i.wrapping_sub(1), right, left) } else { (i, left, right) };
                (lo, hi, left > right, if hi > lo { shade(hi_col.min(nx - 1), j) } else { Shade::Uncut })
            };
            let (lo, hi, faces_plus, s) = pair(j);
            let mut k = j + 1;
            while k < ny && pair(k) == (lo, hi, faces_plus, s) {
                k += 1;
            }
            if hi > lo + 1e-12 {
                let (x, ya, yb) = (x_at(i), y_at(j), y_at(k));
                let corners = if faces_plus {
                    [P3::new(x, ya, lo), P3::new(x, yb, lo), P3::new(x, yb, hi), P3::new(x, ya, hi)]
                } else {
                    [P3::new(x, yb, lo), P3::new(x, ya, lo), P3::new(x, ya, hi), P3::new(x, yb, hi)]
                };
                out.push(Quad { corners, normal: [if faces_plus { 1.0 } else { -1.0 }, 0.0, 0.0], shade: s });
            }
            j = k;
        }
    };
    for i in 0..=nx {
        wall_x(&mut out, i);
    }
    // Walls facing ±Y, the same way.
    let wall_y = |out: &mut Vec<Quad>, j: usize| {
        let mut i = 0;
        while i < nx {
            let pair = |i: usize| {
                let front = if j == 0 { stock.bottom } else { h(i, j - 1) };
                let back = if j == ny { stock.bottom } else { h(i, j) };
                let (hi_row, lo, hi) = if front > back { (j.wrapping_sub(1), back, front) } else { (j, front, back) };
                (lo, hi, front > back, if hi > lo { shade(i, hi_row.min(ny - 1)) } else { Shade::Uncut })
            };
            let (lo, hi, faces_plus, s) = pair(i);
            let mut k = i + 1;
            while k < nx && pair(k) == (lo, hi, faces_plus, s) {
                k += 1;
            }
            if hi > lo + 1e-12 {
                let (y, xa, xb) = (y_at(j), x_at(i), x_at(k));
                let corners = if faces_plus {
                    [P3::new(xb, y, lo), P3::new(xa, y, lo), P3::new(xa, y, hi), P3::new(xb, y, hi)]
                } else {
                    [P3::new(xa, y, lo), P3::new(xb, y, lo), P3::new(xb, y, hi), P3::new(xa, y, hi)]
                };
                out.push(Quad { corners, normal: [0.0, if faces_plus { 1.0 } else { -1.0 }, 0.0], shade: s });
            }
            i = k;
        }
    };
    for j in 0..=ny {
        wall_y(&mut out, j);
    }
    let (xa, xb, ya, yb, z) = (x_at(0), x_at(nx), y_at(0), y_at(ny), stock.bottom);
    out.push(Quad {
        corners: [P3::new(xa, ya, z), P3::new(xa, yb, z), P3::new(xb, yb, z), P3::new(xb, ya, z)],
        normal: [0.0, 0.0, -1.0],
        shade: Shade::Uncut,
    });
    out
}

/// A toolpath as a playback steps through it: its name, its cutter, and each point with whether
/// it is reached at a rapid.
#[derive(Clone, Debug)]
pub struct RunOp {
    pub name: String,
    pub cutter: Cutter,
    pub points: Vec<(P3, bool)>,
}

impl RunOp {
    /// A toolpath's points (its arcs in pieces no longer than `step`, mm) with a flat cutter its
    /// tool's size.
    pub fn new(name: String, path: &Toolpath, step: f64) -> RunOp {
        let r = path.tool.diameter / 2.0;
        let cutter = match path.tool.kind {
            crate::tool::ToolKind::Drill => Cutter::drill(r, path.tool.flute_length),
            crate::tool::ToolKind::FlatEndmill => Cutter::flat(r, path.tool.flute_length),
        };
        RunOp { name, cutter, points: path.points(step) }
    }
}

/// How a tool would crash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crash {
    /// A rapid runs the cutter into material.
    Rapid,
    /// Material stands above the flutes while cutting: the shank rubs.
    Shank,
}

/// Where a playback found a crash: the operation, its moves `first..=last` (indices of the
/// points they go to), the first place it touched, and how far into the playback that is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event {
    pub op: usize,
    pub crash: Crash,
    pub first: usize,
    pub last: usize,
    pub at: P3,
    pub travel: f64,
}

/// A setup's toolpaths playing against its stock.
#[derive(Clone, Debug)]
pub struct Run {
    pub stock: Stock,
    pub ops: Vec<RunOp>,
    /// The operation playing and the point it moves to next.
    op: usize,
    next: usize,
    tip: P3,
    /// Travel played so far, and where each operation starts (cutting mm; rapids count a
    /// [`RAPID_FACTOR`]th).
    travelled: f64,
    starts: Vec<f64>,
    total: f64,
    /// The crashes found so far, in the order found.
    pub events: Vec<Event>,
    /// Counts every change to the stock.
    pub version: u64,
}

fn cost(a: P3, b: P3, rapid: bool) -> f64 {
    let d = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2)).sqrt();
    if rapid { d / RAPID_FACTOR } else { d }
}

impl Run {
    pub fn new(stock: Stock, ops: Vec<RunOp>) -> Run {
        let mut starts = Vec::with_capacity(ops.len());
        let mut total = 0.0;
        for op in &ops {
            starts.push(total);
            total += op.points.windows(2).map(|w| if let [(a, _), (b, r)] = w { cost(*a, *b, *r) } else { 0.0 }).sum::<f64>();
        }
        let tip = ops.first().and_then(|o| o.points.first()).map_or(P3::new(0.0, 0.0, 0.0), |p| p.0);
        Run { stock, ops, op: 0, next: 1, tip, travelled: 0.0, starts, total, events: Vec::new(), version: 0 }
    }

    pub fn done(&self) -> bool {
        self.op >= self.ops.len()
    }

    /// The tool's tip, in machine coordinates.
    pub fn tip(&self) -> P3 {
        self.tip
    }

    /// The operation playing (the last one once done).
    pub fn current(&self) -> Option<&RunOp> {
        self.ops.get(self.op).or_else(|| self.ops.last())
    }

    pub fn op_index(&self) -> usize {
        self.op.min(self.ops.len().saturating_sub(1))
    }

    pub fn travelled(&self) -> f64 {
        self.travelled
    }

    pub fn total(&self) -> f64 {
        self.total
    }

    /// Where operation `k` starts in the playback's travel.
    pub fn start_of(&self, k: usize) -> f64 {
        self.starts.get(k).copied().unwrap_or(self.total)
    }

    /// Note a crash on the move to point `next` of operation `op`; true for a new one (not the
    /// next move of one already found).
    fn note(&mut self, crash: Crash, at: P3) -> bool {
        let (op, next) = (self.op, self.next);
        if let Some(e) = self.events.iter_mut().find(|e| e.op == op && e.crash == crash && e.first <= next && next <= e.last + 1) {
            e.last = e.last.max(next);
            return false;
        }
        self.events.push(Event { op, crash, first: next, last: next, at, travel: self.travelled });
        true
    }

    /// Play `budget` travel (`f64::INFINITY`: to the end). With `halt`, stop after the move
    /// where a new crash is found; true when one was.
    pub fn advance(&mut self, mut budget: f64, halt: bool) -> bool {
        let mut found = false;
        while budget > 0.0 {
            let Some(op) = self.ops.get(self.op) else { return found };
            let Some(&(to, rapid)) = op.points.get(self.next) else {
                // The next operation starts at its first point.
                self.op += 1;
                self.next = 1;
                if let Some((p, _)) = self.ops.get(self.op).and_then(|o| o.points.first()) {
                    self.tip = *p;
                }
                continue;
            };
            let cutter = op.cutter;
            let step = cost(self.tip, to, rapid);
            let reach = if step <= budget {
                budget -= step;
                self.travelled += step;
                to
            } else {
                let t = if step > 0.0 { budget / step } else { 1.0 };
                self.travelled += budget;
                budget = 0.0;
                lerp(self.tip, to, t)
            };
            let hit = if rapid {
                self.stock.rapid_hit(self.tip, reach, &cutter).map(|p| (Crash::Rapid, p))
            } else {
                self.stock.shank_hit(self.tip, reach, &cutter).map(|p| (Crash::Shank, p))
            };
            let new = hit.is_some_and(|(crash, at)| self.note(crash, at));
            // A rapid that crashes cuts too: the material is gone after it.
            if !rapid || hit.is_some() {
                self.stock.sweep(self.tip, reach, &cutter);
                self.version += 1;
            }
            self.tip = reach;
            if reach == to {
                self.next += 1;
            }
            if new {
                found = true;
                if halt {
                    return true;
                }
            }
        }
        found
    }

    /// Back to the uncut stock and the first point.
    pub fn restart(&mut self) {
        self.stock.reset();
        self.op = 0;
        self.next = 1;
        self.travelled = 0.0;
        self.tip = self.ops.first().and_then(|o| o.points.first()).map_or(self.tip, |p| p.0);
        self.version += 1;
    }

    /// Play to `travel` from the start (going back starts over), without halting.
    pub fn seek(&mut self, travel: f64) {
        let travel = if travel.is_finite() { travel.clamp(0.0, self.total) } else { self.total };
        if travel < self.travelled - 1e-9 {
            self.restart();
        }
        let budget = travel - self.travelled;
        if budget > 0.0 {
            self.advance(budget, false);
        }
    }

    /// Play to the end of the move under way (the next point).
    pub fn step(&mut self) {
        let Some(op) = self.ops.get(self.op) else { return };
        let budget = match op.points.get(self.next) {
            Some(&(to, rapid)) => cost(self.tip, to, rapid).max(1e-9),
            None => 1e-9,
        };
        self.advance(budget + 1e-12, false);
    }
}
