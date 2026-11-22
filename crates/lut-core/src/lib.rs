//! Pure LUT core (M18, SPEC §14): the grading model of a library LUT, RGB curves, baking to a 3D table and the
//! `.cube` format. No IO, no clock: the app reads and writes the files, this crate only turns values into values.
//!
//! Colour values are display-referred RGB in 0..1, the same space Resolume's LUT effect looks up in. The order of
//! operations is fixed (see [`Grade::apply`]): imported base table, exposure, white balance, lift/gamma/gain,
//! contrast, saturation, then the curves (master first, then the channel's own).

mod cube;
mod curve;
mod doc;
mod grade;
mod table;

pub use cube::{parse_cube, write_cube, CubeError};
pub use curve::{Curve, Curves, PreparedCurve};
pub use doc::{valid_lut_name, LutDoc, LutDocError, LUT_DOC_VERSION};
pub use grade::{Grade, Wheel};
pub use table::{Table, LUT_SIZE};

/// One RGB colour, 0..1 per channel.
pub type Rgb = [f32; 3];
