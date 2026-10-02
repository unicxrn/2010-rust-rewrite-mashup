//! Launcher state and the per-frame loop.
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui;

use crate::backdrop::Backdrop;
use crate::config::Config;
use crate::controller::Controller;
use crate::fx::Fx;
use crate::game::{self, Launch};
use crate::maps::{self, MapEntry};
use crate::menu_audio::{self, Loaded, MenuAudio};
use crate::paths::{self, Paths};
use crate::thumbs::Thumbs;
use crate::{desktop, games_root, skate, steam};

pub const MW2_MISSING: &str = "MW2 not found. Open Options to install or locate it.";

/// Seconds between retries for thumbnails the backdrop loader may still
/// be writing.
const THUMB_RETRY: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Play,
    QuickPlay,
    Minecraft,
    Skate,
    Options,
    Quit,
}

pub const ITEMS: [Item; 6] = [
    Item::Play,
    Item::QuickPlay,
    Item::Minecraft,
    Item::Skate,
    Item::Options,
    Item::Quit,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    None,
    Options,
    QuickPlay,
    Skate,
    Crash,
    ConfirmReset,
}

pub struct SkateJob {
    pub rx: Receiver<skate::Msg>,
    pub stage: usize,
    pub log: Vec<String>,
}

/// Result of a file-picker dialog run on a worker thread.
enum PickResult {
    Mw2(Option<PathBuf>),
    Skate(Option<PathBuf>),
}

pub struct App {
    pub paths: Paths,
    pub config: Config,
    pub mw2: Option<PathBuf>,
    pub skate_ready: bool,
    pub selected: usize,
    pub panel: Panel,
    /// The last panel that was open, kept so the side panel can still draw
    /// its contents while it slides out.
    pub shown_panel: Panel,
    /// Message and whether it is an error.
    pub notice: Option<(String, bool)>,
    pub skate_job: Option<SkateJob>,
    pub skate_error: Option<String>,
    pub crash: String,
    /// Typed MW2 path, for systems with no file-picker portal.
    pub path_input: String,
    pub game: Option<Child>,
    pub backdrop: Backdrop,
    pub fx: Fx,
    pub audio: MenuAudio,
    /// Quick Play's maps, from the games root's MW2 folder.
    pub maps: Vec<MapEntry>,
    pub thumbs: Thumbs,
    /// The Quick Play tile the keyboard, gamepad or pointer is on.
    pub quick_focus: Option<usize>,
    /// Columns in the Quick Play grid as last drawn, for up/down.
    pub quick_cols: usize,
    /// Scroll the focused tile into view on the next draw.
    pub quick_scroll: bool,
    /// Thumbnails were last asked for while the backdrop loader was still
    /// writing the cache, so one more ask is due once it finishes.
    thumbs_final_retry: bool,
    /// Ignore keys and gamepad on the next `input`.
    skip_input: bool,
    /// Menu sounds being read from MW2 on a worker thread.
    audio_loading: Option<Receiver<Loaded>>,
    /// The MW2 folder the menu sounds came (or are coming) from.
    audio_source: Option<PathBuf>,
    /// The menu item last heard as hovered, so a hover sound plays once per
    /// change.
    heard_item: Option<usize>,
    /// The panel button last heard as hovered.
    heard_button: Option<egui::Id>,
    /// The Quick Play tile last heard as focused.
    heard_tile: Option<usize>,
    /// A click sound asked for this frame; several asks play once.
    pending_click: bool,
    gamepad: Option<gilrs::Gilrs>,
    /// The controllers connected as of this frame, for the status card.
    pub controller: Controller,
    /// A file-picker dialog running on a worker thread, so the UI thread
    /// (and egui's event loop) never blocks on it.
    picker: Option<Receiver<PickResult>>,
}

impl App {
    pub fn new(ctx: &egui::Context) -> Self {
        crate::theme::install(ctx);
        let paths = Paths::detect();
        let _ = std::fs::remove_dir_all(paths.tmp());
        let config = Config::load(&paths.config());
        let audio = MenuAudio::new(config.music, config.ui_sounds);
        let gamepad = gilrs::Gilrs::new().ok();
        let mut app = Self {
            skate_ready: skate::ready(&paths.skate_assets()),
            paths,
            config,
            mw2: None,
            selected: 0,
            panel: Panel::None,
            shown_panel: Panel::None,
            notice: None,
            skate_job: None,
            skate_error: None,
            crash: String::new(),
            path_input: String::new(),
            game: None,
            backdrop: Backdrop::default(),
            fx: Fx::new(ctx),
            audio,
            maps: Vec::new(),
            thumbs: Thumbs::default(),
            quick_focus: None,
            quick_cols: 1,
            quick_scroll: false,
            thumbs_final_retry: false,
            skip_input: false,
            audio_loading: None,
            audio_source: None,
            heard_item: Some(0),
            heard_button: None,
            heard_tile: None,
            pending_click: false,
            controller: Controller::read(gamepad.as_ref()),
            gamepad,
            picker: None,
        };
        let found = app
            .config
            .mw2_path
            .clone()
            .filter(|p| steam::is_mw2(p))
            .or_else(|| steam::find_mw2(&paths::home()));
        if let Some(path) = found {
            app.use_mw2(path);
        }
        app
    }

