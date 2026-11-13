//! The 2.5D operations and the toolpaths they make.
//!
//! Every operation cuts from `top` down to `bottom` in passes no deeper than the cutting data's
//! stepdown, evenly spread. Between cuts the tool lifts just clear of what has been cut so far;
//! at the start and end of the operation it is at the safe height.
//!
//! Directions are climb milling for a spindle turning clockwise (seen from above): a pocket's
//! walls are followed counter-clockwise, its islands and an outside contour clockwise.

use crate::geom2::{Loop, Region};
use crate::tool::{Cutting, Tool, ToolKind};
use crate::toolpath::{Builder, P3, Toolpath};
use crate::{Result, invalid};

/// Most passes (levels, pocket rings, facing lines) one operation makes: a guard against a
/// tiny stepdown or stepover over a large part.
const MAX_PASSES: usize = 20_000;

/// From where to where an operation cuts (machine Z, mm).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Depths {
    /// Where the material starts.
    pub top: f64,
    /// The finished depth.
    pub bottom: f64,
}

/// Heights the tool moves at when not cutting.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Clearance {
    /// Machine Z clear of the stock and the clamps, at the start, the end and between holes far
    /// apart.
    pub safe_z: f64,
    /// How far above the material cut so far the tool lifts between cuts (mm).
    pub retract: f64,
}

impl Default for Clearance {
    fn default() -> Self {
        Clearance { safe_z: 10.0, retract: 1.0 }
    }
}

/// Which side of a contour the tool runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Side {
    /// Outside the profile: cutting a part out.
    Outside,
    /// Inside: cutting a hole or opening to size.
    Inside,
    /// On the line: engraving or a slot as wide as the tool.
    On,
}

/// Bridges left uncut at the bottom of an outside contour, so the part stays held.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tabs {
    pub count: u32,
    /// Width along the profile (mm).
    pub width: f64,
    /// Height above the contour's bottom (mm).
    pub height: f64,
}

/// Facing: lowering the top of the stock over a rectangle.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FaceOp {
    pub min: (f64, f64),
    pub max: (f64, f64),
    pub depths: Depths,
}

/// A pocket: clearing an area to a flat floor, leaving its islands.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PocketOp {
    pub region: Region,
    pub depths: Depths,
}

/// A contour: following a profile to cut along it.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ContourOp {
    pub profile: Loop,
    pub side: Side,
    pub depths: Depths,
    #[cfg_attr(feature = "serde", serde(default))]
    pub tabs: Option<Tabs>,
}

/// Drilling: plunging at points, in pecks when `peck` is set.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DrillOp {
    pub points: Vec<(f64, f64)>,
    pub depths: Depths,
    /// Depth per peck (mm); the drill clears its chips above the hole between pecks.
    #[cfg_attr(feature = "serde", serde(default))]
    pub peck: Option<f64>,
}

/// What an operation does.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "type", rename_all = "snake_case"))]
pub enum Operation {
    Face(FaceOp),
    Pocket(PocketOp),
    Contour(ContourOp),
    Drill(DrillOp),
}

/// How the tool goes down into a contour's or a pocket's passes.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "type", rename_all = "snake_case"))]
pub enum Entry {
    /// Straight down at the plunge feed.
    Plunge,
    /// Down along the pass, no steeper than `angle` (degrees).
    Ramp { angle: f64 },
    /// Down round a circle of `radius` (mm, at most the tool's radius), no steeper than `angle`
    /// (degrees).
    Helix { radius: f64, angle: f64 },
}

/// Finishing passes round a contour at its full depth.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Finish {
    /// How many; the depth passes stay `passes × stepover` off the wall for them.
    pub passes: u32,
    /// How far apart (mm), at most half the tool's radius.
    pub stepover: f64,
    /// Go round the last one once more.
    #[cfg_attr(feature = "serde", serde(default))]
    pub spring: bool,
}

/// How a contour or a pocket goes into the material and along its wall. The default leaves the
/// choice to the planner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Approach {
    /// `None`: the gentlest that fits (a helix, a ramp, else a plunge).
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    pub entry: Option<Entry>,
    /// A contour: a quarter arc of this radius (mm) into and out of each pass, on the side away
    /// from the part.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    pub lead: Option<f64>,
    /// A contour: finishing passes at full depth.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    pub finish: Option<Finish>,
}

impl Approach {
    pub fn is_default(&self) -> bool {
        *self == Approach::default()
    }
}

/// An operation with the tool that cuts it and how.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub name: String,
    pub operation: Operation,
    pub tool: Tool,
    pub cutting: Cutting,
    /// How a contour or a pocket enters (a planner that knows how: see the engine).
    pub approach: Approach,
}

