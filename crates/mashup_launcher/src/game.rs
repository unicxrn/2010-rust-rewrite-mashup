//! Starting the game and reading what it left behind.
use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::paths::Paths;

/// Environment variable carrying a Quick Play game type to the game.
const GAMETYPE: &str = "IW4L_GAMETYPE";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    Menu,
    Minecraft,
    /// Quick Play: straight into `zone` with a game type and bots, and
    /// with `skate`, spawned and on a board.
    Map {
        zone: String,
        gametype: String,
        bots: u8,
        skate: bool,
    },
}

/// The `iw4l` arguments for a launch, and the environment it sets on top
/// of the common one.
pub fn command_parts(launch: &Launch) -> (Vec<String>, Vec<(String, String)>) {
    match launch {
        Launch::Menu => (vec!["menu".to_owned()], Vec::new()),
        Launch::Minecraft => (
            vec!["map".to_owned(), crate::maps::MINECRAFT.to_owned()],
            Vec::new(),
        ),
        Launch::Map {
            zone,
            gametype,
            bots,
            skate,
        } => {
            let mut args = vec!["map".to_owned(), zone.clone()];
            if let Some(script) = crate::maps::launch_cmds(*bots, *skate) {
                args.extend(["--cmds".to_owned(), script]);
            }
            (args, vec![(GAMETYPE.to_owned(), gametype.clone())])
        }
    }
}

pub fn spawn(paths: &Paths, launch: &Launch, skate_ready: bool) -> Result<Child, String> {
    std::fs::create_dir_all(paths.logs()).map_err(|e| e.to_string())?;
    let stderr = std::fs::File::create(paths.data.join("iw4l-artifacts/launcher-stderr.log"))
        .map_err(|e| e.to_string())?;
    let (args, env) = command_parts(launch);
    let mut command = Command::new(paths.tool("iw4l"));
    command
        .args(args)
        .current_dir(&paths.data)
        .env("IW4L_GAMES", paths.games())
        .env_remove("IW4L_SKATE_ASSETS")
        // Only a Quick Play launch picks the game type; never inherit one.
        .env_remove(GAMETYPE)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr);
    if skate_ready {
        command.env("IW4L_SKATE_ASSETS", paths.skate_assets());
    }
    command
        .spawn()
        .map_err(|e| format!("Couldn't start the game: {e}"))
}

/// The last `lines` lines of a text file, or "" when it can't be read.
pub fn tail(path: &Path, lines: usize) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// What to show when the game quit with an error.
pub fn crash_report(paths: &Paths) -> String {
    let log = tail(&paths.logs().join("latest.log"), 20);
    let stderr = tail(&paths.data.join("iw4l-artifacts/launcher-stderr.log"), 10);
    [log, stderr]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_parts_per_launch() {
        let strings = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(command_parts(&Launch::Menu), (strings(&["menu"]), vec![]));
        assert_eq!(
            command_parts(&Launch::Minecraft),
            (strings(&["map", "minecraft:overworld"]), vec![])
        );
        let quick = |bots, skate| Launch::Map {
            zone: "mp_rust".into(),
            gametype: "sd".into(),
            bots,
            skate,
        };
        let env = vec![("IW4L_GAMETYPE".to_owned(), "sd".to_owned())];
        assert_eq!(
            command_parts(&quick(0, false)),
            (strings(&["map", "mp_rust"]), env.clone())
        );
        assert_eq!(
            command_parts(&quick(0, true)),
            (
                strings(&[
                    "map",
                    "mp_rust",
                    "--cmds",
                    "wait world; spawn 0; wait 2s; skate on"
                ]),
                env.clone()
            )
        );
        assert_eq!(
            command_parts(&quick(18, false)),
            (
                strings(&[
                    "map",
                    "mp_rust",
                    "--cmds",
                    "wait world; bot add 16; bot add 2"
                ]),
                env
            )
        );
    }

    #[test]
    fn tail_keeps_last_lines() {
        let dir = crate::paths::test_dir("tail");
        let file = dir.join("log");
        std::fs::write(
            &file,
            (1..=30).map(|n| format!("line {n}\n")).collect::<String>(),
        )
        .unwrap();
        let tail = tail(&file, 3);
        assert_eq!(tail, "line 28\nline 29\nline 30");
        assert_eq!(super::tail(&dir.join("missing"), 3), "");
    }
}
