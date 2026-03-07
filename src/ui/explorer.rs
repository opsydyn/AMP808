use std::path::{Path, PathBuf};

use anyhow::Context;
use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui_explorer::{File, FileExplorer, FileExplorerBuilder, Theme};

use super::App;
use super::keys::Focus;

pub enum ExplorerAction {
    Continue,
    Cancel,
    Selected(PathBuf),
}

pub struct PlaylistExplorer {
    inner: FileExplorer,
}

impl PlaylistExplorer {
    pub fn new(last_playlist_file: Option<&Path>) -> anyhow::Result<Self> {
        let start_dir = default_playlist_dir();
        Self::from_paths(Some(start_dir.as_path()), last_playlist_file)
    }

    fn from_paths(
        start_dir: Option<&Path>,
        last_playlist_file: Option<&Path>,
    ) -> anyhow::Result<Self> {
        let mut builder = FileExplorerBuilder::default()
            .show_hidden(false)
            .theme(Theme::default())
            .filter_map(filter_playlist_entry);

        if let Some(path) = last_playlist_file.filter(|path| path.exists()) {
            builder = builder.working_file(path);
        } else if let Some(path) = start_dir.filter(|path| path.exists()) {
            builder = builder.working_dir(path);
        }

        let inner = builder
            .build()
            .context("failed to create playlist explorer")?;

        Ok(Self { inner })
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> anyhow::Result<ExplorerAction> {
        match key.code {
            KeyCode::Esc => return Ok(ExplorerAction::Cancel),
            KeyCode::Enter => {
                let current = self.inner.current().clone();
                if current.is_dir {
                    self.inner
                        .set_cwd(current.path)
                        .context("failed to enter directory")?;
                    return Ok(ExplorerAction::Continue);
                }
                if is_supported_playlist_path(&current.path) {
                    return Ok(ExplorerAction::Selected(current.path));
                }
                return Ok(ExplorerAction::Continue);
            }
            _ => {}
        }

        self.inner
            .handle(&Event::Key(key))
            .context("failed to handle explorer input")?;
        Ok(ExplorerAction::Continue)
    }

    pub fn widget(&self) -> impl ratatui::widgets::WidgetRef + '_ {
        self.inner.widget()
    }

    pub fn cwd(&self) -> &Path {
        self.inner.cwd().as_path()
    }

    pub fn show_hidden(&self) -> bool {
        self.inner.show_hidden()
    }

    pub fn header_text(&self) -> String {
        format!(
            "{}  [Enter]Open/Load [←/Backspace]Parent [Esc]Back [Ctrl+H]Hidden:{}",
            self.cwd().display(),
            if self.show_hidden() { "On" } else { "Off" }
        )
    }
}

impl App {
    pub fn toggle_playlist_browser(&mut self) {
        if !self.player.supports_local_playlist() {
            self.err = Some("music-app backend does not support local playlist browsing".into());
            return;
        }

        if self.focus == Focus::Browser {
            self.focus = Focus::Playlist;
            return;
        }

        if self.explorer.is_none() {
            match PlaylistExplorer::new(self.last_playlist_file.as_deref()) {
                Ok(explorer) => {
                    self.explorer = Some(explorer);
                }
                Err(err) => {
                    self.err = Some(format!("browser failed: {err}"));
                    return;
                }
            }
        }

        self.command_mode = false;
        self.show_themes = false;
        self.focus = Focus::Browser;
        self.err = None;
    }

    pub fn handle_browser_key(&mut self, key: KeyEvent) {
        let action = match self.explorer.as_mut() {
            Some(explorer) => explorer.handle_key(key),
            None => {
                self.focus = Focus::Playlist;
                return;
            }
        };

        match action {
            Ok(ExplorerAction::Continue) => {}
            Ok(ExplorerAction::Cancel) => {
                self.focus = Focus::Playlist;
            }
            Ok(ExplorerAction::Selected(path)) => {
                self.focus = Focus::Playlist;
                self.load_playlist_from_path(path);
            }
            Err(err) => {
                self.focus = Focus::Playlist;
                self.err = Some(format!("browser failed: {err}"));
            }
        }
    }
}

fn default_playlist_dir() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        let music = home.join("Music");
        if music.exists() {
            return music;
        }
        return home;
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn filter_playlist_entry(file: File) -> Option<File> {
    if is_explorer_visible_path(&file.path) {
        Some(file)
    } else {
        None
    }
}

fn is_explorer_visible_path(path: &Path) -> bool {
    if is_hidden_path(path) {
        return false;
    }

    if path.is_dir() {
        return true;
    }

    is_supported_playlist_path(path)
}

fn is_supported_playlist_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "m3u" | "m3u8"))
}

fn is_hidden_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn filters_to_directories_and_playlist_files() {
        let tmp = std::env::temp_dir().join("cliamp-test-explorer-filter");
        let playlists = tmp.join("playlists");
        std::fs::create_dir_all(&playlists).unwrap();
        std::fs::write(tmp.join("set.m3u"), "").unwrap();
        std::fs::write(tmp.join("set.m3u8"), "").unwrap();
        std::fs::write(tmp.join("track.mp3"), "").unwrap();
        std::fs::write(tmp.join("notes.txt"), "").unwrap();
        std::fs::write(tmp.join(".hidden.m3u"), "").unwrap();

        assert!(is_explorer_visible_path(&playlists));
        assert!(is_explorer_visible_path(&tmp.join("set.m3u")));
        assert!(is_explorer_visible_path(&tmp.join("set.m3u8")));
        assert!(!is_explorer_visible_path(&tmp.join("track.mp3")));
        assert!(!is_explorer_visible_path(&tmp.join("notes.txt")));
        assert!(!is_explorer_visible_path(&tmp.join(".hidden.m3u")));

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn explorer_lists_only_directories_and_playlists() {
        let tmp = std::env::temp_dir().join("cliamp-test-explorer-list");
        let playlists = tmp.join("playlists");
        std::fs::create_dir_all(&playlists).unwrap();
        std::fs::write(tmp.join("set.m3u"), "").unwrap();
        std::fs::write(tmp.join("track.mp3"), "").unwrap();

        let explorer = PlaylistExplorer::from_paths(Some(&tmp), None).unwrap();
        let names: Vec<_> = explorer
            .inner
            .files()
            .as_slice()
            .iter()
            .map(|file| file.name.as_str())
            .collect();

        assert!(names.contains(&"playlists/"));
        assert!(names.contains(&"set.m3u"));
        assert!(!names.contains(&"track.mp3"));

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn escape_cancels_browser() {
        let tmp = std::env::temp_dir().join("cliamp-test-explorer-cancel");
        std::fs::create_dir_all(&tmp).unwrap();

        let mut explorer = PlaylistExplorer::from_paths(Some(&tmp), None).unwrap();
        let action = explorer
            .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();

        match action {
            ExplorerAction::Cancel => {}
            ExplorerAction::Continue | ExplorerAction::Selected(_) => {
                panic!("expected explorer cancel")
            }
        }

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn enter_on_playlist_file_selects_it() {
        let tmp = std::env::temp_dir().join("cliamp-test-explorer-select");
        std::fs::create_dir_all(&tmp).unwrap();
        let playlist = tmp.join("set.m3u");
        std::fs::write(&playlist, "").unwrap();

        let mut explorer = PlaylistExplorer::from_paths(Some(&tmp), Some(&playlist)).unwrap();
        let action = explorer
            .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        match action {
            ExplorerAction::Selected(path) => assert_eq!(path, playlist),
            ExplorerAction::Continue | ExplorerAction::Cancel => {
                panic!("expected playlist selection")
            }
        }

        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
