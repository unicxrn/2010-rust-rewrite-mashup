//! What Quick Play offers: the multiplayer maps the player's MW2 has, their
//! in-game names, the game modes, and the console script a launch runs.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The Minecraft world, offered when the game has `mp_rust` to host it.
pub const MINECRAFT: &str = "minecraft:overworld";

/// Most bots Quick Play adds.
pub const MAX_BOTS: u8 = 20;

/// The game's `bot add` takes at most this many at once.
const BOTS_PER_COMMAND: u8 = 16;

/// Game-type codes (`IW4L_GAMETYPE`) and their menu labels.
pub const MODES: [(&str, &str); 8] = [
    ("dm", "FFA"),
    ("war", "Team Deathmatch"),
    ("dom", "Domination"),
    ("sd", "Search & Destroy"),
    ("ctf", "Capture the Flag"),
    ("koth", "Headquarters"),
    ("sab", "Sabotage"),
    ("dd", "Demolition"),
];

/// MW2's in-game names for its map stems (without `mp_`).
const NAMES: [(&str, &str); 26] = [
    ("afghan", "Afghan"),
    ("boneyard", "Scrapyard"),
    ("brecourt", "Wasteland"),
    ("checkpoint", "Karachi"),
    ("derail", "Derail"),
    ("estate", "Estate"),
    ("favela", "Favela"),
    ("highrise", "Highrise"),
    ("invasion", "Invasion"),
    ("nightshift", "Skidrow"),
    ("quarry", "Quarry"),
    ("rundown", "Rundown"),
    ("rust", "Rust"),
    ("subbase", "Sub Base"),
    ("terminal", "Terminal"),
    ("underpass", "Underpass"),
    ("abandon", "Carnival"),
    ("complex", "Bailout"),
    ("compact", "Salvage"),
    ("storm", "Storm"),
    ("fuel2", "Fuel"),
    ("strike", "Strike"),
    ("trailerpark", "Trailer Park"),
    ("vacant", "Vacant"),
    ("overgrown", "Overgrown"),
    ("crash", "Crash"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapEntry {
    /// What `iw4l map` takes: `mp_rust`, or `minecraft:overworld`.
    pub zone: String,
    pub name: String,
}

impl MapEntry {
    fn new(zone: &str) -> Self {
        Self {
            zone: zone.to_owned(),
            name: display_name(zone),
        }
    }

    pub fn is_minecraft(&self) -> bool {
        self.zone == MINECRAFT
    }
}

/// The in-game name for a zone stem (`mp_boneyard` → Scrapyard); an unknown
/// map shows its stem without `mp_`, uppercased.
pub fn display_name(stem: &str) -> String {
    if stem == MINECRAFT {
        return "Overworld".to_owned();
    }
    let short = stem.strip_prefix("mp_").unwrap_or(stem);
    NAMES
        .iter()
        .find(|(key, _)| *key == short)
        .map_or_else(|| short.to_uppercase(), |(_, name)| (*name).to_owned())
}

/// The label of a game-type code, if it is one of `MODES`.
pub fn mode_label(code: &str) -> Option<&'static str> {
    MODES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, label)| *label)
}

/// The cached loadscreen `backdrop::load` writes for a map, if it has one.
pub fn thumb_path(backdrops: &Path, zone: &str) -> Option<PathBuf> {
    zone.starts_with("mp_")
        .then(|| backdrops.join(format!("loadscreen_{zone}.png")))
}

/// Every `zone/*/mp_<x>.ff` under the games root's MW2 folder that has an
/// `mp_<x>_load.ff`, by in-game name, then the Minecraft world when `mp_rust`
/// is there. The same rule as the game's map discovery: the load zone may
/// sit in any language folder, and an install with no load zones at all
/// offers every map.
pub fn playable_maps(games_mw2: &Path) -> Vec<MapEntry> {
    let mut stems = BTreeSet::new();
    let folders = std::fs::read_dir(games_mw2.join("zone"))
        .into_iter()
        .flatten();
    for folder in folders.flatten() {
        let files = std::fs::read_dir(folder.path()).into_iter().flatten();
        for file in files.flatten() {
            let path = file.path();
            let is_ff = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ff"));
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && is_ff
            {
                let stem = stem.to_ascii_lowercase();
                if stem.starts_with("mp_") {
                    stems.insert(stem);
                }
            }
        }
    }
    let loads: BTreeSet<&str> = stems
        .iter()
        .filter_map(|stem| stem.strip_suffix("_load"))
        .collect();
    let mut maps: Vec<MapEntry> = stems
        .iter()
        .filter(|stem| !stem.ends_with("_load"))
        .filter(|stem| loads.is_empty() || loads.contains(stem.as_str()))
        .map(|stem| MapEntry::new(stem))
        .collect();
    maps.sort_by(|a, b| (a.name.to_lowercase(), &a.zone).cmp(&(b.name.to_lowercase(), &b.zone)));
    if maps.iter().any(|map| map.zone == "mp_rust") {
        maps.push(MapEntry::new(MINECRAFT));
    }
    maps
}

