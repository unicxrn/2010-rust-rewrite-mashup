//! Everything drawn in the window.
use eframe::egui::{
    self, Align2, Color32, CornerRadius, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, pos2,
    vec2,
};

use crate::app::{App, ITEMS, Item, Panel};
use crate::controller::Controller;
use crate::theme::{
    BG, DANGER, GRASS, MUTED, OLIVE, OLIVE_DIM, PANEL, TEXT, WARN, display, mono, tracked,
};

/// Left edge of the title and menu text.
const MARGIN: f32 = 56.0;
/// The selection bar and status card start this far left of the text.
const INSET: f32 = 16.0;
const SIDE: f32 = 440.0;
const MENU_WIDTH: f32 = 400.0;
const CARD_WIDTH: f32 = 480.0;
/// Quick Play's panel: this share of the window, at least `QUICK_MIN`.
const QUICK_SHARE: f32 = 0.68;
const QUICK_MIN: f32 = 560.0;
/// Selection bar and its text for an item that can't run yet.
const BLOCKED_BAR: Color32 = Color32::from_rgb(0x2c, 0x32, 0x22);
const BLOCKED_TEXT: Color32 = Color32::from_rgb(0xa8, 0xad, 0x9b);

/// Vertical metrics, eased between the minimum window height and the
/// default one so nothing overlaps at 900×560.
struct Layout {
    top: f32,
    menu_top: f32,
    big_row: f32,
    row: f32,
    big_gap: f32,
    gap: f32,
    card_bottom: f32,
    /// Status card: its first row's centre below the card's top, and the
    /// pitch between rows.
    card_first: f32,
    card_row: f32,
    /// Space under the last row's centre.
    card_foot: f32,
}

/// Rows on the status card: MW2, Skate 3, Minecraft, controller.
const CARD_ROWS: usize = 4;

impl Layout {
    fn new(rect: Rect) -> Self {
        // Full spacing from 760 px; six menu rows need the room.
        let k = ((rect.height() - 560.0) / 200.0).clamp(0.0, 1.0);
        let top = rect.top() + mix(30.0, 48.0, k);
        Self {
            top,
            menu_top: top + 145.0 + mix(18.0, 48.0, k),
            big_row: mix(44.0, 54.0, k),
            row: mix(31.0, 40.0, k),
            big_gap: mix(8.0, 14.0, k),
            gap: mix(2.0, 6.0, k),
            card_bottom: rect.bottom() - mix(20.0, 38.0, k),
            card_first: mix(42.0, 48.0, k),
            card_row: mix(19.0, 21.0, k),
            card_foot: mix(18.0, 22.0, k),
        }
    }

    fn card_height(&self) -> f32 {
        self.card_first + (CARD_ROWS - 1) as f32 * self.card_row + self.card_foot
    }
}

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let rect = ui.max_rect();
    let layout = Layout::new(rect);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, BG);
    app.backdrop.paint(&ctx, &painter, rect);
    crate::fx::Fx::gradient(&painter, rect);
    title(&painter, &layout, rect);
    menu(app, ui, &layout, rect);
    status(app, &painter, &layout, rect);
    footer(app, &painter, rect);
    app.fx.overlay(
        &painter,
        rect,
        ctx.input(|i| i.time),
        ctx.pixels_per_point(),
    );
    if app.panel == Panel::None {
        main_notice(app, ui, &layout, rect);
    }
    side_panel(app, ui, rect);
}

fn title(painter: &egui::Painter, layout: &Layout, rect: Rect) {
    let x = rect.left() + MARGIN;
    let y = layout.top;
    painter.text(
        pos2(x + 2.0, y),
        Align2::LEFT_TOP,
        tracked("2010 Rust Rewrite"),
        display(17.0),
        MUTED,
    );
    let galley = painter.layout_no_wrap("MASHUP".into(), display(78.0), TEXT);
    let width = galley.size().x;
    painter.galley(pos2(x - 3.0, y + 18.0), galley, TEXT);
    let line_y = y + 112.0;
    painter.rect_filled(
        Rect::from_min_size(pos2(x, line_y), vec2(64.0, 4.0)),
        0.0,
        OLIVE,
    );
    painter.rect_filled(
        Rect::from_min_size(pos2(x + 70.0, line_y + 1.0), vec2(width - 73.0, 2.0)),
        0.0,
        OLIVE_DIM,
    );
    let mut tx = x;
    for (label, color) in [("MW2", OLIVE), ("SKATE 3", WARN), ("MINECRAFT", GRASS)] {
        let g = painter.layout_no_wrap(tracked(label), display(13.0), TEXT);
        let top = line_y + 14.0;
        let mid = top + g.size().y / 2.0;
        painter.rect_filled(
            Rect::from_center_size(pos2(tx + 4.0, mid), vec2(8.0, 8.0)),
            0.0,
            color,
        );
        let right = tx + 15.0 + g.size().x;
        painter.galley(pos2(tx + 15.0, top), g, TEXT);
        tx = right + 22.0;
    }
}

fn item_label(app: &App, item: Item) -> (&'static str, Option<(&'static str, Color32)>) {
    match item {
        Item::Play if app.game.is_some() => ("Play", Some(("IN GAME", OLIVE))),
        Item::Play if app.mw2.is_none() => ("Play", Some(("MW2 NOT FOUND", DANGER))),
        Item::Play => ("Play", None),
        Item::QuickPlay => ("Quick play", None),
        Item::Minecraft => ("Minecraft world", None),
        Item::Skate if app.skate_ready => ("Skate 3", Some(("READY", OLIVE))),
        Item::Skate if app.skate_job.is_some() => ("Skate 3", Some(("CONVERTING", WARN))),
        Item::Skate => ("Set up Skate 3", Some(("OPTIONAL", WARN))),
        Item::Options => ("Options", None),
        Item::Quit => ("Quit", None),
    }
}

