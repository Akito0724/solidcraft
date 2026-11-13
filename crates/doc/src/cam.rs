//! Machining kept in a design: the tool library, setups (where the part sits on the machine) and
//! operations (what is cut, with which tool), and turning them into `solidcraft_cam` jobs against
//! the evaluated model.
//!
//! An operation names the model geometry it cuts the way a sketch's projected link does: a body
//! and a point on the face or edge ([`GeoPick`]). The face's boundary is read from the model's
//! exact edges by projection onto the setup's machine XY plane ([`crate::project::resolve`]), so
//! arcs stay arcs. A setup puts one of the part's faces on the table: the machine's Z comes out of
//! it, and its X and Y run along the model's axes. Its stock is the part's exact box along those
//! axes grown by margins, or a block of a given size; the work origin is a point of that box or of
//! the model.

use serde::{Deserialize, Serialize};
use solidcore_record::{Plane, Vec3};
use solidcraft_cam::{
    Approach, Clearance, ContourOp, Cutting, Depths, DrillOp, FaceOp, Frame, Job, Loop, Material, Operation, PocketOp, Region, Side, Tabs, Tool,
    Toolpath, Vertex,
};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{LinkGeom, LinkKind, LinkSource};

use crate::eval::ModelState;
use crate::{DocError, Document, Result};

/// How far below the stock a cut that goes through it reaches (mm), so the part comes free.
pub const BREAKTHROUGH: f64 = 0.5;

/// The machining a design keeps.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CamData {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub setups: Vec<CamSetup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operations: Vec<CamOperation>,
}

impl CamData {
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty() && self.setups.is_empty() && self.operations.is_empty()
    }

    /// An id no setup or operation has.
    pub fn next_id(&self) -> u64 {
        self.setups.iter().map(|s| s.id).chain(self.operations.iter().map(|o| o.id)).max().unwrap_or(0) + 1
    }

    pub fn setup(&self, id: u64) -> Option<&CamSetup> {
        self.setups.iter().find(|s| s.id == id)
    }

    pub fn tool(&self, number: u32) -> Option<&Tool> {
        self.tools.iter().find(|t| t.number == number)
    }
}

/// Where the work origin is, on the face of the stock that faces up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkOrigin {
    /// The middle of the stock's top.
    #[default]
    TopCenter,
    /// The front-left corner of the stock's top, as seen standing at the machine.
    TopCorner,
}

/// Which way the machine's axes run through the model: Z out of the face on the table, X
/// square to it (Y follows, right-handed). Unit vectors in model coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SetupAxes {
    pub x: [f64; 3],
    pub z: [f64; 3],
}

/// The work origin: a point of the stock's box (each of `at` −1, 0 or 1 along the machine's X,
/// Y and Z: the low side, the middle, the high side), or a point of the model.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkZero {
    Stock { at: [i8; 3] },
    Point { at: [f64; 3] },
}

/// A stock of a given size, the part placed in it: its bottom on the stock's bottom, and across
/// X and Y at `place` (−1, 0 or 1 each: against the low side, centred, against the high side).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixedStock {
    pub size: [f64; 3],
    #[serde(default)]
    pub place: [i8; 2],
}

/// How much bigger than the part the stock is (mm).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StockMargin {
    /// On every side.
    pub side: f64,
    /// Above the part's top (what facing takes off).
    pub top: f64,
    /// Below the part's bottom.
    pub bottom: f64,
}

impl Default for StockMargin {
    fn default() -> Self {
        StockMargin { side: 3.0, top: 1.0, bottom: 0.0 }
    }
}

/// One way the part is held on the machine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CamSetup {
    pub id: u64,
    pub name: String,
    /// The body machined.
    pub body: String,
    pub material: Material,
    /// The face on the machine's table (a point on it), when one was picked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<GeoPick>,
    /// The machine's axes. Default: the model's own, or (an older design's `flipped`) turned over
    /// about X.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axes: Option<SetupAxes>,
    /// The work origin. Default: an older design's `origin`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zero: Option<WorkZero>,
    /// A stock of a given size instead of the part with margins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_stock: Option<FixedStock>,
    /// An older design's work origin (`zero` replaces it).
    #[serde(default)]
    pub origin: WorkOrigin,
    /// An older design's part turned over to cut its bottom (`axes` replaces it).
    #[serde(default)]
    pub flipped: bool,
    #[serde(default)]
    pub margin: StockMargin,
    #[serde(default = "default_safe_z")]
    pub safe_z: f64,
    #[serde(default = "default_retract")]
    pub retract: f64,
}

