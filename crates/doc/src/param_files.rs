//! Shared parameter files: a table of user parameters kept in its own file, which several designs
//! read, so parts of a machine designed as separate files keep the same values (a rail pitch, a
//! key pitch, a screw size). A design keeps the parameters it read as ordinary user parameters,
//! so expressions, the dependency graph and the Parameters window use them as any other, and
//! notes which of them come from which file ([`ParamFile`]). The design opens with the values it
//! last read even when the file is gone.
//!
//! The file is the table `parameters.export` writes as JSON (`[{name, expression, unit?,
//! comment?}]`); a CSV with a header is read too. Reading and writing the file, and finding it
//! relative to the design, is the engine's: this module only applies a table that was read and
//! says what the design would write.

use serde::{Deserialize, Serialize};

use crate::{DocError, Document, Result};

/// A shared parameter file a design reads.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ParamFile {
    /// The file: relative to this design's folder once this design is saved, else as given (`/`
    /// separates folders), as a linked design's path is kept.
    pub path: String,
    /// The parameters the file gave, as last read from it or written to it.
    #[serde(default)]
    pub rows: Vec<SharedRow>,
    /// SHA-256 of the file's bytes as last read or written ([`crate::DesignLink::file_digest`]):
    /// a file whose digest differs now was changed elsewhere since.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_sha256: Option<String>,
}

/// One parameter of a shared file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SharedRow {
    pub name: String,
    pub expression: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

/// What applying a table read from a shared file changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SharedApplied {
    /// The parameters the file gives, in its order.
    pub names: Vec<String>,
    /// Parameters the file gave before and no longer does: kept in the design as its own.
    pub dropped: Vec<String>,
}

/// The JSON a shared file holds for `rows`, as `parameters.export` writes a table.
pub fn table_json(rows: &[SharedRow]) -> String {
    let list: Vec<serde_json::Value> =
        rows.iter().map(|r| serde_json::json!({"name": r.name, "expression": r.expression, "unit": r.unit, "comment": r.comment})).collect();
    serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".into()) + "\n"
}

impl Document {
    /// The shared file `name` comes from, if it is a shared parameter.
    pub fn shared_file_of(&self, name: &str) -> Option<&ParamFile> {
        self.param_files.iter().find(|f| f.rows.iter().any(|r| r.name == name))
    }

    /// The rows `file` would hold now: its parameters as this design has them.
    pub fn shared_rows_now(&self, file: &ParamFile) -> Vec<SharedRow> {
        file.rows
            .iter()
            .map(|r| match self.param(&r.name) {
                Some(p) => SharedRow { name: p.name.clone(), expression: p.expr.clone(), unit: p.unit.clone(), comment: p.comment.clone() },
                None => r.clone(),
            })
            .collect()
    }

    /// The refusal for changing `name` other than by its value, unit or comment: a shared
    /// parameter is renamed or deleted in its file.
    pub(crate) fn shared_guard(&self, name: &str, what: &str) -> Result<()> {
        match self.shared_file_of(name) {
            Some(f) => Err(DocError::Invalid(format!(
                "`{name}` comes from the shared parameter file {}: {what} it in that file, or unlink the file first",
                f.path
            ))),
            None => Ok(()),
        }
    }