/// The toolpath for `job`.
pub fn generate(job: &Job, clearance: &Clearance) -> Result<Toolpath> {
    check(job, clearance)?;
    let mut b = Builder::new();
    match &job.operation {
        Operation::Face(o) => face(&mut b, o, job, clearance)?,
        Operation::Pocket(o) => pocket(&mut b, o, job, clearance)?,
        Operation::Contour(o) => contour(&mut b, o, job, clearance)?,
        Operation::Drill(o) => drill(&mut b, o, job, clearance)?,
    }
    b.retract(clearance.safe_z);
    Ok(Toolpath { name: job.name.clone(), tool: job.tool.clone(), cutting: job.cutting, safe_z: clearance.safe_z, moves: b.moves })
}

/// Whether `job` can be cut at all: its tool, its cutting data, its depths against the tool and
/// the heights, and the tool for the operation.
pub fn check(job: &Job, clearance: &Clearance) -> Result<()> {
    if let Some(p) = job.tool.problem() {
        return Err(invalid(p));
    }
    if let Some(p) = job.cutting.problem() {
        return Err(invalid(format!("{}: {p}", job.name)));
    }
    if !clearance.safe_z.is_finite() || !(clearance.retract >= 0.0) || !clearance.retract.is_finite() {
        return Err(invalid("the safe height and the retract must be numbers, the retract at least 0"));
    }
    let depths = match &job.operation {
        Operation::Face(o) => o.depths,
        Operation::Pocket(o) => o.depths,
        Operation::Contour(o) => o.depths,
        Operation::Drill(o) => o.depths,
    };
    check_depths(&job.name, &depths, &job.tool, clearance)?;
    if job.tool.kind == ToolKind::Drill && !matches!(job.operation, Operation::Drill(_)) {
        return Err(invalid(format!("{}: a drill can only drill", job.name)));
    }
    if job.tool.kind == ToolKind::VBit {
        return Err(invalid(format!("{}: a V-bit is for chamfers and engraving, which can't be made here yet", job.name)));
    }
    Ok(())
}

fn check_depths(name: &str, d: &Depths, tool: &Tool, c: &Clearance) -> Result<()> {
    if !d.top.is_finite() || !d.bottom.is_finite() {
        return Err(invalid(format!("{name}: the top and bottom must be numbers")));
    }
    if !(d.top > d.bottom) {
        return Err(invalid(format!("{name}: the bottom must be below the top")));
    }
    if d.top - d.bottom > tool.flute_length + 1e-9 {
        return Err(invalid(format!(
            "{name}: {:.2} mm deep is more than {}'s flute length ({:.2} mm)",
            d.top - d.bottom,
            tool.name,
            tool.flute_length
        )));
    }
    if !(c.safe_z > d.top + c.retract) {
        return Err(invalid(format!("{name}: the safe height must be above the top of the material and the retract")));
    }
    Ok(())
}

/// The heights of the passes from `top` down to `bottom`, none deeper than `stepdown`.
pub(crate) fn levels(top: f64, bottom: f64, stepdown: f64) -> Result<Vec<f64>> {
    let n = ((top - bottom) / stepdown - 1e-9).ceil().max(1.0);
    if !n.is_finite() || n > MAX_PASSES as f64 {
        return Err(invalid("too many passes: make the depth per pass larger"));
    }
    let n = n as usize;
    Ok((1..=n).map(|i| top - (top - bottom) * i as f64 / n as f64).collect())
}

fn face(b: &mut Builder, o: &FaceOp, job: &Job, c: &Clearance) -> Result<()> {
    let (min, max) = o.min_max();
    if !(max.0 > min.0) || !(max.1 > min.1) {
        return Err(invalid(format!("{}: the area to face is empty", job.name)));
    }
    let r = job.tool.radius();
    let step = job.cutting.stepover * job.tool.diameter;
    // Lines along X from beyond one side to beyond the other, so the tool goes down outside the
    // stock; the outermost lines run on the edges, which the tool's radius covers.
    let (x0, x1) = (min.0 - r - 1.0, max.0 + r + 1.0);
    let lines = ((max.1 - min.1) / step - 1e-9).ceil().max(1.0);
    if lines > MAX_PASSES as f64 {
        return Err(invalid(format!("{}: too many facing lines: make the stepover larger", job.name)));
    }
    let lines = lines as usize;
    let mut above = o.depths.top;
    for z in levels(o.depths.top, o.depths.bottom, job.cutting.stepdown)? {
        let link = above + c.retract;
        if b.at.is_none() {
            b.rapid(P3::new(x0, min.1, c.safe_z));
        }
        b.enter(x0, min.1, link, link, z, job.cutting.plunge_feed);
        for k in 0..=lines {
            let y = min.1 + (max.1 - min.1) * k as f64 / lines as f64;
            let (from, to) = if k % 2 == 0 { (x0, x1) } else { (x1, x0) };
            b.feed(P3::new(from, y, z), job.cutting.feed);
            b.feed(P3::new(to, y, z), job.cutting.feed);
        }
        b.retract(link);
        above = z;
    }
    Ok(())
}

