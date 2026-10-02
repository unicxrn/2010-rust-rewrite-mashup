//! Where the launcher keeps per-user data and finds its bundled tools.
use std::path::{Path, PathBuf};

pub const APP_ID: &str = "2010-rust-rewrite-mashup";

#[derive(Clone, Debug)]
pub struct Paths {
    /// `$XDG_DATA_HOME/2010-rust-rewrite-mashup`; the game's working directory.
    pub data: PathBuf,
    /// Folder holding `iw4l`, `skate-convert` and `extract-xiso`.
    pub bin: PathBuf,
}

impl Paths {
    pub fn detect() -> Self {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home().join(".local/share"))
            .join(APP_ID);
        let bin = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        Self { data, bin }
    }

    pub fn config(&self) -> PathBuf {
        self.data.join("config.json")
    }
    pub fn games(&self) -> PathBuf {
        self.data.join("games")
    }
    pub fn skate_data(&self) -> PathBuf {
        self.data.join("skate-data")
    }
    pub fn skate_assets(&self) -> PathBuf {
        self.skate_data().join("assets")
    }
    pub fn logs(&self) -> PathBuf {
        self.data.join("iw4l-artifacts").join("logs")
    }
    pub fn backdrops(&self) -> PathBuf {
        self.data.join("cache").join("backdrops")
    }
    pub fn tmp(&self) -> PathBuf {
        self.data.join("tmp")
    }
    pub fn tool(&self, name: &str) -> PathBuf {
        self.bin.join(name)
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// A fresh, empty scratch folder for one test.
#[cfg(test)]
pub fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mashup-launcher-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
