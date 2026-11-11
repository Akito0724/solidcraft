//! SolidCraft CAM: 2.5D toolpaths for a three-axis router or mill, and the G-code that drives
//! it.
//!
//! Everything here works in the machine's coordinates (millimetres, Z up, the work origin at 0):
//! whoever asks for a toolpath has already placed the part's profiles in the setup's frame
//! ([`setup::Frame`]). Profiles are closed loops of lines and arcs ([`geom2::Loop`], an arc kept
//! as a bulge), so offsets stay arcs and come out as `G2`/`G3`.
//!
//! - [`tool`]: cutters and the cutting data for a material.
//! - [`ops`]: facing, pockets (with islands), contours (inside, outside or on, with tabs) and
//!   drilling (with pecking), each turned into a [`toolpath::Toolpath`].
//! - [`post`]: GRBL G-code, one program per tool, as a machine without a tool changer needs.
//! - [`sim`]: the stock as a height map, cut down along a toolpath to play it back.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod geom2;
pub mod ops;
pub mod post;
pub mod setup;
pub mod sim;
pub mod tool;
pub mod toolpath;

#[cfg(test)]
mod tests;

pub use geom2::{Loop, Region, Vertex};
pub use ops::{Clearance, ContourOp, Depths, DrillOp, FaceOp, Job, Operation, PocketOp, Side, Tabs, generate};
pub use post::{PostOptions, by_tool, grbl};
pub use setup::Frame;
pub use sim::{Playback, Stock};
pub use tool::{Cutting, Material, Tool, ToolKind};
pub use toolpath::{Move, P3, Toolpath};

/// Why a toolpath could not be made.
#[derive(Clone, Debug, PartialEq)]
pub enum CamError {
    /// An input the operation can't use (a depth, a tool size, an empty profile), with why.
    Invalid(String),
}

impl std::fmt::Display for CamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CamError::Invalid(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for CamError {}

pub type Result<T> = std::result::Result<T, CamError>;

/// The error for an input an operation can't use.
pub(crate) fn invalid(why: impl Into<String>) -> CamError {
    CamError::Invalid(why.into())
}