impl FaceOp {
    fn min_max(&self) -> ((f64, f64), (f64, f64)) {
        ((self.min.0.min(self.max.0), self.min.1.min(self.max.1)), (self.min.0.max(self.max.0), self.min.1.max(self.max.1)))
    }
}

/// The loops a pocket's tool centre follows, outermost ring first: the walls at the tool's
/// radius, then each ring a stepover further in, until nothing is left.
pub(crate) fn pocket_rings(region: &Region, radius: f64, step: f64) -> Result<Vec<Vec<Loop>>> {
    let mut rings = Vec::new();
    for k in 0.. {
        if k > MAX_PASSES {
            return Err(invalid("too many pocket rings: make the stepover larger"));
        }
        let ring = region.inset(radius + step * k as f64);
        if ring.is_empty() {
            break;
        }
        rings.push(ring);
    }
    Ok(rings)
}

fn pocket(b: &mut Builder, o: &PocketOp, job: &Job, c: &Clearance) -> Result<()> {
    if o.region.outer.is_empty() || !o.region.outer.is_finite() || o.region.islands.iter().any(|l| !l.is_finite()) {
        return Err(invalid(format!("{}: the pocket has no boundary", job.name)));
    }
    let step = job.cutting.stepover * job.tool.diameter;
    let rings = pocket_rings(&o.region, job.tool.radius(), step)?;
    if rings.is_empty() {
        return Err(invalid(format!("{}: {} doesn't fit in the pocket", job.name, job.tool.name)));
    }
    let mut above = o.depths.top;
    for z in levels(o.depths.top, o.depths.bottom, job.cutting.stepdown)? {
        let link = above + c.retract;
        // The middle first: the tool works outward and finishes each level on the walls.
        for ring in rings.iter().rev() {
            for l in ring {
                let Some(start) = l.vertices.first() else { continue };
                if b.at.is_none() {
                    b.rapid(P3::new(start.x, start.y, c.safe_z));
                }
                b.enter(start.x, start.y, link, link, z, job.cutting.plunge_feed);
                b.around(l, z, job.cutting.feed);
                b.retract(link);
            }
        }
        above = z;
    }
    Ok(())
}

/// The loops a contour's tool centre follows, in the cutting direction.
pub(crate) fn contour_loops(o: &ContourOp, radius: f64) -> Vec<Loop> {
    let base = o.profile.oriented(true);
    match o.side {
        Side::Outside => base.offset(-radius).into_iter().map(|l| l.oriented(false)).collect(),
        Side::Inside => base.offset(radius).into_iter().map(|l| l.oriented(true)).collect(),
        Side::On => vec![base.oriented(false)],
    }
}

fn contour(b: &mut Builder, o: &ContourOp, job: &Job, c: &Clearance) -> Result<()> {
    if o.profile.is_empty() || !o.profile.is_finite() {
        return Err(invalid(format!("{}: the contour has no profile", job.name)));
    }
    let loops = contour_loops(o, job.tool.radius());
    if loops.is_empty() {
        return Err(invalid(format!("{}: {} doesn't fit inside the profile", job.name, job.tool.name)));
    }
    let tab_top = match o.tabs {
        Some(t) if t.count > 0 => {
            if !(t.width > 0.0) || !(t.height > 0.0) || !t.width.is_finite() || !t.height.is_finite() {
                return Err(invalid(format!("{}: tabs need a width and a height", job.name)));
            }
            Some(o.depths.bottom + t.height)
        }
        _ => None,
    };
    let mut above = o.depths.top;
    for z in levels(o.depths.top, o.depths.bottom, job.cutting.stepdown)? {
        let link = above + c.retract;
        for l in &loops {
            let Some(start) = l.vertices.first() else { continue };
            if b.at.is_none() {
                b.rapid(P3::new(start.x, start.y, c.safe_z));
            }
            // A single loop ends where it started, so the next level goes straight down there (the
            // first goes down from just above the wood).
            let here =
                b.at.filter(|a| loops.len() == 1 && (a.x - start.x).abs() < 1e-6 && (a.y - start.y).abs() < 1e-6 && a.z > z && a.z <= above + 1e-9);
            match here {
                Some(_) => b.feed(P3::new(start.x, start.y, z), job.cutting.plunge_feed),
                None => b.enter(start.x, start.y, link, link, z, job.cutting.plunge_feed),
            }
            match (o.tabs, tab_top) {
                (Some(t), Some(tz)) if z < tz - 1e-9 => {
                    let windows = tab_windows(l.length(), t, job.tool.diameter);
                    around_with_tabs(b, l, z, tz, &windows, job.cutting.feed, job.cutting.plunge_feed);
                }
                _ => b.around(l, z, job.cutting.feed),
            }
            if loops.len() > 1 {
                b.retract(link);
            }
        }
        above = z;
    }
    Ok(())
}