fn menu(app: &mut App, ui: &mut egui::Ui, layout: &Layout, rect: Rect) {
    let ctx = ui.ctx().clone();
    let x = rect.left() + MARGIN;
    let mut y = layout.menu_top;
    let painter = ui.painter_at(rect);
    // Only a moving pointer takes the selection, so a cursor resting over
    // the menu doesn't fight the keyboard or gamepad.
    let pointer_moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    for (index, item) in ITEMS.iter().copied().enumerate() {
        let big = item == Item::Play;
        let height = if big { layout.big_row } else { layout.row };
        let row = Rect::from_min_size(pos2(x - INSET, y), vec2(MENU_WIDTH, height));
        let response = ui.interact(row, ui.id().with(("menu", index)), Sense::click());
        if response.hovered() && pointer_moved && app.panel == Panel::None {
            app.selected = index;
        }
        let blocked =
            matches!(item, Item::Play | Item::QuickPlay | Item::Minecraft) && app.mw2.is_none();
        let on = app.selected == index && app.panel == Panel::None;
        let t = ease(ctx.animate_bool_with_time(ui.id().with(("bar", index)), on, 0.12));

        let (label, tag) = item_label(app, item);
        let size = if big { 34.0 } else { 22.0 };
        let base = if blocked {
            MUTED.gamma_multiply(0.7)
        } else {
            TEXT
        };
        // Dark text on the bright bar; a blocked item gets a dim bar, so it
        // keeps light (but greyed) text there.
        let on_bar = if blocked { BLOCKED_TEXT } else { BG };
        let text_color = lerp(base, on_bar, t);
        let galley = painter.layout_no_wrap(tracked(label), display(size), text_color);
        let tag_galley = tag.map(|(tag, color)| {
            let color = if blocked { color } else { lerp(color, BG, t) };
            (
                painter.layout_no_wrap(tracked(tag), display(11.0), color),
                color,
            )
        });
        let text_x = x + 20.0 * t;
        let mut content_right = text_x + galley.size().x;
        if let Some((g, _)) = &tag_galley {
            content_right += 14.0 + g.size().x + 12.0;
        }

        if t > 0.0 {
            let bar = if blocked { BLOCKED_BAR } else { OLIVE };
            selection_bar(&painter, row, content_right, t, bar);
            marker(
                &painter,
                pos2(x - 2.0, row.center().y),
                if big { 7.0 } else { 5.5 },
                on_bar.gamma_multiply(t),
            );
        }
        let text_top = row.center().y - galley.size().y / 2.0;
        let text_right = text_x + galley.size().x;
        painter.galley(pos2(text_x, text_top), galley, text_color);
        if let Some((g, color)) = tag_galley {
            let pill = Rect::from_min_size(
                pos2(text_right + 14.0, row.center().y - 9.0),
                vec2(g.size().x + 12.0, 18.0),
            );
            painter.rect_stroke(
                pill,
                CornerRadius::ZERO,
                Stroke::new(1.0, color),
                StrokeKind::Inside,
            );
            painter.galley(
                pos2(pill.left() + 6.0, pill.center().y - g.size().y / 2.0),
                g,
                color,
            );
        }
        if response.clicked() && app.panel == Panel::None {
            app.activate(&ctx, item);
        }
        y += height + if big { layout.big_gap } else { layout.gap };
    }
}

/// MW2's highlight: solid olive behind the label that eases in from the
/// left, then fades out towards the right.
fn selection_bar(painter: &egui::Painter, row: Rect, content_right: f32, t: f32, color: Color32) {
    let solid_end = (content_right + 28.0).max(row.left() + row.width() * 0.55);
    let fade_end = (solid_end + 150.0).max(row.right());
    let reveal = row.left() + (fade_end - row.left()) * t;
    let solid = color.gamma_multiply(0.94);
    let clear = Color32::TRANSPARENT;
    let mut mesh = Mesh::default();
    let solid_right = solid_end.min(reveal);
    horizontal_quad(
        &mut mesh,
        row.left_top(),
        pos2(solid_right, row.bottom()),
        solid,
        solid,
    );
    if reveal > solid_end {
        let p = (reveal - solid_end) / (fade_end - solid_end);
        horizontal_quad(
            &mut mesh,
            pos2(solid_end, row.top()),
            pos2(reveal, row.bottom()),
            solid,
            lerp_alpha(solid, clear, p),
        );
    }
    painter.add(mesh);
    painter.rect_filled(
        Rect::from_min_size(row.min, vec2(3.0, row.height())),
        0.0,
        TEXT.gamma_multiply(t),
    );
}

/// A right-pointing filled triangle centred on `center`.
fn marker(painter: &egui::Painter, center: Pos2, half: f32, color: Color32) {
    painter.add(Shape::convex_polygon(
        vec![
            pos2(center.x - half * 0.75, center.y - half),
            pos2(center.x + half * 0.85, center.y),
            pos2(center.x - half * 0.75, center.y + half),
        ],
        color,
        Stroke::NONE,
    ));
}

fn status(app: &App, painter: &egui::Painter, layout: &Layout, rect: Rect) {
    let width = CARD_WIDTH.min(rect.width() - 2.0 * (MARGIN - INSET));
    let height = layout.card_height();
    let card = Rect::from_min_size(
        pos2(rect.left() + MARGIN - INSET, layout.card_bottom - height),
        vec2(width, height),
    );
    painter.rect_filled(card, CornerRadius::ZERO, PANEL);
    painter.rect_stroke(
        card,
        CornerRadius::ZERO,
        Stroke::new(1.0, OLIVE_DIM.gamma_multiply(0.7)),
        StrokeKind::Inside,
    );
    painter.rect_filled(
        Rect::from_min_size(card.min, vec2(card.width(), 2.0)),
        0.0,
        OLIVE,
    );
    painter.text(
        card.min + vec2(16.0, 13.0),
        Align2::LEFT_TOP,
        tracked("System status"),
        display(11.0),
        OLIVE,
    );
    let mw2 = match &app.mw2 {
        Some(path) => (OLIVE, short_path(path, 40)),
        None => (DANGER, MW2_MISSING_SHORT.to_owned()),
    };
    let skate = if app.skate_ready && app.controller == Controller::None {
        (WARN, "Ready. Connect a controller to skate.".to_owned())
    } else if app.skate_ready {
        (OLIVE, "Ready. Press J in a match.".to_owned())
    } else if app.skate_job.is_some() {
        (WARN, "Converting…".to_owned())
    } else {
        (WARN, "Not set up (optional)".to_owned())
    };
    let minecraft = (GRASS, "Downloads from Mojang on first visit".to_owned());
    let controller = controller_status(&app.controller);
    let rows: [(&str, (Color32, String)); CARD_ROWS] = [
        ("MW2", mw2),
        ("SKATE 3", skate),
        ("MINECRAFT", minecraft),
        ("CONTROLLER", controller),
    ];
    for (row, (label, (color, value))) in rows.into_iter().enumerate() {
        let mid = card.top() + layout.card_first + row as f32 * layout.card_row;
        painter.circle_filled(pos2(card.left() + 20.0, mid), 3.5, color);
        painter.text(
            pos2(card.left() + 34.0, mid),
            Align2::LEFT_CENTER,
            tracked(label),
            display(11.0),
            MUTED,
        );
        // A long path keeps its end, anything else its start.
        let room = card.right() - 16.0 - (card.left() + VALUE_X);
        let galley = fit(painter, value, room, label == "MW2" && app.mw2.is_some());
        painter.galley(
            pos2(card.left() + VALUE_X, mid - galley.size().y / 2.0),
            galley,
            TEXT,
        );
    }
}

/// The status card's MW2 row when MW2 is missing; the row's label names it.
const MW2_MISSING_SHORT: &str = "Not found. Open Options to install or locate it.";

/// Where the status card's values start.
const VALUE_X: f32 = 136.0;

/// `text` laid out for the status card, shortened with `…` until it is at
/// most `width` wide, keeping its end when `keep_end`, else its start.
fn fit(
    painter: &egui::Painter,
    text: String,
    width: f32,
    keep_end: bool,
) -> std::sync::Arc<egui::Galley> {
    let mut max = text.chars().count();
    loop {
        let shown = match (max, keep_end) {
            (0, _) => "…".to_owned(),
            (_, true) => short(&text, max),
            (_, false) => short_end(&text, max),
        };
        let galley = painter.layout_no_wrap(shown, display(13.0), TEXT);
        if galley.size().x <= width || max == 0 {
            return galley;
        }
        max -= 1;
    }
}

