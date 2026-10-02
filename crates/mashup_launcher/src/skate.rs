//! Skate 3 setup on a worker thread: extract an ISO if given one, run the
//! converter, stream its progress back to the window.
use std::io::{BufRead, BufReader, Read};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};

use crate::paths::Paths;

pub const REQUIRED: [&str; 4] = [
    "private/skater.glb",
    "private/game.json",
    "private/stock/physics-skeletons.json",
    "private/stock/skater-collections.json",
];

/// Shown under the progress bar, indexed by `Msg::Stage`.
pub const STAGES: [&str; 6] = [
    "Extracting disc image",
    "Reading animation banks",
    "Converting physics",
    "Preparing skater textures",
    "Building skater rig",
    "Ready",
];

/// Free space an ISO extraction needs in the data folder.
const ISO_SPACE: u64 = 7_500_000_000;

pub fn ready(assets: &Path) -> bool {
    REQUIRED.iter().all(|file| assets.join(file).is_file())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Iso(PathBuf),
    Xex(PathBuf),
}

impl Source {
    pub fn classify(path: &Path) -> Result<Self, String> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name.ends_with(".iso") {
            Ok(Self::Iso(path.to_owned()))
        } else if name == "default.xex" {
            Ok(Self::Xex(path.to_owned()))
        } else {
            Err("Pick your Skate 3 .iso, or default.xex from an extracted copy.".into())
        }
    }
}

pub enum Msg {
    Line(String),
    Stage(usize),
    Done(Result<(), String>),
}

pub fn stage_of(line: &str) -> Option<usize> {
    [
        ("Extracting animation banks", 1),
        ("Converting physics", 2),
        ("Preparing the skater model", 3),
        ("Building the skater model", 4),
        ("Skate 3 data ready", 5),
    ]
    .iter()
    .find(|(prefix, _)| line.starts_with(prefix))
    .map(|(_, stage)| *stage)
}

pub fn start(paths: Paths, source: Source) -> Receiver<Msg> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let tmp = paths.tmp().join("skate3");
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| run(&paths, &source, &tmp, &tx)))
            .unwrap_or_else(|_| Err("Skate 3 setup crashed unexpectedly.".to_owned()));
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = tx.send(Msg::Done(result));
    });
    rx
}

fn run(paths: &Paths, source: &Source, tmp: &Path, tx: &Sender<Msg>) -> Result<(), String> {
    let xex = match source {
        Source::Xex(xex) => {
            if !xex.with_file_name("data").is_dir() {
                return Err("The data folder must sit next to default.xex.".into());
            }
            xex.clone()
        }
        Source::Iso(iso) => {
            let _ = tx.send(Msg::Stage(0));
            std::fs::create_dir_all(&paths.data).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_dir_all(tmp);
            if let Some(free) = free_bytes(&paths.data)
                && free < ISO_SPACE
            {
                return Err(format!(
                    "Extracting the ISO needs about 7.5 GB free in {}; {:.1} GB is free.",
                    paths.data.display(),
                    free as f64 / 1e9
                ));
            }
            std::fs::create_dir_all(tmp).map_err(|e| e.to_string())?;
            let mut command = Command::new(paths.tool("extract-xiso"));
            command.arg("-x").arg(iso).arg("-d").arg(tmp);
            stream(command, tx, |_error_line, last_line| {
                format!(
                    "Couldn't extract the ISO: {}",
                    last_line.unwrap_or_else(|| "it stopped unexpectedly.".to_owned())
                )
            })?;
            let xex = tmp.join("default.xex");
            if !xex.is_file() {
                return Err("That ISO doesn't contain a Skate 3 default.xex.".into());
            }
            xex
        }
    };
    let mut command = Command::new(paths.tool("skate-convert"));
    command
        .arg("--xex")
        .arg(&xex)
        .arg("--out")
        .arg(paths.skate_data());
    stream(command, tx, |error_line, last_line| {
        error_line.unwrap_or_else(|| {
            format!(
                "The Skate 3 converter stopped: {}",
                last_line.unwrap_or_else(|| "see the log for details.".to_owned())
            )
        })
    })?;
    if ready(&paths.skate_assets()) {
        Ok(())
    } else {
        Err("The conversion finished but some Skate 3 files are missing.".into())
    }
}

/// The file name `command` runs, for messages that shouldn't show a full
/// (debug-quoted) path.
fn tool_name(command: &Command) -> String {
    Path::new(command.get_program())
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("tool")
        .to_owned()
}

/// `extract-xiso` overwrites its progress in place with `\r`, only emitting a
/// final `\n` at the very end; keep just the text after the last `\r` in a
/// chunk, with any trailing newline trimmed. Invalid UTF-8 is replaced rather
/// than rejected, so one bad chunk never stops the rest of the output.
pub fn clean_line(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_suffix('\n').unwrap_or(&text);
    let text = text.strip_suffix('\r').unwrap_or(text);
    text.split('\r')
        .rfind(|segment| !segment.is_empty())
        .map(str::to_owned)
}

