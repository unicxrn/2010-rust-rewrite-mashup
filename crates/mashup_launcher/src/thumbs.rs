//! Quick Play's map tiles: the cached loadscreens `backdrop::load` writes,
//! shrunk on a worker thread, plus a procedural tile for the Minecraft
//! world.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};

/// Thumbnail width; tiles are at most ~260 px wide, so this stays sharp on
/// a 1.5× display.
pub const THUMB_WIDTH: u32 = 384;

/// New textures uploaded to the GPU per frame.
const UPLOADS_PER_FRAME: usize = 2;

/// One shrunk loadscreen: the zone it belongs to, size and RGBA.
type Thumb = (String, u32, u32, Vec<u8>);

/// Area-average downscale of RGBA to `target` px wide, keeping the aspect;
/// images already that narrow come back unchanged.
pub fn downscale(width: u32, height: u32, rgba: &[u8], target: u32) -> (u32, u32, Vec<u8>) {
    if width <= target || height == 0 || target == 0 {
        return (width, height, rgba.to_vec());
    }
    let out_w = target;
    let out_h = ((u64::from(height) * u64::from(target) + u64::from(width) / 2) / u64::from(width))
        .max(1) as u32;
    // Source span of output cell `i` along an axis of `src` → `dst` px.
    let span = |i: u32, src: u32, dst: u32| {
        let start = (u64::from(i) * u64::from(src) / u64::from(dst)) as usize;
        let end = (u64::from(i + 1) * u64::from(src) / u64::from(dst)) as usize;
        (start, end.max(start + 1))
    };
    let stride = width as usize * 4;
    let mut out = Vec::with_capacity(out_w as usize * out_h as usize * 4);
    for oy in 0..out_h {
        let (y0, y1) = span(oy, height, out_h);
        for ox in 0..out_w {
            let (x0, x1) = span(ox, width, out_w);
            let mut sum = [0u32; 4];
            for y in y0..y1 {
                let row = &rgba[y * stride + x0 * 4..y * stride + x1 * 4];
                for px in row.as_chunks::<4>().0 {
                    for (s, &v) in sum.iter_mut().zip(px) {
                        *s += u32::from(v);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            out.extend(sum.map(|s| ((s + n / 2) / n) as u8));
        }
    }
    (out_w, out_h, out)
}

/// `read_png` output as RGBA8: RGBA passes, grey+alpha and RGB are
/// widened, anything else (16-bit, a size mismatch) is refused, so a
/// foreign file in the cache can't panic the texture upload.
pub fn to_rgba8(width: u32, height: u32, data: Vec<u8>) -> Option<Vec<u8>> {
    let pixels = (width as usize).checked_mul(height as usize)?;
    if pixels == 0 {
        return None;
    }
    match data.len() / pixels {
        _ if !data.len().is_multiple_of(pixels) => None,
        4 => Some(data),
        3 => Some(
            data.as_chunks::<3>()
                .0
                .iter()
                .flat_map(|&[r, g, b]| [r, g, b, 255])
                .collect(),
        ),
        2 => Some(
            data.as_chunks::<2>()
                .0
                .iter()
                .flat_map(|&[v, a]| [v, v, v, a])
                .collect(),
        ),
        _ => None,
    }
}

/// The Minecraft tile: a 16×9 grass-block side, grass blades over dirt,
/// from a fixed hash so it never changes. Drawn nearest-filtered.
pub fn minecraft_pixels() -> ColorImage {
    const W: usize = 16;
    const H: usize = 9;
    let grass = [
        Color32::from_rgb(0x5f, 0xae, 0x3e),
        Color32::from_rgb(0x4f, 0x96, 0x33),
        Color32::from_rgb(0x6c, 0xbd, 0x48),
        Color32::from_rgb(0x43, 0x82, 0x2b),
    ];
    let dirt = [
        Color32::from_rgb(0x79, 0x55, 0x3a),
        Color32::from_rgb(0x8b, 0x67, 0x48),
        Color32::from_rgb(0x66, 0x47, 0x30),
        Color32::from_rgb(0x58, 0x3c, 0x29),
    ];
    let stone = Color32::from_rgb(0x6f, 0x6d, 0x68);
    let hash = |x: usize, y: usize| {
        let mut h = (x as u32).wrapping_mul(0x9e37_79b1) ^ (y as u32).wrapping_mul(0x85eb_ca77);
        h ^= h >> 15;
        h = h.wrapping_mul(0x2c1b_3c6d);
        h ^ (h >> 12)
    };
    let mut pixels = Vec::with_capacity(W * H);
    for y in 0..H {
        for x in 0..W {
            // Grass hangs 2 or 3 px down each column, like the block's side.
            let depth = 2 + (hash(x, 99) % 3 == 0) as usize;
            let h = hash(x, y) as usize;
            let color = if y < depth {
                grass[h % grass.len()]
            } else if h.is_multiple_of(23) {
                stone
            } else {
                // Dirt darkens with depth.
                dirt[h % dirt.len()].gamma_multiply(1.0 - 0.05 * (y - depth) as f32)
            };
            pixels.push(Color32::from_rgb(color.r(), color.g(), color.b()));
        }
    }
    ColorImage::new([W, H], pixels)
}

/// Textures by zone, loaded on demand and kept for the session.
#[derive(Default)]
pub struct Thumbs {
    textures: HashMap<String, TextureHandle>,
    minecraft: Option<TextureHandle>,
    incoming: Option<Receiver<Thumb>>,
    /// When the last worker was started, in `ctx.input(|i| i.time)` units.
    last_request: Option<f64>,
}

impl Thumbs {
    /// Forgets every texture (another MW2 folder, or a reset).
    pub fn clear(&mut self) {
        *self = Self {
            minecraft: self.minecraft.take(),
            ..Self::default()
        };
    }

    pub fn get(&self, zone: &str) -> Option<&TextureHandle> {
        if zone == crate::maps::MINECRAFT {
            return self.minecraft.as_ref();
        }
        self.textures.get(zone)
    }

    /// Whether a worker is still reading thumbnails.
    pub fn loading(&self) -> bool {
        self.incoming.is_some()
    }

    pub fn last_request(&self) -> Option<f64> {
        self.last_request
    }

    /// Starts a worker reading the cached loadscreens of the given zones
    /// that have no texture yet. Missing PNGs are skipped quietly; asking
    /// again later picks up the ones `backdrop::load` has written since.
    pub fn request(&mut self, cache: PathBuf, zones: &[String], now: f64) {
        if self.incoming.is_some() {
            return;
        }
        let wanted: Vec<(String, PathBuf)> = zones
            .iter()
            .filter(|zone| !self.textures.contains_key(*zone))
            .filter_map(|zone| Some((zone.clone(), crate::maps::thumb_path(&cache, zone)?)))
            .filter(|(_, path)| path.is_file())
            .collect();
        self.last_request = Some(now);
        if wanted.is_empty() {
            return;
        }
        let (tx, rx) = channel();
        self.incoming = Some(rx);
        std::thread::spawn(move || {
            for (zone, path) in wanted {
                let Some((w, h, rgba)) = crate::backdrop::read_png_rgba8(&path) else {
                    continue;
                };
                let (w, h, rgba) = downscale(w, h, &rgba, THUMB_WIDTH);
                if tx.send((zone, w, h, rgba)).is_err() {
                    return;
                }
            }
        });
    }

    /// Whether some of `zones` still have no texture.
    pub fn missing(&self, zones: &[String]) -> bool {
        zones
            .iter()
            .any(|zone| zone.starts_with("mp_") && !self.textures.contains_key(zone))
    }

    /// Uploads at most two finished thumbnails, and makes the Minecraft
    /// tile on first use.
    pub fn upload(&mut self, ctx: &egui::Context) {
        if self.minecraft.is_none() {
            self.minecraft = Some(ctx.load_texture(
                "quickplay-minecraft",
                minecraft_pixels(),
                TextureOptions::NEAREST,
            ));
        }
        let Some(rx) = &self.incoming else {
            return;
        };
        for _ in 0..UPLOADS_PER_FRAME {
            match rx.try_recv() {
                Ok((zone, w, h, rgba)) => {
                    let image = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                    let texture = ctx.load_texture(
                        format!("quickplay-{zone}"),
                        image,
                        TextureOptions::LINEAR,
                    );
                    self.textures.insert(zone, texture);
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.incoming = None;
                    return;
                }
            }
        }
        // More may be waiting; come back next frame.
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscale_averages_and_keeps_aspect() {
        // 4×2: left half black, right half white → 2×1 black, white.
        let mut rgba = Vec::new();
        for _ in 0..2 {
            for x in 0..4 {
                let v = if x < 2 { 0 } else { 255 };
                rgba.extend([v, v, v, 255]);
            }
        }
        let (w, h, out) = downscale(4, 2, &rgba, 2);
        assert_eq!((w, h), (2, 1));
        assert_eq!(out, [0, 0, 0, 255, 255, 255, 255, 255]);
        // Odd ratios still cover every output pixel.
        let big = vec![128u8; 1280 * 720 * 4];
        let (w, h, out) = downscale(1280, 720, &big, THUMB_WIDTH);
        assert_eq!((w, h), (384, 216));
        assert_eq!(out.len(), 384 * 216 * 4);
        assert!(out.iter().all(|&v| v == 128));
        // Narrow images pass through.
        assert_eq!(
            downscale(2, 1, &[1, 2, 3, 4, 5, 6, 7, 8], 384).2,
            [1, 2, 3, 4, 5, 6, 7, 8]
        );
    }

    #[test]
    fn to_rgba8_widens_or_refuses() {
        assert_eq!(to_rgba8(1, 1, vec![1, 2, 3, 4]), Some(vec![1, 2, 3, 4]));
        assert_eq!(to_rgba8(1, 1, vec![1, 2, 3]), Some(vec![1, 2, 3, 255]));
        assert_eq!(
            to_rgba8(2, 1, vec![9, 7, 5, 3]),
            Some(vec![9, 9, 9, 7, 5, 5, 5, 3])
        );
        // 16-bit RGBA, a short buffer, an empty image.
        assert_eq!(to_rgba8(1, 1, vec![0; 8]), None);
        assert_eq!(to_rgba8(2, 2, vec![0; 15]), None);
        assert_eq!(to_rgba8(0, 4, Vec::new()), None);
    }

    #[test]
    fn grey_png_in_the_cache_still_loads() {
        let cache = crate::paths::test_dir("thumbs-grey");
        let file = std::fs::File::create(cache.join("loadscreen_mp_rust.png")).unwrap();
        let mut encoder = png::Encoder::new(file, 400, 2);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[90; 800]).unwrap();
        writer.finish().unwrap();
        let mut thumbs = Thumbs::default();
        thumbs.request(cache, &["mp_rust".to_owned()], 0.0);
        let got: Vec<Thumb> = thumbs.incoming.take().unwrap().iter().collect();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].3.len(), (got[0].1 * got[0].2 * 4) as usize);
        assert_eq!(&got[0].3[..4], &[90, 90, 90, 255]);
    }

    #[test]
    fn minecraft_tile_is_deterministic_grass_over_dirt() {
        let a = minecraft_pixels();
        assert_eq!(a.size, [16, 9]);
        assert_eq!(a.pixels, minecraft_pixels().pixels);
        // Top row is grass (green-dominant), bottom row is dirt or stone.
        assert!(
            a.pixels[..16]
                .iter()
                .all(|c| c.g() > c.r() && c.g() > c.b())
        );
        assert!(a.pixels[16 * 8..].iter().all(|c| c.g() <= c.r()));
    }

    #[test]
    fn request_skips_missing_pngs() {
        let cache = crate::paths::test_dir("thumbs");
        let rgba = vec![200u8; 800 * 450 * 4];
        crate::backdrop::write_png(&cache.join("loadscreen_mp_rust.png"), 800, 450, &rgba).unwrap();
        let mut thumbs = Thumbs::default();
        let zones = vec!["mp_rust".to_owned(), "mp_derail".to_owned()];
        thumbs.request(cache, &zones, 0.0);
        let rx = thumbs.incoming.take().unwrap();
        let got: Vec<Thumb> = rx.iter().collect();
        assert_eq!(got.len(), 1);
        let (zone, w, h, _) = &got[0];
        assert_eq!((zone.as_str(), *w, *h), ("mp_rust", 384, 216));
        assert!(thumbs.missing(&zones));
    }
}
