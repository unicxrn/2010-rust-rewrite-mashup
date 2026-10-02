//! Finds a Steam copy of MW2 and checks that a folder really is one.
use std::path::{Path, PathBuf};

pub const MW2_FOLDER: &str = "Call of Duty Modern Warfare 2";

/// MW2's multiplayer Steam app id (its `installscript_10190.vdf`), for
/// `steam://install/`.
pub const MW2_APP_ID: &str = "10190";

/// The `steam://install/<id>` URL that has Steam install MW2.
pub fn install_url() -> String {
    format!("steam://install/{MW2_APP_ID}")
}

/// Native, legacy-symlink and Flatpak Steam roots under `home`.
fn steam_roots(home: &Path) -> Vec<PathBuf> {
    [
        ".local/share/Steam",
        ".steam/steam",
        ".steam/root",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
    ]
    .iter()
    .map(|rel| home.join(rel))
    .collect()
}

/// Every `"path" "<dir>"` pair in a `libraryfolders.vdf`.
pub fn parse_library_paths(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|line| {
            let mut fields = line
                .split('"')
                .map(str::trim)
                .filter(|field| !field.is_empty());
            (fields.next() == Some("path"))
                .then(|| fields.next())
                .flatten()
        })
        .map(|path| PathBuf::from(path.replace(r"\\", r"\")))
        .collect()
}

fn libraries(home: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut push = |path: PathBuf| {
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if !found
            .iter()
            .any(|seen| std::fs::canonicalize(seen).unwrap_or_else(|_| seen.clone()) == key)
        {
            found.push(path);
        }
    };
    for root in steam_roots(home) {
        if let Ok(text) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
            parse_library_paths(&text).into_iter().for_each(&mut push);
        }
        push(root);
    }
    found
}

/// MW2 with its multiplayer maps: `main/` plus `zone/<language>/mp_rust.ff`.
pub fn is_mw2(path: &Path) -> bool {
    if !path.join("main").is_dir() {
        return false;
    }
    std::fs::read_dir(path.join("zone"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.path().join("mp_rust.ff").is_file())
}

pub fn find_mw2(home: &Path) -> Option<PathBuf> {
    libraries(home)
        .into_iter()
        .map(|library| library.join("steamapps/common").join(MW2_FOLDER))
        .find(|path| is_mw2(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VDF: &str = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/u/.local/share/Steam"
		"apps" { "10190" "1" }
	}
	"1"
	{
		"path"		"/mnt/Games Drive/SteamLibrary"
	}
}"#;

    #[test]
    fn parses_every_library_path() {
        assert_eq!(
            parse_library_paths(VDF),
            vec![
                PathBuf::from("/home/u/.local/share/Steam"),
                PathBuf::from("/mnt/Games Drive/SteamLibrary")
            ]
        );
    }

    #[test]
    fn unescapes_backslashes() {
        assert_eq!(
            parse_library_paths(r#""path" "D:\\Steam""#),
            vec![PathBuf::from(r"D:\Steam")]
        );
    }

    #[test]
    fn install_url_targets_mw2s_multiplayer_app_id() {
        assert_eq!(install_url(), "steam://install/10190");
    }

    #[test]
    fn mw2_needs_main_and_a_zone_language_with_mp_rust() {
        let root = crate::paths::test_dir("mw2");
        assert!(!is_mw2(&root));
        std::fs::create_dir_all(root.join("main")).unwrap();
        std::fs::create_dir_all(root.join("zone/english")).unwrap();
        assert!(!is_mw2(&root));
        std::fs::write(root.join("zone/english/mp_rust.ff"), b"IWff").unwrap();
        assert!(is_mw2(&root));
    }

    #[test]
    fn finds_mw2_in_a_secondary_library() {
        let home = crate::paths::test_dir("home");
        let steam = home.join(".local/share/Steam");
        let library = home.join("drive/SteamLibrary");
        std::fs::create_dir_all(steam.join("steamapps")).unwrap();
        std::fs::write(
            steam.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"path\" \"{}\"\n\"path\" \"{}\"",
                steam.display(),
                library.display()
            ),
        )
        .unwrap();
        let mw2 = library.join("steamapps/common").join(MW2_FOLDER);
        std::fs::create_dir_all(mw2.join("main")).unwrap();
        std::fs::create_dir_all(mw2.join("zone/english")).unwrap();
        std::fs::write(mw2.join("zone/english/mp_rust.ff"), b"IWff").unwrap();
        assert_eq!(find_mw2(&home), Some(mw2));
    }
}
