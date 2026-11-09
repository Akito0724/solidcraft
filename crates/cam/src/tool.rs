//! Cutters and the cutting data they run at.

/// What kind of cutter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ToolKind {
    /// A flat end mill, which cuts with its side and its end (centre cutting).
    FlatEndmill,
    /// A twist drill, which only plunges.
    Drill,
}

/// A cutter in the tool library.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tool {
    /// The number the program names it by (`T1`…).
    pub number: u32,
    pub name: String,
    pub kind: ToolKind,
    /// Cutting diameter (mm).
    pub diameter: f64,
    /// How deep the flutes reach (mm): no pass goes deeper below the stock top than this.
    pub flute_length: f64,
    pub flutes: u32,
}

impl Tool {
    /// A flat end mill.
    pub fn endmill(number: u32, diameter: f64, flutes: u32, flute_length: f64) -> Tool {
        Tool { number, name: format!("{diameter} mm {flutes}-flute end mill"), kind: ToolKind::FlatEndmill, diameter, flute_length, flutes }
    }

    pub fn radius(&self) -> f64 {
        self.diameter / 2.0
    }

    /// Why the tool can't cut, if it can't.
    pub fn problem(&self) -> Option<String> {
        if !(self.diameter > 0.0) || !self.diameter.is_finite() || self.diameter > 100.0 {
            return Some(format!("{}: the diameter must be between 0 and 100 mm", self.name));
        }
        if !(self.flute_length > 0.0) || !self.flute_length.is_finite() {
            return Some(format!("{}: the flute length must be more than 0 mm", self.name));
        }
        if self.flutes == 0 {
            return Some(format!("{}: a tool needs at least one flute", self.name));
        }
        None
    }
}

/// The material being cut, for the starting cutting data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Material {
    Wood,
    Acrylic,
    Aluminum,
}

/// How a tool runs: spindle speed, feeds and how much it takes per pass.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cutting {
    /// Spindle speed (rpm).
    pub spindle_rpm: f64,
    /// Cutting feed (mm/min).
    pub feed: f64,
    /// Feed going down into the material (mm/min).
    pub plunge_feed: f64,
    /// Depth taken per pass (mm).
    pub stepdown: f64,
    /// Sideways step between passes of a pocket or a facing, as a fraction of the diameter.
    pub stepover: f64,
}

impl Cutting {
    /// Starting values for a small hobby machine (a 3018-class router: about 10,000 rpm and a
    /// light frame). They are a first guess to adjust on the machine, not a recommendation.
    pub fn starting(material: Material, tool: &Tool) -> Cutting {
        let d = tool.diameter.max(0.1);
        let flutes = f64::from(tool.flutes.max(1));
        let rpm = 10_000.0;
        // Chip load per tooth (mm), depth per pass and stepover as fractions of the diameter.
        let (chip, down, over) = match material {
            Material::Wood => (if d <= 3.2 { 0.025 } else { 0.04 }, 0.5, 0.45),
            Material::Acrylic => (if d <= 3.2 { 0.03 } else { 0.05 }, 0.4, 0.4),
            Material::Aluminum => (if d <= 3.2 { 0.008 } else { 0.012 }, 0.1, 0.3),
        };
        let feed = (chip * flutes * rpm).clamp(100.0, 2000.0);
        let plunge = match tool.kind {
            ToolKind::Drill => (feed / 4.0).max(30.0),
            ToolKind::FlatEndmill => (feed / 3.0).max(30.0),
        };
        Cutting { spindle_rpm: rpm, feed: round1(feed), plunge_feed: round1(plunge), stepdown: round2(d * down), stepover: over }
    }

    /// Why these values can't be used, if they can't.
    pub fn problem(&self) -> Option<String> {
        let positive = |v: f64| v > 0.0 && v.is_finite();
        if !positive(self.spindle_rpm) || self.spindle_rpm > 100_000.0 {
            return Some("the spindle speed must be between 0 and 100,000 rpm".into());
        }
        if !positive(self.feed) || !positive(self.plunge_feed) {
            return Some("the feeds must be more than 0 mm/min".into());
        }
        if !positive(self.stepdown) {
            return Some("the depth per pass must be more than 0 mm".into());
        }
        if !(self.stepover > 0.0 && self.stepover <= 1.0) {
            return Some("the stepover must be more than 0 and at most the tool's diameter".into());
        }
        None
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
