use serde::{Deserialize, Serialize};

/// A tone curve through control points `[x, y]` (0..1), joined by a monotone cubic spline (Fritsch–Carlson), so it
/// never overshoots between points: monotone points give a monotone curve, which avoids banding and inversions.
/// Left of the first point the curve holds the first point's y, right of the last the last's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub points: Vec<[f32; 2]>,
}

impl Default for Curve {
    fn default() -> Self {
        Curve {
            points: vec![[0.0, 0.0], [1.0, 1.0]],
        }
    }
}

impl Curve {
    /// True when the curve maps every value to itself (the default two points).
    pub fn is_identity(&self) -> bool {
        let p = self.prepare();
        (0..=64).all(|i| {
            let x = i as f32 / 64.0;
            (p.eval(x) - x).abs() < 1e-4
        })
    }

    /// Points clamped to 0..1, sorted by x, with duplicate x values merged (the last one wins).
    fn sorted(&self) -> Vec<[f32; 2]> {
        let mut p: Vec<[f32; 2]> = self
            .points
            .iter()
            .filter(|[x, y]| x.is_finite() && y.is_finite())
            .map(|[x, y]| [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)])
            .collect();
        p.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let mut out: Vec<[f32; 2]> = Vec::with_capacity(p.len());
        for q in p {
            match out.last_mut() {
                Some(last) if (q[0] - last[0]).abs() < 1e-6 => *last = q,
                _ => out.push(q),
            }
        }
        out
    }

    /// Precomputes the spline for many evaluations.
    pub fn prepare(&self) -> PreparedCurve {
        let p = self.sorted();
        let n = p.len();
        if n < 2 {
            return PreparedCurve {
                xs: p.iter().map(|q| q[0]).collect(),
                ys: p.iter().map(|q| q[1]).collect(),
                ms: vec![0.0; n],
            };
        }
        let xs: Vec<f32> = p.iter().map(|q| q[0]).collect();
        let ys: Vec<f32> = p.iter().map(|q| q[1]).collect();
        let d: Vec<f32> = (0..n - 1)
            .map(|i| (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]))
            .collect();
        let mut ms = vec![0f32; n];
        ms[0] = d[0];
        ms[n - 1] = d[n - 2];
        for i in 1..n - 1 {
            ms[i] = if d[i - 1] * d[i] <= 0.0 {
                0.0
            } else {
                (d[i - 1] + d[i]) / 2.0
            };
        }
        // Fritsch–Carlson: limit the tangents so each segment stays monotone.
        for i in 0..n - 1 {
            if d[i] == 0.0 {
                ms[i] = 0.0;
                ms[i + 1] = 0.0;
                continue;
            }
            let a = ms[i] / d[i];
            let b = ms[i + 1] / d[i];
            let s = a * a + b * b;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                ms[i] = t * a * d[i];
                ms[i + 1] = t * b * d[i];
            }
        }
        PreparedCurve { xs, ys, ms }
    }
}

/// A curve ready for evaluation (see [`Curve::prepare`]).
#[derive(Debug, Clone)]
pub struct PreparedCurve {
    xs: Vec<f32>,
    ys: Vec<f32>,
    ms: Vec<f32>,
}

impl PreparedCurve {
    pub fn eval(&self, x: f32) -> f32 {
        let n = self.xs.len();
        match n {
            0 => return x,
            1 => return self.ys[0],
            _ => {}
        }
        if x <= self.xs[0] {
            return self.ys[0];
        }
        if x >= self.xs[n - 1] {
            return self.ys[n - 1];
        }
        let i = self
            .xs
            .partition_point(|&v| v <= x)
            .saturating_sub(1)
            .min(n - 2);
        let h = self.xs[i + 1] - self.xs[i];
        let t = (x - self.xs[i]) / h;
        let (t2, t3) = (t * t, t * t * t);
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        (h00 * self.ys[i] + h10 * h * self.ms[i] + h01 * self.ys[i + 1] + h11 * h * self.ms[i + 1])
            .clamp(0.0, 1.0)
    }
}

/// The four RGB curves: `master` applies to all channels first, then each channel's own.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Curves {
    pub master: Curve,
    pub r: Curve,
    pub g: Curve,
    pub b: Curve,
}
