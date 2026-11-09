//! Closed 2D profiles of lines and arcs: what a pocket's boundary, an island or a contour is.
//!
//! A vertex carries the bulge of the segment that starts at it: 0 for a line, `tan(sweep / 4)`
//! for an arc, positive counter-clockwise. Offsets go through `cavalier_contours`, which keeps
//! arcs as arcs.

use cavalier_contours::polyline::{PlineCreation, PlineSource, PlineSourceMut, PlineVertex, Polyline};
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};

/// One vertex of a loop and the bulge of the segment that starts there.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
    #[cfg_attr(feature = "serde", serde(default))]
    pub bulge: f64,
}

/// A closed profile: the last vertex joins the first.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Loop {
    pub vertices: Vec<Vertex>,
}

/// An area to clear: an outer boundary and the islands inside it that stay.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Region {
    pub outer: Loop,
    #[cfg_attr(feature = "serde", serde(default))]
    pub islands: Vec<Loop>,
}

/// One segment of a loop, ready to follow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub bulge: f64,
}

impl Segment {
    /// The arc's centre and direction (`true`: counter-clockwise); `None` for a line.
    pub fn arc(&self) -> Option<((f64, f64), bool)> {
        if self.bulge.abs() < 1e-9 {
            return None;
        }
        let (dx, dy) = (self.to.0 - self.from.0, self.to.1 - self.from.1);
        let mid = ((self.from.0 + self.to.0) / 2.0, (self.from.1 + self.to.1) / 2.0);
        let k = (1.0 - self.bulge * self.bulge) / (4.0 * self.bulge);
        Some(((mid.0 - dy * k, mid.1 + dx * k), self.bulge > 0.0))
    }

    /// The segment's length along its line or arc.
    pub fn length(&self) -> f64 {
        let chord = (self.to.0 - self.from.0).hypot(self.to.1 - self.from.1);
        if self.bulge.abs() < 1e-9 || chord < 1e-12 {
            return chord;
        }
        let sweep = 4.0 * self.bulge.atan();
        let radius = chord / (2.0 * (sweep / 2.0).sin().abs());
        sweep.abs() * radius
    }

    /// The point a fraction `t` (0 to 1) of the way along the segment.
    pub fn at(&self, t: f64) -> (f64, f64) {
        match self.arc() {
            None => (self.from.0 + (self.to.0 - self.from.0) * t, self.from.1 + (self.to.1 - self.from.1) * t),
            Some((c, _)) => {
                let sweep = 4.0 * self.bulge.atan() * t;
                let (sx, sy) = (self.from.0 - c.0, self.from.1 - c.1);
                let (s, co) = sweep.sin_cos();
                (c.0 + sx * co - sy * s, c.1 + sx * s + sy * co)
            }
        }
    }

    /// The part of the segment between fractions `t0` and `t1` of the way along it.
    pub fn between(&self, t0: f64, t1: f64) -> Segment {
        let sweep = 4.0 * self.bulge.atan() * (t1 - t0);
        Segment { from: self.at(t0), to: self.at(t1), bulge: if self.bulge.abs() < 1e-9 { 0.0 } else { (sweep / 4.0).tan() } }
    }
}

impl Loop {
    /// A loop through `points` with straight sides.
    pub fn polygon(points: &[(f64, f64)]) -> Loop {
        Loop { vertices: points.iter().map(|&(x, y)| Vertex { x, y, bulge: 0.0 }).collect() }
    }

    /// A circle, counter-clockwise, as two half arcs.
    pub fn circle(cx: f64, cy: f64, r: f64) -> Loop {
        Loop { vertices: vec![Vertex { x: cx - r, y: cy, bulge: 1.0 }, Vertex { x: cx + r, y: cy, bulge: 1.0 }] }
    }

    /// An axis-aligned rectangle, counter-clockwise.
    pub fn rectangle(min: (f64, f64), max: (f64, f64)) -> Loop {
        Loop::polygon(&[(min.0, min.1), (max.0, min.1), (max.0, max.1), (min.0, max.1)])
    }

