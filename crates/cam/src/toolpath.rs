//! A toolpath: the moves one tool makes, in machine coordinates.

use crate::geom2::{Loop, Segment};
use crate::tool::{Cutting, Tool};

/// A point in machine coordinates (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct P3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl P3 {
    pub fn new(x: f64, y: f64, z: f64) -> P3 {
        P3 { x, y, z }
    }

    fn distance(&self, o: &P3) -> f64 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2) + (self.z - o.z).powi(2)).sqrt()
    }
}

/// One move of the tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Move {
    /// As fast as the machine goes, not cutting (`G0`).
    Rapid(P3),
    /// A straight cut (`G1`).
    Feed { to: P3, feed: f64 },
    /// An arc in the XY plane around `center` (`G2` clockwise, `G3` counter-clockwise), going
    /// to `to.z` along the way (a helix when Z changes).
    Arc { to: P3, center: (f64, f64), ccw: bool, feed: f64 },
}

impl Move {
    pub fn to(&self) -> P3 {
        match self {
            Move::Rapid(p) | Move::Feed { to: p, .. } | Move::Arc { to: p, .. } => *p,
        }
    }
}

/// The moves one operation makes with one tool.
#[derive(Clone, Debug, PartialEq)]
pub struct Toolpath {
    /// The operation's name, written into the program as a comment.
    pub name: String,
    pub tool: Tool,
    pub cutting: Cutting,
    /// The height the tool goes to between operations and at the start and end (mm).
    pub safe_z: f64,
    pub moves: Vec<Move>,
}

impl Toolpath {
    /// How far the tool travels: (rapid, cutting) in mm, starting from above the first move.
    pub fn lengths(&self) -> (f64, f64) {
        let (mut rapid, mut cut) = (0.0, 0.0);
        let mut at: Option<P3> = None;
        for m in &self.moves {
            let to = m.to();
            if let Some(from) = at {
                match m {
                    Move::Rapid(_) => rapid += from.distance(&to),
                    Move::Feed { .. } => cut += from.distance(&to),
                    Move::Arc { center, ccw, .. } => {
                        let seg = arc_segment(from, to, *center, *ccw);
                        cut += seg.length().hypot(to.z - from.z);
                    }
                }
            }
            at = Some(to);
        }
        (rapid, cut)
    }

    /// A rough machining time (s): cutting at the programmed feeds, rapids at `rapid_speed`
    /// (mm/min), accelerations ignored.
    pub fn estimate_seconds(&self, rapid_speed: f64) -> f64 {
        let mut at: Option<P3> = None;
        let mut minutes = 0.0;
        for m in &self.moves {
            let to = m.to();
            if let Some(from) = at {
                minutes += match m {
                    Move::Rapid(_) => from.distance(&to) / rapid_speed.max(1.0),
                    Move::Feed { feed, .. } => from.distance(&to) / feed.max(1.0),
                    Move::Arc { center, ccw, feed, .. } => arc_segment(from, to, *center, *ccw).length() / feed.max(1.0),
                };
            }
            at = Some(to);
        }
        minutes * 60.0
    }

    /// The path as points to draw, each with whether the move to it is a rapid; arcs become short
    /// straight pieces no longer than `step` (mm, at most 256 per arc).
    pub fn points(&self, step: f64) -> Vec<(P3, bool)> {
        let step = if step.is_finite() && step > 1e-3 { step } else { 1.0 };
        let mut out = Vec::with_capacity(self.moves.len());
        let mut at: Option<P3> = None;
        for m in &self.moves {
            let to = m.to();
            match (m, at) {
                (Move::Arc { center, ccw, .. }, Some(from)) => {
                    let seg = arc_segment(from, to, *center, *ccw);
                    let n = (seg.length() / step).ceil().clamp(2.0, 256.0) as usize;
                    for i in 1..=n {
                        let t = i as f64 / n as f64;
                        let (x, y) = if i == n { (to.x, to.y) } else { seg.at(t) };
                        out.push((P3::new(x, y, from.z + (to.z - from.z) * t), false));
                    }
                }
                (m, _) => out.push((to, matches!(m, Move::Rapid(_)))),
            }
            at = Some(to);
        }
        out
    }

    /// The lowest Z the tool reaches, if it moves at all.
    pub fn lowest_z(&self) -> Option<f64> {
        self.moves.iter().map(|m| m.to().z).reduce(f64::min)
    }
}

/// The segment an arc move follows from `from` (its bulge worked out from the centre).
pub(crate) fn arc_segment(from: P3, to: P3, center: (f64, f64), ccw: bool) -> Segment {
    let a0 = (from.y - center.1).atan2(from.x - center.0);
    let a1 = (to.y - center.1).atan2(to.x - center.0);
    let tau = std::f64::consts::TAU;
    let mut sweep = a1 - a0;
    if ccw {
        while sweep <= 1e-12 {
            sweep += tau;
        }
    } else {
        while sweep >= -1e-12 {
            sweep -= tau;
        }
    }
    Segment { from: (from.x, from.y), to: (to.x, to.y), bulge: (sweep / 4.0).tan() }
}

