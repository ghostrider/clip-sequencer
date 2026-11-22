//! The `.cube` text format (Adobe/Resolve "Cube LUT Specification 1.0"): `TITLE`, `LUT_1D_SIZE` or
//! `LUT_3D_SIZE`, optional `DOMAIN_MIN`/`DOMAIN_MAX`, then one `r g b` line per entry, red fastest. `#` starts a
//! comment. Import turns either kind into a 3D [`Table`] over 0..1; export always writes a 3D table.

use crate::table::Table;
use crate::Rgb;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CubeError {
    #[error("line {line}: {msg}")]
    Syntax { line: usize, msg: String },
    #[error("no LUT_1D_SIZE or LUT_3D_SIZE")]
    NoSize,
    #[error("{0}: size must be 2..=256")]
    BadSize(usize),
    #[error("expected {expected} entries, found {found}")]
    Count { expected: usize, found: usize },
    #[error("DOMAIN_MAX must be greater than DOMAIN_MIN in every channel")]
    BadDomain,
}

/// Parses a `.cube` file. A 1D LUT becomes a 3D table of `size` (at least 2, at most 65) that applies the three
/// channel curves; values outside the domain are clamped to it.
pub fn parse_cube(text: &str) -> Result<(Option<String>, Table), CubeError> {
    let mut title = None;
    let mut size1 = None;
    let mut size3 = None;
    let mut dmin: Rgb = [0.0; 3];
    let mut dmax: Rgb = [1.0; 3];
    let mut data: Vec<Rgb> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line_no = i + 1;
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let err = |msg: &str| CubeError::Syntax {
            line: line_no,
            msg: msg.to_string(),
        };
        let (key, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match key {
            "TITLE" => title = Some(rest.trim_matches('"').to_string()),
            "LUT_1D_SIZE" | "LUT_3D_SIZE" => {
                let n: usize = rest.parse().map_err(|_| err("size is not a number"))?;
                if !(2..=256).contains(&n) {
                    return Err(CubeError::BadSize(n));
                }
                if key == "LUT_1D_SIZE" {
                    size1 = Some(n)
                } else {
                    size3 = Some(n)
                }
            }
            "DOMAIN_MIN" | "DOMAIN_MAX" => {
                let v = triple(rest).ok_or_else(|| err("expected three numbers"))?;
                if key == "DOMAIN_MIN" {
                    dmin = v
                } else {
                    dmax = v
                }
            }
            // Other keywords (LUT_1D_INPUT_RANGE, LUT_3D_INPUT_RANGE, vendor extras) are ignored, like most readers do.
            k if k.starts_with(|c: char| c.is_ascii_alphabetic()) => {}
            _ => data.push(triple(line).ok_or_else(|| err("expected three numbers"))?),
        }
    }
    if (0..3).any(|k| dmax[k] <= dmin[k]) {
        return Err(CubeError::BadDomain);
    }
    // Where an input value (0..1) sits on the table's grid, which spans DOMAIN_MIN..DOMAIN_MAX.
    let grid = |k: usize, x: f32| ((x - dmin[k]) / (dmax[k] - dmin[k])).clamp(0.0, 1.0);
    let table = if let Some(n) = size3 {
        if data.len() != n * n * n {
            return Err(CubeError::Count {
                expected: n * n * n,
                found: data.len(),
            });
        }
        let t = Table { size: n, data };
        if dmin == [0.0; 3] && dmax == [1.0; 3] {
            t
        } else {
            // The table's grid spans the domain; re-express it over 0..1 input.
            let size = n.min(65);
            Table::from_fn(size, |c| t.sample(std::array::from_fn(|k| grid(k, c[k]))))
        }
    } else if let Some(n) = size1 {
        if data.len() != n {
            return Err(CubeError::Count {
                expected: n,
                found: data.len(),
            });
        }
        let curve = |k: usize, x: f32| -> f32 {
            let pos = grid(k, x) * (n - 1) as f32;
            let i = (pos.floor() as usize).min(n - 2);
            let f = pos - i as f32;
            data[i][k] * (1.0 - f) + data[i + 1][k] * f
        };
        Table::from_fn(crate::LUT_SIZE, |c| std::array::from_fn(|k| curve(k, c[k])))
    } else {
        return Err(CubeError::NoSize);
    };
    Ok((title, table))
}

fn triple(s: &str) -> Option<Rgb> {
    let mut it = s
        .split_whitespace()
        .map(|x| x.parse::<f32>().ok().filter(|v| v.is_finite()));
    let v = [it.next()??, it.next()??, it.next()??];
    it.next().is_none().then_some(v)
}

/// Writes a 3D `.cube` (0..1 domain, six decimals) with a `TITLE` line.
pub fn write_cube(title: &str, table: &Table) -> String {
    let mut out = String::with_capacity(table.data.len() * 27 + 64);
    let clean: String = title
        .chars()
        .filter(|c| *c != '"' && !c.is_control())
        .collect();
    out.push_str(&format!(
        "TITLE \"{clean}\"\n# Clip Sequencer for Resolume\nLUT_3D_SIZE {}\n",
        table.size
    ));
    for v in &table.data {
        out.push_str(&format!("{:.6} {:.6} {:.6}\n", v[0], v[1], v[2]));
    }
    out
}
