//! The game scans its games root recursively. Point it at symlinks to MW2's
//! `zone` and `main` only, so a Wine/Proton prefix inside the MW2 folder
//! (`dosdevices/z: -> /`) is never walked.
use std::path::Path;

pub fn link(games: &Path, mw2: &Path) -> Result<(), String> {
    let dir = games.join("mw2");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    for name in ["zone", "main"] {
        let link = dir.join(name);
        if link.symlink_metadata().is_ok() {
            std::fs::remove_file(&link)
                .map_err(|e| format!("cannot replace {}: {e}", link.display()))?;
        }
        std::os::unix::fs::symlink(mw2.join(name), &link)
            .map_err(|e| format!("cannot link {}: {e}", link.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_only_zone_and_main_and_relinks() {
        let dir = crate::paths::test_dir("games-root");
        let (a, b) = (dir.join("a"), dir.join("b"));
        for mw2 in [&a, &b] {
            std::fs::create_dir_all(mw2.join("zone")).unwrap();
            std::fs::create_dir_all(mw2.join("main")).unwrap();
            std::fs::create_dir_all(mw2.join("dosdevices")).unwrap();
        }
        let games = dir.join("games");
        link(&games, &a).unwrap();
        link(&games, &b).unwrap();
        assert_eq!(
            std::fs::read_link(games.join("mw2/zone")).unwrap(),
            b.join("zone")
        );
        assert_eq!(
            std::fs::read_link(games.join("mw2/main")).unwrap(),
            b.join("main")
        );
        assert!(!games.join("mw2/dosdevices").exists());
    }
}