    pub fn open(&mut self, panel: Panel) {
        self.panel = panel;
        if panel != Panel::None {
            self.shown_panel = panel;
        }
        // Cheap: just checks the Steam library folders on disk. Catches the
        // case where the player installed MW2 through Steam and reopened
        // Options without restarting the launcher.
        if panel == Panel::Options
            && self.mw2.is_none()
            && let Some(path) = steam::find_mw2(&paths::home())
        {
            self.use_mw2(path);
        }
    }

    /// Asks Steam to install MW2's multiplayer. The player presses Locate
    /// MW2 (or reopens the launcher) once it's done; auto-detection also
    /// retries whenever Options opens and MW2 is still missing.
    pub fn install_mw2_via_steam(&mut self) {
        if let Err(error) = open_external(steam::install_url()) {
            eprintln!("xdg-open steam://install failed: {error}");
            self.notice = Some((
                "Couldn't open Steam. Install Call of Duty: Modern Warfare 2 from Steam, then press Locate MW2.".into(),
                true,
            ));
        }
    }

    pub fn use_mw2(&mut self, path: PathBuf) {
        let path = absolute_path(&path, &paths::home());
        if !steam::is_mw2(&path) {
            self.notice = Some((
                format!(
                    "{} isn't MW2 with its multiplayer maps. Pick the folder that has zone/ and main/ in it.",
                    path.display()
                ),
                true,
            ));
            return;
        }
        if let Err(error) = games_root::link(&self.paths.games(), &path) {
            self.notice = Some((error, true));
            return;
        }
        self.backdrop.start(path.clone(), self.paths.backdrops());
        if self.mw2.as_ref() != Some(&path) {
            self.thumbs.clear();
        }
        if self.audio_source.as_ref() != Some(&path) {
            self.audio_loading = Some(menu_audio::start_loading(
                path.clone(),
                self.audio.needs_output(),
            ));
            self.audio_source = Some(path.clone());
        }
        self.config.mw2_path = Some(path.clone());
        self.mw2 = Some(path);
        self.notice = None;
        self.refresh_maps();
        self.save();
    }

    /// Rescans the games root for Quick Play's maps.
    pub fn refresh_maps(&mut self) {
        self.maps = if self.mw2.is_some() {
            maps::playable_maps(&self.paths.games().join("mw2"))
        } else {
            Vec::new()
        };
        if self.quick_focus.is_some_and(|i| i >= self.maps.len()) {
            self.quick_focus = None;
        }
    }

    fn map_zones(&self) -> Vec<String> {
        self.maps.iter().map(|map| map.zone.clone()).collect()
    }

    /// Opens Quick Play on the first tile, rescanning the maps and asking
    /// for any thumbnails not loaded yet.
    fn open_quick_play(&mut self, ctx: &egui::Context) {
        if self.mw2.is_none() {
            self.notice = Some((MW2_MISSING.into(), true));
            return;
        }
        self.refresh_maps();
        self.quick_focus = (!self.maps.is_empty()).then_some(0);
        // Opening is announced by the click, not a hover on the first tile.
        self.heard_tile = self.quick_focus;
        self.quick_scroll = true;
        let zones = self.map_zones();
        self.thumbs
            .request(self.paths.backdrops(), &zones, ctx.input(|i| i.time));
        self.thumbs_final_retry = self.backdrop.loading();
        self.open(Panel::QuickPlay);
    }

    /// Throws away buffered gamepad events and this frame's keys: they
    /// were pressed for the game.
    fn discard_input(&mut self) {
        if let Some(gilrs) = &mut self.gamepad {
            while gilrs.next_event().is_some() {}
        }
        self.skip_input = true;
    }

    /// Asks again for thumbnails still missing while the backdrop loader
    /// may yet write them (the first run writes the cache progressively),
    /// and once more after it finishes.
    fn retry_thumbs(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let due = self
            .thumbs
            .last_request()
            .is_none_or(|last| now - last >= THUMB_RETRY);
        let writing = self.backdrop.loading();
        if self.panel != Panel::QuickPlay
            || self.thumbs.loading()
            || !due
            || !(writing || self.thumbs_final_retry)
        {
            return;
        }
        self.thumbs_final_retry = writing;
        let zones = self.map_zones();
        if self.thumbs.missing(&zones) {
            self.thumbs.request(self.paths.backdrops(), &zones, now);
        }
    }

    /// Whether Quick Play spawns the player onto a board: chosen, and
    /// Skate 3 is set up.
    pub fn quick_skate(&self) -> bool {
        self.skate_ready && self.config.quick_skate
    }

