mod app;
mod backdrop;
mod config;
mod controller;
mod desktop;
mod fx;
mod game;
mod games_root;
mod maps;
mod menu_audio;
mod paths;
mod skate;
mod steam;
mod theme;
mod thumbs;
mod ui;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("2010 Rust Rewrite Mashup")
        .with_app_id(paths::APP_ID)
        .with_inner_size([1100.0, 640.0])
        .with_min_inner_size([900.0, 560.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(desktop::ICON_PNG) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        // No blocking vsync: on Wayland (NVIDIA especially) a vsync'd swap
        // blocks while the window is hidden or covered by the game, so the
        // launcher stops answering compositor pings ("not responding").
        // Frame pacing comes from `request_repaint_after` instead.
        glow_options: eframe::egui_glow::GlowConfiguration {
            vsync: false,
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "2010 Rust Rewrite Mashup",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(&cc.egui_ctx)))),
    )
}
