#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::*;

fn wood(tool: &Tool) -> Cutting {
    Cutting::starting(Material::Wood, tool)
}

fn job(name: &str, operation: Operation, tool: Tool) -> Job {
    let cutting = wood(&tool);
    Job { name: name.into(), operation, tool, cutting, approach: Default::default() }
}

/// Every XY position the toolpath cuts at (its feed and arc end points).
fn cut_points(t: &Toolpath) -> Vec<P3> {
    t.moves.iter().filter(|m| !matches!(m, Move::Rapid(_))).map(Move::to).collect()
}

fn bounds(points: &[P3]) -> (f64, f64, f64, f64) {
    points.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, p| (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y)))
}

/// The path's signed area seen from above (positive: counter-clockwise), from its end points.
fn turning(points: &[P3]) -> f64 {
    let n = points.len();
    (0..n).map(|i| points[i].x * points[(i + 1) % n].y - points[(i + 1) % n].x * points[i].y).sum::<f64>() / 2.0
}

#[test]
fn a_bulge_gives_the_arc_centre_and_direction() {
    let s = geom2::Segment { from: (0.0, 0.0), to: (2.0, 0.0), bulge: 1.0 };
    let (c, ccw) = s.arc().unwrap();
    assert!((c.0 - 1.0).abs() < 1e-12 && c.1.abs() < 1e-12);
    assert!(ccw);
    assert!((s.length() - std::f64::consts::PI).abs() < 1e-9);
    // A counter-clockwise half circle from (0,0) to (2,0) passes below the chord.
    let mid = s.at(0.5);
    assert!((mid.0 - 1.0).abs() < 1e-9 && (mid.1 + 1.0).abs() < 1e-9, "{mid:?}");
    let half = s.between(0.0, 0.5);
    assert!((half.length() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
}

#[test]
fn levels_spread_the_depth_evenly() {
    let l = ops::levels(0.0, -10.0, 3.0).unwrap();
    assert_eq!(l.len(), 4);
    assert!((l[3] + 10.0).abs() < 1e-12);
    assert!((l[0] + 2.5).abs() < 1e-12);
    assert_eq!(ops::levels(0.0, -6.0, 3.0).unwrap().len(), 2);
    assert!(ops::levels(0.0, -10.0, 1e-9).is_err());
}

#[test]
fn an_outside_contour_runs_clockwise_a_radius_outside_with_tabs_at_the_bottom() {
    let tool = Tool::endmill(1, 6.0, 2, 20.0);
    let op = ContourOp {
        profile: Loop::rectangle((0.0, 0.0), (40.0, 20.0)),
        side: Side::Outside,
        depths: Depths { top: 0.0, bottom: -9.0 },
        tabs: Some(Tabs { count: 4, width: 5.0, height: 2.0 }),
    };
    let t = generate(&job("cut out", Operation::Contour(op), tool), &Clearance::default()).unwrap();
    let bottom: Vec<P3> = cut_points(&t).into_iter().filter(|p| (p.z + 9.0).abs() < 1e-9).collect();
    let (x0, y0, x1, y1) = bounds(&bottom);
    assert!((x0 + 3.0).abs() < 1e-6 && (y0 + 3.0).abs() < 1e-6 && (x1 - 43.0).abs() < 1e-6 && (y1 - 23.0).abs() < 1e-6, "{x0} {y0} {x1} {y1}");
    // The tool lifts over the tabs on the passes below the tabs' top, and never higher.
    assert!(cut_points(&t).iter().any(|p| (p.z + 7.0).abs() < 1e-9), "the tool rides over the tabs");
    let deepest = t.lowest_z().unwrap();
    assert!((deepest + 9.0).abs() < 1e-9);
    // Climb milling outside: clockwise. The corner arcs are clockwise too.
    let last_level: Vec<P3> = t.moves.iter().map(Move::to).filter(|p| p.z < -5.0).collect();
    assert!(turning(&last_level) < 0.0);
    assert!(t.moves.iter().any(|m| matches!(m, Move::Arc { ccw: false, .. })));
    // It ends at the safe height.
    assert_eq!(t.moves.last().map(|m| m.to().z), Some(10.0));
    // Every move down into the wood, after a tab too, is at the plunge feed.
    let mut z = f64::MAX;
    for m in &t.moves {
        if let Move::Feed { to, feed } = m
            && to.z < z - 1e-9
        {
            assert!((feed - t.cutting.plunge_feed).abs() < 1e-9, "down to {} at {feed}", to.z);
        }
        z = m.to().z;
    }
    // One loop: the next level goes straight down where the last ended, without lifting.
    let rapids_below_top = t.moves.iter().filter(|m| matches!(m, Move::Rapid(p) if p.z < 0.0)).count();
    assert_eq!(rapids_below_top, 0);
    // The first plunge is a rapid to just above the wood, then the plunge feed.
    let first_feed = t.moves.iter().position(|m| matches!(m, Move::Feed { .. })).unwrap();
    assert!(matches!(t.moves[first_feed - 1], Move::Rapid(p) if (p.z - 1.0).abs() < 1e-9), "{:?}", &t.moves[..first_feed]);
}

#[test]
fn a_pocket_stays_a_radius_inside_its_walls_and_around_its_islands() {
    let tool = Tool::endmill(2, 3.175, 2, 12.0);
    let region = Region { outer: Loop::rectangle((0.0, 0.0), (30.0, 20.0)), islands: vec![Loop::circle(15.0, 10.0, 3.0)] };
    let t = generate(&job("pocket", Operation::Pocket(PocketOp { region, depths: Depths { top: 0.0, bottom: -4.0 } }), tool), &Clearance::default())
        .unwrap();
    let r = 3.175 / 2.0;
    let points = cut_points(&t);
    let (x0, y0, x1, y1) = bounds(&points);
    assert!(x0 >= r - 1e-6 && y0 >= r - 1e-6 && x1 <= 30.0 - r + 1e-6 && y1 <= 20.0 - r + 1e-6, "{x0} {y0} {x1} {y1}");
    for p in &points {
        let d = (p.x - 15.0).hypot(p.y - 10.0);
        assert!(d >= 3.0 + r - 1e-6, "the tool cuts into the island at {p:?}");
    }
    // The walls are reached at the floor.
    assert!(points.iter().any(|p| (p.x - r).abs() < 1e-6 && (p.z + 4.0).abs() < 1e-9));
    // Arcs around the island are clockwise (climb).
    assert!(t.moves.iter().any(|m| matches!(m, Move::Arc { ccw: false, .. })));
}

#[test]
fn a_tool_too_big_for_the_pocket_is_refused() {
    let tool = Tool::endmill(1, 12.0, 2, 20.0);
    let region = Region { outer: Loop::rectangle((0.0, 0.0), (10.0, 10.0)), islands: vec![] };
    let e = generate(&job("tight", Operation::Pocket(PocketOp { region, depths: Depths { top: 0.0, bottom: -2.0 } }), tool), &Clearance::default());
    assert!(e.is_err());
}

#[test]
fn depths_tools_and_clearance_are_checked() {
    let tool = Tool::endmill(1, 3.0, 2, 5.0);
    let deep = ContourOp { profile: Loop::circle(0.0, 0.0, 10.0), side: Side::Inside, depths: Depths { top: 0.0, bottom: -8.0 }, tabs: None };
    assert!(generate(&job("deep", Operation::Contour(deep.clone()), tool.clone()), &Clearance::default()).is_err(), "deeper than the flutes");
    let upside = ContourOp { depths: Depths { top: -1.0, bottom: 0.0 }, ..deep.clone() };
    assert!(generate(&job("upside", Operation::Contour(upside), tool.clone()), &Clearance::default()).is_err());
    let low = Clearance { safe_z: 0.5, retract: 1.0 };
    let ok = ContourOp { depths: Depths { top: 0.0, bottom: -2.0 }, ..deep };
    assert!(generate(&job("low", Operation::Contour(ok.clone()), tool.clone()), &low).is_err());
    let mut bad = job("nan", Operation::Contour(ok), tool);
    bad.cutting.feed = f64::NAN;
    assert!(generate(&bad, &Clearance::default()).is_err());
}

#[test]
fn drilling_pecks_and_clears_between_holes() {
    let drill = Tool {
        details: Default::default(),
        number: 3,
        name: "3 mm drill".into(),
        kind: ToolKind::Drill,
        diameter: 3.0,
        flute_length: 20.0,
        flutes: 2,
    };
    let op = DrillOp { points: vec![(10.0, 0.0), (0.0, 0.0), (5.0, 5.0)], depths: Depths { top: 0.0, bottom: -6.0 }, peck: Some(2.0) };
    let t = generate(&job("holes", Operation::Drill(op), drill.clone()), &Clearance::default()).unwrap();
    let plunges = t.moves.iter().filter(|m| matches!(m, Move::Feed { to, .. } if to.z < -1e-9)).count();
    assert_eq!(plunges, 9, "three pecks at each of three holes");
    // Nearest first from the origin.
    let first_hole = t.moves.iter().find(|m| matches!(m, Move::Feed { .. })).map(Move::to).unwrap();
    assert!(first_hole.x.abs() < 1e-9 && first_hole.y.abs() < 1e-9);
    // A drill can't mill.
    let pocket = Operation::Pocket(PocketOp {
        region: Region { outer: Loop::rectangle((0.0, 0.0), (20.0, 20.0)), islands: vec![] },
        depths: Depths { top: 0.0, bottom: -1.0 },
    });
    assert!(generate(&job("no", pocket, drill), &Clearance::default()).is_err());
}

#[test]
fn facing_starts_outside_the_stock_and_covers_it() {
    let tool = Tool::endmill(4, 6.0, 2, 20.0);
    let op = FaceOp { min: (0.0, 0.0), max: (50.0, 30.0), depths: Depths { top: 1.0, bottom: 0.0 } };
    let t = generate(&job("face", Operation::Face(op), tool), &Clearance::default()).unwrap();
    let cutting: Vec<P3> = cut_points(&t).into_iter().filter(|p| p.z < 0.5).collect();
    let (x0, y0, x1, y1) = bounds(&cutting);
    assert!(x0 < -3.0 && x1 > 53.0 && y0.abs() < 1e-9 && (y1 - 30.0).abs() < 1e-9);
    // The first plunge is beside the stock, not on it.
    let first_cut = t.moves.iter().find(|m| matches!(m, Move::Feed { .. })).map(Move::to).unwrap();
    assert!(first_cut.x < 0.0);
}

#[test]
fn grbl_programs_use_arcs_drop_repeated_words_and_end_safely() {
    let tool = Tool::endmill(1, 3.175, 2, 12.0);
    let op = ContourOp { profile: Loop::circle(0.0, 0.0, 10.0), side: Side::Inside, depths: Depths { top: 0.0, bottom: -3.0 }, tabs: None };
    let t = generate(&job("hole (M20)", Operation::Contour(op), tool), &Clearance::default()).unwrap();
    let text = grbl(&[&t], &PostOptions { title: "test part".into(), ..PostOptions::default() });
    assert!(text.starts_with("(test part)\n"));
    assert!(text.contains("G21 G90 G94 G17"));
    assert!(text.contains("M3 S10000"));
    assert!(text.contains("G3 ") && text.contains(" I") && text.contains(" J"), "{text}");
    assert!(text.contains("(hole  M20)"), "parentheses can't nest in a comment: {text}");
    assert!(!text.contains("NaN") && !text.contains("inf"));
    let tail: Vec<&str> = text.lines().rev().take(3).collect();
    // Up to the safe height at a rapid (written as G0 when the move before it fed).
    assert_eq!(&tail[..2], ["M30", "M5"]);
    assert!(tail[2] == "G0 Z10" || tail[2] == "Z10", "{tail:?}");
    assert_eq!(text.matches("Z10\n").count(), 2, "safe height at the start and once at the end");
    // The feed is written once until it changes.
    let feeds = text.lines().filter(|l| l.contains(" F")).count();
    assert!(feeds <= 4, "{feeds} feed words:\n{text}");
    // Without arcs, the same program in short lines.
    let lines = grbl(&[&t], &PostOptions { arcs: false, ..PostOptions::default() });
    assert!(!lines.contains("G3") && !lines.contains("G2 "));
}

#[test]
fn programs_split_where_the_tool_changes() {
    let a = Tool::endmill(1, 3.175, 2, 12.0);
    let b = Tool::endmill(2, 6.0, 2, 20.0);
    let c = Clearance::default();
    let contour = |tool: Tool| {
        let op = ContourOp {
            profile: Loop::rectangle((0.0, 0.0), (40.0, 30.0)),
            side: Side::Outside,
            depths: Depths { top: 0.0, bottom: -2.0 },
            tabs: None,
        };
        generate(&job("contour", Operation::Contour(op), tool), &c).unwrap()
    };
    let paths = vec![contour(a.clone()), contour(a), contour(b.clone()), contour(Tool::endmill(1, 3.175, 2, 12.0))];
    let groups = by_tool(&paths);
    assert_eq!(groups.iter().map(|(t, l)| (t.number, l.len())).collect::<Vec<_>>(), vec![(1, 2), (2, 1), (1, 1)]);
}

#[test]
fn a_frame_carries_points_both_ways_and_stays_right_handed() {
    // Turned over about X: the model's top faces the table.
    let f = Frame::new([10.0, 0.0, 20.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]).unwrap();
    assert_eq!(f.y, [0.0, -1.0, 0.0]);
    assert_eq!(f.to_machine([10.0, 5.0, 20.0]), [0.0, -5.0, 0.0]);
    assert_eq!(f.to_model(f.to_machine([3.0, 4.0, 5.0])), [3.0, 4.0, 5.0]);
    // On its side: the model's −Y face on the table, machine X along the model's X.
    let side = Frame::new([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
    assert_eq!(side.y, [0.0, 0.0, -1.0]);
    assert_eq!(side.to_machine([2.0, 3.0, 4.0]), [2.0, -4.0, 3.0]);
    assert_eq!(side.dir_to_model([0.0, 0.0, 1.0]), [0.0, 1.0, 0.0]);
    // Axes that aren't square, or are zero, make no frame.
    assert!(Frame::new([0.0; 3], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]).is_none());
    assert!(Frame::new([0.0; 3], [0.0; 3], [0.0, 0.0, 1.0]).is_none());
}

#[test]
fn starting_cutting_data_suits_the_material() {
    let t = Tool::endmill(1, 3.175, 2, 12.0);
    let wood = Cutting::starting(Material::Wood, &t);
    let alu = Cutting::starting(Material::Aluminum, &t);
    assert!(wood.feed > alu.feed && wood.stepdown > alu.stepdown);
    assert!(wood.problem().is_none() && alu.problem().is_none());
}

#[test]
fn a_toolpath_draws_as_points_with_its_arcs_in_short_pieces() {
    let tool = Tool::endmill(1, 3.175, 2, 12.0);
    let op = ContourOp { profile: Loop::circle(0.0, 0.0, 10.0), side: Side::On, depths: Depths { top: 0.0, bottom: -1.0 }, tabs: None };
    let t = generate(&job("ring", Operation::Contour(op), tool), &Clearance::default()).unwrap();
    let points = t.points(1.0);
    assert!(points.len() > 60, "a 10 mm circle in 1 mm pieces: {}", points.len());
    assert!(points.first().is_some_and(|(_, rapid)| *rapid));
    // Every point on the cut is on the circle.
    for (p, rapid) in &points {
        if !rapid && (p.z + 1.0).abs() < 1e-9 {
            assert!((p.x.hypot(p.y) - 10.0).abs() < 1e-6, "{p:?}");
        }
    }
    assert!(t.points(f64::NAN).len() > 2, "a bad step still draws");
}

#[test]
fn the_stock_is_cut_where_the_cutter_passes_and_nowhere_else() {
    use crate::sim::{Cutter, Run, RunOp, Stock};
    let mut stock = Stock::new([-10.0, -10.0, -5.0], [10.0, 10.0, 0.0], 0.25).unwrap();
    assert!(stock.nx >= 80 && stock.ny >= 80);
    // A 3 mm cutter along X at Z -2.
    let flat = Cutter::flat(1.5, 12.0);
    stock.sweep(P3::new(-5.0, 0.0, -2.0), P3::new(5.0, 0.0, -2.0), &flat);
    assert!((at(&stock, 0.0, 0.0) + 2.0).abs() < 1e-9);
    assert!((at(&stock, 0.0, 1.2) + 2.0).abs() < 1e-9, "inside the cutter's width");
    assert_eq!(at(&stock, 0.0, 3.0), 0.0, "outside it");
    assert_eq!(at(&stock, 8.0, 0.0), 0.0, "past the end of the move");
    // Never below the stock's bottom; nothing above its top.
    let small = Cutter::flat(1.0, 60.0);
    stock.sweep(P3::new(0.0, 5.0, -50.0), P3::new(0.0, 5.0, -50.0), &small);
    assert_eq!(at(&stock, 0.0, 5.0), -5.0);
    stock.sweep(P3::new(0.0, -5.0, 3.0), P3::new(0.0, -5.0, 3.0), &small);
    assert_eq!(at(&stock, 0.0, -5.0), 0.0);
    // Removed volume ≈ a 10 × 3 mm slot (plus round ends) 2 mm deep, and the plunge.
    let slot = 10.0 * 3.0 * 2.0 + std::f64::consts::PI * 1.5 * 1.5 * 2.0;
    let hole = std::f64::consts::PI * 1.0 * 5.0;
    assert!((stock.removed() - slot - hole).abs() < 0.15 * (slot + hole), "{}", stock.removed());
    // A sloped move cuts a ramp.
    let mut ramp = Stock::new([-10.0, -10.0, -5.0], [10.0, 10.0, 0.0], 0.25).unwrap();
    ramp.sweep(P3::new(-8.0, 0.0, 0.0), P3::new(8.0, 0.0, -4.0), &flat);
    // At X 0 the lowest of the cutter is its edge 1.5 mm on, where the tip is 0.375 mm lower.
    assert!((at(&ramp, 0.0, 0.0) + 2.375).abs() < 0.05, "{}", at(&ramp, 0.0, 0.0));
    // A whole contour played back: the ring around the part goes to the bottom, the part stays.
    let tool = Tool::endmill(1, 3.0, 2, 12.0);
    let op =
        ContourOp { profile: Loop::rectangle((-5.0, -5.0), (5.0, 5.0)), side: Side::Outside, depths: Depths { top: 0.0, bottom: -5.5 }, tabs: None };
    let path = generate(&job("ring", Operation::Contour(op), tool), &Clearance::default()).unwrap();
    let stock = Stock::new([-10.0, -10.0, -5.0], [10.0, 10.0, 0.0], 0.25).unwrap();
    let mut run = Run::new(stock, vec![RunOp::new("ring".into(), &path, 0.5)]);
    assert!(!run.advance(f64::INFINITY, true), "a contour's own moves don't crash: {:?}", run.events);
    assert!(run.done() && run.events.is_empty());
    assert_eq!(at(&run.stock, 0.0, 0.0), 0.0, "the part is untouched");
    assert_eq!(at(&run.stock, 6.5, 0.0), -5.0, "the slot around it is through");
    assert_eq!(at(&run.stock, 9.5, 9.5), 0.0, "the corner of the stock beyond the cutter stays");
    // Bad blocks make no stock.
    assert!(Stock::new([0.0; 3], [0.0; 3], 1.0).is_none());
    assert!(Stock::new([0.0; 3], [f64::NAN, 1.0, 1.0], 1.0).is_none());
    // A huge block is split into at most MAX_CELLS columns.
    let big = Stock::new([0.0; 3], [5000.0, 5000.0, 10.0], 0.1).unwrap();
    assert!(big.nx * big.ny <= crate::sim::MAX_CELLS);
}

/// The height of the column nearest `(x, y)`.
fn at(s: &crate::sim::Stock, x: f64, y: f64) -> f64 {
    let i = ((x - s.x0) / s.dx).round() as usize;
    let j = ((y - s.y0) / s.dy).round() as usize;
    s.column(i, j).unwrap().z
}

#[test]
fn a_cutter_cuts_its_own_shape() {
    use crate::sim::{Cutter, Stock};
    let fresh = || Stock::new([-10.0, -10.0, -10.0], [10.0, 10.0, 0.0], 0.1).unwrap();
    // A ball end leaves a round groove: as deep as its tip in the middle, 1 mm higher 2.65 mm
    // out on a 4 mm ball (r − √(r² − d²) with r = 4, d = √7).
    let mut s = fresh();
    s.sweep(P3::new(-8.0, 0.0, -3.0), P3::new(8.0, 0.0, -3.0), &Cutter::ball(4.0, 20.0));
    assert!((at(&s, 0.0, 0.0) + 3.0).abs() < 1e-3);
    assert!((at(&s, 0.0, 7f64.sqrt()) + 2.0).abs() < 0.05, "{}", at(&s, 0.0, 7f64.sqrt()));
    assert_eq!(at(&s, 0.0, 4.5), 0.0);
    // A drill's 118° point: the hole's wall is 0.6 r above its tip.
    let drill = Cutter::drill(3.0, 30.0);
    assert!((drill.point_length() - 3.0 / 59f64.to_radians().tan()).abs() < 1e-9);
    let mut s = fresh();
    s.sweep(P3::new(0.0, 0.0, -6.0), P3::new(0.0, 0.0, -6.0), &drill);
    assert!((at(&s, 0.0, 0.0) + 6.0).abs() < 0.05);
    assert!((at(&s, 2.9, 0.0) + 6.0 - 2.9 / 59f64.to_radians().tan()).abs() < 0.1);
    // Cutters that aren't one cut nothing.
    let mut s = fresh();
    s.sweep(P3::new(0.0, 0.0, -6.0), P3::new(1.0, 0.0, -6.0), &Cutter::flat(f64::NAN, 10.0));
    s.sweep(P3::new(f64::INFINITY, 0.0, -6.0), P3::new(1.0, 0.0, -6.0), &Cutter::flat(1.0, 10.0));
    assert_eq!(s.removed(), 0.0);
}

#[test]
fn a_playback_finds_rapids_into_material_and_shanks_that_rub() {
    use crate::sim::{Crash, Cutter, Run, RunOp, Stock};
    let fresh = || Stock::new([-10.0, -10.0, -10.0], [10.0, 10.0, 0.0], 0.25).unwrap();
    let p = |x: f64, y: f64, z: f64| P3::new(x, y, z);
    // Down at a feed, then a rapid sideways through the stock, then up.
    let op = RunOp {
        name: "bad".into(),
        cutter: Cutter::flat(1.5, 20.0),
        points: vec![(p(-5.0, 0.0, 5.0), true), (p(-5.0, 0.0, -2.0), false), (p(5.0, 0.0, -2.0), true), (p(5.0, 0.0, 5.0), true)],
    };
    let mut run = Run::new(fresh(), vec![op]);
    assert!(run.advance(f64::INFINITY, true), "the rapid sideways crashes");
    assert!(!run.done(), "and the playback halts there");
    let e = run.events[0];
    assert_eq!(e.crash, Crash::Rapid);
    assert!(e.at.x > -5.0 && e.at.x < 5.0 && (e.at.z + 2.0).abs() < 1e-9, "{e:?}");
    // Played on, the same crash isn't found again; the rapid cut its way through.
    assert!(!run.advance(f64::INFINITY, true));
    assert!(run.done() && run.events.len() == 1);
    assert!((at(&run.stock, 0.0, 0.0) + 2.0).abs() < 1e-9);
    // Going back and playing again finds nothing new.
    run.seek(0.0);
    assert_eq!(run.stock.removed(), 0.0);
    run.seek(run.total());
    assert_eq!(run.events.len(), 1);
    // A cut 8 mm deep with 5 mm flutes: the shank rubs on the material above them.
    let deep = RunOp {
        name: "deep".into(),
        cutter: Cutter::flat(1.5, 5.0),
        points: vec![(p(-5.0, 0.0, 5.0), true), (p(-5.0, 0.0, -8.0), false), (p(5.0, 0.0, -8.0), false)],
    };
    let mut run = Run::new(fresh(), vec![deep]);
    run.advance(f64::INFINITY, false);
    assert_eq!(run.events.len(), 1, "one crash over the moves it lasts: {:?}", run.events);
    assert_eq!(run.events[0].crash, Crash::Shank);
    assert_eq!((run.events[0].first, run.events[0].last), (1, 2));
    // With long enough flutes it's fine.
    let fine = RunOp {
        name: "fine".into(),
        cutter: Cutter::flat(1.5, 12.0),
        points: vec![(p(-5.0, 0.0, 5.0), true), (p(-5.0, 0.0, -8.0), false), (p(5.0, 0.0, -8.0), false), (p(5.0, 0.0, 5.0), true)],
    };
    let mut run = Run::new(fresh(), vec![fine]);
    assert!(!run.advance(f64::INFINITY, true) && run.events.is_empty());
}

#[test]
fn a_playback_seeks_steps_and_knows_where_each_operation_starts() {
    use crate::sim::{Cutter, Run, RunOp, Stock};
    let p = |x: f64, y: f64, z: f64| P3::new(x, y, z);
    let line = |name: &str, y: f64| RunOp {
        name: name.into(),
        cutter: Cutter::flat(1.0, 20.0),
        points: vec![(p(-5.0, y, 5.0), true), (p(-5.0, y, -1.0), false), (p(5.0, y, -1.0), false), (p(5.0, y, 5.0), true)],
    };
    let stock = Stock::new([-10.0, -10.0, -10.0], [10.0, 10.0, 0.0], 0.25).unwrap();
    let mut run = Run::new(stock, vec![line("a", -3.0), line("b", 3.0)]);
    // Each: 6 down, 10 across, 0.6 up (a 6 mm rapid counts a tenth).
    assert!((run.total() - 2.0 * 16.6).abs() < 1e-9, "{}", run.total());
    assert!((run.start_of(1) - 16.6).abs() < 1e-9);
    run.step();
    assert!((run.tip().z + 1.0).abs() < 1e-9 && (run.tip().x + 5.0).abs() < 1e-9, "a move: down");
    run.step();
    assert!((run.tip().x - 5.0).abs() < 1e-9, "the next: across");
    run.seek(run.start_of(1) + 1.0);
    assert_eq!(run.op_index(), 1);
    assert!((at(&run.stock, 0.0, -3.0) + 1.0).abs() < 1e-9, "the first is cut");
    assert_eq!(at(&run.stock, 0.0, 3.0), 0.0, "the second not yet");
    let version = run.version;
    run.seek(5.0);
    assert!(run.version > version);
    assert_eq!(at(&run.stock, 0.0, -3.0), 0.0, "back before the first's cut");
    run.seek(f64::NAN);
    assert!(run.done());
}

#[test]
fn the_stock_is_compared_with_the_part_and_drawn_as_merged_quads() {
    use crate::sim::{Shade, Stock, surface};
    // A 10 × 10 stock 0 … 2 high; the part a 4 × 4 block 1 high in its middle.
    let mut s = Stock::new([0.0, 0.0, 0.0], [10.0, 10.0, 2.0], 0.5).unwrap();
    let (a, b) = ((3.0, 3.0), (7.0, 7.0));
    let tris = [[[a.0, a.1, 1.0], [b.0, a.1, 1.0], [b.0, b.1, 1.0]], [[a.0, a.1, 1.0], [b.0, b.1, 1.0], [a.0, b.1, 1.0]]];
    let target = s.target(tris);
    let k = |x: f64, y: f64, s: &Stock| (((y - s.y0) / s.dy).round() as usize, ((x - s.x0) / s.dx).round() as usize);
    let (j, i) = k(5.0, 5.0, &s);
    assert_eq!(target[j * s.nx + i], 1.0);
    let (j0, i0) = k(1.0, 1.0, &s);
    assert_eq!(target[j0 * s.nx + i0], f64::NEG_INFINITY);
    // Uncut: 1 mm left over the part, plain stock beyond it.
    assert_eq!(s.shade(Some(&target), i, j, 0.05), Shade::Left(2));
    assert_eq!(s.shade(Some(&target), i0, j0, 0.05), Shade::Uncut);
    assert_eq!(s.shade(None, i, j, 0.05), Shade::Uncut);
    // Cut to 1.3: a little left; to the part; below it; and beyond the part.
    let set = |s: &mut Stock, i: usize, j: usize, h: f64| {
        let n = s.nx;
        s.heights[j * n + i] = h;
    };
    set(&mut s, i, j, 1.3);
    assert_eq!(s.shade(Some(&target), i, j, 0.05), Shade::Left(1));
    set(&mut s, i, j, 1.03);
    assert_eq!(s.shade(Some(&target), i, j, 0.05), Shade::OnPart);
    set(&mut s, i, j, 0.5);
    assert_eq!(s.shade(Some(&target), i, j, 0.05), Shade::Gouge);
    set(&mut s, i0, j0, 0.0);
    assert_eq!(s.shade(Some(&target), i0, j0, 0.05), Shade::Cut);
    // Beside the part's edge a column cut to the floor isn't a gouge.
    let (je, ie) = k(2.75, 5.0, &s);
    set(&mut s, ie, je, 0.0);
    assert_ne!(s.shade(Some(&target), ie, je, 0.05), Shade::Gouge);
    // An uncut block is six quads; each faces the way its corners turn.
    let block = Stock::new([0.0, 0.0, 0.0], [10.0, 10.0, 2.0], 0.5).unwrap();
    let quads = surface(&block, |_, _| Shade::Uncut);
    assert_eq!(quads.len(), 6, "{quads:?}");
    let faces_out = |q: &crate::sim::Quad| {
        let [p0, p1, p2, _] = q.corners;
        let (u, v) = ([p1.x - p0.x, p1.y - p0.y, p1.z - p0.z], [p2.x - p0.x, p2.y - p0.y, p2.z - p0.z]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        n[0] * q.normal[0] + n[1] * q.normal[1] + n[2] * q.normal[2] > 0.0
    };
    assert!(quads.iter().all(faces_out));
    // A pocket adds its floor and four walls and splits the top around it; still all outward.
    let mut pocket = block.clone();
    for j in 8..12 {
        for i in 8..12 {
            set(&mut pocket, i, j, 1.0);
        }
    }
    let quads = surface(&pocket, |_, _| Shade::Cut);
    assert!(quads.iter().all(faces_out));
    let walls = quads.iter().filter(|q| q.normal[2] == 0.0 && q.corners.iter().all(|c| c.z >= 1.0)).count();
    assert_eq!(walls, 4, "{quads:?}");
    let area: f64 = quads
        .iter()
        .filter(|q| q.normal == [0.0, 0.0, 1.0])
        .map(|q| (q.corners[2].x - q.corners[0].x).abs() * (q.corners[2].y - q.corners[0].y).abs())
        .sum();
    assert!((area - 100.0).abs() < 1e-9, "the tops cover the block once: {area}");
}

#[test]
fn a_tool_keeps_its_shape_holder_and_cutting_data_per_material() {
    use crate::sim::{Crash, Cutter, Run, RunOp, Stock};
    use crate::tool::{Holder, HolderSection, Preset};
    let mut t = Tool::endmill(1, 6.0, 2, 15.0);
    assert!(t.problem().is_none());
    // Its own data for wood over the starting values; acrylic keeps the starting values.
    t.details.presets.push(Preset {
        material: Material::Wood,
        spindle_rpm: Some(12_000.0),
        feed: Some(900.0),
        plunge_feed: None,
        stepdown: None,
        stepover: None,
    });
    let wood = t.cutting(Material::Wood);
    assert_eq!((wood.spindle_rpm, wood.feed), (12_000.0, 900.0));
    assert_eq!(wood.stepdown, Cutting::starting(Material::Wood, &t).stepdown);
    assert_eq!(t.cutting(Material::Acrylic), Cutting::starting(Material::Acrylic, &t));
    // A bull nose needs its corner; a V-bit its tip narrower than itself; a holder its sections.
    let mut bull = t.clone();
    bull.kind = ToolKind::BullNose;
    assert!(bull.problem().is_some());
    bull.details.corner_radius = Some(1.0);
    assert!(bull.problem().is_none());
    let mut v = t.clone();
    v.kind = ToolKind::VBit;
    v.details.tip_diameter = Some(6.0);
    assert!(v.problem().is_some());
    let mut held = t.clone();
    held.details.holder = Some(Holder { name: "ER11".into(), sections: vec![] });
    assert!(held.problem().is_some());
    held.details.holder = Some(Holder { name: "ER11".into(), sections: vec![HolderSection { diameter: 19.0, length: 20.0 }] });
    held.details.stickout = Some(18.0);
    assert!(held.problem().is_none());
    // The cutters they cut as.
    assert_eq!(Cutter::of(&t).point_length(), 0.0);
    let ball = {
        let mut b = t.clone();
        b.kind = ToolKind::BallEndmill;
        Cutter::of(&b)
    };
    assert!((ball.point_length() - 3.0).abs() < 1e-9);
    let bull = Cutter::of(&bull);
    assert_eq!(bull.lift(1.5), Some(0.0), "flat inside the corners");
    assert!((bull.point_length() - 1.0).abs() < 1e-9);
    let cut = Cutter::of(&held);
    assert_eq!((cut.stickout, cut.holder_radius), (18.0, 9.5));
    // A cut 20 mm deep with 18 mm out of the holder: the holder meets the stock's top.
    let stock = Stock::new([-30.0, -30.0, -25.0], [30.0, 30.0, 0.0], 0.5).unwrap();
    let p = |x: f64, z: f64| P3::new(x, 0.0, z);
    let mut deep = cut;
    deep.flute_length = 30.0;
    let op = RunOp { name: "deep".into(), cutter: deep, points: vec![(p(-10.0, 5.0), true), (p(-10.0, -20.0), false), (p(10.0, -20.0), false)] };
    let mut run = Run::new(stock, vec![op]);
    run.advance(f64::INFINITY, false);
    assert!(run.events.iter().any(|e| e.crash == Crash::Holder), "{:?}", run.events);
}
