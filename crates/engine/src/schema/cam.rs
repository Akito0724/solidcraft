//! Parameters and results of the machining commands (`cam.*`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Add a cutter to the design's tool library, or change the one with that number (what is left
/// out stays as it was).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CamToolParams {
    /// The number programs call it by (T1…T99).
    #[schemars(range(min = 1, max = 99))]
    pub number: u32,
    #[serde(flatten)]
    pub fields: CamToolFields,
}

/// What a cutter is: each left out keeps what it was (or, for a new tool, its default).
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct CamToolFields {
    /// Cutting diameter (mm): required for a new tool.
    #[serde(default)]
    pub diameter: Option<f64>,
    /// Default: 2.
    #[serde(default)]
    pub flutes: Option<u32>,
    /// How deep it can cut (mm). Default: 4 × the diameter.
    #[serde(default)]
    pub flute_length: Option<f64>,
    /// `flat_endmill` (default), `ball_endmill`, `bull_nose`, `drill` or `v_bit`.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// The shank's diameter, the length from tip to the end of the shank, and how far it stands
    /// out of the holder (mm); 0 clears each.
    #[serde(default)]
    pub shank_diameter: Option<f64>,
    #[serde(default)]
    pub overall_length: Option<f64>,
    #[serde(default)]
    pub stickout: Option<f64>,
    /// A bull nose's corner radius (mm).
    #[serde(default)]
    pub corner_radius: Option<f64>,
    /// A drill's or V-bit's included point angle (degrees); 0 clears it.
    #[serde(default)]
    pub tip_angle: Option<f64>,
    /// A V-bit's flat tip (mm); 0: pointed.
    #[serde(default)]
    pub tip_diameter: Option<f64>,
    #[serde(default)]
    pub maker: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// The holder, as cylinders from its face up; `{"sections": []}` removes it.
    #[serde(default)]
    pub holder: Option<CamHolderParam>,
    /// Cutting data per material over the starting values; replaces all of them.
    #[serde(default)]
    pub presets: Option<Vec<CamPresetParam>>,
}

/// A tool holder: its name and its sections `[diameter, length]` (mm) from its face up.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct CamHolderParam {
    #[serde(default)]
    pub name: String,
    pub sections: Vec<[f64; 2]>,
}

/// Cutting data for one material: each left out keeps the starting value.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct CamPresetParam {
    /// `wood`, `acrylic` or `aluminum`.
    pub material: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spindle_rpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plunge_feed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stepdown: Option<f64>,
    /// A fraction of the diameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stepover: Option<f64>,
}

/// The shared tool library (a file of this computer's, every design's to take tools from).
/// `path` names another library file; by default the user's.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamLibraryParams {
    #[serde(default)]
    pub path: Option<String>,
}

/// Add a tool to the shared library (no `id`) or change one.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CamLibraryToolParams {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    /// The number it takes in a design, when that number is free (default 1).
    #[serde(default)]
    pub number: Option<u32>,
    #[serde(flatten)]
    pub fields: CamToolFields,
}

/// Add a holder to the shared library (no `id`) or change one.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamLibraryHolderParams {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub sections: Vec<[f64; 2]>,
}

/// Remove a tool or a holder from the shared library (designs keep their copies).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamLibraryDeleteParams {
    #[serde(default)]
    pub path: Option<String>,
    pub id: String,
}

/// Copy a library tool into the design (as `number`, default its own or the next free one), or
/// bring the design's copy of it up to date.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamLibraryUseParams {
    #[serde(default)]
    pub path: Option<String>,
    pub id: String,
    #[serde(default)]
    pub number: Option<u32>,
}

/// Put a tool of the design into the shared library: as a new entry, or over the one it was
/// copied from.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamLibraryStoreParams {
    #[serde(default)]
    pub path: Option<String>,
    pub number: u32,
}

/// Write the shared library to a file (`to`), or add a file's tools and holders to it (`from`).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamLibraryFileParams {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub from: Option<String>,
}

/// The shared tool library.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamLibrary {
    /// The file it lives in.
    pub path: String,
    pub tools: Vec<CamLibraryTool>,
    pub holders: Vec<CamLibraryHolder>,
    /// Import: how many tools and holders were added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CamLibraryTool {
    pub id: String,
    /// Changes with what the tool is.
    pub revision: String,
    pub tool: CamToolInfo,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CamLibraryHolder {
    pub id: String,
    pub name: String,
    /// `[diameter, length]` from its face up (mm).
    pub sections: Vec<[f64; 2]>,
}

