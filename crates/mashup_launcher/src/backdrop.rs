//! The player's own MW2 loadscreens as the launcher backdrop: decoded once
//! from `main/iw_*.iwd` with the game's IWI decoder, cached as PNG.
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui::{self, Color32, ColorImage, Mesh, Rect, TextureHandle, TextureOptions, pos2};

const HOLD: f64 = 12.0;
const FADE: f64 = 1.5;

/// New textures uploaded to the GPU per frame, so a burst of decoded
/// loadscreens doesn't stall a paint.
const UPLOADS_PER_FRAME: usize = 2;

pub fn loadscreen_names<'a>(entries: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.strip_prefix("images/")?.strip_suffix(".iwi"))
        .filter(|name| name.starts_with("loadscreen_mp_") && *name != "loadscreen_mp_dlc")
        .map(str::to_owned)
        .collect();
    names.sort();
    names.dedup();
    names
}

fn iwd_entries(main: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(main).into_iter().flatten().flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("iwd"))
            && let Ok(file) = std::fs::File::open(&path)
            && let Ok(archive) = zip::ZipArchive::new(file)
        {
            names.extend(archive.file_names().map(str::to_owned));
        }
    }
    names
}

/// Counter giving each temp file written in this process a distinct name, so
/// concurrent `write_png` calls (e.g. two decoder threads, or a re-run) never
/// collide on the same `.tmp` path.
static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Writes to a uniquely-named temp file next to `path` and renames it over
/// `path` on success, so a reader never sees a half-written cache file. The
/// temp file is removed if any step fails.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let n = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = PathBuf::from(format!("{}.{}.{n}.tmp", path.display(), std::process::id()));
    match write_png_unchecked(&tmp, width, height, rgba) {
        Ok(()) => std::fs::rename(&tmp, path).map_err(|e| e.to_string()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn write_png_unchecked(tmp: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(tmp).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    // `finish` validates the stream, writes IEND and flushes the BufWriter.
    writer.finish().map_err(|e| e.to_string())
}

pub fn read_png(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("png too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    Ok((info.width, info.height, buf))
}

/// A cached PNG as RGBA8, or `None` when it is missing or in a format
/// `to_rgba8` refuses.
pub fn read_png_rgba8(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h, data) = read_png(path).ok()?;
    Some((w, h, crate::thumbs::to_rgba8(w, h, data)?))
}

/// Decoded images, one per message, as they become available. Cached PNGs
/// come first; missing ones are decoded from the IWDs and cached.
pub fn load(mw2: PathBuf, cache: PathBuf) -> Receiver<(u32, u32, Vec<u8>)> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(&cache);
        let main = mw2.join("main");
        let mut names = loadscreen_names(iwd_entries(&main).iter().map(String::as_str));
        names.insert(0, "menu_mp_image".to_owned());
        for name in names {
            let png = cache.join(format!("{name}.png"));
            let image = read_png_rgba8(&png).or_else(|| {
                let (w, h, rgba) =
                    asset_material::decode_ui_image_from_main(&main, &name).ok()??;
                let _ = write_png(&png, w, h, &rgba);
                Some((w, h, rgba))
            });
            if let Some(image) = image
                && tx.send(image).is_err()
            {
                return;
            }
        }
    });
    rx
}

/// Cross-fading, slowly panning backdrop fed by `load`.
#[derive(Default)]
pub struct Backdrop {
    incoming: Option<Receiver<(u32, u32, Vec<u8>)>>,
    textures: Vec<TextureHandle>,
    /// Index of the texture currently on screen.
    shown: usize,
    /// When `shown` started being displayed, in `ctx.input(|i| i.time)`
    /// units. `None` until the first texture exists, so the slideshow never
    /// "runs" while the backdrop is still just the procedural placeholder.
    slot_start: Option<f64>,
    /// The index the crossfade is heading into, locked in on the first frame
    /// of the fade window so a texture arriving mid-fade can't retarget it
    /// (e.g. len=2, shown=1 fading to 0, a 3rd texture arrives: without this
    /// lock `(shown + 1) % len` would jump to 2 instead of finishing at 0).
    fading_to: Option<usize>,
}

impl Backdrop {
    pub fn start(&mut self, mw2: PathBuf, cache: PathBuf) {
        self.incoming = Some(load(mw2, cache));
        self.textures.clear();
        self.shown = 0;
        self.slot_start = None;
        self.fading_to = None;
    }

    /// Whether loadscreens are still being decoded (and cached).
    pub fn loading(&self) -> bool {
        self.incoming.is_some()
    }

