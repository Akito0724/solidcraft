//! Where the part sits on the machine: the work origin, and which way the machine's axes run
//! through the model.
//!
//! A setup's frame is the machine's coordinate system in the model: an origin and three unit
//! axes (right-handed; the spindle comes down −Z). The model's profiles are found in the frame's
//! XY plane before any toolpath is made, so operations only ever see machine coordinates.

/// The machine frame of a setup.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Frame {
    /// The work origin, in model coordinates (mm).
    pub origin: [f64; 3],
    /// The machine's X, Y and Z as unit vectors in model coordinates.
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub z: [f64; 3],
}

impl Default for Frame {
    fn default() -> Self {
        Frame { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0] }
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `a × b`.
pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

impl Frame {
    /// The frame at `origin` with machine X along `x` and Z along `z` (Y follows, right-handed).
    /// `None` unless both are finite, non-zero and square to each other.
    pub fn new(origin: [f64; 3], x: [f64; 3], z: [f64; 3]) -> Option<Frame> {
        let unit = |v: [f64; 3]| {
            let n = dot(v, v).sqrt();
            (n.is_finite() && n > 1e-12).then(|| v.map(|c| c / n))
        };
        let (x, z) = (unit(x)?, unit(z)?);
        if dot(x, z).abs() > 1e-6 || !origin.iter().all(|v| v.is_finite()) {
            return None;
        }
        Some(Frame { origin, x, y: cross(z, x), z })
    }

    /// A model point in machine coordinates.
    pub fn to_machine(&self, p: [f64; 3]) -> [f64; 3] {
        let d = [p[0] - self.origin[0], p[1] - self.origin[1], p[2] - self.origin[2]];
        [dot(d, self.x), dot(d, self.y), dot(d, self.z)]
    }

    /// A machine point in model coordinates.
    pub fn to_model(&self, m: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|i| self.origin[i] + m[0] * self.x[i] + m[1] * self.y[i] + m[2] * self.z[i])
    }

    /// A machine direction in model coordinates.
    pub fn dir_to_model(&self, m: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|i| m[0] * self.x[i] + m[1] * self.y[i] + m[2] * self.z[i])
    }
}