    pub fn is_empty(&self) -> bool {
        self.vertices.len() < 2
    }

    /// Whether every coordinate and bulge is a finite number.
    pub fn is_finite(&self) -> bool {
        self.vertices.iter().all(|v| v.x.is_finite() && v.y.is_finite() && v.bulge.is_finite())
    }

    /// The signed area: positive for a counter-clockwise loop.
    pub fn area(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.pline().area() }
    }

    pub fn is_ccw(&self) -> bool {
        self.area() > 0.0
    }

    /// The same loop run the other way.
    pub fn reversed(&self) -> Loop {
        let mut p = self.pline();
        p.invert_direction_mut();
        Loop::from_pline(&p)
    }

    /// This loop counter-clockwise (`ccw`) or clockwise.
    pub fn oriented(&self, ccw: bool) -> Loop {
        if self.is_ccw() == ccw { self.clone() } else { self.reversed() }
    }

    /// The loop's segments, the last one closing it.
    pub fn segments(&self) -> Vec<Segment> {
        let n = self.vertices.len();
        (0..n)
            .filter_map(|i| {
                let a = self.vertices.get(i)?;
                let b = self.vertices.get((i + 1) % n)?;
                Some(Segment { from: (a.x, a.y), to: (b.x, b.y), bulge: a.bulge })
            })
            .collect()
    }

    /// The length around the loop.
    pub fn length(&self) -> f64 {
        self.segments().iter().map(Segment::length).sum()
    }

    /// The loop's extent: (min, max), or `None` when it is empty.
    pub fn bounds(&self) -> Option<((f64, f64), (f64, f64))> {
        if self.is_empty() {
            return None;
        }
        let ext = self.pline().extents()?;
        Some(((ext.min_x, ext.min_y), (ext.max_x, ext.max_y)))
    }

    /// The loops `distance` to the left of this loop (inward for a counter-clockwise loop);
    /// a negative distance goes the other way. Several where the offset splits the loop, none
    /// where it closes up.
    pub fn offset(&self, distance: f64) -> Vec<Loop> {
        if self.is_empty() {
            return Vec::new();
        }
        if distance.abs() < 1e-12 {
            return vec![self.clone()];
        }
        let out: Vec<Polyline<f64>> = self.pline().parallel_offset(distance);
        out.iter().filter(|p| p.is_closed() && p.vertex_count() > 1).map(Loop::from_pline).collect()
    }

    pub(crate) fn pline(&self) -> Polyline<f64> {
        let mut p = Polyline::with_capacity(self.vertices.len(), true);
        for v in &self.vertices {
            p.add_vertex(PlineVertex::new(v.x, v.y, v.bulge));
        }
        p
    }

    pub(crate) fn from_pline<P: PlineSource<Num = f64>>(p: &P) -> Loop {
        Loop { vertices: p.iter_vertexes().map(|v| Vertex { x: v.x, y: v.y, bulge: v.bulge }).collect() }
    }
}

impl Region {
    /// The loops of the area `distance` inside this region's edges (its boundary moved in, its
    /// islands grown): counter-clockwise outer loops and clockwise island loops. Empty when
    /// nothing is left.
    pub fn inset(&self, distance: f64) -> Vec<Loop> {
        if self.outer.is_empty() {
            return Vec::new();
        }
        let plines = std::iter::once(self.outer.oriented(true).pline())
            .chain(self.islands.iter().filter(|l| !l.is_empty()).map(|l| l.oriented(false).pline()));
        let shape = Shape::from_plines(plines);
        let inset = if distance.abs() < 1e-12 { shape } else { shape.parallel_offset(distance, &ShapeOffsetOptions::default()) };
        inset.ccw_plines.iter().chain(inset.cw_plines.iter()).map(|p| Loop::from_pline(&p.polyline)).filter(|l| !l.is_empty()).collect()
    }
}