/// The status card's controller row: the first pad's name (and how many
/// more), or why there is none.
fn controller_status(controller: &Controller) -> (Color32, String) {
    match controller {
        Controller::Connected { name, count: 1 } => (OLIVE, short_end(name, 40)),
        Controller::Connected { name, count } => {
            (OLIVE, format!("{} +{}", short_end(name, 36), count - 1))
        }
        Controller::None => (WARN, "None connected (needed for skating)".to_owned()),
        Controller::Unavailable => (MUTED, "Unavailable".to_owned()),
    }
}

fn footer(app: &App, painter: &egui::Painter, rect: Rect) {
    let pos = rect.right_bottom() - vec2(MARGIN - INSET, 24.0);
    let text = format!(
        "v\u{2009}{}",
        tracked(&format!(
            "{} · Linux · Unofficial fan project",
            env!("CARGO_PKG_VERSION")
        ))
    );
    painter.text(pos, Align2::RIGHT_BOTTOM, text, display(10.0), MUTED);
    if app.game.is_some() {
        painter.text(
            pos - vec2(0.0, 18.0),
            Align2::RIGHT_BOTTOM,
            tracked("In game"),
            display(12.0),
            OLIVE,
        );
    } else if app.mw2.is_none() {
        painter.text(
            pos - vec2(0.0, 18.0),
            Align2::RIGHT_BOTTOM,
            crate::app::MW2_MISSING,
            display(12.0),
            DANGER,
        );
    }
}

/// The notice on the main screen: top right, clear of the title.
fn main_notice(app: &mut App, ui: &mut egui::Ui, layout: &Layout, rect: Rect) {
    let width = 400.0_f32.min(rect.width() - 520.0);
    let min = pos2(rect.right() - MARGIN + INSET - width, layout.top - 6.0);
    let painter = ui.painter_at(rect);
    notice_box(app, ui, &painter, min, width);
}

/// The notice inside a side panel, laid out in its flow.
fn panel_notice(app: &mut App, ui: &mut egui::Ui) {
    let width = ui.available_width();
    let min = ui.cursor().min;
    let painter = ui.painter().clone();
    if let Some(height) = notice_box(app, ui, &painter, min, width) {
        ui.allocate_exact_size(vec2(width, height), Sense::hover());
    }
}

/// Draws the current notice, if any, and returns its height.
fn notice_box(
    app: &mut App,
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    min: Pos2,
    width: f32,
) -> Option<f32> {
    let (text, error) = app.notice.clone()?;
    let color = if error { DANGER } else { OLIVE };
    let galley = painter.layout(text, display(13.0), TEXT, width - 58.0);
    let height = (galley.size().y + 24.0).max(44.0);
    let area = Rect::from_min_size(min, vec2(width, height));
    painter.rect_filled(area, CornerRadius::ZERO, PANEL);
    painter.rect_stroke(
        area,
        CornerRadius::ZERO,
        Stroke::new(1.0, color.gamma_multiply(0.45)),
        StrokeKind::Inside,
    );
    painter.rect_filled(
        Rect::from_min_size(area.min, vec2(3.0, area.height())),
        0.0,
        color,
    );
    painter.galley(
        pos2(area.left() + 18.0, area.center().y - galley.size().y / 2.0),
        galley,
        TEXT,
    );
    let close =
        Rect::from_center_size(pos2(area.right() - 20.0, area.center().y), vec2(22.0, 22.0));
    let response = ui.interact(close, ui.id().with("notice-close"), Sense::click());
    let cross = if response.hovered() { TEXT } else { MUTED };
    let c = close.center();
    for (a, b) in [
        (vec2(-5.0, -5.0), vec2(5.0, 5.0)),
        (vec2(-5.0, 5.0), vec2(5.0, -5.0)),
    ] {
        painter.line_segment([c + a, c + b], Stroke::new(1.5, cross));
    }
    if response.clicked() {
        app.notice = None;
    }
    Some(height)
}

fn side_panel(app: &mut App, ui: &mut egui::Ui, rect: Rect) {
    let open = app.panel != Panel::None;
    let t = ease(
        ui.ctx()
            .animate_bool_with_time(ui.id().with("side"), open, 0.18),
    );
    if t <= 0.0 {
        return;
    }
    let quick = app.shown_panel == Panel::QuickPlay;
    let width = if quick {
        (rect.width() * QUICK_SHARE)
            .max(QUICK_MIN)
            .min(rect.width())
    } else {
        SIDE.min(rect.width() * 0.5)
    };
    let shade = ui.painter_at(rect);
    shade.rect_filled(rect, 0.0, Color32::from_black_alpha((120.0 * t) as u8));
    let panel = Rect::from_min_size(
        pos2(rect.right() - width * t, rect.top()),
        vec2(width, rect.height()),
    );
    // Clicking the dimmed area beside the panel closes it.
    let outside = Rect::from_min_max(rect.min, pos2(panel.left(), rect.bottom()));
    if crate::app::closable(app.panel, app.skate_job.is_some())
        && ui
            .interact(outside, ui.id().with("side-outside"), Sense::click())
            .clicked()
    {
        app.close_panel();
    }
    shade.rect_filled(panel, 0.0, BG.gamma_multiply(0.97));
    shade.rect_filled(
        Rect::from_min_size(panel.min, vec2(2.0, panel.height())),
        0.0,
        OLIVE,
    );
    let inner = panel.shrink2(vec2(30.0, 34.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    if !open {
        child.disable();
    }
    child.spacing_mut().item_spacing = vec2(10.0, 10.0);
    if quick {
        // Its own layout: fixed controls over a scrolling map grid.
        quick_play(app, &mut child);
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut child, |ui| match app.shown_panel {
            Panel::Options => options(app, ui),
            Panel::QuickPlay => {}
            Panel::Skate => skate(app, ui),
            Panel::Crash => crash(app, ui),
            Panel::ConfirmReset => confirm_reset(app, ui),
            Panel::None => {}
        });
}

fn heading(app: &mut App, ui: &mut egui::Ui, text: &str, accent: Color32) {
    ui.label(
        egui::RichText::new(tracked(text))
            .font(display(26.0))
            .color(TEXT),
    );
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), Sense::hover());
    ui.painter()
        .rect_filled(Rect::from_min_size(r.min, vec2(48.0, 3.0)), 0.0, accent);
    ui.painter().rect_filled(
        Rect::from_min_max(
            pos2(r.left() + 54.0, r.top() + 1.0),
            pos2(r.right(), r.top() + 2.0),
        ),
        0.0,
        OLIVE_DIM.gamma_multiply(0.6),
    );
    ui.add_space(10.0);
    panel_notice(app, ui);
}

fn caption(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(tracked(text))
            .font(display(11.0))
            .color(OLIVE),
    );
}

fn body(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).font(display(14.0)).color(MUTED));
}

fn value(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).font(display(14.0)).color(TEXT));
}

/// Frame-temp key for the panel button or toggle under the pointer, read
/// by `App` to play the hover sound once per change.
pub fn hovered_key() -> egui::Id {
    egui::Id::new("ui-sound-hovered")
}

/// Frame-temp key set when a panel button or toggle was clicked.
pub fn clicked_key() -> egui::Id {
    egui::Id::new("ui-sound-clicked")
}

/// Notes a button's hover and click for the interface sounds.
fn sound_cues(ui: &egui::Ui, response: &egui::Response) {
    if !ui.is_enabled() {
        return;
    }
    if response.hovered() {
        ui.ctx()
            .data_mut(|d| d.insert_temp(hovered_key(), Some(response.id)));
    }
    if response.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(clicked_key(), true));
    }
}

