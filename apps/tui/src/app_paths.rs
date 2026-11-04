use std::path::PathBuf;

pub const APP_NAME: &str = "amp808";
pub const LEGACY_APP_NAME: &str = "cliamp";
pub const INTERIM_APP_NAME: &str = "amp808-tui";
pub const BINARY_NAME: &str = APP_NAME;
pub const NAVIDROME_CLIENT_NAME: &str = APP_NAME;
pub const YTDL_TEMP_PREFIX: &str = "amp808-ytdl";

fn collect_unique_paths(paths: impl IntoIterator<Item = Option<PathBuf>>) -> Vec<PathBuf> {
    let mut unique = Vec::new();
    for path in paths.into_iter().flatten() {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    unique
}

fn prefer_existing_dir(primary: PathBuf, fallbacks: &[PathBuf]) -> PathBuf {
    if primary.exists() {
        return primary;
    }

    for candidate in fallbacks {
        if candidate.exists() {
            return candidate.clone();
        }
    }

    primary
}

fn config_dir_for(app_name: &str) -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config").join(app_name))
}

pub fn config_dir() -> Option<PathBuf> {
    config_dir_for(APP_NAME)
}

pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("config.toml"))
}

pub fn legacy_config_file() -> Option<PathBuf> {
    config_dir_for(LEGACY_APP_NAME).map(|dir| dir.join("config.toml"))
}

pub fn interim_config_file() -> Option<PathBuf> {
    config_dir_for(INTERIM_APP_NAME).map(|dir| dir.join("config.toml"))
}

pub fn config_search_paths() -> Vec<PathBuf> {
    // Search order matters: prefer the new app name, then the interim rename,
    // then the original cliamp path.
    collect_unique_paths([config_file(), interim_config_file(), legacy_config_file()])
}

pub fn theme_search_dirs() -> Vec<PathBuf> {
    // Load legacy themes first so the current app path can override them.
    collect_unique_paths([
        config_dir_for(LEGACY_APP_NAME).map(|dir| dir.join("themes")),
        config_dir_for(INTERIM_APP_NAME).map(|dir| dir.join("themes")),
        config_dir().map(|dir| dir.join("themes")),
    ])
}

pub fn preferred_music_save_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let new_dir = home.join("Music").join(APP_NAME);
    let interim_dir = home.join("Music").join(INTERIM_APP_NAME);
    let legacy_dir = home.join("Music").join(LEGACY_APP_NAME);

    Some(prefer_existing_dir(new_dir, &[interim_dir, legacy_dir]))
}

pub fn display_path(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(relative) = path.strip_prefix(&home)
    {
        return format!("~/{}", relative.display());
    }

    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::{collect_unique_paths, prefer_existing_dir};

    #[test]
    fn collect_unique_paths_preserves_order() {
        let base = std::env::temp_dir();
        let first = base.join("amp808-app-paths-first");
        let second = base.join("amp808-app-paths-second");

        let paths = collect_unique_paths([
            Some(first.clone()),
            Some(second.clone()),
            Some(first.clone()),
            None,
        ]);

        assert_eq!(paths, vec![first, second]);
    }

    #[test]
    fn prefer_existing_dir_uses_fallback_when_primary_missing() {
        let root = std::env::temp_dir().join(format!("amp808-app-paths-{}", std::process::id()));
        let primary = root.join("primary");
        let fallback = root.join("fallback");
        std::fs::create_dir_all(&fallback).unwrap();

        let selected = prefer_existing_dir(primary.clone(), std::slice::from_ref(&fallback));
        assert_eq!(selected, fallback);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn prefer_existing_dir_keeps_primary_when_present() {
        let root =
            std::env::temp_dir().join(format!("amp808-app-paths-primary-{}", std::process::id()));
        let primary = root.join("primary");
        let fallback = root.join("fallback");
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&fallback).unwrap();

        let selected = prefer_existing_dir(primary.clone(), &[fallback]);
        assert_eq!(selected, primary);

        std::fs::remove_dir_all(&root).unwrap();
    }
}
