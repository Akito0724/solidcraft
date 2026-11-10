#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::*;

fn wood(tool: &Tool) -> Cutting {
    Cutting::starting(Material::Wood, tool)
}

fn job(name: &str, operation: Operation, tool: Tool) -> Job {
    let cutting = wood(&tool);
    Job { name: name.into(), operation, tool, cutting }
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
    let drill = Tool { number: 3, name: "3 mm drill".into(), kind: ToolKind::Drill, diameter: 3.0, flute_length: 20.0, flutes: 2 };
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
fn turning_the_part_over_mirrors_y_and_keeps_arcs_round() {
    let f = Frame { origin: [10.0, 0.0, 20.0], flipped: true };
    assert_eq!(f.to_machine([10.0, 5.0, 20.0]), [0.0, -5.0, 0.0]);
    assert_eq!(f.to_model(f.to_machine([3.0, 4.0, 5.0])), [3.0, 4.0, 5.0]);
    assert_eq!(f.z_to_machine(0.0), 20.0);
    let circle = Loop::circle(10.0, 0.0, 5.0);
    let m = f.loop_to_machine(&circle);
    // Mirrored, the counter-clockwise circle runs clockwise, and stays a circle of radius 5.
    assert!(!m.is_ccw());
    assert!((m.area().abs() - std::f64::consts::PI * 25.0).abs() < 1e-6);
}

#[test]
fn starting_cutting_data_suits_the_material() {
    let t = Tool::endmill(1, 3.175, 2, 12.0);
    let wood = Cutting::starting(Material::Wood, &t);
    let alu = Cutting::starting(Material::Aluminum, &t);
    assert!(wood.feed > alu.feed && wood.stepdown > alu.stepdown);
    assert!(wood.problem().is_none() && alu.problem().is_none());
}