/// A flat MW2-style button: outlined, fills with its accent on hover.
fn button(ui: &mut egui::Ui, text: &str, accent: Color32) -> bool {
    styled_button(ui, text, accent, false)
}

/// The panel's main action: filled with its accent, brightening on hover.
fn primary_button(ui: &mut egui::Ui, text: &str, accent: Color32) -> bool {
    styled_button(ui, text, accent, true)
}

fn styled_button(ui: &mut egui::Ui, text: &str, accent: Color32, primary: bool) -> bool {
    let font = display(13.0);
    let galley = ui
        .painter()
        .layout_no_wrap(tracked(text), font.clone(), TEXT);
    let size = vec2(galley.size().x + 32.0, 34.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    sound_cues(ui, &response);
    let enabled = ui.is_enabled();
    let t = ui
        .ctx()
        .animate_bool_with_time(response.id, enabled && response.hovered(), 0.1);
    let painter = ui.painter();
    let fill = if primary {
        lerp(accent, TEXT, 0.25 * t)
    } else {
        accent.gamma_multiply(0.14 + 0.8 * t)
    };
    painter.rect_filled(rect, CornerRadius::ZERO, fill);
    painter.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, accent),
        StrokeKind::Inside,
    );
    let color = if primary { BG } else { lerp(TEXT, BG, t) };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        tracked(text),
        font,
        color,
    );
    enabled && response.clicked()
}

/// A square MW2-style check box with a label.
fn toggle(ui: &mut egui::Ui, text: &str, on: &mut bool) -> bool {
    accent_toggle(ui, text, on, (OLIVE, OLIVE_DIM), (14.0, 22.0))
}

/// `toggle` in another accent (and its dim, at rest), with a `font` size
/// and `height`.
fn accent_toggle(
    ui: &mut egui::Ui,
    text: &str,
    on: &mut bool,
    (accent, dim): (Color32, Color32),
    (font, height): (f32, f32),
) -> bool {
    let font = display(font);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, TEXT);
    let size = vec2(18.0 + 12.0 + galley.size().x, height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    sound_cues(ui, &response);
    let painter = ui.painter();
    let check = Rect::from_min_size(pos2(rect.left(), rect.center().y - 9.0), vec2(18.0, 18.0));
    let stroke = if response.hovered() { accent } else { dim };
    painter.rect_filled(check, CornerRadius::ZERO, Color32::from_black_alpha(120));
    painter.rect_stroke(
        check,
        CornerRadius::ZERO,
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    if *on {
        painter.rect_filled(check.shrink(4.0), CornerRadius::ZERO, accent);
    }
    painter.galley(
        pos2(
            check.right() + 12.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        TEXT,
    );
    if response.clicked() {
        *on = !*on;
        return true;
    }
    false
}

fn back(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(14.0);
    if button(ui, "Back", OLIVE_DIM) {
        app.close_panel();
    }
}

fn options(app: &mut App, ui: &mut egui::Ui) {
    heading(app, ui, "Options", OLIVE);
    caption(ui, "MW2 folder");
    match &app.mw2 {
        Some(path) => value(ui, &path.display().to_string()),
        None => body(
            ui,
            "Not found. Pick the folder that contains zone/ and main/.",
        ),
    }
    let pick = if app.mw2.is_some() {
        "Change MW2 folder"
    } else {
        "Locate MW2"
    };
    if app.picker_open() {
        body(ui, "Waiting for the file dialog…");
    }
    ui.horizontal(|ui| {
        if button(ui, pick, OLIVE) {
            app.pick_mw2(ui.ctx());
        }
        if app.mw2.is_none() && button(ui, "Install MW2 on Steam", OLIVE_DIM) {
            app.install_mw2_via_steam();
        }
    });
    ui.horizontal(|ui| {
        let field = egui::TextEdit::singleline(&mut app.path_input)
            .hint_text(
                egui::RichText::new("or type the path here").color(MUTED.gamma_multiply(0.8)),
            )
            .font(display(13.0))
            .text_color(TEXT)
            .frame(
                egui::Frame::NONE
                    .fill(Color32::from_black_alpha(140))
                    .stroke(Stroke::new(1.0, OLIVE_DIM))
                    .inner_margin(egui::Margin::symmetric(8, 8)),
            )
            .desired_width(ui.available_width() - 90.0);
        ui.add(field);
        if button(ui, "Use", OLIVE_DIM) {
            let path = std::path::PathBuf::from(app.path_input.trim());
            app.use_mw2(path);
        }
    });
    caption(ui, "Desktop");
    let mut on = app.config.add_to_menu;
    if toggle(ui, "Add to app menu", &mut on) {
        app.set_menu_entry(on);
    }
    caption(ui, "Audio · M mutes all");
    let mut on = app.config.music;
    if toggle(ui, "Menu music", &mut on) {
        app.set_music(on);
    }
    let mut on = app.config.ui_sounds;
    if toggle(ui, "Interface sounds", &mut on) {
        app.set_ui_sounds(on);
    }
    caption(ui, "Data");
    body(ui, &app.paths.data.display().to_string());
    ui.horizontal(|ui| {
        if button(ui, "Open data folder", OLIVE_DIM) {
            let _ = std::fs::create_dir_all(&app.paths.data);
            let _ = crate::app::open_external(&app.paths.data);
        }
        // The game reads from the folders Reset deletes.
        let idle = app.game.is_none();
        if ui
            .add_enabled_ui(idle, |ui| button(ui, "Reset", DANGER))
            .inner
        {
            app.open(Panel::ConfirmReset);
        }
    });
    back(app, ui);
}

fn skate(app: &mut App, ui: &mut egui::Ui) {
    heading(app, ui, "Skate 3", WARN);
    if let Some(job) = &app.skate_job {
        let stages = crate::skate::STAGES;
        let stage = job.stage.min(stages.len() - 1);
        caption(
            ui,
            &format!("Step {} of {} · {}", stage + 1, stages.len(), stages[stage]),
        );
        let progress = (stage as f32 + 0.5) / stages.len() as f32;
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
        ui.painter()
            .rect_filled(r, 0.0, OLIVE_DIM.gamma_multiply(0.4));
        ui.painter().rect_filled(
            Rect::from_min_size(r.min, vec2(r.width() * progress, r.height())),
            0.0,
            WARN,
        );
        let log = job
            .log
            .iter()
            .rev()
            .take(14)
            .rev()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        egui::Frame::NONE
            .fill(Color32::from_black_alpha(170))
            .stroke(Stroke::new(1.0, OLIVE_DIM.gamma_multiply(0.5)))
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.set_min_size(vec2(ui.available_width(), 220.0));
                ui.label(egui::RichText::new(log).font(mono(12.0)).color(MUTED));
            });
        body(ui, "This takes a minute or two. Keep the launcher open.");
        return;
    }
    caption(ui, "Status");
    if app.skate_ready {
        value(ui, "Set up and ready.");
        body(
            ui,
            "In any match, press J (or click both sticks) to drop onto a board. Skating needs a controller.",
        );
        if app.controller == Controller::None {
            ui.label(
                egui::RichText::new("No controller connected. Plug one in to skate.")
                    .font(display(13.0))
                    .color(WARN),
            );
        }
    } else {
        value(ui, "Not set up. It's optional.");
        body(
            ui,
            "Skating needs your own Xbox 360 copy of Skate 3. Pick the .iso, or default.xex from an extracted copy with its data folder next to it.",
        );
    }
    if let Some(error) = &app.skate_error {
        ui.label(egui::RichText::new(error).font(display(13.0)).color(DANGER));
    }
    if app.picker_open() {
        body(ui, "Waiting for the file dialog…");
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if app.skate_ready && primary_button(ui, "Start skate session", WARN) {
            let ctx = ui.ctx().clone();
            app.start_skate_session(&ctx);
        }
        let (label, accent) = if app.skate_ready {
            ("Convert again", OLIVE_DIM)
        } else if app.skate_error.is_some() {
            ("Retry", WARN)
        } else {
            ("Choose ISO or default.xex", WARN)
        };
        if button(ui, label, accent) {
            app.pick_skate(ui.ctx());
        }
    });
    if !app.skate_ready {
        caption(ui, "What the converter does");
        ui.scope(steps);
    }
    back(app, ui);
}