    /// The Quick Play launch for `zone` with the current mode, bots and
    /// skate choice.
    pub fn quick_play_launch(&self, zone: &str) -> Launch {
        Launch::Map {
            zone: zone.to_owned(),
            gametype: self.config.quick_mode.clone(),
            bots: self.config.quick_bots.min(maps::MAX_BOTS),
            skate: self.quick_skate(),
        }
    }

    /// Quick Play: straight into `zone` with the chosen mode, bots and
    /// skate choice.
    pub fn quick_launch(&mut self, ctx: &egui::Context, zone: &str) {
        self.pending_click = true;
        let was_running = self.game.is_some();
        let launch = self.quick_play_launch(zone);
        self.launch(ctx, launch);
        if !was_running && self.game.is_some() {
            self.config.push_recent(zone);
            self.save();
            // Back on the main menu when the game exits.
            self.panel = Panel::None;
        }
    }

    pub fn set_quick_skate(&mut self, on: bool) {
        self.config.quick_skate = on;
        self.save();
    }

    /// The Skate panel's Start: Quick Play with skate on spawn chosen.
    pub fn start_skate_session(&mut self, ctx: &egui::Context) {
        if !self.skate_ready {
            return;
        }
        self.pending_click = true;
        self.set_quick_skate(true);
        self.open_quick_play(ctx);
    }

    /// Re-reads the connected controllers. gilrs only updates its record
    /// of them as its events are drained, which `input` or
    /// `discard_input` do every frame.
    fn refresh_controller(&mut self) {
        self.controller = Controller::read(self.gamepad.as_ref());
    }

    /// Gamepad shoulder buttons: the next or previous game mode.
    fn cycle_mode(&mut self, step: i32) {
        let modes = maps::MODES.len() as i32;
        let current = maps::MODES
            .iter()
            .position(|(code, _)| *code == self.config.quick_mode)
            .map_or(if step > 0 { -1 } else { 0 }, |i| i as i32);
        let next = (current + step).rem_euclid(modes) as usize;
        self.config.quick_mode = maps::MODES[next].0.to_owned();
        self.audio.hover();
        self.save();
    }

    /// Gamepad triggers: one bot more or fewer.
    fn step_bots(&mut self, step: i32) {
        let bots = (i32::from(self.config.quick_bots) + step).clamp(0, i32::from(maps::MAX_BOTS));
        if bots as u8 != self.config.quick_bots {
            self.config.quick_bots = bots as u8;
            self.audio.hover();
            self.save();
        }
    }

    pub fn save(&mut self) {
        if let Err(error) = self.config.save(&self.paths.config()) {
            self.notice = Some((error, true));
        }
    }

    /// Whether a file-picker dialog is currently open on its worker thread.
    pub fn picker_open(&self) -> bool {
        self.picker.is_some()
    }

