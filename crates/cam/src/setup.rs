//! Where the part sits on the machine: the work origin, and which side faces up.
//!
//! The model is Z up. A setup's frame puts the work origin at a model point and, for the second
//! side of a part, turns the part over about the machine's X axis. The model's profiles are
//! carried into the frame before any toolpath is made, so operations only ever see machine
//! coordinates.

use crate::geom2::{Loop, Vertex};

/// The machine frame of a setup.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Frame {
    /// The work origin, in model coordinates (mm).
    pub origin: [f64; 3],
    /// The part turned over (about X) for its second side: the model's bottom faces up.
    #[cfg_attr(feature = "serde", serde(default))]
    pub flipped: bool,
}

impl Default for Frame {
    fn default() -> Self {
        Frame { origin: [0.0; 3], flipped: false }
    }
}

impl Frame {
    /// A model point in machine coordinates.
    pub fn to_machine(&self, p: [f64; 3]) -> [f64; 3] {
        let d = [p[0] - self.origin[0], p[1] - self.origin[1], p[2] - self.origin[2]];
        if self.flipped { [d[0], -d[1], -d[2]] } else { d }
    }

    /// A machine point in model coordinates.
    pub fn to_model(&self, m: [f64; 3]) -> [f64; 3] {
        let d = if self.flipped { [m[0], -m[1], -m[2]] } else { m };
        [d[0] + self.origin[0], d[1] + self.origin[1], d[2] + self.origin[2]]
    }

    /// A model height (Z) as a machine height.
    pub fn z_to_machine(&self, z: f64) -> f64 {
        if self.flipped { -(z - self.origin[2]) } else { z - self.origin[2] }
    }

    /// A machine height (Z) as a model height.
    pub fn z_to_model(&self, z: f64) -> f64 {
        if self.flipped { self.origin[2] - z } else { z + self.origin[2] }
    }

    /// A profile drawn in the model's XY plane, seen from the machine. Turning the part over
    /// mirrors Y, which turns arcs the other way.
    pub fn loop_to_machine(&self, l: &Loop) -> Loop {
        let vertices = l
            .vertices
            .iter()
            .map(|v| {
                let m = self.to_machine([v.x, v.y, 0.0]);
                Vertex { x: m[0], y: m[1], bulge: if self.flipped { -v.bulge } else { v.bulge } }
            })
            .collect();
        Loop { vertices }
    }
}