    /// Apply the table `text` read from the shared file kept as `path` (a new link, or an
    /// update of one this design has), whose bytes have digest `digest`. All or nothing: a name
    /// the design already uses for something else (its own parameter, a dimension, a feature
    /// input, another shared file's parameter) refuses the table, and so does a row the
    /// parameters can't take. Parameters the file no longer gives stay as the design's own.
    pub fn apply_shared_table(&mut self, path: &str, text: &str, digest: String) -> Result<SharedApplied> {
        let table = crate::params::param_table(text)?;
        if table.is_empty() {
            return Err(DocError::Invalid(format!("{path}: the shared parameter file has no parameters")));
        }
        let at = self.param_files.iter().position(|f| f.path == path);
        let ours: Vec<String> = at.and_then(|i| self.param_files.get(i)).map(|f| f.rows.iter().map(|r| r.name.clone()).collect()).unwrap_or_default();
        let mut seen = std::collections::BTreeSet::new();
        for (name, _, unit, _) in &table {
            if !seen.insert(name.as_str()) {
                return Err(DocError::Invalid(format!("{path}: `{name}` is in the file twice")));
            }
            if unit.as_deref().is_some_and(crate::text_params::is_text_unit) {
                return Err(DocError::Invalid(format!("{path}: `{name}` is a text parameter, which a shared file can't hold yet")));
            }
            if ours.iter().any(|n| n == name) {
                continue;
            }
            if let Some(other) = self.shared_file_of(name) {
                return Err(DocError::Invalid(format!("`{name}` already comes from the shared parameter file {}", other.path)));
            }
            if self.name_taken(name) {
                return Err(DocError::Invalid(format!(
                    "this design already has a parameter `{name}`: rename it, or remove it from {path}, before linking the file"
                )));
            }
        }
        let rows: Vec<serde_json::Value> =
            table.iter().map(|(n, e, u, c)| serde_json::json!({"name": n, "expression": e, "unit": u, "comment": c})).collect();
        let as_json = serde_json::to_string(&rows).map_err(|e| DocError::Invalid(e.to_string()))?;
        let mut next = self.clone();
        let names = next.import_params(&as_json)?;
        let kept: Vec<SharedRow> = names
            .iter()
            .filter_map(|n| next.param(n))
            .map(|p| SharedRow { name: p.name.clone(), expression: p.expr.clone(), unit: p.unit.clone(), comment: p.comment.clone() })
            .collect();
        let dropped: Vec<String> = ours.into_iter().filter(|n| !names.contains(n)).collect();
        let file = ParamFile { path: path.to_string(), rows: kept, file_sha256: Some(digest) };
        match at {
            Some(i) => {
                if let Some(slot) = next.param_files.get_mut(i) {
                    *slot = file;
                }
            }
            None => next.param_files.push(file),
        }
        *self = next;
        Ok(SharedApplied { names, dropped })
    }

    /// Note that the file kept as `path` now holds `rows`, written by this design with
    /// digest `digest`.
    pub fn shared_written(&mut self, path: &str, rows: Vec<SharedRow>, digest: String) {
        if let Some(f) = self.param_files.iter_mut().find(|f| f.path == path) {
            f.rows = rows;
            f.file_sha256 = Some(digest);
        }
    }

    /// Start sharing the user parameters `names` through a new file kept as `path`: they stay
    /// as they are, and the design notes they come from that file. Returns the rows to write.
    pub fn share_params(&mut self, path: &str, names: &[String]) -> Result<Vec<SharedRow>> {
        if names.is_empty() {
            return Err(DocError::Invalid("no parameters to share".into()));
        }
        if self.param_files.iter().any(|f| f.path == path) {
            return Err(DocError::Invalid(format!("{path} is already a shared parameter file of this design")));
        }
        let mut rows = Vec::new();
        for name in names {
            self.shared_guard(name, "share")?;
            let p = self
                .param(name)
                .filter(|p| !p.model)
                .ok_or_else(|| DocError::Invalid(format!("`{name}` is not a user parameter: only user parameters can be shared")))?;
            if rows.iter().any(|r: &SharedRow| r.name == *name) {
                continue;
            }
            rows.push(SharedRow { name: p.name.clone(), expression: p.expr.clone(), unit: p.unit.clone(), comment: p.comment.clone() });
        }
        self.param_files.push(ParamFile { path: path.to_string(), rows: rows.clone(), file_sha256: None });
        Ok(rows)
    }

    /// Stop reading the shared file kept as `path`. With `keep`, its parameters stay as the
    /// design's own; without, they are deleted (refused while something uses one).
    pub fn unlink_param_file(&mut self, path: &str, keep: bool) -> Result<Vec<String>> {
        let i = self.param_files.iter().position(|f| f.path == path).ok_or_else(|| DocError::Unknown(format!("shared parameter file {path}")))?;
        let mut next = self.clone();
        let file = next.param_files.remove(i);
        let names: Vec<String> = file.rows.into_iter().map(|r| r.name).collect();
        if !keep {
            for n in &names {
                if next.param(n).is_some() {
                    next.remove_param(n)?;
                }
            }
        }
        *self = next;
        Ok(names)
    }
}