    pub fn paint(&mut self, ctx: &egui::Context, painter: &egui::Painter, rect: Rect) {
        if let Some(rx) = &self.incoming {
            let mut uploaded = 0;
            loop {
                if uploaded >= UPLOADS_PER_FRAME {
                    // More may be waiting in the channel; come back next frame
                    // instead of stalling this one on GPU uploads.
                    ctx.request_repaint();
                    break;
                }
                match rx.try_recv() {
                    Ok((w, h, rgba)) => {
                        let image =
                            ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                        let name = format!("backdrop-{}", self.textures.len());
                        self.textures
                            .push(ctx.load_texture(name, image, TextureOptions::LINEAR));
                        uploaded += 1;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.incoming = None;
                        break;
                    }
                }
            }
        }

        if self.textures.is_empty() {
            paint_placeholder(painter, rect);
            return;
        }

        let now = ctx.input(|i| i.time);
        // Copy out, advance locally, write back: keeps `shown`'s updates
        // and this read from aliasing the same `&mut self` borrow.
        let mut slot_start = *self.slot_start.get_or_insert(now);
        let len = self.textures.len();
        while now - slot_start >= HOLD {
            self.shown = self.fading_to.take().unwrap_or((self.shown + 1) % len);
            slot_start += HOLD;
        }
        self.slot_start = Some(slot_start);

        let local = now - slot_start;
        let current = self.shown;
        self.draw(painter, rect, current, local, 1.0);
        if local > HOLD - FADE && len > 1 {
            let next = *self.fading_to.get_or_insert((current + 1) % len);
            let alpha = ((local - (HOLD - FADE)) / FADE) as f32;
            self.draw(painter, rect, next, local - HOLD, alpha);
        }
    }

    /// Cover-fit with a slow zoom and drift ("Ken Burns").
    fn draw(&self, painter: &egui::Painter, rect: Rect, index: usize, local: f64, alpha: f32) {
        let texture = &self.textures[index];
        let [tw, th] = texture.size().map(|v| v as f32);
        let (aspect_view, aspect_tex) = (rect.width() / rect.height(), tw / th);
        let (mut uw, mut uh) = if aspect_tex > aspect_view {
            (aspect_view / aspect_tex, 1.0)
        } else {
            (1.0, aspect_tex / aspect_view)
        };
        let p = ((local + FADE) / (HOLD + FADE)).clamp(0.0, 1.0) as f32;
        let zoom = 1.0 - 0.08 * p;
        uw *= zoom;
        uh *= zoom;
        let dir = if index.is_multiple_of(2) { 1.0 } else { -1.0 };
        let cx = 0.5 + dir * (1.0 - uw) * 0.5 * (p - 0.5);
        let cy = 0.5 - (1.0 - uh) * 0.25 * (p - 0.5);
        let uv = Rect::from_center_size(pos2(cx, cy), egui::vec2(uw, uh));
        painter.image(
            texture.id(),
            rect,
            uv,
            Color32::from_white_alpha((alpha * 255.0) as u8),
        );
    }
}

/// Shown before MW2 has been found or any loadscreen has decoded: a subtle
/// dark olive diagonal gradient with a faint vignette, so the very first
/// screen still looks intentional. `Fx::overlay` draws grain and scanlines
/// over this separately.
fn paint_placeholder(painter: &egui::Painter, rect: Rect) {
    let near = Color32::from_rgb(0x1a, 0x20, 0x12);
    let far = crate::theme::BG;
    let mut mesh = Mesh::default();
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(rect.left_top(), far);
    mesh.colored_vertex(rect.right_top(), near);
    mesh.colored_vertex(rect.right_bottom(), far);
    mesh.colored_vertex(rect.left_bottom(), far);
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
    painter.add(mesh);

    let olive = crate::theme::OLIVE;
    let vignette = Color32::from_rgba_unmultiplied(olive.r(), olive.g(), olive.b(), 10);
    painter.rect_filled(rect, 0.0, vignette);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_full_size_map_loadscreens() {
        let names = [
            "images/loadscreen_mp_rust.iwi",
            "images/loadscreen_mp_dlc.iwi",
            "images/other.iwi",
        ];
        assert_eq!(
            loadscreen_names(names.iter().copied()),
            vec!["loadscreen_mp_rust".to_owned()]
        );
    }

    #[test]
    fn png_round_trip() {
        let dir = crate::paths::test_dir("png");
        let path = dir.join("a.png");
        let rgba = vec![10u8; 4 * 3 * 2];
        write_png(&path, 3, 2, &rgba).unwrap();
        assert_eq!(read_png(&path).unwrap(), (3, 2, rgba));
    }

    /// Run against a real MW2 install:
    /// `MASHUP_MW2=/path/to/mw2 cargo test -p mashup_launcher --profile play -- --ignored backdrop`
    #[test]
    #[ignore]
    fn loads_real_loadscreens() {
        let Some(mw2) = std::env::var_os("MASHUP_MW2").map(PathBuf::from) else {
            panic!("set MASHUP_MW2=<path to MW2 install> to run this test");
        };
        let cache = crate::paths::test_dir("backdrop-real");
        let rx = load(mw2, cache.clone());
        let mut images = Vec::new();
        while let Ok(image) = rx.recv() {
            images.push(image);
        }
        assert!(
            images.len() >= 20,
            "expected at least 20 loadscreens, got {}",
            images.len()
        );
        for (w, _h, _rgba) in &images {
            assert!(*w >= 1000, "image narrower than expected: {w}px");
        }
        for (i, (w, h, rgba)) in images.iter().take(3).enumerate() {
            write_png(&cache.join(format!("sample-{i}.png")), *w, *h, rgba).unwrap();
            eprintln!("sample-{i}.png written to {}", cache.display());
        }
    }
}