/// One cutter of the library.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamToolInfo {
    pub number: u32,
    pub name: String,
    /// `flat_endmill`, `ball_endmill`, `bull_nose`, `drill` or `v_bit`.
    pub kind: String,
    pub diameter: f64,
    pub flutes: u32,
    pub flute_length: f64,
    /// The shank's diameter and how far the tool stands out of its holder, as used.
    pub shank_diameter: f64,
    pub stickout: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overall_length: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_radius: Option<f64>,
    /// A drill's or V-bit's point angle (degrees), as used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_angle: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_diameter: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub maker: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<CamHolderParam>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<CamPresetParam>,
    /// The shared library entry it was copied from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    /// Changes with what the tool is (not its number): the same as the library entry's while
    /// the two are alike.
    pub revision: String,
}

/// What `cam.tool` returns: the library after the change.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamToolsResult {
    pub tools: Vec<CamToolInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Make a setup (no `id`) or change one: the body machined, the material, the face on the
/// machine's table (machine Z comes out of it), the machine's X, the work origin and the stock.
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
    /// The face that sits on the machine's table: a flat face facing along one of the model's
    /// axes. Machine Z comes out of it through the part; machine X is set to the nearest model
    /// axis square to it unless `x_axis` is given.
    #[serde(default)]
    pub table: Option<CamPickParam>,
    /// Machine X as a model axis, e.g. [0, -1, 0]: square to machine Z.
    #[serde(default)]
    pub x_axis: Option<[f64; 3]>,
    /// The work origin, a point of the stock's box in machine terms: `top_center` (default),
    /// `bottom_center`, or a corner `top_front_left`… `bottom_back_right` (front: machine −Y,
    /// left: machine −X; `top_corner` is `top_front_left`).
    #[serde(default)]
    pub origin: Option<String>,
    /// The work origin at a point of the model instead (a vertex), model coordinates.
    #[serde(default)]
    pub origin_point: Option<[f64; 3]>,
    /// An older form: turn the part over about X (the model's top on the table).
    #[serde(default)]
    pub flipped: Option<bool>,
    /// `margin` (the part grown by the margins) or `fixed` (a block of `stock_size`).
    #[serde(default)]
    pub stock: Option<String>,
    /// The fixed stock's size along machine X, Y, Z (mm). Default: the stock as it is now.
    #[serde(default)]
    pub stock_size: Option<[f64; 3]>,
    /// Where the part sits in a fixed stock seen from above: `center` (default), `front_left`,
    /// `front_right`, `back_left`, `back_right`, `front`, `back`, `left` or `right`. Its bottom is
    /// on the stock's bottom.
    #[serde(default)]
    pub stock_place: Option<String>,
    /// Work the setup out and report it without changing the design (a preview).
    #[serde(default)]
    pub dry_run: Option<bool>,
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
    /// The machine frame in the model.
    pub frame: CamFrame,
    /// The stock's corners in machine coordinates (mm).
    pub stock_min: [f64; 3],
    pub stock_max: [f64; 3],
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A setup's machine frame: the work origin and the machine's X, Y and Z, in model coordinates.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamFrame {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub z: [f64; 3],
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
    /// How far to move the cut's bottom from where the model puts it (mm, up when positive):
    /// the part's top for facing, the floor for a pocket, where the material under a contour or
    /// in a hole begins. Only a cut through the part may go below it (default: 0.5 mm below the
    /// stock). The toolpath is refused where it would cut into the model.
    #[serde(default)]
    pub offset: Option<f64>,
    /// Drop a changed operation's offset: back to where the model puts the bottom.
    #[serde(default)]
    pub default_offset: Option<bool>,
    /// Contour: tabs to hold the part; `count` 0 removes them.
    #[serde(default)]
    pub tabs: Option<CamTabsParam>,
    /// Drill: depth per peck (mm); 0 drills in one go.
    #[serde(default)]
    pub peck: Option<f64>,
    /// Contour and pocket: how the tool goes down into each pass: `auto` (default: a helix in a
    /// pocket, else a ramp, else a plunge, whichever fits), `plunge`, `ramp` or `helix`.
    #[serde(default)]
    pub entry: Option<String>,
    /// The steepest a ramp or helix goes down (degrees, default 3).
    #[serde(default)]
    pub entry_angle: Option<f64>,
    /// A helix's radius (mm, at most the tool's radius; default three quarters of it).
    #[serde(default)]
    pub helix_radius: Option<f64>,
    /// Contour: a quarter arc of this radius into and out of each pass (mm); 0 removes it.
    #[serde(default)]
    pub lead: Option<f64>,
    /// Contour: finishing passes at full depth; 0 removes them.
    #[serde(default)]
    pub finish_passes: Option<u32>,
    /// Contour: how far apart the finishing passes are (mm, default 0.2).
    #[serde(default)]
    pub finish_stepover: Option<f64>,
    /// Contour: go round the last finishing pass once more.
    #[serde(default)]
    pub spring_pass: Option<bool>,
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
    /// Go back to the material's starting cutting data before applying the values given.
    #[serde(default)]
    pub reset_cutting: Option<bool>,
}

