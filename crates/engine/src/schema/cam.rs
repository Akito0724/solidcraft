//! Parameters and results of the machining commands (`cam.*`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Add a cutter to the design's tool library, or replace the one with the same number.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamToolParams {
    /// The number programs call it by (T1…T99).
    #[schemars(range(min = 1, max = 99))]
    pub number: u32,
    /// Cutting diameter (mm).
    pub diameter: f64,
    /// Default: 2.
    #[serde(default)]
    pub flutes: Option<u32>,
    /// How deep it can cut (mm). Default: 4 × the diameter.
    #[serde(default)]
    pub flute_length: Option<f64>,
    /// `flat_endmill` (default) or `drill`.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

/// One cutter of the library.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamToolInfo {
    pub number: u32,
    pub name: String,
    /// `flat_endmill` or `drill`.
    pub kind: String,
    pub diameter: f64,
    pub flutes: u32,
    pub flute_length: f64,
}

/// What `cam.tool` returns: the library after the change.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamToolsResult {
    pub tools: Vec<CamToolInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Make a setup (no `id`) or change one: the body machined, the material, the work origin,
/// whether the part is turned over, and the stock around it.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamSetupParams {
    /// The setup to change. Default: a new one.
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
    /// The body machined. Default for a new setup: the model's only body.
    #[serde(default)]
    pub body: Option<String>,
    /// `wood` (default), `acrylic` or `aluminum`: the starting cutting data.
    #[serde(default)]
    pub material: Option<String>,
    /// `top_center` (default) or `top_corner` (front left), on the stock's upward face.
    #[serde(default)]
    pub origin: Option<String>,
    /// Turn the part over to cut its bottom.
    #[serde(default)]
    pub flipped: Option<bool>,
    /// Stock beyond the part on each side (mm). Default 3.
    #[serde(default)]
    pub margin_side: Option<f64>,
    /// Stock above the part (mm). Default 1.
    #[serde(default)]
    pub margin_top: Option<f64>,
    /// Stock below the part (mm). Default 0.
    #[serde(default)]
    pub margin_bottom: Option<f64>,
    /// Height clear of the stock and clamps (mm, machine Z). Default 10.
    #[serde(default)]
    pub safe_z: Option<f64>,
    /// Lift between cuts (mm). Default 1.
    #[serde(default)]
    pub retract: Option<f64>,
}

/// What `cam.setup` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamSetupResult {
    pub id: u64,
    pub name: String,
    pub body: String,
    /// The stock's size: X, Y, Z (mm).
    pub stock: [f64; 3],
    /// The part's top in machine Z: below 0 when the stock has a margin above it.
    pub part_top: f64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A face or edge of the model: a point on it, and its body (default: the setup's).
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamPickParam {
    pub at: [f64; 3],
    #[serde(default)]
    pub body: Option<String>,
}

/// Bridges left at the bottom of a contour.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamTabsParam {
    pub count: u32,
    /// Along the profile (mm).
    pub width: f64,
    /// Above the contour's bottom (mm).
    pub height: f64,
}

/// Add an operation to a setup (no `id`) or change one. It is refused when its toolpath can't
/// be made, saying why.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamOperationParams {
    /// The operation to change. Default: a new one.
    #[serde(default)]
    pub id: Option<u64>,
    /// The setup. Default: the only one.
    #[serde(default)]
    pub setup: Option<u64>,
    /// `face`, `pocket`, `contour` or `drill`. Required for a new operation.
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    /// The tool's number in the library. Required for a new operation.
    #[serde(default)]
    pub tool: Option<u32>,
    #[serde(default)]
    pub name: Option<String>,
    /// Pocket: its floor. Contour: the face whose outer boundary is followed.
    #[serde(default)]
    pub face: Option<CamPickParam>,
    /// Drill: the holes' round edges.
    #[serde(default)]
    pub edges: Option<Vec<CamPickParam>>,
    /// Contour: `outside` (default), `inside` or `on`.
    #[serde(default)]
    pub side: Option<String>,
    /// Below the stock's top (mm). Face default: down to the part's top; contour and drill
    /// default: through the stock.
    #[serde(default)]
    pub depth: Option<f64>,
    /// Contour: tabs to hold the part; `count` 0 removes them.
    #[serde(default)]
    pub tabs: Option<CamTabsParam>,
    /// Drill: depth per peck (mm); 0 drills in one go.
    #[serde(default)]
    pub peck: Option<f64>,
    /// Cutting data other than the material's starting values.
    #[serde(default)]
    pub spindle_rpm: Option<f64>,
    #[serde(default)]
    pub feed: Option<f64>,
    #[serde(default)]
    pub plunge_feed: Option<f64>,
    #[serde(default)]
    pub stepdown: Option<f64>,
    /// As a fraction of the tool's diameter.
    #[serde(default)]
    pub stepover: Option<f64>,
}