/// Seconds between the player spawning and `skate on`, so the spawn has
/// reached the world (the game drops a skate toggle made while not alive).
const SKATE_DELAY: &str = "2s";

/// The `--cmds` script for a Quick Play launch, run once the world is up:
/// `bots` bots (clamped to `MAX_BOTS`, in `bot add` lines of at most 16),
/// then, for `skate`, a spawn with the default class and a board. `None`
/// when there is nothing to run.
///
/// Bots go first. `bot add` only queues them for the host and needs no
/// player, while `spawn` holds the script until the player is in the game
/// and a spawn that never gets admitted aborts the script, dropping every
/// command still queued; bots first still get their match then, and join
/// while the player spawns rather than seconds after. `skate on` waits
/// for the spawn because the game drops a skate toggle made while the
/// player isn't alive; `spawn` returns at `InGame`, before the spawn is
/// necessarily live, so a short `wait` follows it.
pub fn launch_cmds(bots: u8, skate: bool) -> Option<String> {
    let mut left = bots.min(MAX_BOTS);
    if left == 0 && !skate {
        return None;
    }
    let mut script = "wait world".to_owned();
    while left > 0 {
        let batch = left.min(BOTS_PER_COMMAND);
        script.push_str(&format!("; bot add {batch}"));
        left -= batch;
    }
    if skate {
        script.push_str(&format!("; spawn 0; wait {SKATE_DELAY}; skate on"));
    }
    Some(script)
}

