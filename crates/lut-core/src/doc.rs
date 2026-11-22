use crate::curve::Curves;
use crate::grade::{Grade, Pipeline};
use crate::table::{Table, LUT_SIZE};
use serde::{Deserialize, Serialize};

/// Version of the `.lut.json` file format (SPEC §14.2).
pub const LUT_DOC_VERSION: u32 = 1;

/// One library LUT as stored in `<data-dir>/luts/<name>.lut.json`. The name is the file name, not stored inside.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LutDoc {
    pub version: u32,
    #[serde(default)]
    pub grade: Grade,
    #[serde(default)]
    pub curves: Curves,
    /// An imported table (resampled to [`LUT_SIZE`]) that the controls apply on top of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Table>,
}

impl Default for LutDoc {
    fn default() -> Self {
        LutDoc {
            version: LUT_DOC_VERSION,
            grade: Grade::default(),
            curves: Curves::default(),
            base: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LutDocError {
    #[error("not a LUT file: {0}")]
    Json(#[from] serde_json::Error),
    #[error("LUT file version {0} is newer than this app understands ({LUT_DOC_VERSION})")]
    TooNew(u32),
    #[error("the base table is broken: {0}")]
    BadBase(String),
}

impl LutDoc {
    /// A library entry that uses an imported table as is.
    pub fn from_base(base: Table) -> LutDoc {
        LutDoc {
            base: Some(base.resample(LUT_SIZE)),
            ..LutDoc::default()
        }
    }

    pub fn from_json(text: &str) -> Result<LutDoc, LutDocError> {
        let doc: LutDoc = serde_json::from_str(text)?;
        if doc.version > LUT_DOC_VERSION {
            return Err(LutDocError::TooNew(doc.version));
        }
        if let Some(b) = &doc.base {
            if b.size < 2 || b.data.len() != b.size * b.size * b.size {
                return Err(LutDocError::BadBase(format!(
                    "size {} with {} entries",
                    b.size,
                    b.data.len()
                )));
            }
        }
        Ok(doc)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a LutDoc always serialises")
    }

    /// Bakes the whole transform (base, grade, curves) into a [`LUT_SIZE`]³ table.
    pub fn bake(&self) -> Table {
        let p = Pipeline::new(self.base.as_ref(), &self.grade, &self.curves);
        Table::from_fn(LUT_SIZE, |c| p.apply(c))
    }
}

/// Whether `name` can be a library LUT name: it is used as the file name on Windows (`<name>.cube`, which
/// Resolume's LUT effect shows). Not empty, at most 64 characters, no `<>:"/\|?*` or control characters, no
/// leading or trailing space or dot, not a reserved device name (CON, NUL, COM1, …).
pub fn valid_lut_name(name: &str) -> bool {
    if name.is_empty() || name.chars().count() > 64 {
        return false;
    }
    if name.starts_with([' ', '.']) || name.ends_with([' ', '.']) {
        return false;
    }
    if name
        .chars()
        .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return false;
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    !reserved
}
