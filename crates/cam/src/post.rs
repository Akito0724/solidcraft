//! GRBL G-code: one program per tool, since a machine without a tool changer stops for each.

use crate::tool::{Tool, ToolKind};
use crate::toolpath::{Move, Toolpath};

/// How programs are written.
#[derive(Clone, Debug, PartialEq)]
pub struct PostOptions {
    /// The program's title, written as its first comment.
    pub title: String,
    /// Digits after the decimal point for coordinates.
    pub decimals: usize,
    /// Seconds to wait after starting the spindle, so it is at speed before the first cut.
    pub spindle_dwell: f64,
    /// Write arcs as `G2`/`G3` (otherwise as short straight moves, for controllers without arcs).
    pub arcs: bool,
}

impl Default for PostOptions {
    fn default() -> Self {
        PostOptions { title: String::new(), decimals: 3, spindle_dwell: 2.0, arcs: true }
    }
}

/// The toolpaths grouped by tool, keeping their order: a new group starts where the tool changes.
pub fn by_tool(paths: &[Toolpath]) -> Vec<(Tool, Vec<&Toolpath>)> {
    let mut groups: Vec<(Tool, Vec<&Toolpath>)> = Vec::new();
    for p in paths {
        match groups.last_mut() {
            Some((tool, list)) if tool.number == p.tool.number && tool.diameter == p.tool.diameter => list.push(p),
            _ => groups.push((p.tool.clone(), vec![p])),
        }
    }
    groups
}

/// The G-code for `paths`, all cut with one tool (the first path's), as one program.
pub fn grbl(paths: &[&Toolpath], options: &PostOptions) -> String {
    let mut w = Writer::new(options.decimals);
    let Some(first) = paths.first() else { return String::new() };
    let tool = &first.tool;
    if !options.title.is_empty() {
        w.line(format!("({})", comment(&options.title)));
    }
    w.line(format!("(T{} {} mm {}: {})", tool.number, w.num(tool.diameter), kind_name(tool.kind), comment(&tool.name)));
    let (rapid, cut) = paths.iter().map(|p| p.lengths()).fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
    let seconds: f64 = paths.iter().map(|p| p.estimate_seconds(3000.0)).sum();
    w.line(format!("(cutting {} mm, rapids {} mm, about {} min)", w.num1(cut), w.num1(rapid), w.num1(seconds / 60.0)));
    w.line("G21 G90 G94 G17".into());
    let safe = paths.iter().map(|p| p.safe_z).fold(f64::MIN, f64::max);
    w.line(format!("G0 Z{}", w.num(safe)));
    w.z = Some(safe);
    let rpm = first.cutting.spindle_rpm;
    w.line(format!("M3 S{}", rpm.round()));
    if options.spindle_dwell > 0.0 {
        w.line(format!("G4 P{}", w.num1(options.spindle_dwell)));
    }
    for p in paths {
        w.line(format!("({})", comment(&p.name)));
        if (p.cutting.spindle_rpm - rpm).abs() > 0.5 {
            w.line(format!("S{}", p.cutting.spindle_rpm.round()));
        }
        let mut at: Option<crate::toolpath::P3> = None;
        for m in &p.moves {
            match m {
                Move::Rapid(to) => w.motion("G0", to.x, to.y, to.z, None),
                Move::Feed { to, feed } => w.motion("G1", to.x, to.y, to.z, Some(*feed)),
                Move::Arc { to, center, ccw, feed } => match (options.arcs, at) {
                    (true, Some(from)) => {
                        let code = if *ccw { "G3" } else { "G2" };
                        w.arc(code, to.x, to.y, to.z, center.0 - from.x, center.1 - from.y, *feed);
                    }
                    (_, Some(from)) => {
                        let seg = crate::toolpath::arc_segment(from, *to, *center, *ccw);
                        let pieces = ((seg.length() / 0.5).ceil() as usize).clamp(1, 720);
                        for i in 1..=pieces {
                            let t = i as f64 / pieces as f64;
                            let (x, y) = seg.at(t);
                            w.motion("G1", x, y, from.z + (to.z - from.z) * t, Some(*feed));
                        }
                    }
                    (_, None) => w.motion("G1", to.x, to.y, to.z, Some(*feed)),
                },
            }
            at = Some(m.to());
        }
    }
    if !w.same(w.z, safe) {
        w.line(format!("G0 Z{}", w.num(safe)));
    }
    w.line("M5".into());
    w.line("M30".into());
    w.out
}