/// The converter's phases as a quiet numbered list, so the player knows
/// what the progress bar will walk through.
fn steps(ui: &mut egui::Ui) {
    let stages = crate::skate::STAGES;
    ui.spacing_mut().item_spacing.y = 4.0;
    for (n, stage) in stages[..stages.len() - 1].iter().enumerate() {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
        let painter = ui.painter();
        painter.text(
            pos2(r.left(), r.center().y),
            Align2::LEFT_CENTER,
            format!("0{}", n + 1),
            display(12.0),
            WARN.gamma_multiply(0.8),
        );
        painter.rect_filled(
            Rect::from_center_size(pos2(r.left() + 30.0, r.center().y), vec2(8.0, 1.0)),
            0.0,
            OLIVE_DIM,
        );
        painter.text(
            pos2(r.left() + 44.0, r.center().y),
            Align2::LEFT_CENTER,
            *stage,
            display(13.0),
            MUTED,
        );
    }
}

fn crash(app: &mut App, ui: &mut egui::Ui) {
    heading(app, ui, "Game closed", DANGER);
    body(ui, "The game stopped with an error. The end of its log:");
    egui::Frame::NONE
        .fill(Color32::from_black_alpha(170))
        .stroke(Stroke::new(1.0, DANGER.gamma_multiply(0.4)))
        .inner_margin(10.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let text = if app.crash.is_empty() {
                        "(the game left no log)"
                    } else {
                        app.crash.as_str()
                    };
                    ui.label(egui::RichText::new(text).font(mono(11.0)).color(MUTED));
                });
        });
    ui.horizontal(|ui| {
        if button(ui, "Copy log", OLIVE) {
            ui.ctx().copy_text(app.crash.clone());
        }
        if button(ui, "Open log folder", OLIVE_DIM) {
            let _ = crate::app::open_external(app.paths.logs());
        }
    });
    back(app, ui);
}

fn confirm_reset(app: &mut App, ui: &mut egui::Ui) {
    heading(app, ui, "Reset", DANGER);
    let idle = app.game.is_none() && app.skate_job.is_none();
    if !idle {
        body(
            ui,
            "Quit the game, or wait for Skate 3 setup to finish, first.",
        );
    }
    body(
        ui,
        "This forgets your MW2 folder and deletes the converted Skate 3 data. The Minecraft download is kept.",
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled_ui(idle, |ui| button(ui, "Reset everything", DANGER))
            .inner
        {
            app.reset();
        }
        if button(ui, "Cancel", OLIVE_DIM) {
            app.open(Panel::Options);
        }
    });
    ui.add_space(18.0);
    caption(ui, "Or remove all launcher data");
    body(
        ui,
        "Deletes everything the launcher wrote: your MW2 folder choice, the converted Skate 3 data, the backdrop cache, the Minecraft download and the app menu entry. Minecraft (about 125 MB) downloads again next time.",
    );
    ui.add_space(4.0);
    if ui
        .add_enabled_ui(idle, |ui| primary_button(ui, "Remove everything", DANGER))
        .inner
    {
        app.remove_all_data();
    }
}

/// Width of the label column beside Quick Play's controls.
const FORM_LABEL: f32 = 76.0;
/// Height of a chip, and of a form row.
const CHIP_HEIGHT: f32 = 30.0;
/// Space between map tiles.
const TILE_GAP: f32 = 12.0;
/// Tile width the grid's column count aims for.
const TILE_TARGET: f32 = 200.0;
/// Height kept under the map grid for Back and the key hints.
const QUICK_FOOTER: f32 = 58.0;
/// Room on the grid's right for the floating scroll bar.
const SCROLL_BAR_ROOM: f32 = 8.0;

fn quick_play(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    app.thumbs.upload(&ctx);
    heading(app, ui, "Quick play", OLIVE);
    let mut launch: Option<String> = None;

    form_row(ui, "Mode", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for (code, label) in crate::maps::MODES {
                let selected = app.config.quick_mode == code;
                if chip(ui, label, selected, OLIVE) && !selected {
                    app.config.quick_mode = code.to_owned();
                    app.save();
                }
            }
        });
    });
    form_row(ui, "Bots", |ui| bots_slider(app, ui));
    if app.skate_ready {
        form_row(ui, "Skate", |ui| skate_toggle(app, ui));
    }
    // Only maps this MW2 still has.
    let recent: Vec<&crate::maps::MapEntry> = app
        .config
        .recent_maps
        .iter()
        .filter_map(|zone| app.maps.iter().find(|map| &map.zone == zone))
        .collect();
    if !recent.is_empty() {
        form_row(ui, "Recent", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for map in recent {
                    if recent_chip(ui, &map.name) {
                        launch = Some(map.zone.clone());
                    }
                }
            });
        });
    }

    ui.add_space(2.0);
    maps_header(app, ui);
    let grid_height = (ui.available_height() - QUICK_FOOTER).max(TILE_TARGET * 0.5);
    egui::ScrollArea::vertical()
        .id_salt("quickplay-maps")
        .max_height(grid_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(zone) = map_grid(app, ui) {
                launch = Some(zone);
            }
        });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if button(ui, "Back", OLIVE_DIM) {
            app.close_panel();
        }
        key_hints(
            ui,
            &[("Esc", "Back"), ("Enter", "Launch"), ("Arrows", "Select")],
        );
    });

    if let Some(zone) = launch {
        app.quick_launch(&ctx, &zone);
    }
}

/// A label in the left column, the control beside it.
fn form_row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(FORM_LABEL, CHIP_HEIGHT), Sense::hover());
        ui.painter().text(
            pos2(r.left(), r.center().y),
            Align2::LEFT_CENTER,
            tracked(label),
            display(11.0),
            OLIVE,
        );
        ui.vertical(add);
    });
}

/// A segmented-control chip: olive-filled when selected, an olive outline
/// on hover. Returns whether it was clicked.
fn chip(ui: &mut egui::Ui, text: &str, selected: bool, accent: Color32) -> bool {
    let font = display(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap(tracked(text), font.clone(), TEXT);
    let size = vec2(galley.size().x + 22.0, CHIP_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    sound_cues(ui, &response);
    let t = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.1);
    let painter = ui.painter();
    let text_color = if selected {
        painter.rect_filled(rect, CornerRadius::ZERO, accent);
        BG
    } else {
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_black_alpha(120));
        painter.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, lerp(OLIVE_DIM, accent, t)),
            StrokeKind::Inside,
        );
        lerp(MUTED, TEXT, t)
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        tracked(text),
        font,
        text_color,
    );
    response.clicked()
}