impl CamSetup {
    /// The machine's X and Z in the model.
    pub fn axes(&self) -> SetupAxes {
        self.axes.unwrap_or(SetupAxes { x: [1.0, 0.0, 0.0], z: if self.flipped { [0.0, 0.0, -1.0] } else { [0.0, 0.0, 1.0] } })
    }

    /// The work origin.
    pub fn zero(&self) -> WorkZero {
        self.zero.unwrap_or(match self.origin {
            WorkOrigin::TopCenter => WorkZero::Stock { at: [0, 0, 1] },
            WorkOrigin::TopCorner => WorkZero::Stock { at: [-1, -1, 1] },
        })
    }
}

fn default_safe_z() -> f64 {
    10.0
}

fn default_retract() -> f64 {
    1.0
}

/// A face or edge of the model, by its body and a point on it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeoPick {
    pub body: String,
    pub at: [f64; 3],
}

/// What an operation cuts. How deep is the model's business: each cut goes down to where the
/// model puts its bottom (the part's top for facing, the floor for a pocket, where the material
/// under a contour or in a hole begins, or through the part), moved by `offset` (mm, up when
/// positive). Only a cut that goes through the part may be moved below that (by default it goes
/// [`BREAKTHROUGH`] below), so no setting cuts into the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpSpec {
    /// Facing the stock's top down to the part's top.
    Face {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<f64>,
    },
    /// Clearing down to a floor face, around the islands in it.
    Pocket {
        face: GeoPick,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<f64>,
        /// How it goes down into the material.
        #[serde(default, skip_serializing_if = "Approach::is_default")]
        approach: Approach,
    },
    /// Following the outer boundary of a face, down to where the material beside it begins, or
    /// through the part.
    Contour {
        face: GeoPick,
        side: Side,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tabs: Option<Tabs>,
        /// How it goes down into the material, leads into and out of each pass, and finishing
        /// passes.
        #[serde(default, skip_serializing_if = "Approach::is_default")]
        approach: Approach,
    },
    /// Drilling at the centres of circular edges, down to the holes' bottom, or through the part.
    Drill {
        edges: Vec<GeoPick>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        peck: Option<f64>,
    },
}

/// An operation of a setup.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CamOperation {
    pub id: u64,
    pub name: String,
    pub setup: u64,
    /// The tool's number in the library.
    pub tool: u32,
    #[serde(flatten)]
    pub spec: OpSpec,
    /// Cutting data other than the material's starting values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutting: Option<Cutting>,
}

/// A setup worked out against the model: its frame, the stock in machine coordinates and each
/// operation as a job, or why it can't be one.
#[derive(Clone, Debug)]
pub struct CamPlan {
    pub frame: Frame,
    pub clearance: Clearance,
    /// The stock's extent in machine coordinates.
    pub stock_min: [f64; 3],
    pub stock_max: [f64; 3],
    /// The part's top (the face that faces up) and bottom in machine Z.
    pub part_top: f64,
    pub part_bottom: f64,
    pub jobs: Vec<(u64, std::result::Result<Job, String>)>,
}