/// Where the tool centre lifts over the tabs: (start, end) distances along a loop of `length`.
/// A tab of `width` on the part needs the centre lifted over its width and the tool's.
pub(crate) fn tab_windows(length: f64, tabs: Tabs, diameter: f64) -> Vec<(f64, f64)> {
    let n = tabs.count.max(1) as f64;
    let spacing = length / n;
    let half = ((tabs.width + diameter) / 2.0).min(spacing * 0.45);
    (0..tabs.count).map(|i| (i as f64 + 0.5) * spacing).map(|mid| (mid - half, mid + half)).collect()
}

/// Once around `l` at `z`, rising to `tab_z` over the `windows`; going back down is at the
/// `plunge` feed.
fn around_with_tabs(b: &mut Builder, l: &Loop, z: f64, tab_z: f64, windows: &[(f64, f64)], feed: f64, plunge: f64) {
    let mut along = 0.0;
    let mut height = z;
    for seg in l.segments() {
        let len = seg.length();
        if len < 1e-12 {
            continue;
        }
        let mut cuts: Vec<f64> =
            windows.iter().flat_map(|&(a, e)| [a, e]).filter(|&e| e > along + 1e-9 && e < along + len - 1e-9).map(|e| (e - along) / len).collect();
        cuts.sort_by(f64::total_cmp);
        let mut t0 = 0.0;
        for t1 in cuts.into_iter().chain(std::iter::once(1.0)) {
            let piece = seg.between(t0, t1);
            let mid = along + len * (t0 + t1) / 2.0;
            let z_here = if windows.iter().any(|&(a, e)| mid > a && mid < e) { tab_z.max(z) } else { z };
            if (z_here - height).abs() > 1e-9 {
                b.feed(P3::new(piece.from.0, piece.from.1, z_here), if z_here < height { plunge } else { feed });
                height = z_here;
            }
            b.segment(&piece, z_here, feed);
            t0 = t1;
        }
        along += len;
    }
    if (height - z).abs() > 1e-9
        && let Some(at) = b.at
    {
        b.feed(P3::new(at.x, at.y, z), plunge);
    }
}

fn drill(b: &mut Builder, o: &DrillOp, job: &Job, c: &Clearance) -> Result<()> {
    if o.points.is_empty() {
        return Err(invalid(format!("{}: there are no holes to drill", job.name)));
    }
    if o.points.iter().any(|p| !p.0.is_finite() || !p.1.is_finite()) {
        return Err(invalid(format!("{}: a hole's position is not a number", job.name)));
    }
    if let Some(peck) = o.peck
        && (!(peck > 0.0) || !peck.is_finite() || (o.depths.top - o.depths.bottom) / peck > MAX_PASSES as f64)
    {
        return Err(invalid(format!("{}: the peck depth must be more than 0 mm", job.name)));
    }
    let link = o.depths.top + c.retract;
    for (x, y) in nearest_order(&o.points) {
        if b.at.is_none() {
            b.rapid(P3::new(x, y, c.safe_z));
        }
        match o.peck {
            None => b.enter(x, y, link, link, o.depths.bottom, job.cutting.plunge_feed),
            Some(peck) => {
                let mut depth = o.depths.top;
                b.enter(x, y, link, link, link, job.cutting.plunge_feed);
                while depth > o.depths.bottom + 1e-9 {
                    let next = (depth - peck).max(o.depths.bottom);
                    // Back down to just above where the last peck stopped, then cut the next.
                    b.rapid(P3::new(x, y, (depth + c.retract.min(0.5)).min(link)));
                    b.feed(P3::new(x, y, next), job.cutting.plunge_feed);
                    b.rapid(P3::new(x, y, link));
                    depth = next;
                }
            }
        }
        b.retract(link);
    }
    Ok(())
}

/// The points in a short travelling order: nearest first, from the work origin.
fn nearest_order(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut left: Vec<(f64, f64)> = points.to_vec();
    let mut at = (0.0, 0.0);
    let mut out = Vec::with_capacity(left.len());
    while !left.is_empty() {
        let d = |p: &(f64, f64)| (p.0 - at.0).powi(2) + (p.1 - at.1).powi(2);
        let i = (0..left.len()).min_by(|&a, &b| d(&left[a]).total_cmp(&d(&left[b]))).unwrap_or(0);
        at = left.swap_remove(i);
        out.push(at);
    }
    out
}
