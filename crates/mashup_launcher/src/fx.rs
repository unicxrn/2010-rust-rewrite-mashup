//! Film grain, scanlines and the readability gradient over the backdrop.
use eframe::egui::{
    self, Color32, ColorImage, Mesh, Pos2, Rect, TextureHandle, TextureOptions, pos2,
};

pub struct Fx {
    grain: TextureHandle,
    scan: TextureHandle,
}

impl Fx {
    pub fn new(ctx: &egui::Context) -> Self {
        let size = 256;
        let mut seed = 0x9e37_79b9_u32;
        let pixels = (0..size * size)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let v = (seed & 0xff) as u8;
                let alpha = v / 9;
                // Mix dark and light speckles so the grain reads like film
                // grain rather than just lifting blacks.
                if seed & 0x100 == 0 {
                    Color32::from_black_alpha(alpha)
                } else {
                    Color32::from_white_alpha(alpha)
                }
            })
            .collect();
        let grain = ctx.load_texture(
            "grain",
            ColorImage {
                size: [size, size],
                pixels,
                source_size: egui::vec2(size as f32, size as f32),
            },
            TextureOptions::NEAREST_REPEAT,
        );
        let scan = ctx.load_texture(
            "scan",
            ColorImage {
                size: [1, 3],
                pixels: vec![
                    Color32::TRANSPARENT,
                    Color32::TRANSPARENT,
                    Color32::from_black_alpha(46),
                ],
                source_size: egui::vec2(1.0, 3.0),
            },
            TextureOptions::NEAREST_REPEAT,
        );
        Self { grain, scan }
    }

    /// Dark wash from the left so menu text always reads, plus a bottom fade.
    pub fn gradient(painter: &egui::Painter, rect: Rect) {
        let mut mesh = Mesh::default();
        let left = Color32::from_black_alpha(225);
        let mid = Color32::from_black_alpha(140);
        let right = Color32::from_black_alpha(40);
        let x1 = rect.left() + rect.width() * 0.48;
        quad(
            &mut mesh,
            rect.left_top(),
            pos2(x1, rect.bottom()),
            left,
            mid,
        );
        quad(
            &mut mesh,
            pos2(x1, rect.top()),
            rect.right_bottom(),
            mid,
            right,
        );
        painter.add(mesh);
        let fade = Rect::from_min_max(
            pos2(rect.left(), rect.bottom() - 160.0),
            rect.right_bottom(),
        );
        let mut mesh = Mesh::default();
        let (top, bottom) = (Color32::TRANSPARENT, Color32::from_black_alpha(200));
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(fade.left_top(), top);
        mesh.colored_vertex(fade.right_top(), top);
        mesh.colored_vertex(fade.right_bottom(), bottom);
        mesh.colored_vertex(fade.left_bottom(), bottom);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
        painter.add(mesh);
    }

    /// Grain jittered every frame and fixed scanlines, one every 3 physical
    /// pixels regardless of display scale.
    pub fn overlay(&self, painter: &egui::Painter, rect: Rect, time: f64, pixels_per_point: f32) {
        let jitter = ((time * 24.0).floor() * 0.618_034).fract() as f32;
        let uv = Rect::from_min_size(
            pos2(jitter, jitter * 1.7),
            egui::vec2(rect.width() / 256.0, rect.height() / 256.0),
        );
        painter.image(self.grain.id(), rect, uv, Color32::WHITE);
        let scan_uv = Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(1.0, rect.height() * pixels_per_point / 3.0),
        );
        painter.image(self.scan.id(), rect, scan_uv, Color32::WHITE);
    }
}

/// Horizontal gradient quad from `left` colour to `right` colour.
fn quad(mesh: &mut Mesh, min: Pos2, max: Pos2, left: Color32, right: Color32) {
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(min, left);
    mesh.colored_vertex(pos2(max.x, min.y), right);
    mesh.colored_vertex(max, right);
    mesh.colored_vertex(pos2(min.x, max.y), left);
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
}
