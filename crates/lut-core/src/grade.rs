use crate::curve::{Curves, PreparedCurve};
use crate::table::Table;
use crate::Rgb;
use serde::{Deserialize, Serialize};

/// A colour wheel value: one master value plus one per channel. For lift the values add, for gamma and gain they
/// multiply (see [`Grade::apply`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Wheel {
    pub master: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Wheel {
    pub const fn uniform(v: f32) -> Wheel {
        Wheel {
            master: v,
            r: v,
            g: v,
            b: v,
        }
    }
    fn channel(&self, k: usize) -> f32 {
        [self.r, self.g, self.b][k]
    }
}

/// The primary grading controls of a library LUT. Every default changes nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grade {
    /// Stops, -4..4: multiplies by 2^exposure.
    pub exposure: f32,
    /// -1 (cool, blue) .. 1 (warm, orange).
    pub temperature: f32,
    /// -1 (green) .. 1 (magenta).
    pub tint: f32,
    /// Added to the shadows: v + lift·(1 − v). -0.5..0.5.
    pub lift: Wheel,
    /// Midtones: v^(1/gamma). 0.2..5.
    pub gamma: Wheel,
    /// Multiplies: v·gain. 0..4.
    pub gain: Wheel,
    /// Around mid grey 0.5: 0..3, 1 = unchanged.
    pub contrast: f32,
    /// 0 = grey, 1 = unchanged, up to 3.
    pub saturation: f32,
}

impl Default for Grade {
    fn default() -> Self {
        Grade {
            exposure: 0.0,
            temperature: 0.0,
            tint: 0.0,
            lift: Wheel::uniform(0.0),
            gamma: Wheel::uniform(1.0),
            gain: Wheel::uniform(1.0),
            contrast: 1.0,
            saturation: 1.0,
        }
    }
}

/// Rec. 709 luma weights (the saturation control keeps luma).
const LUMA: Rgb = [0.2126, 0.7152, 0.0722];

impl Grade {
    /// Applies the controls to one colour, without the curves. Order: exposure, white balance, lift/gamma/gain,
    /// contrast, saturation; clamped to 0..1.
    pub fn apply(&self, c: Rgb) -> Rgb {
        let mut v = c;
        let ex = 2f32.powf(self.exposure);
        let t = self.temperature.clamp(-1.0, 1.0) * 0.25;
        let n = self.tint.clamp(-1.0, 1.0) * 0.25;
        let wb = [1.0 + t, 1.0 - n, 1.0 - t];
        for k in 0..3 {
            let mut x = v[k] * ex * wb[k];
            let gain = self.gain.master * self.gain.channel(k);
            let lift = self.lift.master + self.lift.channel(k);
            let gamma = (self.gamma.master * self.gamma.channel(k)).max(0.01);
            x *= gain;
            x += lift * (1.0 - x);
            x = x.max(0.0).powf(1.0 / gamma);
            x = (x - 0.5) * self.contrast.max(0.0) + 0.5;
            v[k] = x;
        }
        let y = LUMA[0] * v[0] + LUMA[1] * v[1] + LUMA[2] * v[2];
        let s = self.saturation.max(0.0);
        for x in &mut v {
            *x = (y + (*x - y) * s).clamp(0.0, 1.0);
        }
        v
    }
}

/// Grade plus curves on top of an optional base table: the whole transform of a library LUT.
pub(crate) struct Pipeline<'a> {
    base: Option<&'a Table>,
    grade: &'a Grade,
    master: PreparedCurve,
    channels: [PreparedCurve; 3],
}

impl<'a> Pipeline<'a> {
    pub(crate) fn new(base: Option<&'a Table>, grade: &'a Grade, curves: &Curves) -> Self {
        Pipeline {
            base,
            grade,
            master: curves.master.prepare(),
            channels: [curves.r.prepare(), curves.g.prepare(), curves.b.prepare()],
        }
    }

    pub(crate) fn apply(&self, c: Rgb) -> Rgb {
        let c = match self.base {
            Some(t) => t.sample(c),
            None => c,
        };
        let mut v = self.grade.apply(c);
        for (k, x) in v.iter_mut().enumerate() {
            *x = self.channels[k].eval(self.master.eval(*x));
        }
        v
    }
}