    pub fn pick_mw2(&mut self, ctx: &egui::Context) {
        if self.picker.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.picker = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let path = pollster::block_on(
                rfd::AsyncFileDialog::new()
                    .set_title("Select your Modern Warfare 2 folder")
                    .pick_folder(),
            )
            .map(|handle| handle.path().to_owned());
            let _ = tx.send(PickResult::Mw2(path));
            ctx.request_repaint();
        });
    }

    pub fn pick_skate(&mut self, ctx: &egui::Context) {
        if self.picker.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.picker = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let path = pollster::block_on(
                rfd::AsyncFileDialog::new()
                    .set_title("Select your Skate 3 ISO or default.xex")
                    .add_filter("Skate 3 (Xbox 360)", &["iso", "ISO", "xex"])
                    .pick_file(),
            )
            .map(|handle| handle.path().to_owned());
            let _ = tx.send(PickResult::Skate(path));
            ctx.request_repaint();
        });
    }

    /// Applies a chosen Skate 3 source: classifies it and starts the
    /// conversion job, or records why it couldn't.
    fn start_skate(&mut self, path: PathBuf) {
        match skate::Source::classify(&path) {
            Ok(source) => {
                self.skate_error = None;
                self.skate_job = Some(SkateJob {
                    rx: skate::start(self.paths.clone(), source),
                    stage: 0,
                    log: Vec::new(),
                });
            }
            Err(error) => self.skate_error = Some(error),
        }
    }

    /// Applies the result of a finished file-picker dialog.
    fn apply_pick(&mut self, result: PickResult) {
        match result {
            PickResult::Mw2(Some(path)) => self.use_mw2(path),
            PickResult::Mw2(None) => {}
            PickResult::Skate(Some(path)) => self.start_skate(path),
            PickResult::Skate(None) => {}
        }
    }

    pub fn launch(&mut self, ctx: &egui::Context, launch: Launch) {
        if self.mw2.is_none() {
            self.notice = Some((MW2_MISSING.into(), true));
            return;
        }
        if self.game.is_some() {
            self.notice = Some(("The game is already running.".into(), false));
            return;
        }
        match game::spawn(&self.paths, &launch, self.skate_ready) {
            Ok(child) => {
                self.game = Some(child);
                self.audio.game_started();
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            Err(error) => self.notice = Some((error, true)),
        }
    }

    pub fn activate(&mut self, ctx: &egui::Context, item: Item) {
        self.pending_click = true;
        match item {
            Item::Play => self.launch(ctx, Launch::Menu),
            Item::QuickPlay => self.open_quick_play(ctx),
            Item::Minecraft => self.launch(ctx, Launch::Minecraft),
            Item::Skate => self.open(Panel::Skate),
            Item::Options => self.open(Panel::Options),
            Item::Quit => {
                if !self.refuse_while_busy() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// Whether the game or a Skate 3 setup is running, saying so in the
    /// notice. The game runs from inside the AppImage mount, which goes away
    /// when the launcher exits, so the launcher must outlive both.
    fn refuse_while_busy(&mut self) -> bool {
        let reason = if self.game.is_some() {
            "The game is still running. Quit it first."
        } else if self.skate_job.is_some() {
            "Skate 3 setup is still running. Wait for it to finish."
        } else {
            return false;
        };
        self.notice = Some((reason.into(), true));
        true
    }

    pub fn set_menu_entry(&mut self, on: bool) {
        let home = paths::home();
        let result = if on {
            match desktop::launcher_exec() {
                Some(exec) => desktop::install(&home, &exec),
                None => Err("Couldn't find the launcher's own path.".to_owned()),
            }
        } else {
            desktop::uninstall(&home);
            Ok(())
        };
        match result {
            Ok(()) => {
                self.config.add_to_menu = on;
                self.save();
            }
            Err(error) => self.notice = Some((error, true)),
        }
    }

    pub fn set_music(&mut self, on: bool) {
        self.config.music = on;
        self.audio.set_music_enabled(on);
        self.save();
    }

    pub fn set_ui_sounds(&mut self, on: bool) {
        self.config.ui_sounds = on;
        self.audio.set_ui_enabled(on);
        self.save();
    }

    /// Key M: mute everything if anything is on, else turn both on. It is a
    /// plain on/off for the pair; it does not remember and restore a mix
    /// where only one of the two was on.
    pub fn toggle_mute(&mut self) {
        let on = !(self.config.music || self.config.ui_sounds);
        self.config.music = on;
        self.config.ui_sounds = on;
        self.audio.set_music_enabled(on);
        self.audio.set_ui_enabled(on);
        self.save();
    }

    /// Closes the side panel (Back, Esc, click outside), with the click
    /// sound.
    pub fn close_panel(&mut self) {
        self.pending_click = true;
        self.panel = Panel::None;
    }

    /// Plays the hover sound when the hovered menu item or panel button
    /// changed this frame.
    fn hover_sounds(&mut self, ctx: &egui::Context) {
        // The selection can't move while a panel is open, so closing one
        // doesn't count as a new hover.
        let item = Some(self.selected);
        let button = ctx
            .data_mut(|d| d.remove_temp::<Option<egui::Id>>(crate::ui::hovered_key()))
            .flatten();
        let new_item = menu_audio::changed(&mut self.heard_item, item);
        let new_button = menu_audio::changed(&mut self.heard_button, button);
        let tile = self.quick_focus.filter(|_| self.panel == Panel::QuickPlay);
        let new_tile = menu_audio::changed(&mut self.heard_tile, tile);
        if new_item || new_button || new_tile {
            self.audio.hover();
        }
        let clicked = ctx
            .data_mut(|d| d.remove_temp::<bool>(crate::ui::clicked_key()))
            .is_some_and(|clicked| clicked);
        if std::mem::take(&mut self.pending_click) || clicked {
            self.audio.click();
        }
    }

    pub fn reset(&mut self) {
        if self.refuse_while_busy() {
            return;
        }
        let _ = std::fs::remove_file(self.paths.config());
        let _ = std::fs::remove_dir_all(self.paths.games());
        let _ = std::fs::remove_dir_all(self.paths.skate_data());
        let _ = std::fs::remove_dir_all(self.paths.backdrops());
        let _ = std::fs::remove_dir_all(self.paths.tmp());
        if self.config.add_to_menu {
            desktop::uninstall(&paths::home());
        }
        self.forget_everything();
        // `use_mw2` clears any stale error notice when it finds MW2 again.
        if let Some(path) = steam::find_mw2(&paths::home()) {
            self.use_mw2(path);
        }
    }

    /// Deletes the whole data folder (config, games-root symlinks, Skate 3
    /// data, backdrop cache, the Minecraft download, logs) and the app-menu
    /// entry.
    pub fn remove_all_data(&mut self) {
        if self.refuse_while_busy() {
            return;
        }
        if self.config.add_to_menu {
            desktop::uninstall(&paths::home());
        }
        if let Err(error) = remove_data_dir(&self.paths.data) {
            self.notice = Some((
                format!("Couldn't remove {}: {error}", self.paths.data.display()),
                true,
            ));
            return;
        }
        self.forget_everything();
        self.notice = Some((
            "Launcher data removed. Minecraft (about 125 MB) downloads again next time.".into(),
            false,
        ));
    }

    /// Back to a first start's state, after Reset or Remove everything
    /// deleted what it was read from.
    fn forget_everything(&mut self) {
        self.config = Config::default();
        // Sounds reload from whichever MW2 is found next.
        self.audio.stop_music();
        self.audio_loading = None;
        self.audio_source = None;
        self.audio.set_music_enabled(self.config.music);
        self.audio.set_ui_enabled(self.config.ui_sounds);
        self.mw2 = None;
        self.skate_ready = false;
        self.skate_error = None;
        self.backdrop = Backdrop::default();
        self.maps.clear();
        self.thumbs.clear();
        self.quick_focus = None;
        self.panel = Panel::None;
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.picker {
            match rx.try_recv() {
                Ok(result) => {
                    self.picker = None;
                    self.apply_pick(result);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.picker = None,
            }
        }
        if let Some(rx) = &self.audio_loading {
            match rx.try_recv() {
                Ok(Loaded { sounds, output }) => {
                    self.audio_loading = None;
                    match sounds {
                        Ok(sounds) => self.audio.set_sounds(sounds, output),
                        Err(error) => eprintln!("menu sounds unavailable: {error}"),
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.audio_loading = None,
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.refuse_while_busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        // `Ok(None)` means still running; an `Err` means we can't wait on it
        // any more, so treat it as gone.
        let exited = match self.game.as_mut().map(Child::try_wait) {
            None | Some(Ok(None)) => None,
            Some(Ok(Some(status))) => Some(status.success()),
            Some(Err(_)) => Some(true),
        };
        if let Some(success) = exited {
            self.game = None;
            self.audio.game_stopped();
            // On Wayland a client can't observe or undo its own
            // minimisation, so this restore may be ignored and the crash
            // panel only seen once the player brings the window back.
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            if !success {
                self.crash = game::crash_report(&self.paths);
                self.open(Panel::Crash);
            }
        }
        let mut finished = None;
        if let Some(job) = &mut self.skate_job {
            loop {
                match job.rx.try_recv() {
                    Ok(skate::Msg::Line(line)) => job.log.push(line),
                    Ok(skate::Msg::Stage(stage)) => job.stage = stage,
                    Ok(skate::Msg::Done(result)) => {
                        finished = Some(result);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished = Some(Err("Skate 3 setup stopped unexpectedly.".to_owned()));
                        break;
                    }
                }
            }
        }
        if let Some(result) = finished {
            self.skate_job = None;
            match result {
                Ok(()) => {
                    self.skate_ready = true;
                    self.notice = Some((
                        "Skate 3 is ready. Press J in a match to skate.".into(),
                        false,
                    ));
                }
                Err(error) => self.skate_error = Some(error),
            }
        }
    }

    fn input(&mut self, ctx: &egui::Context) {
        // While the game runs the launcher may still get frames (Wayland
        // can't tell it is minimised) and queued events (X11 replays them
        // on restore); none of it is meant for the launcher.
        if self.game.is_some() || std::mem::take(&mut self.skip_input) {
            return;
        }
        let (mut up, mut down, mut left, mut right, mut accept, mut back, mute) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::M),
            )
        });
        // Shoulder buttons (mode) and triggers (bots), for Quick Play.
        let (mut mode_step, mut bot_step) = (0_i32, 0_i32);
        // M typed into the MW2 path field is just a letter.
        if mute && !ctx.text_edit_focused() {
            self.toggle_mute();
        }
        if let Some(gilrs) = &mut self.gamepad {
            while let Some(event) = gilrs.next_event() {
                if let gilrs::EventType::ButtonPressed(button, _) = event.event {
                    match button {
                        gilrs::Button::DPadUp => up = true,
                        gilrs::Button::DPadDown => down = true,
                        gilrs::Button::DPadLeft => left = true,
                        gilrs::Button::DPadRight => right = true,
                        gilrs::Button::LeftTrigger => mode_step -= 1,
                        gilrs::Button::RightTrigger => mode_step += 1,
                        gilrs::Button::LeftTrigger2 => bot_step -= 1,
                        gilrs::Button::RightTrigger2 => bot_step += 1,
                        gilrs::Button::South => accept = true,
                        gilrs::Button::East => back = true,
                        _ => {}
                    }
                }
            }
        }
        if back && closable(self.panel, self.skate_job.is_some()) {
            if self.panel == Panel::ConfirmReset {
                self.pending_click = true;
                self.open(Panel::Options);
            } else {
                self.close_panel();
            }
            return;
        }
        if self.panel == Panel::QuickPlay {
            if mode_step != 0 {
                self.cycle_mode(mode_step.signum());
            }
            if bot_step != 0 {
                self.step_bots(bot_step);
            }
            // A widget holding keyboard focus (Tab) keeps its keys.
            if ctx.memory(|m| m.focused().is_some()) {
                return;
            }
            let dx = i32::from(right) - i32::from(left);
            let dy = i32::from(down) - i32::from(up);
            if dx != 0 || dy != 0 {
                self.quick_focus =
                    maps::grid_move(self.quick_focus, self.maps.len(), self.quick_cols, dx, dy);
                self.quick_scroll = true;
            }
            if accept && let Some(map) = self.quick_focus.and_then(|i| self.maps.get(i)) {
                let zone = map.zone.clone();
                self.quick_launch(ctx, &zone);
            }
            return;
        }
        if self.panel != Panel::None {
            return;
        }
        if up {
            self.selected = (self.selected + ITEMS.len() - 1) % ITEMS.len();
        }
        if down {
            self.selected = (self.selected + 1) % ITEMS.len();
        }
        if accept {
            self.activate(ctx, ITEMS[self.selected]);
        }
    }
}

impl eframe::App for App {
    /// Runs even while the window is minimised (the game is up), so the
    /// launcher notices the game exiting and comes back.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let was_running = self.game.is_some();
        self.poll(ctx);
        if !input_live(was_running, self.game.is_some()) {
            self.discard_input();
        }
        // Connections still register while the game runs: draining above
        // updates gilrs's record without acting on any button.
        self.refresh_controller();
        self.retry_thumbs(ctx);
        // Fades run on the audio thread; this only pauses faded-out music.
        self.audio.tick();
        // ~30 fps for the grain and backdrop, focused or not; while the game
        // runs, only poll for its exit.
        let wait = if self.game.is_some() { 500 } else { 33 };
        ctx.request_repaint_after(std::time::Duration::from_millis(wait));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.input(&ctx);
        crate::ui::draw(self, ui);
        self.hover_sounds(&ctx);
    }
}

/// Hands `target` (a folder or URL) to the desktop's opener. The opener
/// is waited for on its own thread so it never lingers as a zombie.
pub fn open_external(target: impl AsRef<std::ffi::OsStr>) -> std::io::Result<()> {
    let mut child = std::process::Command::new("xdg-open").arg(target).spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Deletes `data` and everything in it. `std::fs::remove_dir_all` never
/// follows a symlink inside the tree (it unlinks the symlink itself rather
/// than recursing through it), which matters here: `games/mw2/zone` and
/// `games/mw2/main` are symlinks into the player's own MW2 install.
fn remove_data_dir(data: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(data) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Whether the launcher acts on keys and gamepad this frame: not while the
/// game was running when the frame began, nor on the frame it exited.
pub fn input_live(running_before_poll: bool, running_now: bool) -> bool {
    !running_before_poll && !running_now
}

/// Whether Back, Esc or a click outside may close `panel`. Only the Skate
/// panel stays put while its conversion runs.
pub fn closable(panel: Panel, skate_job_running: bool) -> bool {
    panel != Panel::None && !(panel == Panel::Skate && skate_job_running)
}

/// Expands a leading `~` to `home` and makes the path absolute, resolving
/// symlinks when the path exists.
pub fn absolute_path(path: &Path, home: &Path) -> PathBuf {
    let expanded = match path.strip_prefix("~") {
        Ok(rest) => home.join(rest),
        Err(_) => path.to_path_buf(),
    };
    match std::fs::canonicalize(&expanded) {
        Ok(path) => path,
        Err(_) => std::path::absolute(&expanded).unwrap_or(expanded),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal `App` for exercising pure state transitions, backed by a
    /// scratch data folder instead of the player's real one.
    fn test_app(name: &str) -> App {
        let ctx = egui::Context::default();
        App {
            paths: Paths {
                data: paths::test_dir(name),
                bin: PathBuf::from("."),
            },
            config: Config::default(),
            mw2: None,
            skate_ready: false,
            selected: 0,
            panel: Panel::None,
            shown_panel: Panel::None,
            notice: None,
            skate_job: None,
            skate_error: None,
            crash: String::new(),
            path_input: String::new(),
            game: None,
            backdrop: Backdrop::default(),
            fx: Fx::new(&ctx),
            audio: MenuAudio::new(true, true),
            maps: Vec::new(),
            thumbs: Thumbs::default(),
            quick_focus: None,
            quick_cols: 1,
            quick_scroll: false,
            thumbs_final_retry: false,
            skip_input: false,
            audio_loading: None,
            audio_source: None,
            heard_item: Some(0),
            heard_button: None,
            heard_tile: None,
            pending_click: false,
            gamepad: None,
            controller: Controller::Unavailable,
            picker: None,
        }
    }

    #[test]
    fn apply_pick_mw2_none_leaves_mw2_untouched() {
        let mut app = test_app("apply-pick-mw2-none");
        app.apply_pick(PickResult::Mw2(None));
        assert!(app.mw2.is_none());
        assert!(app.notice.is_none());
    }

    #[test]
    fn apply_pick_skate_none_starts_no_job() {
        let mut app = test_app("apply-pick-skate-none");
        app.apply_pick(PickResult::Skate(None));
        assert!(app.skate_job.is_none());
        assert!(app.skate_error.is_none());
    }

    #[test]
    fn apply_pick_skate_bad_extension_records_error() {
        let mut app = test_app("apply-pick-skate-bad");
        app.apply_pick(PickResult::Skate(Some(PathBuf::from("/x/game.7z"))));
        assert!(app.skate_job.is_none());
        assert!(app.skate_error.is_some());
    }

    #[test]
    fn picker_open_reflects_a_pending_receiver() {
        let mut app = test_app("picker-open-pending");
        assert!(!app.picker_open());
        let (_tx, rx) = channel();
        app.picker = Some(rx);
        assert!(app.picker_open());
    }

    #[test]
    fn toggle_mute_turns_all_off_then_all_on() {
        let mut app = test_app("toggle-mute");
        app.config.ui_sounds = false;
        app.toggle_mute();
        assert!(!app.config.music && !app.config.ui_sounds);
        app.toggle_mute();
        assert!(app.config.music && app.config.ui_sounds);
        assert_eq!(Config::load(&app.paths.config()), app.config);
    }

    #[test]
    fn input_is_live_only_with_no_game_this_frame() {
        assert!(input_live(false, false));
        // Running, just started, or exited during this frame's poll.
        assert!(!input_live(true, true));
        assert!(!input_live(false, true));
        assert!(!input_live(true, false));
    }

    #[test]
    fn only_a_converting_skate_panel_refuses_to_close() {
        assert!(!closable(Panel::None, false));
        assert!(closable(Panel::Skate, false));
        assert!(!closable(Panel::Skate, true));
        assert!(closable(Panel::QuickPlay, true));
        assert!(closable(Panel::Options, true));
    }

    /// One frame of `input` with `key` pressed.
    fn press(app: &mut App, key: egui::Key) {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| app.input(ui.ctx()));
        // The font atlas upload is nobody's here.
        output.textures_delta.clear();
    }

    #[test]
    fn keys_are_ignored_while_the_game_runs_and_on_its_exit_frame() {
        let mut app = test_app("input-gate");
        press(&mut app, egui::Key::ArrowDown);
        assert_eq!(app.selected, 1);
        // A stand-in child process (`true`), not the game.
        app.game = Some(std::process::Command::new("true").spawn().unwrap());
        press(&mut app, egui::Key::ArrowDown);
        press(&mut app, egui::Key::M);
        assert_eq!(app.selected, 1);
        assert!(app.config.music);
        let _ = app.game.take().map(|mut child| child.wait());
        // The exit frame discards; the next one listens again.
        app.discard_input();
        press(&mut app, egui::Key::ArrowDown);
        assert_eq!(app.selected, 1);
        press(&mut app, egui::Key::ArrowDown);
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn quick_play_sits_under_play() {
        assert_eq!(&ITEMS[..2], &[Item::Play, Item::QuickPlay]);
    }

    #[test]
    fn quick_play_needs_mw2() {
        let mut app = test_app("quick-play-no-mw2");
        let ctx = egui::Context::default();
        app.activate(&ctx, Item::QuickPlay);
        assert_eq!(app.panel, Panel::None);
        assert!(app.notice.as_ref().is_some_and(|(_, error)| *error));
    }

    /// A failed start (no `iw4l` in an empty bin folder) remembers nothing
    /// and keeps the panel open with the error.
    #[test]
    fn quick_launch_failure_keeps_recent_maps() {
        let mut app = test_app("quick-launch-fail");
        app.paths.bin = app.paths.data.join("empty-bin");
        app.mw2 = Some(app.paths.data.join("mw2"));
        app.open(Panel::QuickPlay);
        app.quick_launch(&egui::Context::default(), "mp_rust");
        assert!(app.game.is_none());
        assert!(app.config.recent_maps.is_empty());
        assert_eq!(app.panel, Panel::QuickPlay);
        assert!(app.notice.as_ref().is_some_and(|(_, error)| *error));
    }

    #[test]
    fn quick_play_skates_only_when_chosen_and_set_up() {
        let mut app = test_app("quick-skate");
        app.config.quick_bots = 20;
        let skate = |app: &App| match app.quick_play_launch("mp_rust") {
            Launch::Map { skate, .. } => skate,
            other => panic!("not a map launch: {other:?}"),
        };
        assert!(!skate(&app));
        // Chosen, but Skate 3 isn't set up (or was reset).
        app.config.quick_skate = true;
        assert!(!skate(&app));
        app.skate_ready = true;
        assert!(skate(&app));
        assert_eq!(
            app.quick_play_launch("mp_rust"),
            Launch::Map {
                zone: "mp_rust".into(),
                gametype: "war".into(),
                bots: 20,
                skate: true,
            }
        );
        app.config.quick_skate = false;
        assert!(!skate(&app));
    }

    #[test]
    fn start_skate_session_chooses_skate_and_needs_mw2_for_quick_play() {
        let ctx = egui::Context::default();
        let mut app = test_app("skate-session");
        app.start_skate_session(&ctx);
        assert!(!app.config.quick_skate, "not set up: nothing changes");
        app.skate_ready = true;
        app.start_skate_session(&ctx);
        assert!(app.config.quick_skate);
        assert_eq!(Config::load(&app.paths.config()), app.config);
        // No MW2 here, so Quick Play doesn't open and says why.
        assert_eq!(app.panel, Panel::None);
        assert!(app.notice.is_some());
        app.mw2 = Some(app.paths.data.join("mw2"));
        app.start_skate_session(&ctx);
        assert_eq!(app.panel, Panel::QuickPlay);
    }

    #[test]
    fn gamepad_cycles_mode_and_steps_bots() {
        let mut app = test_app("quick-gamepad");
        assert_eq!(app.config.quick_mode, "war");
        app.cycle_mode(1);
        assert_eq!(app.config.quick_mode, "dom");
        app.cycle_mode(-1);
        app.cycle_mode(-1);
        assert_eq!(app.config.quick_mode, "dm");
        app.cycle_mode(-1);
        assert_eq!(app.config.quick_mode, "dd");
        app.config.quick_mode = "unknown".into();
        app.cycle_mode(1);
        assert_eq!(app.config.quick_mode, "dm");
        app.config.quick_bots = 19;
        app.step_bots(1);
        app.step_bots(1);
        assert_eq!(app.config.quick_bots, maps::MAX_BOTS);
        app.config.quick_bots = 0;
        app.step_bots(-1);
        assert_eq!(app.config.quick_bots, 0);
        app.step_bots(1);
        assert_eq!(Config::load(&app.paths.config()), app.config);
    }

    #[test]
    fn absolute_path_expands_tilde_and_relative() {
        let home = Path::new("/nonexistent-home");
        assert_eq!(absolute_path(Path::new("~"), home), home);
        assert_eq!(
            absolute_path(Path::new("~/games/mw2"), home),
            home.join("games/mw2")
        );
        assert_eq!(
            absolute_path(Path::new("/nonexistent/mw2"), home),
            PathBuf::from("/nonexistent/mw2")
        );
        let relative = absolute_path(Path::new("some/dir"), home);
        assert!(relative.is_absolute());
        assert!(relative.ends_with("some/dir"));
        // `~user` is not expanded.
        assert!(absolute_path(Path::new("~bob/x"), home).ends_with("~bob/x"));
    }

    /// Removing the data dir must unlink `games/mw2`'s symlinks rather than
    /// recurse through them: they point at the player's real MW2 install,
    /// which must survive a Remove everything.
    #[test]
    fn remove_data_dir_does_not_follow_symlinks_out() {
        let scratch = paths::test_dir("remove-data-dir");
        let outside = scratch.join("outside-mw2");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), b"keep me").unwrap();
        let data = scratch.join("data");
        std::fs::create_dir_all(data.join("games/mw2")).unwrap();
        std::os::unix::fs::symlink(&outside, data.join("games/mw2/zone")).unwrap();

        remove_data_dir(&data).unwrap();

        assert!(!data.exists());
        assert!(outside.join("keep.txt").is_file());
    }

    #[test]
    fn remove_data_dir_on_a_missing_dir_is_fine() {
        let scratch = paths::test_dir("remove-data-dir-missing");
        remove_data_dir(&scratch.join("never-existed")).unwrap();
    }

    #[test]
    fn remove_all_data_refuses_while_busy() {
        let mut app = test_app("remove-all-data-busy");
        app.game = Some(std::process::Command::new("true").spawn().unwrap());
        app.remove_all_data();
        assert!(app.paths.data.exists());
        let _ = app.game.take().map(|mut child| child.wait());
        app.game = None;
        let (_tx, rx) = channel();
        app.skate_job = Some(SkateJob {
            rx,
            stage: 0,
            log: Vec::new(),
        });
        app.remove_all_data();
        assert!(app.paths.data.exists());
    }

    #[test]
    fn reset_refuses_while_skate_setup_runs() {
        let mut app = test_app("reset-busy");
        app.config.quick_bots = 11;
        app.save();
        std::fs::create_dir_all(app.paths.tmp().join("skate3")).unwrap();
        let (_tx, rx) = channel();
        app.skate_job = Some(SkateJob {
            rx,
            stage: 0,
            log: Vec::new(),
        });
        app.reset();
        assert!(app.paths.config().is_file());
        assert!(app.paths.tmp().exists());
        assert!(app.notice.as_ref().is_some_and(|(_, error)| *error));
    }

    #[test]
    fn remove_all_data_deletes_the_data_folder_and_resets_config() {
        let mut app = test_app("remove-all-data");
        std::fs::create_dir_all(&app.paths.data).unwrap();
        app.config.quick_bots = 11;
        app.save();
        assert!(app.paths.config().is_file());

        app.remove_all_data();

        assert!(!app.paths.data.exists());
        assert_eq!(app.config, Config::default());
        assert!(app.mw2.is_none());
        assert!(app.notice.is_some());
    }

    #[test]
    fn absolute_path_resolves_existing_symlinks() {
        let dir = crate::paths::test_dir("absolute");
        std::fs::create_dir_all(dir.join("real")).unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();
        assert_eq!(
            absolute_path(&dir.join("link"), Path::new("/")),
            std::fs::canonicalize(dir.join("real")).unwrap()
        );
    }
}