impl Document {
    /// The plan of setup `id` against the model `st`.
    pub fn cam_plan(&self, st: &ModelState, id: u64) -> Result<CamPlan> {
        let setup = self.cam.setup(id).ok_or_else(|| DocError::Unknown(format!("setup {id}")))?;
        let body = st.body(&setup.body).ok_or_else(|| DocError::Invalid(format!("{}: there is no body `{}`", setup.name, setup.body)))?;
        let b = body.body.exact_bounds().map_err(|e| DocError::Invalid(format!("{}: {e}", setup.name)))?;
        let axes = setup.axes();
        // The machine's axes about the model's origin first; the work origin moves it after.
        let turned =
            Frame::new([0.0; 3], axes.x, axes.z).ok_or_else(|| DocError::Invalid(format!("{}: the setup's axes are not square", setup.name)))?;
        // The part's box along the machine's axes (they run along the model's, so it is exact).
        let corners: Vec<[f64; 3]> = (0..8)
            .map(|k| {
                let c = [
                    if k & 1 == 0 { b.min.x } else { b.max.x },
                    if k & 2 == 0 { b.min.y } else { b.max.y },
                    if k & 4 == 0 { b.min.z } else { b.max.z },
                ];
                turned.to_machine(c)
            })
            .collect();
        let part_lo = [0, 1, 2].map(|i| corners.iter().map(|c| c[i]).fold(f64::MAX, f64::min));
        let part_hi = [0, 1, 2].map(|i| corners.iter().map(|c| c[i]).fold(f64::MIN, f64::max));
        let (lo, hi) = match setup.fixed_stock {
            None => {
                let m = setup.margin;
                if ![m.side, m.top, m.bottom].iter().all(|v| v.is_finite() && *v >= 0.0 && *v <= 1000.0) {
                    return Err(DocError::Invalid(format!("{}: the stock margins must be between 0 and 1000 mm", setup.name)));
                }
                ([part_lo[0] - m.side, part_lo[1] - m.side, part_lo[2] - m.bottom], [part_hi[0] + m.side, part_hi[1] + m.side, part_hi[2] + m.top])
            }
            Some(f) => {
                let part = [0, 1, 2].map(|i| part_hi[i] - part_lo[i]);
                if !f.size.iter().all(|v| v.is_finite() && *v > 0.0 && *v <= 10_000.0) {
                    return Err(DocError::Invalid(format!("{}: the stock's size must be between 0 and 10000 mm", setup.name)));
                }
                if (0..3).any(|i| f.size[i] < part[i] - 1e-6) {
                    return Err(DocError::Invalid(format!(
                        "{}: the part ({:.1} × {:.1} × {:.1} mm) doesn't fit in the stock",
                        setup.name, part[0], part[1], part[2]
                    )));
                }
                let mut lo = [0.0; 3];
                for i in 0..2 {
                    let spare = f.size[i] - part[i];
                    // −1: the part against the stock's low side, 1: against its high side.
                    lo[i] = part_lo[i] - spare * (1.0 + f64::from(f.place[i].clamp(-1, 1))) / 2.0;
                }
                lo[2] = part_lo[2];
                (lo, [lo[0] + f.size[0], lo[1] + f.size[1], lo[2] + f.size[2]])
            }
        };
        let zero = match setup.zero() {
            WorkZero::Stock { at } => [0, 1, 2].map(|i| match at[i].clamp(-1, 1) {
                -1 => lo[i],
                1 => hi[i],
                _ => (lo[i] + hi[i]) / 2.0,
            }),
            WorkZero::Point { at } => {
                if !at.iter().all(|v| v.is_finite()) {
                    return Err(DocError::Invalid(format!("{}: the work origin is not a point", setup.name)));
                }
                turned.to_machine(at)
            }
        };
        let frame = Frame { origin: turned.to_model(zero), ..turned };
        let stock_min = [0, 1, 2].map(|i| lo[i] - zero[i]);
        let stock_max = [0, 1, 2].map(|i| hi[i] - zero[i]);
        let part_top = part_hi[2] - zero[2];
        let part_bottom = part_lo[2] - zero[2];
        // Clear of the stock's top, wherever the origin is.
        let clearance = Clearance { safe_z: stock_max[2] + setup.safe_z, retract: setup.retract };
        let plan = CamPlan { frame, clearance, stock_min, stock_max, part_top, part_bottom, jobs: Vec::new() };
        let jobs = self
            .cam
            .operations
            .iter()
            .filter(|o| o.setup == id)
            .map(|o| (o.id, self.cam_job(st, setup, &plan, o).map_err(|e| format!("{}: {e}", o.name))))
            .collect();
        Ok(CamPlan { jobs, ..plan })
    }