/// Builds a toolpath's moves, keeping where the tool is.
pub(crate) struct Builder {
    pub moves: Vec<Move>,
    pub at: Option<P3>,
}

impl Builder {
    pub fn new() -> Builder {
        Builder { moves: Vec::new(), at: None }
    }

    fn push(&mut self, m: Move) {
        let to = m.to();
        if self.at.is_some_and(|a| a.distance(&to) < 1e-9) {
            return;
        }
        self.moves.push(m);
        self.at = Some(to);
    }

    pub fn rapid(&mut self, p: P3) {
        self.push(Move::Rapid(p));
    }

    pub fn feed(&mut self, p: P3, feed: f64) {
        self.push(Move::Feed { to: p, feed });
    }

    /// Up to `z` at once, if the tool is lower.
    pub fn retract(&mut self, z: f64) {
        if let Some(a) = self.at
            && a.z < z
        {
            self.rapid(P3::new(a.x, a.y, z));
        }
    }

    /// To (x, y) at height `clear` over the air, then down to `z`: rapid to `rapid_to` (just
    /// above the material), feeding the rest.
    pub fn enter(&mut self, x: f64, y: f64, clear: f64, rapid_to: f64, z: f64, plunge: f64) {
        let close = self.at.is_some_and(|a| (a.x - x).abs() < 1e-9 && (a.y - y).abs() < 1e-9);
        if !close {
            self.retract(clear);
            self.rapid(P3::new(x, y, self.at.map_or(clear, |a| a.z.max(clear))));
        }
        let z_now = self.at.map_or(clear, |a| a.z);
        if z_now > rapid_to {
            self.rapid(P3::new(x, y, rapid_to));
        }
        self.feed(P3::new(x, y, z), plunge);
    }

    /// Along one segment at height `z`.
    pub fn segment(&mut self, s: &Segment, z: f64, feed: f64) {
        let to = P3::new(s.to.0, s.to.1, z);
        match s.arc() {
            Some((center, ccw)) => self.push(Move::Arc { to, center, ccw, feed }),
            None => self.feed(to, feed),
        }
    }

    /// Once around `l` at height `z`, starting and ending at its first vertex.
    pub fn around(&mut self, l: &Loop, z: f64, feed: f64) {
        for s in l.segments() {
            self.segment(&s, z, feed);
        }
    }
}

/// The unit direction in XY a move leaves `from` in, and arrives at its end in (an arc's
/// tangents); `None` for a move with no length seen from above.
fn directions(from: P3, m: &Move) -> Option<((f64, f64), (f64, f64))> {
    let unit = |x: f64, y: f64| {
        let n = x.hypot(y);
        (n > 1e-9).then(|| (x / n, y / n))
    };
    match *m {
        Move::Rapid(_) => None,
        Move::Feed { to, .. } => unit(to.x - from.x, to.y - from.y).map(|d| (d, d)),
        Move::Arc { to, center, ccw, .. } => {
            // The tangent turns a quarter from the radius, the way the arc goes.
            let tangent = |p: P3| {
                let (rx, ry) = (p.x - center.0, p.y - center.1);
                if ccw { unit(-ry, rx) } else { unit(ry, -rx) }
            };
            Some((tangent(from)?, tangent(to)?))
        }
    }
}

/// Slow the feed through corners: where a cut turns by more than `angle` (degrees) from the one
/// before it, its first `length` mm go at `factor` × its feed; an arc tighter than `length` in
/// radius goes at that feed all the way. Rapids and moves straight up or down end a run.
pub fn slow_corners(path: &mut Toolpath, angle: f64, factor: f64, length: f64) {
    if !(factor.is_finite() && factor > 0.0 && factor < 1.0 && length.is_finite() && length > 0.0) {
        return;
    }
    let limit = angle.to_radians().cos();
    let mut out = Vec::with_capacity(path.moves.len());
    let mut at: Option<P3> = None;
    let mut heading: Option<(f64, f64)> = None;
    for m in std::mem::take(&mut path.moves) {
        let from = at.unwrap_or(m.to());
        at = Some(m.to());
        let Some((start, end)) = directions(from, &m) else {
            heading = None;
            out.push(m);
            continue;
        };
        let turned = heading.is_some_and(|h| h.0 * start.0 + h.1 * start.1 < limit);
        heading = Some(end);
        match m {
            Move::Feed { to, feed } if turned => {
                let len = (to.x - from.x).hypot(to.y - from.y);
                if len > length * 1.5 {
                    let t = length / len;
                    let mid = P3::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t, from.z + (to.z - from.z) * t);
                    out.push(Move::Feed { to: mid, feed: feed * factor });
                    out.push(Move::Feed { to, feed });
                } else {
                    out.push(Move::Feed { to, feed: feed * factor });
                }
            }
            Move::Arc { to, center, ccw, feed } => {
                let r = (to.x - center.0).hypot(to.y - center.1);
                let slow = turned || r < length;
                out.push(Move::Arc { to, center, ccw, feed: if slow { feed * factor } else { feed } });
            }
            other => out.push(other),
        }
    }
    path.moves = out;
}
