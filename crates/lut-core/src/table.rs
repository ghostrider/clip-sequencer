use crate::Rgb;
use serde::{Deserialize, Serialize};

/// Edge length of every table the app bakes and writes (33³ is the common `.cube` size).
pub const LUT_SIZE: usize = 33;

/// A 3D lookup table: `size`³ RGB entries, **red fastest**, then green, then blue (the `.cube` order).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub size: usize,
    pub data: Vec<Rgb>,
}

impl Table {
    /// The table that changes nothing.
    pub fn identity(size: usize) -> Table {
        Table::from_fn(size, |c| c)
    }

    /// Builds a table by evaluating `f` at every grid point.
    pub fn from_fn(size: usize, mut f: impl FnMut(Rgb) -> Rgb) -> Table {
        assert!(size >= 2, "a LUT needs at least 2 points per axis");
        let step = 1.0 / (size - 1) as f32;
        let mut data = Vec::with_capacity(size * size * size);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    data.push(f([r as f32 * step, g as f32 * step, b as f32 * step]));
                }
            }
        }
        Table { size, data }
    }

    fn at(&self, r: usize, g: usize, b: usize) -> Rgb {
        self.data[(b * self.size + g) * self.size + r]
    }

    /// Trilinear lookup; the input is clamped to 0..1.
    pub fn sample(&self, c: Rgb) -> Rgb {
        let n = (self.size - 1) as f32;
        let mut i0 = [0usize; 3];
        let mut fr = [0f32; 3];
        for k in 0..3 {
            let x = c[k].clamp(0.0, 1.0) * n;
            let i = (x.floor() as usize).min(self.size - 2);
            i0[k] = i;
            fr[k] = x - i as f32;
        }
        let mut out = [0f32; 3];
        for (db, wb) in [(0, 1.0 - fr[2]), (1, fr[2])] {
            for (dg, wg) in [(0, 1.0 - fr[1]), (1, fr[1])] {
                for (dr, wr) in [(0, 1.0 - fr[0]), (1, fr[0])] {
                    let w = wr * wg * wb;
                    if w == 0.0 {
                        continue;
                    }
                    let v = self.at(i0[0] + dr, i0[1] + dg, i0[2] + db);
                    for k in 0..3 {
                        out[k] += v[k] * w;
                    }
                }
            }
        }
        out
    }

    /// The same table at another size (trilinear resampling).
    pub fn resample(&self, size: usize) -> Table {
        if size == self.size {
            return self.clone();
        }
        Table::from_fn(size, |c| self.sample(c))
    }

    /// Little-endian f32 RGB triples in table order: what the UI preview uploads as a 3D texture.
    pub fn to_f32_le_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.data.len() * 12);
        for v in &self.data {
            for x in v {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        out
    }
}