    fn cam_job(&self, st: &ModelState, setup: &CamSetup, plan: &CamPlan, op: &CamOperation) -> Result<Job> {
        let tool = self.cam.tool(op.tool).ok_or_else(|| DocError::Invalid(format!("there is no tool T{} in the library", op.tool)))?.clone();
        let cutting = op.cutting.unwrap_or_else(|| tool.cutting(setup.material));
        let frame = &plan.frame;
        let body = st.body(&setup.body).ok_or_else(|| DocError::Invalid(format!("there is no body `{}`", setup.body)))?;
        let top = plan.stock_max[2];
        // Where the cut's bottom goes: the model's bottom for it, moved by the offset. Only a cut
        // through the part (`base` None: no material under it) may go below; by default it goes
        // BREAKTHROUGH below the stock.
        let bottom = |base: Option<f64>, offset: Option<f64>| -> Result<f64> {
            let through = base.is_none();
            let o = match offset {
                None if through => -BREAKTHROUGH,
                None => 0.0,
                Some(v) if !v.is_finite() || v.abs() > 1000.0 => return Err(DocError::Invalid("the offset must be a number of mm".into())),
                Some(v) if v < 0.0 && !through => {
                    return Err(DocError::Invalid("only a cut that goes through the part can reach below where the model puts its bottom".into()));
                }
                Some(v) => v,
            };
            Ok(base.unwrap_or(plan.stock_min[2]) + o)
        };
        let radius = tool.diameter / 2.0;
        let operation = match &op.spec {
            OpSpec::Face { offset } => {
                let bottom = bottom(Some(plan.part_top), *offset)?;
                if bottom >= top - 1e-9 {
                    return Err(DocError::Invalid("there is nothing to face: the stock has no margin above the part".into()));
                }
                Operation::Face(FaceOp {
                    min: (plan.stock_min[0], plan.stock_min[1]),
                    max: (plan.stock_max[0], plan.stock_max[1]),
                    depths: Depths { top, bottom },
                })
            }
            OpSpec::Pocket { face, offset, .. } => {
                let (region, z, up) = self.cam_face(st, frame, face)?;
                if !up {
                    return Err(DocError::Invalid("pick the pocket's floor, a face that faces up in this setup".into()));
                }
                Operation::Pocket(PocketOp { region, depths: Depths { top, bottom: bottom(Some(z), *offset)? } })
            }
            OpSpec::Contour { face, side, offset, tabs, .. } => {
                let (region, z, _) = self.cam_face(st, frame, face)?;
                // Where the tool sweeps: beside the outline on its side, two radii wide.
                let reach = match side {
                    Side::Outside => [-0.05, -radius, 0.05 - 2.0 * radius],
                    Side::Inside => [0.05, radius, 2.0 * radius - 0.05],
                    Side::On => [0.05 - radius, 0.0, radius - 0.05],
                };
                let mut probes = Vec::new();
                for d in reach {
                    for l in region.outer.offset(d) {
                        probes.extend(along(&l, 24));
                    }
                }
                let base = first_material(&body.body, frame, &probes, z, plan.stock_min[2])?;
                Operation::Contour(ContourOp {
                    profile: region.outer,
                    side: *side,
                    depths: Depths { top, bottom: bottom(base, *offset)? },
                    tabs: *tabs,
                })
            }
            OpSpec::Drill { edges, offset, peck } => {
                if edges.is_empty() {
                    return Err(DocError::Invalid("pick the holes' circular edges".into()));
                }
                let mut points = Vec::new();
                // The shallowest hole's bottom, so no hole is drilled deeper than the model's.
                let mut base: Option<f64> = None;
                let mut through = true;
                for e in edges {
                    let ((cx, cy), r) = self.cam_circle(st, frame, e)?;
                    points.push((cx, cy));
                    let a = 0.7 * r.min(radius);
                    let probes = [(cx, cy), (cx + a, cy), (cx - a, cy), (cx, cy + a), (cx, cy - a)];
                    if let Some(floor) = first_material(&body.body, frame, &probes, frame.to_machine(e.at)[2], plan.stock_min[2])? {
                        through = false;
                        base = Some(base.map_or(floor, |b: f64| b.max(floor)));
                    }
                }
                let base = if through { None } else { base };
                Operation::Drill(DrillOp { points, depths: Depths { top, bottom: bottom(base, *offset)? }, peck: *peck })
            }
        };
        let approach = match &op.spec {
            OpSpec::Pocket { approach, .. } | OpSpec::Contour { approach, .. } => *approach,
            OpSpec::Face { .. } | OpSpec::Drill { .. } => Approach::default(),
        };
        let stock = Some([(plan.stock_min[0], plan.stock_min[1]), (plan.stock_max[0], plan.stock_max[1])]);
        Ok(Job { name: op.name.clone(), operation, tool, cutting, approach, stock })
    }