/// A recent map: a chip with a small play marker, launching on click.
fn recent_chip(ui: &mut egui::Ui, name: &str) -> bool {
    let font = display(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap(tracked(name), font.clone(), TEXT);
    let size = vec2(galley.size().x + 40.0, CHIP_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    sound_cues(ui, &response);
    let t = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.1);
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::ZERO,
        OLIVE.gamma_multiply(0.1 + 0.84 * t),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, lerp(OLIVE_DIM, OLIVE, t)),
        StrokeKind::Inside,
    );
    let color = lerp(TEXT, BG, t);
    marker(
        painter,
        pos2(rect.left() + 14.0, rect.center().y),
        4.5,
        lerp(OLIVE, BG, t),
    );
    painter.galley(
        pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    response.clicked()
}

/// Bots, 0 to 20: an olive track with a square thumb and the count in
/// large figures. Drag or click; Left/Right when focused with Tab.
fn bots_slider(app: &mut App, ui: &mut egui::Ui) {
    let max = crate::maps::MAX_BOTS;
    let width = 260.0_f32.min(ui.available_width() - 120.0).max(120.0);
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, CHIP_HEIGHT), Sense::click_and_drag());
    sound_cues(ui, &response);
    if response.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    ..Default::default()
                },
            );
        });
    }
    let track = Rect::from_min_max(
        pos2(rect.left() + 7.0, rect.center().y - 2.0),
        pos2(rect.right() - 7.0, rect.center().y + 2.0),
    );
    let old = app.config.quick_bots.min(max);
    let mut bots = old;
    if (response.dragged() || response.is_pointer_button_down_on())
        && let Some(p) = response.interact_pointer_pos()
    {
        let f = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
        bots = (f * f32::from(max)).round() as u8;
    }
    if response.has_focus() {
        let (l, r) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
            )
        });
        if l {
            bots = bots.saturating_sub(1);
        }
        if r {
            bots = (bots + 1).min(max);
        }
    }
    if bots != old {
        app.config.quick_bots = bots;
        // A tick per step, like MW2's sliders.
        app.audio.hover();
    }
    if (bots != old && !response.dragged()) || response.drag_stopped() {
        app.save();
    }

    let active = response.hovered() || response.dragged() || response.has_focus();
    let t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("hot"), active, 0.1);
    let painter = ui.painter();
    let f = f32::from(bots) / f32::from(max);
    let x = track.left() + track.width() * f;
    painter.rect_filled(track, CornerRadius::ZERO, OLIVE_DIM.gamma_multiply(0.45));
    painter.rect_filled(
        Rect::from_min_max(track.min, pos2(x, track.bottom())),
        CornerRadius::ZERO,
        OLIVE,
    );
    for step in (0..=max).step_by(4) {
        let tx = track.left() + track.width() * f32::from(step) / f32::from(max);
        painter.rect_filled(
            Rect::from_center_size(pos2(tx, track.bottom() + 7.0), vec2(1.0, 4.0)),
            0.0,
            MUTED.gamma_multiply(0.6),
        );
    }
    let thumb = Rect::from_center_size(pos2(x, track.center().y), vec2(14.0, 14.0));
    painter.rect_filled(thumb, CornerRadius::ZERO, lerp(OLIVE, TEXT, t));
    painter.rect_stroke(
        thumb,
        CornerRadius::ZERO,
        Stroke::new(1.0, BG),
        StrokeKind::Inside,
    );
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(2.0),
            CornerRadius::ZERO,
            Stroke::new(1.0, OLIVE_DIM),
            StrokeKind::Outside,
        );
    }
    let number = if bots == 0 { MUTED } else { TEXT };
    let figure = painter.layout_no_wrap(format!("{bots:02}"), display(30.0), number);
    let figure_x = rect.right() + 18.0;
    let figure_w = figure.size().x;
    painter.galley(
        pos2(figure_x, rect.center().y - figure.size().y / 2.0),
        figure,
        number,
    );
    let note = if bots == 0 { "None" } else { "Bots" };
    painter.text(
        pos2(figure_x + figure_w + 8.0, rect.center().y + 6.0),
        Align2::LEFT_BOTTOM,
        tracked(note),
        display(10.0),
        MUTED,
    );
}

/// Skate on spawn: an amber check box (skating's colour) with what it
/// does beside it, or under it when the row is too narrow.
fn skate_toggle(app: &mut App, ui: &mut egui::Ui) {
    const HINT: &str = "Spawns with the default class, then drops you on a board.";
    let label = tracked("Skate on spawn");
    let hint = egui::RichText::new(HINT).font(display(11.0)).color(MUTED);
    let width = |text: String, size: f32| {
        ui.painter()
            .layout_no_wrap(text, display(size), TEXT)
            .size()
            .x
    };
    let needed = 30.0 + width(label.clone(), 13.0) + 12.0 + width(HINT.to_owned(), 11.0);
    let mut on = app.config.quick_skate;
    let mut add = |ui: &mut egui::Ui| {
        let clicked = accent_toggle(
            ui,
            &label,
            &mut on,
            (WARN, WARN.gamma_multiply(0.5)),
            (13.0, CHIP_HEIGHT),
        );
        ui.add(egui::Label::new(hint.clone()).selectable(false));
        clicked
    };
    let clicked = if ui.available_width() >= needed {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            add(ui)
        })
        .inner
    } else {
        ui.spacing_mut().item_spacing.y = 0.0;
        add(ui)
    };
    if clicked {
        app.set_quick_skate(on);
    }
}

/// "MAPS" with the count, over the grid.
fn maps_header(app: &App, ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    let painter = ui.painter();
    let label = painter.layout_no_wrap(tracked("Maps"), display(11.0), OLIVE);
    let label_w = label.size().x;
    painter.galley(
        pos2(r.left(), r.center().y - label.size().y / 2.0),
        label,
        OLIVE,
    );
    painter.text(
        pos2(r.left() + label_w + 10.0, r.center().y),
        Align2::LEFT_CENTER,
        format!("{}", app.maps.len()),
        display(11.0),
        MUTED,
    );
    let mode = crate::maps::mode_label(&app.config.quick_mode).unwrap_or("Custom mode");
    let bots = match app.config.quick_bots.min(crate::maps::MAX_BOTS) {
        0 => "no bots".to_owned(),
        1 => "1 bot".to_owned(),
        n => format!("{n} bots"),
    };
    let skate = if app.quick_skate() { " · skate" } else { "" };
    painter.text(
        pos2(r.right(), r.center().y),
        Align2::RIGHT_CENTER,
        tracked(&format!("{mode} · {bots}{skate}")),
        display(10.0),
        MUTED,
    );
    painter.rect_filled(
        Rect::from_min_max(
            pos2(r.left() + label_w + 30.0, r.center().y),
            pos2(r.left() + label_w + 31.0 + 40.0, r.center().y + 1.0),
        ),
        0.0,
        OLIVE_DIM.gamma_multiply(0.6),
    );
}

/// Columns for a grid `width` wide, aiming at `TILE_TARGET` per tile.
fn grid_columns(width: f32) -> usize {
    (((width + TILE_GAP) / (TILE_TARGET + TILE_GAP)).round() as usize).max(2)
}