/// Moves a focused index around a grid of `count` cells in rows of `cols`.
/// Left/right wrap along the list; up/down move a row and stop at the
/// ends, landing on the last cell when the row below is short. With no
/// focus yet, any move focuses the first cell.
pub fn grid_move(
    focus: Option<usize>,
    count: usize,
    cols: usize,
    dx: i32,
    dy: i32,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let cols = cols.max(1);
    let Some(index) = focus.filter(|&i| i < count) else {
        return Some(0);
    };
    let mut index = index as i64;
    let count = count as i64;
    if dx != 0 {
        index = (index + i64::from(dx)).rem_euclid(count);
    }
    if dy < 0 && index >= cols as i64 {
        index -= cols as i64;
    } else if dy > 0 {
        let row = index / cols as i64;
        let last_row = (count - 1) / cols as i64;
        if row < last_row {
            index = (index + cols as i64).min(count - 1);
        }
    }
    Some(index as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn playable_maps_need_a_load_zone_and_dedup_across_languages() {
        let mw2 = crate::paths::test_dir("maps");
        let zone = mw2.join("zone");
        for file in [
            "english/mp_rust.ff",
            "english/mp_rust_load.ff",
            "german/mp_rust.ff",
            "german/mp_rust_load.ff",
            "english/mp_boneyard.ff",
            "english/mp_boneyard_load.ff",
            // No load zone: not playable.
            "english/mp_afghan.ff",
            // A load zone alone is not a map.
            "english/mp_x_load.ff",
            // Load zone in another folder still counts, as in the game.
            "dlc/mp_abandon.ff",
            "english/mp_abandon_load.ff",
            "english/mp_newmap.ff",
            "english/mp_newmap_load.ff",
            "english/common_mp.ff",
            "english/mp_derail.txt",
        ] {
            touch(&zone.join(file));
        }
        let maps = playable_maps(&mw2);
        let zones: Vec<&str> = maps.iter().map(|m| m.zone.as_str()).collect();
        assert_eq!(
            zones,
            [
                "mp_abandon",
                "mp_newmap",
                "mp_rust",
                "mp_boneyard",
                MINECRAFT
            ]
        );
        let names: Vec<&str> = maps.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            ["Carnival", "NEWMAP", "Rust", "Scrapyard", "Overworld"]
        );
        assert!(maps.last().unwrap().is_minecraft());
    }

    #[test]
    fn no_rust_no_minecraft_and_no_loads_offers_everything() {
        let mw2 = crate::paths::test_dir("maps-no-loads");
        touch(&mw2.join("zone/english/mp_derail.ff"));
        touch(&mw2.join("zone/english/MP_Estate.FF"));
        let zones: Vec<String> = playable_maps(&mw2).into_iter().map(|m| m.zone).collect();
        assert_eq!(zones, ["mp_derail", "mp_estate"]);
        assert!(playable_maps(&mw2.join("missing")).is_empty());
    }

    #[test]
    fn display_names_use_in_game_names() {
        assert_eq!(display_name("mp_boneyard"), "Scrapyard");
        assert_eq!(display_name("mp_nightshift"), "Skidrow");
        assert_eq!(display_name("mp_checkpoint"), "Karachi");
        assert_eq!(display_name("mp_fuel2"), "Fuel");
        assert_eq!(display_name("mp_trailerpark"), "Trailer Park");
        assert_eq!(display_name("mp_something"), "SOMETHING");
        assert_eq!(display_name(MINECRAFT), "Overworld");
    }

    #[test]
    fn modes_and_thumbs() {
        assert_eq!(mode_label("war"), Some("Team Deathmatch"));
        assert_eq!(mode_label("dm"), Some("FFA"));
        assert_eq!(mode_label("gun"), None);
        let cache = Path::new("/c");
        assert_eq!(
            thumb_path(cache, "mp_rust"),
            Some(PathBuf::from("/c/loadscreen_mp_rust.png"))
        );
        assert_eq!(thumb_path(cache, MINECRAFT), None);
    }

    #[test]
    fn launch_cmds_split_bots_at_sixteen_and_clamp() {
        assert_eq!(launch_cmds(0, false), None);
        assert_eq!(
            launch_cmds(1, false).as_deref(),
            Some("wait world; bot add 1")
        );
        assert_eq!(
            launch_cmds(16, false).as_deref(),
            Some("wait world; bot add 16")
        );
        assert_eq!(
            launch_cmds(17, false).as_deref(),
            Some("wait world; bot add 16; bot add 1")
        );
        assert_eq!(launch_cmds(25, false), launch_cmds(20, false));
        assert_eq!(launch_cmds(25, true), launch_cmds(20, true));
    }

    #[test]
    fn launch_cmds_for_every_bots_and_skate_combo() {
        let cases = [
            (0, false, None),
            (6, false, Some("wait world; bot add 6")),
            (20, false, Some("wait world; bot add 16; bot add 4")),
            (0, true, Some("wait world; spawn 0; wait 2s; skate on")),
            (
                6,
                true,
                Some("wait world; bot add 6; spawn 0; wait 2s; skate on"),
            ),
            (
                20,
                true,
                Some("wait world; bot add 16; bot add 4; spawn 0; wait 2s; skate on"),
            ),
        ];
        for (bots, skate, script) in cases {
            assert_eq!(
                launch_cmds(bots, skate).as_deref(),
                script,
                "{bots} bots, skate {skate}"
            );
        }
    }

    /// `skate on` must come after the spawn and its wait, and be last.
    #[test]
    fn skate_follows_the_spawn() {
        for bots in [0, 6, 20] {
            let script = launch_cmds(bots, true).unwrap();
            let commands: Vec<&str> = script.split("; ").collect();
            let spawn = commands.iter().position(|c| *c == "spawn 0").unwrap();
            assert_eq!(&commands[spawn..], ["spawn 0", "wait 2s", "skate on"]);
            assert!(commands[..spawn].iter().all(|c| !c.starts_with("skate")));
        }
    }

    #[test]
    fn grid_move_wraps_sideways_and_stops_vertically() {
        // 7 cells in rows of 3: 0 1 2 / 3 4 5 / 6
        assert_eq!(grid_move(None, 7, 3, 0, 1), Some(0));
        assert_eq!(grid_move(None, 0, 3, 0, 1), None);
        assert_eq!(grid_move(Some(0), 7, 3, -1, 0), Some(6));
        assert_eq!(grid_move(Some(6), 7, 3, 1, 0), Some(0));
        assert_eq!(grid_move(Some(2), 7, 3, 1, 0), Some(3));
        assert_eq!(grid_move(Some(1), 7, 3, 0, -1), Some(1));
        assert_eq!(grid_move(Some(4), 7, 3, 0, -1), Some(1));
        assert_eq!(grid_move(Some(4), 7, 3, 0, 1), Some(6));
        assert_eq!(grid_move(Some(6), 7, 3, 0, 1), Some(6));
        // A stale focus past the end restarts at the first cell.
        assert_eq!(grid_move(Some(9), 7, 3, 0, 1), Some(0));
    }
}