    /// The face at `pick` seen down the machine's Z (`frame`): its boundary in machine XY (outer
    /// loop and the holes in it), its machine height and whether it faces up. Refused for a face
    /// that isn't flat and square to the machine's Z.
    pub fn cam_face(&self, st: &ModelState, frame: &Frame, pick: &GeoPick) -> Result<(Region, f64, bool)> {
        let body = st.body(&pick.body).ok_or_else(|| DocError::Invalid(format!("there is no body `{}`", pick.body)))?;
        let at = finite_point(pick.at)?;
        let (vals, _) = self.param_values();
        let src = LinkSource::Face { body: pick.body.clone(), at };
        let r = crate::project::resolve(self, &vals, st, &machine_plane(frame), LinkKind::Project, &src)?;
        let index = r.face_index.ok_or_else(|| DocError::Invalid("the pick is not on a face".into()))?;
        let plane = body.body.planar_face(index).ok_or_else(|| DocError::Invalid("the face is not flat".into()))?;
        let n = plane.normal();
        let up = n.x * frame.z[0] + n.y * frame.z[1] + n.z * frame.z[2];
        if up.abs() < 1.0 - 1e-6 {
            return Err(DocError::Invalid("the face is not level: it must face straight up or down".into()));
        }
        let loops = chain(&r.geom)?;
        let mut loops: Vec<Loop> = loops.into_iter().filter(|l| l.area().abs() > 1e-9).collect();
        loops.sort_by(|a, b| b.area().abs().total_cmp(&a.area().abs()));
        let mut it = loops.into_iter();
        let outer = it.next().ok_or_else(|| DocError::Invalid("the face has no boundary".into()))?;
        let z = frame.to_machine([plane.origin.x, plane.origin.y, plane.origin.z])[2];
        Ok((Region { outer: outer.oriented(true), islands: it.map(|l| l.oriented(false)).collect() }, z, up > 0.0))
    }

    /// The circle of the edge at `pick` seen down the machine's Z: its centre (machine XY) and
    /// radius.
    pub fn cam_circle(&self, st: &ModelState, frame: &Frame, pick: &GeoPick) -> Result<((f64, f64), f64)> {
        st.body(&pick.body).ok_or_else(|| DocError::Invalid(format!("there is no body `{}`", pick.body)))?;
        let at = finite_point(pick.at)?;
        let (vals, _) = self.param_values();
        let src = LinkSource::Edge { body: pick.body.clone(), at };
        let r = crate::project::resolve(self, &vals, st, &machine_plane(frame), LinkKind::Project, &src)?;
        for g in &r.geom {
            match g {
                LinkGeom::Circle(c, radius) => return Ok(((c.x, c.y), *radius)),
                LinkGeom::Arc { c, a, .. } => return Ok(((c.x, c.y), (a.x - c.x).hypot(a.y - c.y))),
                _ => {}
            }
        }
        Err(DocError::Invalid("the edge is not a circle seen from above: pick a hole's round edge".into()))
    }

    /// The machine axes a face on the table gives: Z out of the table through the part (against
    /// the face's outward normal), and X the model axis nearest the model's X square to it. The
    /// face must be flat and face along one of the model's axes.
    pub fn cam_table_axes(&self, st: &ModelState, pick: &GeoPick) -> Result<SetupAxes> {
        let body = st.body(&pick.body).ok_or_else(|| DocError::Invalid(format!("there is no body `{}`", pick.body)))?;
        let at = finite_point(pick.at)?;
        let (vals, _) = self.param_values();
        let src = LinkSource::Face { body: pick.body.clone(), at };
        let r = crate::project::resolve(self, &vals, st, &Plane::XY, LinkKind::Project, &src)?;
        let index = r.face_index.ok_or_else(|| DocError::Invalid("the pick is not on a face".into()))?;
        let plane = body.body.planar_face(index).ok_or_else(|| DocError::Invalid("the face is not flat".into()))?;
        let n = plane.normal();
        let z = axis_along([-n.x, -n.y, -n.z])
            .ok_or_else(|| DocError::Invalid("the face on the table must face along one of the model's axes".into()))?;
        let x = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
            .into_iter()
            .find(|a: &[f64; 3]| (a[0] * z[0] + a[1] * z[1] + a[2] * z[2]).abs() < 0.5)
            .unwrap_or([1.0, 0.0, 0.0]);
        Ok(SetupAxes { x, z })
    }
}