/// One operation and its toolpath.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamOpSummary {
    pub id: u64,
    pub name: String,
    /// `face`, `pocket`, `contour` or `drill`.
    pub kind: String,
    pub tool: u32,
    /// Cutting travel (mm).
    pub cut_mm: f64,
    /// Rapid travel (mm).
    pub rapid_mm: f64,
    /// A rough machining time (min).
    pub minutes: f64,
    /// Why its toolpath can't be made, when it can't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What `cam.operation` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamOperationResult {
    pub operation: CamOpSummary,
    pub setup: u64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Delete an operation, or a setup with its operations, or (`tool`) a cutter no operation uses.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamDeleteParams {
    /// A setup's or an operation's id.
    #[serde(default)]
    pub id: Option<u64>,
    /// A tool's number.
    #[serde(default)]
    pub tool: Option<u32>,
}

/// What `cam.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamDeleted {
    /// The setups and operations deleted.
    pub ids: Vec<u64>,
    /// The tool deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<u32>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Work out the toolpaths of a setup, or of every setup, and report them.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamGenerateParams {
    #[serde(default)]
    pub setup: Option<u64>,
}

/// A setup and its operations' toolpaths.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamSetupSummary {
    pub id: u64,
    pub name: String,
    pub body: String,
    /// `wood`, `acrylic` or `aluminum`.
    pub material: String,
    pub flipped: bool,
    /// The stock's size: X, Y, Z (mm).
    pub stock: [f64; 3],
    pub operations: Vec<CamOpSummary>,
    /// Why the setup can't be worked out (its body is gone…), when it can't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What `cam.generate` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamGenerated {
    pub setups: Vec<CamSetupSummary>,
    pub tools: Vec<CamToolInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Write a setup's G-code: one GRBL program per tool, in the operations' order, named after
/// `path` with the order and tool added (`case.nc` → `case_01_T2.nc`).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamExportParams {
    #[schemars(length(min = 1))]
    pub path: String,
    /// Default: the only setup.
    #[serde(default)]
    pub setup: Option<u64>,
    /// Write arcs as G2/G3 (default true); false writes short straight moves.
    #[serde(default)]
    pub arcs: Option<bool>,
}

/// One program written.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamProgram {
    pub path: String,
    pub tool: u32,
    pub operations: Vec<String>,
    pub minutes: f64,
}

/// What `cam.export` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamExported {
    pub files: Vec<CamProgram>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The toolpaths of a setup in model coordinates, to draw over the model.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamToolpathsParams {
    /// Default: the only setup.
    #[serde(default)]
    pub setup: Option<u64>,
    /// Longest straight piece an arc is drawn with (mm). Default 1.
    #[serde(default)]
    pub step: Option<f64>,
}

/// One operation's toolpath as runs of points: cutting runs and rapid moves apart.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamPathLines {
    pub operation: u64,
    pub name: String,
    pub tool: u32,
    /// Runs of points the tool cuts along (model coordinates, mm).
    pub cuts: Vec<Vec<[f64; 3]>>,
    /// Rapid moves, from and to.
    pub rapids: Vec<[[f64; 3]; 2]>,
    /// Why its toolpath can't be made, when it can't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What `cam.toolpaths` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamToolpaths {
    pub setup: u64,
    pub paths: Vec<CamPathLines>,
    /// True when the points were cut short (very long toolpaths).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
