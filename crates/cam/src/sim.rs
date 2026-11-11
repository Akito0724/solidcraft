//! Stock removal: the stock as a height map (a grid of columns seen from above, each as high as
//! the material left in it), cut down wherever a flat-bottomed cutter passes. For 2.5D work
//! (every cut is made from above) this is exact to the grid's cell size, and it is cheap enough
//! to play a toolpath back move by move.

use crate::toolpath::{P3, Toolpath};

/// Most columns a stock is split into (a 300 × 300 grid).
pub const MAX_CELLS: usize = 90_000;

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
        let ny = ((d / cell).ceil() as usize).clamp(1, MAX_CELLS / nx.max(1));
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
        let (cx, cy) = self.pitch();
        Some(P3::new(self.x0 + i as f64 * cx, self.y0 + j as f64 * cy, h))
    }

    fn pitch(&self) -> (f64, f64) {
        (self.dx, self.dy)
    }

    /// Cut down to `tip.z` every column whose centre is within `radius` of the tip.
    pub fn plunge(&mut self, tip: P3, radius: f64) {
        if !(tip.x.is_finite() && tip.y.is_finite() && tip.z.is_finite() && radius.is_finite()) || tip.z >= self.top {
            return;
        }
        let (cx, cy) = self.pitch();
        let r = radius.max(cx.min(cy) / 2.0);
        let i0 = ((tip.x - r - self.x0) / cx).ceil().max(0.0) as usize;
        let i1 = (((tip.x + r - self.x0) / cx).floor().max(-1.0) + 1.0) as usize;
        let j0 = ((tip.y - r - self.y0) / cy).ceil().max(0.0) as usize;
        let j1 = (((tip.y + r - self.y0) / cy).floor().max(-1.0) + 1.0) as usize;
        let z = tip.z.max(self.bottom);
        for j in j0..j1.min(self.ny) {
            let y = self.y0 + j as f64 * cy - tip.y;
            for i in i0..i1.min(self.nx) {
                let x = self.x0 + i as f64 * cx - tip.x;
                if x * x + y * y <= r * r
                    && let Some(h) = self.heights.get_mut(j * self.nx + i)
                    && *h > z
                {
                    *h = z;
                }
            }
        }
    }

    /// Cut along a straight move from `a` to `b` (in steps of half a column).
    pub fn sweep(&mut self, a: P3, b: P3, radius: f64) {
        if a.z >= self.top && b.z >= self.top {
            return;
        }
        let len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        let n = ((len / (self.dx.min(self.dy) / 2.0)).ceil() as usize).clamp(1, 100_000);
        for k in 0..=n {
            let t = k as f64 / n as f64;
            self.plunge(P3::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t), radius);
        }
    }

    /// The material removed so far (mm³).
    pub fn removed(&self) -> f64 {
        let (cx, cy) = self.pitch();
        self.heights.iter().map(|h| (self.top - h).max(0.0)).sum::<f64>() * cx * cy
    }
}

/// A toolpath as the moves a playback steps through: each point, whether it is reached at a
/// rapid, and the cutter's radius.
#[derive(Clone, Debug)]
pub struct Playback {
    pub radius: f64,
    pub points: Vec<(P3, bool)>,
}

impl Playback {
    /// The toolpath's points, its arcs in pieces no longer than `step` (mm).
    pub fn new(path: &Toolpath, step: f64) -> Playback {
        Playback { radius: path.tool.diameter / 2.0, points: path.points(step) }
    }
}

/// Cut `stock` along `points[from..to]` (each from the one before it; rapids cut nothing).
pub fn cut(stock: &mut Stock, radius: f64, points: &[(P3, bool)], from: usize, to: usize) {
    let to = to.min(points.len());
    for k in from.max(1)..to {
        if let (Some((a, _)), Some((b, rapid))) = (points.get(k - 1), points.get(k))
            && !rapid
        {
            stock.sweep(*a, *b, radius);
        }
    }
}
