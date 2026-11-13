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
    /// A ball end mill: its end a half sphere.
    BallEndmill,
    /// A bull nose end mill: a flat end with rounded corners ([`ToolDetails::corner_radius`]).
    BullNose,
    /// A V-bit for chamfers and engraving: a cone of [`ToolDetails::tip_angle`] from a tip of
    /// [`ToolDetails::tip_diameter`].
    VBit,
}

impl ToolKind {
    /// Whether it mills sideways (contours, pockets, facing).
    pub fn mills(self) -> bool {
        matches!(self, ToolKind::FlatEndmill | ToolKind::BallEndmill | ToolKind::BullNose)
    }
}

/// One cylinder of a holder, from the holder's face up.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HolderSection {
    /// Diameter and length (mm).
    pub diameter: f64,
    pub length: f64,
}

/// What holds the tool (a collet chuck, a collet nut), as stacked cylinders from its face up.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Holder {
    pub name: String,
    pub sections: Vec<HolderSection>,
}

impl Holder {
    /// Why it can't be used, if it can't.
    pub fn problem(&self) -> Option<String> {
        if self.sections.is_empty() || self.sections.len() > 8 {
            return Some(format!("{}: a holder has 1 to 8 sections", self.name));
        }
        let ok = |v: f64| v.is_finite() && v > 0.0 && v <= 500.0;
        if !self.sections.iter().all(|s| ok(s.diameter) && ok(s.length)) {
            return Some(format!("{}: a holder's sections must be 0 to 500 mm across and long", self.name));
        }
        None
    }

    pub fn length(&self) -> f64 {
        self.sections.iter().map(|s| s.length).sum()
    }
}

/// Cutting data a tool keeps for one material, over the starting values (each `None` keeps
/// the starting value).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Preset {
    pub material: Material,
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    pub spindle_rpm: Option<f64>,
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    pub feed: Option<f64>,
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    pub plunge_feed: Option<f64>,
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    pub stepdown: Option<f64>,
    /// A fraction of the diameter.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    pub stepover: Option<f64>,
}

/// The shared tool library entry a tool was copied from, and the revision it was copied at.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LibraryLink {
    pub id: String,
    pub revision: String,
}

/// What a tool keeps besides what every cut needs: its whole shape, the holder, where it came
/// from, and cutting data per material.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct ToolDetails {
    /// The plain shank's diameter (mm); `None`: the cutting diameter.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub shank_diameter: Option<f64>,
    /// From tip to the end of the shank (mm).
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub overall_length: Option<f64>,
    /// How far it stands out of the holder (mm); `None`: the flute length plus 5 mm.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub stickout: Option<f64>,
    /// A bull nose's corner radius (mm).
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub corner_radius: Option<f64>,
    /// A drill's or V-bit's included point angle (degrees): 118 for a drill by default.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub tip_angle: Option<f64>,
    /// A V-bit's flat tip (mm); `None`: pointed.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub tip_diameter: Option<f64>,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "String::is_empty"))]
    pub maker: String,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "String::is_empty"))]
    pub model: String,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "String::is_empty"))]
    pub notes: String,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub holder: Option<Holder>,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Vec::is_empty"))]
    pub presets: Vec<Preset>,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub library: Option<LibraryLink>,
}

impl ToolDetails {
    pub fn is_empty(&self) -> bool {
        *self == ToolDetails::default()
    }
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
    /// Its whole shape, holder and cutting data per material.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "ToolDetails::is_empty"))]
    pub details: ToolDetails,
}

impl Tool {
    /// A flat end mill.
    pub fn endmill(number: u32, diameter: f64, flutes: u32, flute_length: f64) -> Tool {
        Tool {
            number,
            name: format!("{diameter} mm {flutes}-flute end mill"),
            kind: ToolKind::FlatEndmill,
            diameter,
            flute_length,
            flutes,
            details: ToolDetails::default(),
        }
    }

    /// The shank's diameter.
    pub fn shank(&self) -> f64 {
        self.details.shank_diameter.unwrap_or(self.diameter)
    }

    /// How far it stands out of the holder.
    pub fn stickout(&self) -> f64 {
        self.details.stickout.unwrap_or(self.flute_length + 5.0)
    }

    /// The point's included angle (degrees) of a drill or V-bit.
    pub fn tip_angle(&self) -> f64 {
        self.details.tip_angle.unwrap_or(match self.kind {
            ToolKind::VBit => 60.0,
            _ => 118.0,
        })
    }

    /// Its cutting data for `material`: the starting values, with what it keeps for the
    /// material over them.
    pub fn cutting(&self, material: Material) -> Cutting {
        let mut c = Cutting::starting(material, self);
        if let Some(p) = self.details.presets.iter().find(|p| p.material == material) {
            let set = |v: &mut f64, o: Option<f64>| {
                if let Some(o) = o.filter(|o| o.is_finite() && *o > 0.0) {
                    *v = o;
                }
            };
            set(&mut c.spindle_rpm, p.spindle_rpm);
            set(&mut c.feed, p.feed);
            set(&mut c.plunge_feed, p.plunge_feed);
            set(&mut c.stepdown, p.stepdown);
            set(&mut c.stepover, p.stepover);
        }
        c
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
        let d = &self.details;
        let within = |v: Option<f64>, lo: f64, hi: f64| v.is_none_or(|v| v.is_finite() && v > lo && v <= hi);
        if !within(d.shank_diameter, 0.0, 100.0) || !within(d.overall_length, 0.0, 500.0) || !within(d.stickout, 0.0, 500.0) {
            return Some(format!("{}: the shank, overall length and stickout must be 0 to 500 mm", self.name));
        }
        if d.overall_length.is_some_and(|l| l < self.flute_length) {
            return Some(format!("{}: the overall length is shorter than the flutes", self.name));
        }
        if self.kind == ToolKind::BullNose && !d.corner_radius.is_some_and(|r| r.is_finite() && r > 0.0 && r < self.radius()) {
            return Some(format!("{}: a bull nose needs a corner radius above 0 and below the tool's radius", self.name));
        }
        if !within(d.tip_angle, 0.0, 179.0) {
            return Some(format!("{}: the point angle must be above 0 and below 180°", self.name));
        }
        if d.tip_diameter.is_some_and(|t| !(t.is_finite() && t >= 0.0 && t < self.diameter)) {
            return Some(format!("{}: the tip must be narrower than the tool", self.name));
        }
        if let Some(problem) = d.holder.as_ref().and_then(Holder::problem) {
            return Some(problem);
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
            ToolKind::FlatEndmill | ToolKind::BallEndmill | ToolKind::BullNose | ToolKind::VBit => (feed / 3.0).max(30.0),
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