/// Lines from `reader`, split the same way `clean_line` cleans a chunk.
fn lines_from<R: Read>(reader: R) -> impl Iterator<Item = String> {
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    std::iter::from_fn(move || {
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => return None,
                Ok(_) => {
                    if let Some(line) = clean_line(&buf) {
                        return Some(line);
                    }
                }
            }
        }
    })
}

/// Runs `command`, forwarding stdout lines as `Msg::Line`/`Msg::Stage`. On a
/// non-zero exit, `on_fail` builds the error message from the converter's
/// `ERROR:` line (if any) and the last non-empty line seen on stdout or
/// stderr (preferring stderr).
fn stream(
    mut command: Command,
    tx: &Sender<Msg>,
    on_fail: impl FnOnce(Option<String>, Option<String>) -> String,
) -> Result<(), String> {
    let program = tool_name(&command);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't start {program}: {e}"))?;

    let stderr_thread = child
        .stderr
        .take()
        .map(|stderr| std::thread::spawn(move || lines_from(stderr).collect::<Vec<_>>()));

    let mut error_line = None;
    let mut last_stdout = None;
    if let Some(stdout) = child.stdout.take() {
        for line in lines_from(stdout) {
            if let Some(stage) = stage_of(&line) {
                let _ = tx.send(Msg::Stage(stage));
            }
            if let Some(message) = line.strip_prefix("ERROR: ") {
                error_line = Some(message.to_owned());
            }
            last_stdout = Some(line.clone());
            let _ = tx.send(Msg::Line(line));
        }
    }

    let stderr_lines = stderr_thread
        .and_then(|thread| thread.join().ok())
        .unwrap_or_else(Vec::new);
    let last_stderr = stderr_lines.into_iter().next_back();

    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(on_fail(error_line, last_stderr.or(last_stdout)))
    }
}

/// Free bytes on the filesystem holding `dir`, via coreutils `df`.
fn free_bytes(dir: &Path) -> Option<u64> {
    let output = Command::new("df")
        .args(["--output=avail", "-B1"])
        .arg(dir)
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .nth(1)?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_sources() {
        assert_eq!(
            Source::classify(Path::new("/x/Skate 3.ISO")),
            Ok(Source::Iso("/x/Skate 3.ISO".into()))
        );
        assert_eq!(
            Source::classify(Path::new("/x/default.xex")),
            Ok(Source::Xex("/x/default.xex".into()))
        );
        assert!(Source::classify(Path::new("/x/game.7z")).is_err());
    }

    #[test]
    fn maps_converter_lines_to_stages() {
        assert_eq!(
            stage_of("Extracting animation banks, graphs and gameplay inputs"),
            Some(1)
        );
        assert_eq!(stage_of("Building the skater model and rig"), Some(4));
        assert_eq!(stage_of("Skate 3 data ready"), Some(5));
        assert_eq!(stage_of("[ABIN] something"), None);
    }

    #[test]
    fn ready_needs_all_four_files() {
        let dir = crate::paths::test_dir("skate-ready");
        assert!(!ready(&dir));
        for file in REQUIRED {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }
        assert!(ready(&dir));
    }

    #[test]
    fn clean_line_keeps_last_carriage_return_segment() {
        assert_eq!(
            clean_line(b"extracting a (5) [10%]\rextracting a (5) [100%]\r\n"),
            Some("extracting a (5) [100%]".to_owned())
        );
    }

    #[test]
    fn clean_line_tolerates_invalid_utf8() {
        let cleaned =
            clean_line(b"before\xff\xfeafter\n").expect("lossy decode still yields a line");
        assert!(cleaned.starts_with("before"));
        assert!(cleaned.ends_with("after"));
    }

    #[test]
    fn stream_prefers_the_converters_error_line() {
        let (tx, _rx) = channel();
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("echo ERROR: boom; echo trailing 1>&2; exit 1");
        let result = stream(command, &tx, |error_line, last_line| {
            error_line.unwrap_or_else(|| {
                format!("stopped: {}", last_line.unwrap_or_else(|| "?".to_owned()))
            })
        });
        assert_eq!(result, Err("boom".to_owned()));
    }

    #[test]
    fn stream_falls_back_to_the_last_stderr_line() {
        let (tx, _rx) = channel();
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("echo first 1>&2; echo oops 1>&2; exit 1");
        let result = stream(command, &tx, |error_line, last_line| {
            error_line.unwrap_or_else(|| {
                format!("stopped: {}", last_line.unwrap_or_else(|| "?".to_owned()))
            })
        });
        assert_eq!(result, Err("stopped: oops".to_owned()));
    }

    #[test]
    fn stream_spawn_failure_names_just_the_tool() {
        let (tx, _rx) = channel();
        let command = Command::new("/does/not/exist/definitely-missing-tool");
        let result = stream(command, &tx, |_, _| "unreached".to_owned());
        assert_eq!(
            result,
            Err(
                "Couldn't start definitely-missing-tool: No such file or directory (os error 2)"
                    .to_owned()
            )
        );
    }
}