/// The map tiles; returns the zone clicked this frame.
fn map_grid(app: &mut App, ui: &mut egui::Ui) -> Option<String> {
    if app.maps.is_empty() {
        body(
            ui,
            "No multiplayer maps found. Quick Play lists zone/*/mp_*.ff from your MW2 folder.",
        );
        return None;
    }
    let width = ui.available_width() - SCROLL_BAR_ROOM;
    let cols = grid_columns(width);
    app.quick_cols = cols;
    let tile_w = ((width - TILE_GAP * (cols - 1) as f32) / cols as f32).floor();
    let tile_h = (tile_w * 9.0 / 16.0).round();
    let rows = app.maps.len().div_ceil(cols);
    let height = rows as f32 * tile_h + (rows - 1) as f32 * TILE_GAP;
    let (area, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let pointer_moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let enabled = ui.is_enabled();
    let bots = app.config.quick_bots;
    let mut clicked = None;
    for (index, map) in app.maps.iter().enumerate() {
        let (row, col) = (index / cols, index % cols);
        let tile = Rect::from_min_size(
            area.min
                + vec2(
                    col as f32 * (tile_w + TILE_GAP),
                    row as f32 * (tile_h + TILE_GAP),
                ),
            vec2(tile_w, tile_h),
        );
        let response = ui.interact(tile, ui.id().with(("tile", index)), Sense::click());
        if enabled && response.hovered() && pointer_moved {
            app.quick_focus = Some(index);
        }
        let focused = app.quick_focus == Some(index);
        if focused && app.quick_scroll {
            ui.scroll_to_rect(tile.expand(4.0), None);
            app.quick_scroll = false;
        }
        let t = ease(ui.ctx().animate_bool_with_time(
            ui.id().with(("tile-hot", index)),
            focused && enabled,
            0.12,
        ));
        if ui.is_rect_visible(tile) {
            paint_tile(ui.painter(), tile, map, app.thumbs.get(&map.zone), t, bots);
        }
        if enabled && response.clicked() {
            clicked = Some(map.zone.clone());
        }
    }
    clicked
}

fn paint_tile(
    painter: &egui::Painter,
    tile: Rect,
    map: &crate::maps::MapEntry,
    texture: Option<&egui::TextureHandle>,
    t: f32,
    bots: u8,
) {
    let accent = if map.is_minecraft() { GRASS } else { OLIVE };
    match texture {
        Some(texture) => {
            // Cover-fit, slightly dimmed until focused.
            let [tw, th] = texture.size().map(|v| v as f32);
            let (view, tex) = (tile.width() / tile.height(), tw / th);
            let (uw, uh) = if tex > view {
                (view / tex, 1.0)
            } else {
                (1.0, tex / view)
            };
            let uv = Rect::from_center_size(pos2(0.5, 0.5), vec2(uw, uh));
            let shade = mix(196.0, 255.0, t) as u8;
            painter.image(texture.id(), tile, uv, Color32::from_gray(shade));
        }
        None => placeholder_tile(painter, tile, t),
    }
    // Dark foot for the name.
    let mut mesh = Mesh::default();
    let foot = pos2(tile.left(), tile.top() + tile.height() * 0.42);
    vertical_quad(
        &mut mesh,
        foot,
        tile.right_bottom(),
        Color32::TRANSPARENT,
        Color32::from_black_alpha(215),
    );
    painter.add(mesh);

    let font = if tile.width() < 170.0 { 12.0 } else { 14.0 };
    let mut name = painter.layout_no_wrap(tracked(&map.name), display(font), TEXT);
    if name.size().x > tile.width() - 20.0 {
        name = painter.layout_no_wrap(tracked(&map.name), display(font - 2.0), TEXT);
    }
    let name_pos = pos2(tile.left() + 10.0, tile.bottom() - 9.0 - name.size().y);
    // An olive tick before the name slides in with focus.
    painter.rect_filled(
        Rect::from_min_size(
            pos2(tile.left() + 10.0, name_pos.y - 6.0),
            vec2(14.0 + 16.0 * t, 2.0),
        ),
        0.0,
        accent.gamma_multiply(0.55 + 0.45 * t),
    );
    painter.galley(name_pos, name, TEXT);

    if map.is_minecraft() {
        tag(
            painter,
            tile.left_top() + vec2(8.0, 8.0),
            Align2::LEFT_TOP,
            "Minecraft",
            GRASS,
            true,
        );
        if bots > 0 {
            tag(
                painter,
                tile.right_top() + vec2(-8.0, 8.0),
                Align2::RIGHT_TOP,
                "Bots untested",
                WARN,
                false,
            );
        }
    }

    let rest = if map.is_minecraft() {
        GRASS.gamma_multiply(0.7)
    } else {
        OLIVE_DIM.gamma_multiply(0.55)
    };
    painter.rect_stroke(
        tile,
        CornerRadius::ZERO,
        Stroke::new(1.0, rest),
        StrokeKind::Inside,
    );
    if t > 0.0 {
        painter.rect_stroke(
            tile,
            CornerRadius::ZERO,
            Stroke::new(2.0, accent.gamma_multiply(t)),
            StrokeKind::Inside,
        );
    }
}

/// A tile with no loadscreen (yet): dark olive with fine diagonal hatching.
fn placeholder_tile(painter: &egui::Painter, tile: Rect, t: f32) {
    painter.rect_filled(
        tile,
        CornerRadius::ZERO,
        lerp(
            Color32::from_rgb(0x16, 0x1b, 0x10),
            Color32::from_rgb(0x1e, 0x25, 0x15),
            t,
        ),
    );
    let clip = painter.with_clip_rect(tile.intersect(painter.clip_rect()));
    let hatch = Stroke::new(1.0, OLIVE_DIM.gamma_multiply(0.22));
    let mut x = tile.left() - tile.height();
    while x < tile.right() {
        clip.line_segment(
            [pos2(x, tile.bottom()), pos2(x + tile.height(), tile.top())],
            hatch,
        );
        x += 12.0;
    }
}

/// A small label box: filled (dark text) or outlined on a dark ground.
fn tag(
    painter: &egui::Painter,
    anchor: Pos2,
    align: Align2,
    text: &str,
    color: Color32,
    filled: bool,
) {
    let text_color = if filled { BG } else { color };
    let galley = painter.layout_no_wrap(tracked(text), display(9.5), text_color);
    let size = galley.size() + vec2(10.0, 6.0);
    let rect = align.anchor_size(anchor, size);
    if filled {
        painter.rect_filled(rect, CornerRadius::ZERO, color);
    } else {
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_black_alpha(185));
        painter.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, color),
            StrokeKind::Inside,
        );
    }
    painter.galley(rect.min + vec2(5.0, 3.0), galley, text_color);
}

