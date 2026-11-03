//! Thumbnails (SPEC §8).
//!
//! - [`ThumbCache`]: clip thumbnails fetched from Resolume over REST and cached on disk as
//!   `<cache>/<clip id>-<last_update>.png`. A request names the version it wants (`v`); a cached
//!   file of that version is served without asking Resolume, so inactive decks and an offline
//!   Resolume still show the last known thumbnails.
//! - [`render_mosaic`]: the sequence thumbnail: name on top, one row per lane, member clip
//!   thumbnails in order with widths proportional to step length.

use std::path::{Path, PathBuf};

use image::{imageops, Rgba, RgbaImage};
use resolume_client::rest::RestClient;

use crate::engine::view::SequenceView;

pub struct ThumbCache {
    dir: PathBuf,
    rest: RestClient,
}

impl ThumbCache {
    pub fn new(dir: PathBuf, rest: RestClient) -> Self {
        Self { dir, rest }
    }

    fn path(&self, id: u64, version: &str) -> PathBuf {
        let safe: String = version
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        self.dir.join(format!("{id}-{safe}.png"))
    }

    /// Whether this version of the clip's thumbnail is on disk.
    pub async fn is_cached(&self, id: u64, version: &str) -> bool {
        tokio::fs::try_exists(self.path(id, version))
            .await
            .unwrap_or(false)
    }

    /// PNG for a clip. `version` is the `thumbnail.last_update` the caller saw.
    pub async fn clip(&self, id: u64, version: Option<&str>) -> Option<Vec<u8>> {
        if let Some(v) = version {
            if let Ok(png) = tokio::fs::read(self.path(id, v)).await {
                return Some(png);
            }
        }
        match self.rest.thumbnail(id).await {
            Ok(t) => {
                let v = t.etag.as_deref().or(version).unwrap_or("0").to_owned();
                self.store(id, &v, &t.png).await;
                Some(t.png)
            }
            Err(e) => {
                log::debug!("thumbnail {id} from Resolume: {e}");
                self.newest_cached(id).await
            }
        }
    }

    async fn store(&self, id: u64, version: &str, png: &[u8]) {
        if tokio::fs::create_dir_all(&self.dir).await.is_err() {
            return;
        }
        let path = self.path(id, version);
        let tmp = path.with_extension("tmp");
        if tokio::fs::write(&tmp, png).await.is_ok() && tokio::fs::rename(&tmp, &path).await.is_ok()
        {
            // Drop older versions of this clip's thumbnail.
            for old in self.cached_files(id).await {
                if old != path {
                    let _ = tokio::fs::remove_file(old).await;
                }
            }
        }
    }

    async fn cached_files(&self, id: u64) -> Vec<PathBuf> {
        let prefix = format!("{id}-");
        let mut out = Vec::new();
        if let Ok(mut rd) = tokio::fs::read_dir(&self.dir).await {
            while let Ok(Some(e)) = rd.next_entry().await {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with(&prefix) && name.ends_with(".png") {
                    out.push(e.path());
                }
            }
        }
        out
    }

