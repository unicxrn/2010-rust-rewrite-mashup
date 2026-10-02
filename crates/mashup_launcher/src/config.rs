//! The launcher's saved choices, `config.json` in the data folder.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mw2_path: Option<PathBuf>,
    pub add_to_menu: bool,
    /// MW2's main-menu theme behind the launcher.
    #[serde(default = "on")]
    pub music: bool,
    /// Hover and click blips.
    #[serde(default = "on")]
    pub ui_sounds: bool,
    /// Quick Play's game type, an `IW4L_GAMETYPE` code.
    #[serde(default = "quick_mode")]
    pub quick_mode: String,
    /// Bots Quick Play adds, 0 to `maps::MAX_BOTS`.
    #[serde(default = "quick_bots")]
    pub quick_bots: u8,
    /// Zones last launched from Quick Play, most recent first.
    #[serde(default = "Vec::new")]
    pub recent_maps: Vec<String>,
    /// Quick Play spawns the player with the default class and puts them
    /// on a board. Only honoured while Skate 3 is set up.
    pub quick_skate: bool,
}

/// Recent maps Quick Play remembers.
pub const RECENT_MAPS: usize = 3;

fn on() -> bool {
    true
}

fn quick_mode() -> String {
    "war".to_owned()
}

fn quick_bots() -> u8 {
    6
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mw2_path: None,
            add_to_menu: false,
            music: on(),
            ui_sounds: on(),
            quick_mode: quick_mode(),
            quick_bots: quick_bots(),
            recent_maps: Vec::new(),
            quick_skate: false,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Self {
        let parsed: Option<Config> = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        // Missing or unreadable config file, or bad JSON: start fresh.
        let Some(mut config) = parsed else {
            return Config::default();
        };
        config.sanitize();
        config
    }

    /// Replaces Quick Play choices the launcher doesn't offer (a hand
    /// edit, or a mode from a newer version) with the defaults.
    fn sanitize(&mut self) {
        if crate::maps::mode_label(&self.quick_mode).is_none() {
            self.quick_mode = quick_mode();
        }
        self.quick_bots = self.quick_bots.min(crate::maps::MAX_BOTS);
    }

    /// Puts `zone` first among the recent maps, once, keeping at most
    /// `RECENT_MAPS`.
    pub fn push_recent(&mut self, zone: &str) {
        self.recent_maps.retain(|z| z != zone);
        self.recent_maps.insert(0, zone.to_owned());
        self.recent_maps.truncate(RECENT_MAPS);
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_defaults_on_missing_or_bad_file() {
        let dir = crate::paths::test_dir("config");
        let path = dir.join("config.json");
        assert_eq!(Config::load(&path), Config::default());
        let config = Config {
            mw2_path: Some("/games/mw2".into()),
            add_to_menu: true,
            music: false,
            ui_sounds: false,
            quick_mode: "sd".into(),
            quick_bots: 17,
            recent_maps: vec!["mp_rust".into(), "minecraft:overworld".into()],
            quick_skate: true,
        };
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path), config);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Config::load(&path), Config::default());
    }

    #[test]
    fn sound_defaults_on_for_old_configs() {
        assert!(Config::default().music && Config::default().ui_sounds);
        let old: Config =
            serde_json::from_str(r#"{"mw2_path":null,"skate_skipped":true,"add_to_menu":false}"#)
                .unwrap();
        assert!(old.music && old.ui_sounds);
        let muted: Config = serde_json::from_str(r#"{"music":false}"#).unwrap();
        assert!(!muted.music && muted.ui_sounds);
    }

    #[test]
    fn old_configs_with_skate_skipped_still_load() {
        let dir = crate::paths::test_dir("config-skate-skipped");
        let path = dir.join("config.json");
        std::fs::write(
            &path,
            r#"{"mw2_path":"/games/mw2","skate_skipped":true,"add_to_menu":true,"quick_bots":3}"#,
        )
        .unwrap();
        let config = Config::load(&path);
        assert_eq!(config.mw2_path, Some(PathBuf::from("/games/mw2")));
        assert!(config.add_to_menu);
        assert_eq!(config.quick_bots, 3);
        config.save(&path).unwrap();
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("skate_skipped")
        );
    }

    #[test]
    fn quick_play_defaults_for_old_configs() {
        let old: Config = serde_json::from_str(r#"{"music":false,"skate_skipped":true}"#).unwrap();
        assert_eq!(old.quick_mode, "war");
        assert_eq!(old.quick_bots, 6);
        assert!(old.recent_maps.is_empty());
        assert!(!old.quick_skate);
        assert_eq!(old.quick_mode, Config::default().quick_mode);
        assert_eq!(old.quick_bots, Config::default().quick_bots);
    }

    #[test]
    fn quick_skate_defaults_off_for_old_configs() {
        assert!(!Config::default().quick_skate);
        let old: Config =
            serde_json::from_str(r#"{"quick_mode":"sd","quick_bots":3,"recent_maps":["mp_rust"]}"#)
                .unwrap();
        assert!(!old.quick_skate);
        assert_eq!(old.quick_bots, 3);
        let on: Config = serde_json::from_str(r#"{"quick_skate":true}"#).unwrap();
        assert!(on.quick_skate);
    }

    #[test]
    fn unknown_quick_mode_falls_back_to_war() {
        let dir = crate::paths::test_dir("config-bad-mode");
        let path = dir.join("config.json");
        std::fs::write(&path, r#"{"quick_mode":"gungame","quick_bots":99}"#).unwrap();
        let config = Config::load(&path);
        assert_eq!(config.quick_mode, "war");
        assert_eq!(config.quick_bots, crate::maps::MAX_BOTS);
        std::fs::write(&path, r#"{"quick_mode":"sd"}"#).unwrap();
        assert_eq!(Config::load(&path).quick_mode, "sd");
    }

    #[test]
    fn push_recent_dedups_and_keeps_three() {
        let mut config = Config::default();
        for zone in ["mp_rust", "mp_derail", "mp_rust", "mp_estate", "mp_favela"] {
            config.push_recent(zone);
        }
        assert_eq!(config.recent_maps, ["mp_favela", "mp_estate", "mp_rust"]);
        config.push_recent("mp_rust");
        assert_eq!(config.recent_maps, ["mp_rust", "mp_favela", "mp_estate"]);
    }
}