/// Keycap hints, right-aligned in the rest of the row.
fn key_hints(ui: &mut egui::Ui, hints: &[(&str, &str)]) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
    let painter = ui.painter();
    let mut x = r.right();
    for (key, label) in hints {
        let text = painter.layout_no_wrap(tracked(label), display(10.0), MUTED);
        x -= text.size().x;
        painter.galley(pos2(x, r.center().y - text.size().y / 2.0), text, MUTED);
        let cap = painter.layout_no_wrap(tracked(key), display(9.5), TEXT);
        let cap_rect = Rect::from_min_size(
            pos2(x - 8.0 - cap.size().x - 10.0, r.center().y - 9.0),
            vec2(cap.size().x + 10.0, 18.0),
        );
        painter.rect_stroke(
            cap_rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, OLIVE_DIM),
            StrokeKind::Inside,
        );
        painter.galley(
            pos2(
                cap_rect.left() + 5.0,
                cap_rect.center().y - cap.size().y / 2.0,
            ),
            cap,
            TEXT,
        );
        x = cap_rect.left() - 18.0;
    }
}

fn vertical_quad(mesh: &mut Mesh, min: Pos2, max: Pos2, top: Color32, bottom: Color32) {
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(min, top);
    mesh.colored_vertex(pos2(max.x, min.y), top);
    mesh.colored_vertex(max, bottom);
    mesh.colored_vertex(pos2(min.x, max.y), bottom);
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
}

fn horizontal_quad(mesh: &mut Mesh, min: Pos2, max: Pos2, left: Color32, right: Color32) {
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(min, left);
    mesh.colored_vertex(pos2(max.x, min.y), right);
    mesh.colored_vertex(max, right);
    mesh.colored_vertex(pos2(min.x, max.y), left);
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
}

fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Opaque blend of two colours.
fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| mix(f32::from(x), f32::from(y), t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Premultiplied blend that keeps alpha, for fading to transparent.
fn lerp_alpha(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| mix(f32::from(x), f32::from(y), t).round() as u8;
    Color32::from_rgba_premultiplied(
        m(a.r(), b.r()),
        m(a.g(), b.g()),
        m(a.b(), b.b()),
        m(a.a(), b.a()),
    )
}

/// A path for the status card: `~` for home, and when still too long the
/// leading folders give way to `…`, keeping whole trailing components.
fn short_path(path: &std::path::Path, max: usize) -> String {
    let home = crate::paths::home();
    let text = match path.strip_prefix(&home) {
        Ok(rest) if home != std::path::Path::new("/") => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    };
    if text.chars().count() <= max {
        return text;
    }
    let mut kept = String::new();
    for part in text.rsplit('/') {
        let candidate = if kept.is_empty() {
            part.to_owned()
        } else {
            format!("{part}/{kept}")
        };
        if candidate.chars().count() + 2 > max {
            break;
        }
        kept = candidate;
    }
    if kept.is_empty() {
        return short(&text, max);
    }
    format!("…/{kept}")
}

/// Keeps the start of `text`, which for a name is the informative part.
fn short_end(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_owned();
    }
    format!(
        "{}…",
        chars[..max - 1].iter().collect::<String>().trim_end()
    )
}

/// Keeps the end of `text`, which for a path is the informative part.
fn short(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_owned();
    }
    format!(
        "…{}",
        chars[chars.len() - (max - 1)..].iter().collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_keeps_the_tail() {
        assert_eq!(short("abc", 5), "abc");
        assert_eq!(short("/a/b/c/d/e", 5), "…/d/e");
    }

    #[test]
    fn short_end_keeps_the_start() {
        assert_eq!(short_end("Xbox Controller", 20), "Xbox Controller");
        assert_eq!(short_end("Microsoft X-Box 360 pad 0", 10), "Microsoft…");
        assert_eq!(short_end("ab cdef", 4), "ab…");
    }

    #[test]
    fn controller_row_per_state() {
        assert_eq!(
            controller_status(&Controller::from_names(["Xbox Wireless Controller"])),
            (OLIVE, "Xbox Wireless Controller".to_owned())
        );
        assert_eq!(
            controller_status(&Controller::from_names(["DualSense", "Pad"])),
            (OLIVE, "DualSense +1".to_owned())
        );
        let (color, text) = controller_status(&Controller::from_names(["x".repeat(60)]));
        assert_eq!((color, text.chars().count()), (OLIVE, 40));
        assert_eq!(controller_status(&Controller::None).0, WARN);
        assert_eq!(
            controller_status(&Controller::Unavailable),
            (MUTED, "Unavailable".to_owned())
        );
    }

    #[test]
    fn status_card_height_at_full_size() {
        let tall = Layout::new(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 900.0)));
        assert_eq!(tall.card_height(), 112.0 + 21.0);
    }

    /// The longest status label and value fit their columns on the card.
    #[test]
    fn status_rows_fit_the_card() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter();
            let label = painter.layout_no_wrap(tracked("CONTROLLER"), display(11.0), MUTED);
            assert!(34.0 + label.size().x + 6.0 < VALUE_X, "{}", label.size().x);
            let room = CARD_WIDTH - 16.0 - VALUE_X;
            for text in [
                MW2_MISSING_SHORT,
                "None connected (needed for skating)",
                "Ready. Connect a controller to skate.",
                "Microsoft X-Box 360 pad",
            ] {
                let value = painter.layout_no_wrap(text.to_owned(), display(13.0), TEXT);
                assert!(value.size().x <= room, "{text}: {}", value.size().x);
                assert_eq!(fit(painter, text.to_owned(), room, false).text(), text);
            }
            let wide = controller_status(&Controller::from_names(["W".repeat(60)])).1;
            let fitted = fit(painter, wide, room, false);
            assert!(fitted.size().x <= room);
            assert!(fitted.text().starts_with('W') && fitted.text().ends_with('…'));
            let path = fit(painter, "/a/".repeat(80), room, true);
            assert!(path.size().x <= room && path.text().starts_with('…'));
        });
        output.textures_delta.clear();
    }

    #[test]
    fn short_path_keeps_whole_trailing_folders() {
        let path = std::path::Path::new("/mnt/lib/steamapps/common/Call of Duty Modern Warfare 2");
        assert_eq!(
            short_path(path, 45),
            "…/common/Call of Duty Modern Warfare 2"
        );
    }

    /// The menu clears the title and the status card at every height from
    /// the minimum up.
    #[test]
    fn layout_fits_at_every_height() {
        let small = (ITEMS.len() - 1) as f32;
        for height in (560..=1200).step_by(10) {
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(900.0, height as f32));
            let l = Layout::new(rect);
            let menu_bottom =
                l.menu_top + l.big_row + l.big_gap + small * l.row + (small - 1.0) * l.gap;
            assert!(l.card_height() >= 112.0, "card too short at {height} px");
            assert!(l.card_row >= 19.0);
            assert!(
                menu_bottom + 8.0 < l.card_bottom - l.card_height(),
                "menu overlaps the status card at {height} px"
            );
            assert!(l.card_bottom <= rect.bottom() - 20.0);
            // The title's tag row ends ~142 px below `top`.
            assert!(
                l.menu_top - l.top >= 160.0,
                "menu meets the title at {height} px"
            );
        }
    }

    #[test]
    fn quick_play_grid_columns() {
        // Default 1100 px window: 748 px panel, 688 px inside.
        assert_eq!(grid_columns(688.0), 3);
        // Minimum 900 px window: 612 px panel, 552 px inside.
        assert_eq!(grid_columns(552.0), 3);
        assert_eq!(grid_columns(1300.0), 6);
        assert_eq!(grid_columns(200.0), 2);
        let panel = |w: f32| (w * QUICK_SHARE).max(QUICK_MIN).min(w);
        assert_eq!(panel(900.0), 612.0);
        assert_eq!(panel(600.0), 560.0);
    }
}