fn kind_name(k: ToolKind) -> &'static str {
    match k {
        ToolKind::FlatEndmill => "flat end mill",
        ToolKind::Drill => "drill",
        ToolKind::BallEndmill => "ball end mill",
        ToolKind::BullNose => "bull nose end mill",
        ToolKind::VBit => "V-bit",
    }
}

/// Text safe inside a G-code comment: no parentheses, no line breaks, ASCII only (GRBL reads
/// bytes).
fn comment(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '(' | ')' | ';' => ' ',
            c if c.is_ascii() && !c.is_control() => c,
            _ => '?',
        })
        .collect::<String>()
        .trim()
        .chars()
        .take(120)
        .collect()
}

/// Writes lines, leaving out words that repeat the modal state (motion, coordinates, feed).
struct Writer {
    out: String,
    decimals: usize,
    motion: Option<&'static str>,
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
    f: Option<f64>,
}

impl Writer {
    fn new(decimals: usize) -> Writer {
        Writer { out: String::new(), decimals: decimals.clamp(1, 6), motion: None, x: None, y: None, z: None, f: None }
    }

    fn line(&mut self, l: String) {
        self.out.push_str(&l);
        self.out.push('\n');
    }

    fn num(&self, v: f64) -> String {
        let s = format!("{v:.*}", self.decimals);
        let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
        if s == "-0" { "0".into() } else { s }
    }

    fn num1(&self, v: f64) -> String {
        format!("{v:.1}")
    }

    fn same(&self, a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| self.num(a) == self.num(b))
    }

    fn motion(&mut self, code: &'static str, x: f64, y: f64, z: f64, feed: Option<f64>) {
        let mut words = Vec::new();
        let (sx, sy, sz) = (self.same(self.x, x), self.same(self.y, y), self.same(self.z, z));
        if sx && sy && sz {
            return;
        }
        if self.motion != Some(code) {
            words.push(code.to_string());
        }
        if !sx {
            words.push(format!("X{}", self.num(x)));
        }
        if !sy {
            words.push(format!("Y{}", self.num(y)));
        }
        if !sz {
            words.push(format!("Z{}", self.num(z)));
        }
        if let Some(f) = feed
            && !self.f.is_some_and(|old| (old - f).abs() < 0.05)
        {
            words.push(format!("F{}", self.num1(f)));
            self.f = Some(f);
        }
        self.line(words.join(" "));
        (self.motion, self.x, self.y, self.z) = (Some(code), Some(x), Some(y), Some(z));
    }

    fn arc(&mut self, code: &'static str, x: f64, y: f64, z: f64, i: f64, j: f64, feed: f64) {
        let mut words = vec![code.to_string(), format!("X{}", self.num(x)), format!("Y{}", self.num(y))];
        if !self.same(self.z, z) {
            words.push(format!("Z{}", self.num(z)));
        }
        words.push(format!("I{}", self.num(i)));
        words.push(format!("J{}", self.num(j)));
        if !self.f.is_some_and(|old| (old - feed).abs() < 0.05) {
            words.push(format!("F{}", self.num1(feed)));
            self.f = Some(feed);
        }
        self.line(words.join(" "));
        (self.motion, self.x, self.y, self.z) = (Some(code), Some(x), Some(y), Some(z));
    }
}