    async fn newest_cached(&self, id: u64) -> Option<Vec<u8>> {
        let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
        for p in self.cached_files(id).await {
            if let Ok(m) = tokio::fs::metadata(&p).await.and_then(|m| m.modified()) {
                if newest.as_ref().is_none_or(|(t, _)| m > *t) {
                    newest = Some((m, p));
                }
            }
        }
        tokio::fs::read(newest?.1).await.ok()
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

pub const MOSAIC_W: u32 = 320;
pub const MOSAIC_H: u32 = 240;
const HEADER_H: u32 = 36;
const BG: Rgba<u8> = Rgba([14, 15, 17, 255]);
const REST: Rgba<u8> = Rgba([28, 30, 34, 255]);
const UNRESOLVED: Rgba<u8> = Rgba([60, 20, 24, 255]);
const MISSING_THUMB: Rgba<u8> = Rgba([38, 41, 46, 255]);
const ACCENT: Rgba<u8> = Rgba([242, 163, 58, 255]);

/// What a mosaic cell shows.
pub enum Cell {
    Thumb(RgbaImage),
    /// Clip without a thumbnail available.
    Blank,
    Rest,
    Unresolved,
}

/// Cells of a sequence: per lane, (length in eighths, what to draw). `thumb` loads a clip
/// thumbnail by clip id.
pub async fn mosaic_cells<F, Fut>(view: &SequenceView, thumb: F) -> Vec<Vec<(u32, Cell)>>
where
    F: Fn(u64) -> Fut,
    Fut: std::future::Future<Output = Option<Vec<u8>>>,
{
    let mut lanes = Vec::new();
    for (li, lane) in view.sequence.lanes.iter().enumerate() {
        let res = view.resolution.as_ref().and_then(|r| r.lanes.get(li));
        let mut cells = Vec::new();
        for (si, step) in lane.steps.iter().enumerate() {
            let r = res.and_then(|l| l.steps.get(si));
            let empty_slot = r.is_some_and(|r| r.state == "empty");
            let cell = match (!step.is_rest() && !empty_slot, r.and_then(|r| r.clip_id)) {
                (false, _) => Cell::Rest,
                (true, Some(id)) => match thumb(id)
                    .await
                    .and_then(|png| image::load_from_memory(&png).ok())
                {
                    Some(img) => Cell::Thumb(img.to_rgba8()),
                    None => Cell::Blank,
                },
                (true, None) if r.is_some() => Cell::Unresolved,
                (true, None) => Cell::Blank, // deck clips unknown
            };
            cells.push((step.length, cell));
        }
        lanes.push(cells);
    }
    lanes
}

/// Renders the mosaic PNG (320×240, like Resolume's own thumbnails).
pub fn render_mosaic(name: &str, lanes: &[Vec<(u32, Cell)>]) -> Vec<u8> {
    let mut img = RgbaImage::from_pixel(MOSAIC_W, MOSAIC_H, BG);
    // Header: accent bar and the name.
    for x in 0..MOSAIC_W {
        for y in HEADER_H - 3..HEADER_H {
            img.put_pixel(x, y, ACCENT);
        }
    }
    draw_text(&mut img, 8, 8, name, 2, Rgba([230, 230, 227, 255]));

    let rows = lanes.len().max(1) as u32;
    let area_h = MOSAIC_H - HEADER_H;
    for (li, cells) in lanes.iter().enumerate() {
        let y0 = HEADER_H + area_h * li as u32 / rows;
        let y1 = HEADER_H + area_h * (li as u32 + 1) / rows;
        let total: u64 = cells
            .iter()
            .map(|(len, _)| u64::from(*len))
            .sum::<u64>()
            .max(1);
        let mut acc = 0u64;
        for (len, cell) in cells {
            let x0 = (acc * u64::from(MOSAIC_W) / total) as u32;
            acc += u64::from(*len);
            let x1 = (acc * u64::from(MOSAIC_W) / total) as u32;
            // 1 px gaps between cells and rows.
            let (cx0, cy0) = (x0 + 1, y0 + 1);
            let (w, h) = (x1.saturating_sub(cx0 + 1), y1.saturating_sub(cy0 + 1));
            if w == 0 || h == 0 {
                continue;
            }
            match cell {
                Cell::Thumb(src) => {
                    let fitted = cover(src, w, h);
                    imageops::overlay(&mut img, &fitted, i64::from(cx0), i64::from(cy0));
                }
                Cell::Blank => fill(&mut img, cx0, cy0, w, h, MISSING_THUMB),
                Cell::Rest => fill(&mut img, cx0, cy0, w, h, REST),
                Cell::Unresolved => fill(&mut img, cx0, cy0, w, h, UNRESOLVED),
            }
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .expect("PNG encoding to memory");
    out.into_inner()
}

/// Scales `src` to cover w×h and crops the centre.
fn cover(src: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let (sw, sh) = (src.width().max(1), src.height().max(1));
    let scale = (w as f32 / sw as f32).max(h as f32 / sh as f32);
    let (rw, rh) = (
        ((sw as f32 * scale).ceil() as u32).max(w),
        ((sh as f32 * scale).ceil() as u32).max(h),
    );
    let resized = imageops::resize(src, rw, rh, imageops::FilterType::Triangle);
    imageops::crop_imm(&resized, (rw - w) / 2, (rh - h) / 2, w, h).to_image()
}

fn fill(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, c: Rgba<u8>) {
    for yy in y..(y + h).min(img.height()) {
        for xx in x..(x + w).min(img.width()) {
            img.put_pixel(xx, yy, c);
        }
    }
}

fn draw_text(img: &mut RgbaImage, x: u32, y: u32, text: &str, scale: u32, color: Rgba<u8>) {
    use font8x8::UnicodeFonts;
    let mut cx = x;
    for ch in text.chars() {
        if cx + 8 * scale > img.width() {
            break;
        }
        let glyph = font8x8::BASIC_FONTS
            .get(ch)
            .or_else(|| font8x8::LATIN_FONTS.get(ch))
            .unwrap_or_else(|| font8x8::BASIC_FONTS.get('?').expect("ascii"));
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..8u32 {
                if bits & (1 << col) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let (px, py) = (cx + col * scale + dx, y + row as u32 * scale + dy);
                            if px < img.width() && py < img.height() {
                                img.put_pixel(px, py, color);
                            }
                        }
                    }
                }
            }
        }
        cx += 8 * scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mosaic_is_a_320x240_png_with_proportional_cells() {
        let red = RgbaImage::from_pixel(160, 120, Rgba([255, 0, 0, 255]));
        let lanes = vec![
            vec![(8, Cell::Thumb(red)), (8, Cell::Rest)],
            vec![(4, Cell::Unresolved), (12, Cell::Blank)],
        ];
        let png = render_mosaic("§ Drop Ü", &lanes);
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!((img.width(), img.height()), (MOSAIC_W, MOSAIC_H));
        // Lane 1, left half is the red thumbnail, right half the rest colour.
        let lane1_y = HEADER_H + (MOSAIC_H - HEADER_H) / 4;
        assert_eq!(img.get_pixel(80, lane1_y), &Rgba([255, 0, 0, 255]));
        assert_eq!(img.get_pixel(240, lane1_y), &REST);
        // Lane 2: first quarter unresolved.
        let lane2_y = HEADER_H + 3 * (MOSAIC_H - HEADER_H) / 4;
        assert_eq!(img.get_pixel(40, lane2_y), &UNRESOLVED);
        assert_eq!(img.get_pixel(200, lane2_y), &MISSING_THUMB);
        // Some text pixels were drawn in the header.
        assert!((8..200).any(|x| img.get_pixel(x, 14) != &BG));
    }

    #[tokio::test]
    async fn cache_serves_known_versions_without_resolume() {
        let dir = tempfile::tempdir().unwrap();
        // Nothing listens on this port: every REST call fails.
        let cache = ThumbCache::new(
            dir.path().to_owned(),
            RestClient::new("http://127.0.0.1:9/api/v1"),
        );
        assert_eq!(cache.clip(7, Some("123")).await, None);
        cache.store(7, "123", b"png-v1").await;
        assert_eq!(
            cache.clip(7, Some("123")).await.as_deref(),
            Some(&b"png-v1"[..])
        );
        // Unknown version and Resolume offline: newest cached file.
        assert_eq!(
            cache.clip(7, Some("999")).await.as_deref(),
            Some(&b"png-v1"[..])
        );
        cache.store(7, "456", b"png-v2").await;
        assert_eq!(cache.cached_files(7).await.len(), 1, "old version removed");
        assert_eq!(cache.clip(7, None).await.as_deref(), Some(&b"png-v2"[..]));
    }
}