/// The cutting data an operation runs with.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
pub struct CamCuttingInfo {
    pub spindle_rpm: f64,
    /// mm/min.
    pub feed: f64,
    /// mm/min.
    pub plunge_feed: f64,
    /// Depth per pass (mm).
    pub stepdown: f64,
    /// As a fraction of the tool's diameter.
    pub stepover: f64,
}

/// A contour's tabs.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
pub struct CamTabsInfo {
    pub count: u32,
    pub width: f64,
    pub height: f64,
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
    /// The cutting data it runs with (when its toolpath can be made).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutting: Option<CamCuttingInfo>,
    /// Whether that differs from the material's starting values on purpose.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub custom_cutting: bool,
    /// Its offset from the bottom the model gives (mm), when one was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
    /// The lowest its tool goes, below the stock's top (machine Z, mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lowest: Option<f64>,
    /// Contour: `outside`, `inside` or `on`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    /// Contour: its tabs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabs: Option<CamTabsInfo>,
    /// Drill: depth per peck (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peck: Option<f64>,
    /// Contour and pocket: `auto`, `plunge`, `ramp` or `helix`, and the ramp or helix's angle
    /// (degrees) and the helix's radius (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_angle: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helix_radius: Option<f64>,
    /// Contour: the lead arcs' radius (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<f64>,
    /// Contour: finishing passes, how far apart (mm), and whether the last is gone round twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_passes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_stepover: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub spring_pass: bool,
}

/// Move an operation earlier (`by` < 0) or later among its setup's operations: the order they
/// are cut and written in.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamMoveParams {
    pub id: u64,
    #[schemars(range(min = -1000, max = 1000))]
    pub by: i32,
}

/// What `cam.move` returns: the setup's operations in their new order.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamMoved {
    pub setup: u64,
    pub operations: Vec<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
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
    /// Machine Z runs down the model's Z: the model's top is on the table.
    pub flipped: bool,
    /// The point on the face on the table, when one was picked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<[f64; 3]>,
    /// Machine X and Z as model axes.
    pub x_axis: [f64; 3],
    pub z_axis: [f64; 3],
    /// The work origin: a point of the stock's box (`top_center`, `bottom_front_left`…) or
    /// `point`, a point of the model (`origin_point`).
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_point: Option<[f64; 3]>,
    /// The stock beyond the part: on each side, above and below (mm).
    pub margins: [f64; 3],
    /// `margin` or `fixed`.
    pub stock_mode: String,
    /// A fixed stock's size and where the part sits in it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock_size: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock_place: Option<String>,
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

/// What `cam.preview` returns: the operation as `cam.operation` would make it, and its toolpath
/// in model coordinates, without changing the design.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamPreview {
    pub operation: CamOpSummary,
    pub setup: u64,
    /// Runs of points the tool cuts along (model coordinates, mm).
    pub cuts: Vec<Vec<[f64; 3]>>,
    /// Rapid moves, from and to.
    pub rapids: Vec<[[f64; 3]; 2]>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A setup's toolpaths in machine coordinates, to play back against its stock.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CamSimulationParams {
    /// Default: the only setup.
    #[serde(default)]
    pub setup: Option<u64>,
    /// Longest straight piece an arc is played back as (mm). Default 0.5.
    #[serde(default)]
    pub step: Option<f64>,
}

/// One operation to play back: its cutter and its points, in machine coordinates.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamSimSteps {
    pub operation: u64,
    pub name: String,
    pub tool: u32,
    /// The cutter's radius (mm).
    pub radius: f64,
    /// `flat` (an end mill) or `drill` (a 118° point).
    pub cutter: String,
    /// How far up the cutter its flutes reach (mm): the shank is above.
    pub flute_length: f64,
    /// The whole tool, as the design keeps it: its shape, shank and holder.
    pub tool_data: serde_json::Value,
    /// Each point as [x, y, z, rapid]: `rapid` is 1 when the move to it is a rapid, else 0.
    pub points: Vec<[f64; 4]>,
}

/// What `cam.simulation` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamSimulation {
    pub setup: u64,
    /// The body the setup machines.
    pub body: String,
    /// The machine frame in the model.
    pub frame: CamFrame,
    /// The stock's corners in machine coordinates (mm).
    pub stock_min: [f64; 3],
    pub stock_max: [f64; 3],
    /// The operations that can be made, in the order they are cut.
    pub operations: Vec<CamSimSteps>,
    /// True when the points were cut short (very long toolpaths).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `cam.toolpaths` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CamToolpaths {
    pub setup: u64,
    /// The stock's lowest and highest corners (model coordinates, mm).
    pub stock: [[f64; 3]; 2],
    pub paths: Vec<CamPathLines>,
    /// True when the points were cut short (very long toolpaths).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
