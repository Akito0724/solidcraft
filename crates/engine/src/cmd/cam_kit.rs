//! Contours and pockets planned by the CAM toolkit (`solidcraft-camkit`, krzim's): leads,
//! ramp and helix entries, holding tabs and finishing passes, with links that stay down where the
//! stock's top is clear. Facing and drilling stay with `solidcraft-cam`, which also checks every
//! job first (tool, cutting data, depths against the flutes).
//!
//! The setup's machine frame, depths and cutting data go in as the toolkit's `Cut`; its moves
//! come back as `solidcraft-cam` moves, so the program, the gouge check and the simulation see
//! them like any other.

use std::f64::consts::FRAC_PI_2;

use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_cam::{Clearance, ContourOp, Entry, Job, Loop, Move, Operation, P3, PocketOp, Side, Toolpath};
use solidcraft_camkit as kit;

/// The steepest a ramp or a helix goes down when none is asked for (degrees).
const ENTRY_ANGLE: f64 = 3.0;
/// A helix's radius when none is asked for, as a share of the tool's radius.
const HELIX_SHARE: f64 = 0.75;
/// How closely arcs given as splines are followed (mm); and vertical arcs, written as lines.
const TOLERANCE: f64 = 0.01;
/// How steeply the tool climbs onto and off a tab (degrees).
const TAB_RAMP: f64 = 45.0;

/// The toolpath for `job`: a contour or a pocket by the toolkit, anything else as before.
pub(crate) fn generate(job: &Job, clearance: &Clearance) -> Result<Toolpath, String> {
    match &job.operation {
        Operation::Contour(o) => planned(job, clearance, |entry| contour(job, o, clearance, entry)),
        Operation::Pocket(o) => planned(job, clearance, |entry| pocket(job, o, clearance, entry)),
        Operation::Face(_) | Operation::Drill(_) => solidcraft_cam::generate(job, clearance).map_err(|e| e.to_string()),
    }
}

/// Check the job, then plan it with the entry asked for; with none asked for, the gentlest that
/// fits: a helix (pockets), a ramp, a plunge.
fn planned(job: &Job, clearance: &Clearance, plan: impl Fn(kit::Entry) -> kit::Result<kit::Toolpath>) -> Result<Toolpath, String> {
    solidcraft_cam::check(job, clearance).map_err(|e| e.to_string())?;
    let r = job.tool.diameter / 2.0;
    let tries: Vec<kit::Entry> = match job.approach.entry {
        Some(e) => vec![entry(e, r)],
        None => {
            let ramp = kit::Entry::Ramp { angle: ENTRY_ANGLE.to_radians() };
            match job.operation {
                Operation::Pocket(_) => {
                    vec![kit::Entry::Helix { radius: r * HELIX_SHARE, angle: ENTRY_ANGLE.to_radians() }, ramp, kit::Entry::Plunge]
                }
                _ => vec![ramp, kit::Entry::Plunge],
            }
        }
    };
    let mut first: Option<String> = None;
    for e in tries {
        match plan(e) {
            Ok(path) => return Ok(converted(job, clearance, &path)),
            Err(why) => {
                first.get_or_insert_with(|| words(&why));
            }
        }
    }
    Err(first.unwrap_or_else(|| "nothing could be planned".into()))
}