/// The model axis (±X, ±Y, ±Z) `v` runs along, when it runs along one.
pub fn axis_along(v: [f64; 3]) -> Option<[f64; 3]> {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if !n.is_finite() || n < 1e-12 {
        return None;
    }
    (0..3).find(|&i| (v[i] / n).abs() > 1.0 - 1e-6).map(|i| {
        let mut a = [0.0; 3];
        if let Some(c) = a.get_mut(i) {
            *c = v[i].signum();
        }
        a
    })
}

/// The machine's XY plane at the work origin, for projecting the model into machine XY.
fn machine_plane(frame: &Frame) -> Plane {
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    Plane { origin: v(frame.origin), x: v(frame.x), y: v(frame.y) }
}

impl Document {
    /// Where the toolpath of setup `id`'s operation would cut into the model: the first point
    /// found (model coordinates), or `None`. The flat-bottomed cutter is sampled along its cuts
    /// just above its tip, across its width, wherever its tip is below the part's top.
    pub fn cam_gouge(&self, st: &ModelState, id: u64, plan: &CamPlan, path: &Toolpath) -> Option<[f64; 3]> {
        let setup = self.cam.setup(id)?;
        let body = st.body(&setup.body)?;
        let radius = (path.tool.diameter / 2.0 - 0.05).max(0.0);
        // Every cut sampled at most 1 mm apart: a straight plunge is a single move.
        let points = path.points(1.0);
        let mut below = Vec::new();
        for pair in points.windows(2) {
            let [(a, _), (b, rapid)] = pair else { continue };
            if *rapid {
                continue;
            }
            let d = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2)).sqrt();
            let n = (d.ceil() as usize).clamp(1, 10_000);
            for k in 1..=n {
                let t = k as f64 / n as f64;
                let p = solidcraft_cam::P3::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t);
                if p.z < plan.part_top - 1e-6 {
                    below.push(p);
                }
            }
            if below.len() > 200_000 {
                break;
            }
        }
        let stride = (below.len() * 9).div_ceil(GOUGE_PROBES).max(1);
        let ring: Vec<(f64, f64)> = std::iter::once((0.0, 0.0))
            .chain((0..8).map(|k| {
                let a = f64::from(k) * std::f64::consts::FRAC_PI_4;
                (radius * a.cos(), radius * a.sin())
            }))
            .collect();
        for p in below.iter().step_by(stride) {
            for (dx, dy) in &ring {
                let m = plan.frame.to_model([p.x + dx, p.y + dy, p.z + 0.05]);
                if matches!(body.body.classify_point(Vec3::new(m[0], m[1], m[2])), Ok(solidcraft_kernel::PointClass::Inside)) {
                    return Some(m);
                }
            }
        }
        None
    }
}

/// The most points the gouge check classifies against the model.
const GOUGE_PROBES: usize = 3000;

/// `n` points spread along a loop.
fn along(l: &Loop, n: usize) -> Vec<(f64, f64)> {
    let segments = l.segments();
    let total: f64 = segments.iter().map(|s| s.length()).sum();
    if total <= 1e-9 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let mut d = total * (k as f64 + 0.5) / n as f64;
        for s in &segments {
            let len = s.length();
            if d <= len || std::ptr::eq(s, segments.last().unwrap_or(s)) {
                out.push(s.at(if len > 1e-12 { (d / len).clamp(0.0, 1.0) } else { 0.0 }));
                break;
            }
            d -= len;
        }
    }
    out
}

