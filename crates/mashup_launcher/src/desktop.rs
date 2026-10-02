//! The "Add to app menu" entry and its icon.
use std::path::{Path, PathBuf};

use crate::paths::APP_ID;

pub const ICON_PNG: &[u8] = include_bytes!("../../../packaging/linux/icon-256.png");

/// Escapes a path for use inside the double-quoted `Exec=` value of a
/// Desktop Entry, per the spec: `"`, backtick, `$` and `\` are
/// backslash-escaped, then every backslash in the result is doubled again
/// (the value is itself a quoted string), and finally `%` becomes `%%`.
pub fn escape_exec(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut step1 = String::with_capacity(raw.len());
    for c in raw.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            step1.push('\\');
        }
        step1.push(c);
    }
    let mut step2 = String::with_capacity(step1.len() * 2);
    for c in step1.chars() {
        if c == '\\' {
            step2.push('\\');
        }
        step2.push(c);
    }
    step2.replace('%', "%%")
}

pub fn entry(exec: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=2010 Rust Rewrite Mashup\n\
         Comment=MW2, Skate 3 and Minecraft in one Rust game\nExec=\"{}\"\nIcon={APP_ID}\n\
         Terminal=false\nCategories=Game;ActionGame;\nStartupWMClass={APP_ID}\n",
        escape_exec(exec)
    )
}

fn desktop_file(home: &Path) -> PathBuf {
    home.join(".local/share/applications")
        .join(format!("{APP_ID}.desktop"))
}

fn icon_file(home: &Path) -> PathBuf {
    home.join(".local/share/icons/hicolor/256x256/apps")
        .join(format!("{APP_ID}.png"))
}

/// The AppImage itself when running from one, else this executable; `None`
/// when neither is available.
pub fn launcher_exec() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
}

pub fn install(home: &Path, exec: &Path) -> Result<(), String> {
    // A newline would end the `Exec=` line and start a new key.
    if exec.to_string_lossy().chars().any(char::is_control) {
        return Err(format!(
            "Couldn't add to the app menu: the launcher's path {:?} has a control character in it.",
            exec
        ));
    }
    for (path, bytes) in [
        (desktop_file(home), entry(exec).into_bytes()),
        (icon_file(home), ICON_PNG.to_vec()),
    ] {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        std::fs::write(&path, bytes)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    Ok(())
}

pub fn uninstall(home: &Path) {
    let _ = std::fs::remove_file(desktop_file(home));
    let _ = std::fs::remove_file(icon_file(home));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_quotes_exec_and_names_icon() {
        let text = entry(Path::new("/opt/My Apps/Mashup.AppImage"));
        assert!(text.contains("Exec=\"/opt/My Apps/Mashup.AppImage\"\n"));
        assert!(text.contains("Icon=2010-rust-rewrite-mashup\n"));
        assert!(text.starts_with("[Desktop Entry]\n"));
    }

    #[test]
    fn escape_exec_handles_quotes_backslashes_dollar_backtick_and_percent() {
        let raw = r#"/a/100% $tuff/"q"/b\c`"#;
        let expected = r#"/a/100%% \\$tuff/\\"q\\"/b\\\\c\\`"#;
        assert_eq!(escape_exec(Path::new(raw)), expected);
    }

    #[test]
    fn install_refuses_control_characters_in_the_path() {
        let home = crate::paths::test_dir("desktop-control");
        assert!(install(&home, Path::new("/x/app\nTerminal=true")).is_err());
        assert!(install(&home, Path::new("/x/a\tb")).is_err());
        assert!(!home.join(".local/share/applications").exists());
    }

    #[test]
    fn install_then_uninstall() {
        let home = crate::paths::test_dir("desktop");
        install(&home, Path::new("/x/app")).unwrap();
        assert!(
            home.join(".local/share/applications/2010-rust-rewrite-mashup.desktop")
                .is_file()
        );
        assert!(
            home.join(".local/share/icons/hicolor/256x256/apps/2010-rust-rewrite-mashup.png")
                .is_file()
        );
        uninstall(&home);
        assert!(
            !home
                .join(".local/share/applications/2010-rust-rewrite-mashup.desktop")
                .exists()
        );
    }
}