/// The toolkit's error in plain words (without its kind).
fn words(e: &kit::CamError) -> String {
    match e {
        kit::CamError::Invalid(s) | kit::CamError::Unsupported(s) | kit::CamError::TooLarge(s) | kit::CamError::Unsafe(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The entry asked for; a helix without a radius takes [`HELIX_SHARE`] of the tool's (`r`).
fn entry(e: Entry, r: f64) -> kit::Entry {
    match e {
        Entry::Plunge => kit::Entry::Plunge,
        Entry::Ramp { angle } => kit::Entry::Ramp { angle: angle.to_radians() },
        Entry::Helix { radius, angle } => {
            kit::Entry::Helix { radius: if radius > 0.0 { radius } else { r * HELIX_SHARE }, angle: angle.to_radians() }
        }
    }
}

fn tool(job: &Job) -> kit::Tool {
    let t = &job.tool;
    let kind = match t.kind {
        solidcraft_cam::ToolKind::BallEndmill => kit::ToolKind::BallEndmill,
        solidcraft_cam::ToolKind::BullNose => kit::ToolKind::BullNose,
        _ => kit::ToolKind::FlatEndmill,
    };
    let mut k = kit::Tool::new(t.number, kind, t.diameter, t.flute_length);
    k.flutes = t.flutes.max(1);
    if kind == kit::ToolKind::BullNose {
        k.corner_radius = t.details.corner_radius;
    }
    k.shank_diameter = t.details.shank_diameter;
    k
}

fn cut(job: &Job, clearance: &Clearance, top: f64, bottom: f64) -> kit::Cut {
    kit::Cut {
        heights: kit::Heights { clearance: clearance.safe_z, retract: top + clearance.retract, top, bottom },
        feeds: kit::Feeds { cut: job.cutting.feed, plunge: job.cutting.plunge_feed },
        stepdown: job.cutting.stepdown,
    }
}

/// A loop of lines and arcs as the toolkit's.
fn loop2(l: &Loop) -> Loop2 {
    let segs = l
        .segments()
        .into_iter()
        .map(|s| {
            let (a, b) = (Vec2::new(s.from.0, s.from.1), Vec2::new(s.to.0, s.to.1));
            match s.arc() {
                None => Seg2::Line { a, b },
                Some(((cx, cy), _)) => {
                    let radius = (s.from.0 - cx).hypot(s.from.1 - cy);
                    let start = (s.from.1 - cy).atan2(s.from.0 - cx);
                    Seg2::Arc { center: Vec2::new(cx, cy), radius, start, sweep: 4.0 * s.bulge.atan() }
                }
            }
        })
        .collect();
    Loop2 { segs }
}

/// Clear of the part where a link runs at or above the stock's top: nothing of this operation
/// stands higher.
fn above(top: f64) -> impl Fn(Vec2, Vec2, f64) -> bool {
    move |_, _, z| z >= top - 1e-9
}

fn contour(job: &Job, o: &ContourOp, clearance: &Clearance, entry: kit::Entry) -> kit::Result<kit::Toolpath> {
    let region = Region2 { outer: loop2(&o.profile), holes: Vec::new() };
    let side = match o.side {
        Side::Outside => kit::Side::Outside,
        Side::Inside => kit::Side::Inside,
        Side::On => kit::Side::On,
    };
    let params = kit::ContourParams { side, climb: true, stock_to_leave: 0.0, tolerance: TOLERANCE, ramp_angle: 0.0 };
    let a = &job.approach;
    let opts = kit::ContourOptions {
        lead: a.lead.filter(|r| *r > 0.0).map(|radius| kit::Lead { radius, sweep: FRAC_PI_2, line: 0.0 }),
        entry: Some(entry),
        tabs: o.tabs.map(|t| kit::Tabs {
            layout: kit::TabLayout::Count(t.count as usize),
            width: t.width,
            height: t.height,
            ramp_angle: TAB_RAMP.to_radians(),
        }),
        finish_passes: a.finish.map_or(0, |f| f.passes as usize),
        finish_stepover: a.finish.map_or(0.0, |f| f.stepover),
        spring_pass: a.finish.is_some_and(|f| f.spring),
        linking: kit::Linking::StayDown,
        vertical_lead: None,
    };
    let top = o.depths.top;
    kit::contour_with(&[region], &tool(job), &params, &opts, &cut(job, clearance, top, o.depths.bottom), &above(top))
}

fn pocket(job: &Job, o: &PocketOp, clearance: &Clearance, entry: kit::Entry) -> kit::Result<kit::Toolpath> {
    let region = Region2 { outer: loop2(&o.region.outer), holes: o.region.islands.iter().map(loop2).collect() };
    let stepover = job.cutting.stepover * job.tool.diameter;
    let params = kit::PocketParams { stepover, climb: true, stock_to_leave: 0.0, tolerance: TOLERANCE, ramp_angle: 0.0 };
    let opts = kit::PocketOptions { entry: Some(entry), linking: kit::Linking::StayDown };
    let top = o.depths.top;
    kit::pocket_with(&[region], &tool(job), &params, &opts, &cut(job, clearance, top, o.depths.bottom), &above(top))
}

fn p3(v: Vec3) -> P3 {
    P3::new(v.x, v.y, v.z)
}

/// The toolkit's moves as `solidcraft-cam`'s, ending at the safe height: vertical arcs as lines
/// within [`TOLERANCE`], dwells left out (none is planned here).
fn converted(job: &Job, clearance: &Clearance, path: &kit::Toolpath) -> Toolpath {
    let mut moves = Vec::with_capacity(path.moves.len() + 1);
    let mut at: Option<Vec3> = None;
    for m in &path.moves {
        match *m {
            kit::Move::Rapid { to } => moves.push(Move::Rapid(p3(to))),
            kit::Move::Feed { to, feed } => moves.push(Move::Feed { to: p3(to), feed }),
            kit::Move::Arc { to, center, ccw, feed } => moves.push(Move::Arc { to: p3(to), center: (center.x, center.y), ccw, feed }),
            kit::Move::PlaneArc { to, center, plane, ccw, feed } => {
                let from = at.unwrap_or(to);
                for p in (kit::PlaneArc { from, to, center, plane, ccw }).points(TOLERANCE) {
                    moves.push(Move::Feed { to: p3(p), feed });
                }
            }
            kit::Move::Dwell { .. } => {}
        }
        if let Some(t) = m.target() {
            at = Some(t);
        }
    }
    if let Some(last) = moves.last().map(|m| m.to())
        && last.z < clearance.safe_z - 1e-9
    {
        moves.push(Move::Rapid(P3::new(last.x, last.y, clearance.safe_z)));
    }
    Toolpath { name: job.name.clone(), tool: job.tool.clone(), cutting: job.cutting, safe_z: clearance.safe_z, moves }
}