/// Going down the machine's Z from the face at height `from` to `to` over each of `probes`
/// (machine XY), the highest height where the body's material begins: the bottom a cut there may
/// reach (`from` itself where the material is right under the face). `None` when there is none
/// down to `to` (the cut may go through).
fn first_material(body: &solidcraft_kernel::Body, frame: &Frame, probes: &[(f64, f64)], from: f64, to: f64) -> Result<Option<f64>> {
    const STEP: f64 = 0.5;
    let inside = |x: f64, y: f64, z: f64| {
        let m = frame.to_model([x, y, z]);
        matches!(body.classify_point(Vec3::new(m[0], m[1], m[2])), Ok(solidcraft_kernel::PointClass::Inside))
    };
    let span = from - to;
    if !span.is_finite() || span <= 0.0 {
        return Ok(None);
    }
    let steps = ((span / STEP).ceil() as usize).clamp(1, 4000);
    let mut best: Option<f64> = None;
    for &(x, y) in probes.iter().take(256) {
        let mut above = from;
        for k in 0..=steps {
            // Just under the face first, then every STEP.
            let z = from - if k == 0 { 0.05 } else { span.min(k as f64 * STEP) };
            // Already below a higher floor found elsewhere: nothing here can raise it.
            if best.is_some_and(|b| z < b) {
                break;
            }
            if inside(x, y, z) {
                // Between `above` (empty) and `z` (material): halve down to 0.01 mm.
                let (mut hi, mut lo) = (above, z);
                for _ in 0..8 {
                    let mid = (hi + lo) / 2.0;
                    if inside(x, y, mid) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                best = Some(best.map_or(hi, |b| b.max(hi)));
                break;
            }
            above = z;
        }
    }
    Ok(best)
}

fn finite_point(at: [f64; 3]) -> Result<Vec3> {
    if at.iter().all(|v| v.is_finite()) {
        Ok(Vec3::new(at[0], at[1], at[2]))
    } else {
        Err(DocError::Invalid("the pick's point must be numbers".into()))
    }
}

/// One piece of a boundary: from, to, bulge.
type Piece = ((f64, f64), (f64, f64), f64);

/// The closed loops the projected edges make.
fn chain(geom: &[LinkGeom]) -> Result<Vec<Loop>> {
    const TOL: f64 = 1e-3;
    let mut loops = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    for g in geom {
        match g {
            LinkGeom::Point(_) => {}
            LinkGeom::Line(a, b) => pieces.push(((a.x, a.y), (b.x, b.y), 0.0)),
            LinkGeom::Arc { c, a, b } => {
                let a0 = (a.y - c.y).atan2(a.x - c.x);
                let mut sweep = (b.y - c.y).atan2(b.x - c.x) - a0;
                while sweep <= 1e-12 {
                    sweep += std::f64::consts::TAU;
                }
                pieces.push(((a.x, a.y), (b.x, b.y), (sweep / 4.0).tan()));
            }
            LinkGeom::Circle(c, r) => loops.push(Loop::circle(c.x, c.y, *r)),
            _ => return Err(DocError::Invalid("the face has a free-form edge, which can't be machined yet: only lines and arcs".into())),
        }
    }
    let near = |p: (f64, f64), q: (f64, f64)| (p.0 - q.0).hypot(p.1 - q.1) < TOL;
    while let Some(first) = pieces.pop() {
        let start = first.0;
        let mut end = first.1;
        let mut vertices = vec![Vertex { x: first.0.0, y: first.0.1, bulge: first.2 }];
        while !near(end, start) {
            let found = pieces.iter().position(|p| near(p.0, end) || near(p.1, end));
            let Some(i) = found else {
                return Err(DocError::Invalid("the face's boundary doesn't close".into()));
            };
            let p = pieces.swap_remove(i);
            let (from, to, bulge) = if near(p.0, end) { p } else { (p.1, p.0, -p.2) };
            vertices.push(Vertex { x: from.0, y: from.1, bulge });
            end = to;
        }
        loops.push(Loop { vertices });
    }
    Ok(loops)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use solidcore_record::Vec2;

    #[test]
    fn projected_edges_chain_into_loops_whichever_way_they_run() {
        let v = Vec2::new;
        let geom = vec![
            LinkGeom::Line(v(0.0, 0.0), v(10.0, 0.0)),
            LinkGeom::Line(v(10.0, 10.0), v(10.0, 0.0)),
            LinkGeom::Arc { c: v(5.0, 10.0), a: v(10.0, 10.0), b: v(0.0, 10.0) },
            LinkGeom::Line(v(0.0, 10.0), v(0.0, 0.0)),
            LinkGeom::Circle(v(5.0, 5.0), 1.0),
        ];
        let loops = chain(&geom).unwrap();
        assert_eq!(loops.len(), 2);
        let outer = loops.iter().max_by(|a, b| a.area().abs().total_cmp(&b.area().abs())).unwrap();
        let expected = 100.0 + std::f64::consts::PI * 25.0 / 2.0;
        assert!((outer.area().abs() - expected).abs() < 1e-6, "{}", outer.area());
        let open = vec![LinkGeom::Line(v(0.0, 0.0), v(1.0, 0.0)), LinkGeom::Line(v(1.0, 0.0), v(1.0, 1.0))];
        assert!(chain(&open).is_err());
    }
}
