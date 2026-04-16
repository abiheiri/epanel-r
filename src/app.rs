//! Application state and logic for epanel.
//!
//! A TUI panel for managing links and notes using a JSON data model.
//!
//! Author: Al Biheiri <al@forgottheaddress.com>

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
#[cfg(target_os = "macos")]
use std::time::SystemTime;

use anyhow::Result;
use chrono::{DateTime, Utc};
use crossterm::event::{KeyCode, KeyModifiers};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const VERSION: &str = match option_env!("EPANEL_VERSION") {
    Some(v) => v,
    None => "v0.0.1",
};

const ROOT_FOLDER_ID: Uuid = Uuid::from_u128(0);

// ---------------------------------------------------------------------------
// Data models (mirror the Swift epanel JSON schema)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub id: Uuid,
    pub text: String,
    pub date: DateTime<Utc>,
}

impl Entry {
    pub fn new(text: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            text,
            date: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Folder {
    pub id: Uuid,
    pub name: String,
    pub entries: Vec<Entry>,
    pub subfolders: Vec<Folder>,
    #[serde(rename = "isCollapsed")]
    pub is_collapsed: bool,
}

impl Folder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            entries: vec![],
            subfolders: vec![],
            is_collapsed: false,
        }
    }


}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EPanelData {
    #[serde(rename = "rootFolder")]
    pub root_folder: Folder,
    pub notes: String,
}

impl EPanelData {
    pub fn empty() -> Self {
        Self {
            root_folder: Folder {
                id: ROOT_FOLDER_ID,
                name: "/".to_string(),
                entries: vec![],
                subfolders: vec![],
                is_collapsed: false,
            },
            notes: String::new(),
        }
    }
}

/// Rusts 'fs' functions does not expand '~' to the users home dir;
/// this helper makes sure paths typed in the TUI settings resolve.
pub(crate) fn expand_tilde(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if let Some(comp) = path.components().next() {
        if let std::path::Component::Normal(os) = comp {
            if os == "~" {
                if let Some(home) = dirs::home_dir() {
                    return home.join(path.strip_prefix("~").unwrap_or(path));
                }
            }
        }
    }
    path.to_path_buf()
}

// ---------------------------------------------------------------------------
// UI state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentTab {
    Links,
    Notes,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    SearchInput,
    LinksList,
    NotesText,
    SettingsLinksPath,
    SettingsNotesPath,
    #[cfg(target_os = "macos")]
    SettingsSafariSync,
    SettingsSaveButton,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Popup {
    AddEntry { text: String, selected_folder: Uuid },
    NewFolder { name: String, selected_parent: Uuid },
    RenameFolder { folder_id: Uuid, name: String },
    MoveItem { item_id: Uuid, selected_folder: Uuid, is_folder: bool },
    ConfirmDeleteSelected { entry_count: usize, folder_count: usize, subfolder_count: usize },
    Alert { message: String },
    Help,
    ExportJSON { path: String },
    ImportJSON { path: String },
    ConfirmImportJSON { path: String },
}

#[derive(Debug, Clone)]
pub enum FlatItemKind {
    Folder,
    Entry,
}

#[derive(Debug, Clone)]
pub struct FlatItem {
    pub id: Uuid,
    pub kind: FlatItemKind,
    pub depth: usize,
    pub name: String,
    pub is_collapsed: bool,
}

pub struct App {
    pub current_tab: CurrentTab,
    pub focus: Focus,
    pub data: EPanelData,
    pub search_input: String,
    pub search_expanded_folders: HashSet<Uuid>,
    pub selected_item_ids: HashSet<Uuid>,
    pub links_cursor: Option<usize>,
    pub flat_items: Vec<FlatItem>,
    pub notes_text: String,
    pub notes_cursor: (usize, usize),
    pub settings_links_path: String,
    pub settings_notes_path: String,
    #[cfg(target_os = "macos")]
    pub safari_sync_enabled: bool,
    #[cfg(target_os = "macos")]
    pub safari_sync_path: String,
    #[cfg(target_os = "macos")]
    pub last_sync_date: Option<DateTime<Utc>>,
    #[cfg(target_os = "macos")]
    pub sync_tx: Option<std::sync::mpsc::Sender<(Vec<Folder>, Folder)>>,
    #[cfg(target_os = "macos")]
    pub sync_manager: Option<crate::safari_sync::SafariSyncManager>,
    #[cfg(target_os = "macos")]
    pub last_safari_writeback: Option<SystemTime>,
    #[cfg(target_os = "macos")]
    pub safari_permission_warned: bool,
    pub popup: Option<Popup>,
    pub message: Option<String>,
    pub config_dir: PathBuf,
    pub save_after: Option<Instant>,
    #[cfg(target_os = "macos")]
    pub safari_writeback_after: Option<Instant>,
}

impl App {
    pub fn new() -> Self {
        let config_dir = Self::default_config_dir();
        let default_data_path = config_dir.to_string_lossy().into_owned();
        #[cfg(target_os = "macos")]
        let safari_default = dirs::home_dir()
            .map(|p| p.join("Library/Safari/Bookmarks.plist"))
            .unwrap_or_else(|| PathBuf::from("~/Library/Safari/Bookmarks.plist"))
            .to_string_lossy()
            .into_owned();
        Self {
            current_tab: CurrentTab::Links,
            focus: Focus::SearchInput,
            data: EPanelData::empty(),
            search_input: String::new(),
            search_expanded_folders: HashSet::new(),
            selected_item_ids: HashSet::new(),
            links_cursor: None,
            flat_items: vec![],
            notes_text: String::new(),
            notes_cursor: (0, 0),
            settings_links_path: default_data_path.clone(),
            settings_notes_path: default_data_path,
            #[cfg(target_os = "macos")]
            safari_sync_enabled: false,
            #[cfg(target_os = "macos")]
            safari_sync_path: safari_default,
            #[cfg(target_os = "macos")]
            last_sync_date: None,
            #[cfg(target_os = "macos")]
            sync_tx: None,
            #[cfg(target_os = "macos")]
            sync_manager: None,
            #[cfg(target_os = "macos")]
            last_safari_writeback: None,
            #[cfg(target_os = "macos")]
            safari_permission_warned: false,
            popup: None,
            message: None,
            config_dir: config_dir.clone(),
            save_after: None,
            #[cfg(target_os = "macos")]
            safari_writeback_after: None,
        }
    }

    pub fn data_changed(&mut self) {
        self.save_after = Some(Instant::now() + std::time::Duration::from_secs(1));
        #[cfg(target_os = "macos")]
        {
            self.safari_writeback_after = Some(Instant::now() + std::time::Duration::from_secs(2));
        }
    }

    /// Returns true if any timer fired and state may have changed.
    pub fn check_timers(&mut self) -> bool {
        #[allow(unused_mut)]
        let mut changed = false;
        if let Some(instant) = self.save_after {
            if instant <= Instant::now() {
                self.save_after = None;
                let _ = self.save();
            }
        }
        #[cfg(target_os = "macos")]
        {
            if let Some(instant) = self.safari_writeback_after {
                if instant <= Instant::now() {
                    self.safari_writeback_after = None;
                    if self.safari_sync_enabled && expand_tilde(&self.safari_sync_path).exists() {
                        if let Err(_) = self.writeback_safari() {
                            if !self.safari_permission_warned {
                                self.safari_permission_warned = true;
                                self.popup = Some(Popup::Alert {
                                    message: "Safari sync requires Full Disk Access.\n\n1. Press Enter to open System Settings\n2. Add your terminal app to Full Disk Access\n3. Restart epanel and re-enable Safari Sync.".to_string(),
                                });
                                changed = true;
                            }
                        } else {
                            self.safari_permission_warned = false;
                            self.message = Some("Synced to Safari".to_string());
                            if let Ok(meta) = fs::metadata(expand_tilde(&self.safari_sync_path)) {
                                if let Ok(modified) = meta.modified() {
                                    self.last_safari_writeback = Some(modified);
                                }
                            }
                            changed = true;
                        }
                    }
                }
            }
        }
        changed
    }

    pub fn next_timer_deadline(&self) -> Option<Instant> {
        #[allow(unused_mut)]
        let mut deadline: Option<Instant> = self.save_after;
        #[cfg(target_os = "macos")]
        {
            if let Some(d) = self.safari_writeback_after {
                if deadline.map(|current| d < current).unwrap_or(true) {
                    deadline = Some(d);
                }
            }
        }
        deadline
    }

    fn default_config_dir() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join("epanel"))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn settings_file_path(&self) -> PathBuf {
        self.config_dir.join("settings.txt")
    }

    fn data_file_path(&self) -> PathBuf {
        expand_tilde(&self.settings_links_path).join("epanel.json")
    }

    fn notes_file_path(&self) -> PathBuf {
        expand_tilde(&self.settings_notes_path).join("notes.txt")
    }

    // -----------------------------------------------------------------------
    // Load / save settings and JSON data
    // -----------------------------------------------------------------------

    pub fn load(&mut self) -> Result<()> {
        let settings_path = self.settings_file_path();
        if fs::metadata(&settings_path).is_ok() {
            let content = fs::read_to_string(&settings_path)?;
            for line in content.lines() {
                if let Some((key, val)) = line.split_once('=') {
                    match key.trim() {
                        "links_path" => self.settings_links_path = val.trim().to_string(),
                        "notes_path" => self.settings_notes_path = val.trim().to_string(),
                        #[cfg(target_os = "macos")]
                        "safari_sync" => self.safari_sync_enabled = val.trim() == "true",
                        #[cfg(target_os = "macos")]
                        "safari_sync_path" => self.safari_sync_path = val.trim().to_string(),
                        _ => {}
                    }
                }
            }
        }

        // If the directory you previously saved for links or notes no longer exists; fallback to the default config directory
        if !expand_tilde(&self.settings_links_path).exists() {
            self.settings_links_path = self.config_dir.to_string_lossy().into_owned();
        }
        if !expand_tilde(&self.settings_notes_path).exists() {
            self.settings_notes_path = self.config_dir.to_string_lossy().into_owned();
        }

        let data_path = self.data_file_path();
        if fs::metadata(&data_path).is_ok() {
            let content = fs::read_to_string(&data_path)?;
            self.data = serde_json::from_str(&content)?;
            self.ensure_valid_root();
            self.notes_text = self.data.notes.clone();
        }

        self.rebuild_flat_items();
        Ok(())
    }

    fn ensure_valid_root(&mut self) {
        if self.data.root_folder.id != ROOT_FOLDER_ID {
            self.data.root_folder.id = ROOT_FOLDER_ID;
            self.data.root_folder.name = "/".to_string();
            self.data.root_folder.is_collapsed = false;
        }
    }

    pub fn save(&mut self) -> Result<()> {
        fs::create_dir_all(expand_tilde(&self.settings_links_path))?;
        fs::create_dir_all(expand_tilde(&self.settings_notes_path))?;
        fs::create_dir_all(&self.config_dir)?;

        self.data.notes = self.notes_text.clone();
        let json = serde_json::to_string_pretty(&self.data)?;
        fs::write(self.data_file_path(), json)?;

        // Keep a plain-text notes mirror for convenience
        fs::write(self.notes_file_path(), &self.notes_text)?;

        #[allow(unused_mut)]
        let mut settings = format!(
            "links_path={}\nnotes_path={}\n",
            self.settings_links_path, self.settings_notes_path
        );
        #[cfg(target_os = "macos")]
        {
            settings.push_str(&format!("safari_sync={}\n", self.safari_sync_enabled));
            settings.push_str(&format!("safari_sync_path={}\n", self.safari_sync_path));
            if self.safari_sync_enabled && expand_tilde(&self.safari_sync_path).exists() {
                if self.writeback_safari().is_ok() {
                    self.safari_permission_warned = false;
                    if let Ok(meta) = fs::metadata(expand_tilde(&self.safari_sync_path)) {
                        if let Ok(modified) = meta.modified() {
                            self.last_safari_writeback = Some(modified);
                        }
                    }
                }
            }
        }
        fs::write(self.settings_file_path(), settings)?;

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn writeback_safari(&mut self) -> Result<()> {
        crate::safari_sync::writeback_safari_plist(expand_tilde(&self.safari_sync_path), &self.data.root_folder)
    }

    // -----------------------------------------------------------------------
    // Keyboard Input handling
    // -----------------------------------------------------------------------

    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> bool {
        if modifiers.contains(KeyModifiers::CONTROL) && key == KeyCode::Char('c') {
            return true;
        }

        match key {
            KeyCode::F(1) => {
                self.current_tab = CurrentTab::Links;
                self.focus = Focus::SearchInput;
                return false;
            }
            KeyCode::F(2) => {
                self.current_tab = CurrentTab::Notes;
                self.focus = Focus::NotesText;
                return false;
            }
            KeyCode::F(3) => {
                self.current_tab = CurrentTab::Settings;
                self.focus = Focus::SettingsLinksPath;
                return false;
            }
            _ => {}
        }

        if let Some(popup) = self.popup.clone() {
            self.handle_popup_key(key, popup);
            return false;
        }

        if key == KeyCode::Esc {
            if !self.search_input.is_empty() {
                self.search_input.clear();
                self.search_expanded_folders.clear();
                self.rebuild_flat_items();
            }
            return false;
        }

        if key == KeyCode::Char('?')
            || (modifiers.contains(KeyModifiers::SHIFT) && key == KeyCode::Char('/'))
        {
            self.popup = Some(Popup::Help);
            return false;
        }

        match self.current_tab {
            CurrentTab::Links => self.handle_links(key),
            CurrentTab::Notes => self.handle_notes(key),
            CurrentTab::Settings => self.handle_settings(key),
        }

        false
    }

    fn handle_links(&mut self, key: KeyCode) {
        match self.focus {
            Focus::SearchInput => match key {
                KeyCode::Char(c) => {
                    self.search_input.push(c);
                    self.rebuild_flat_items();
                }
                KeyCode::Backspace => {
                    self.search_input.pop();
                    self.rebuild_flat_items();
                }
                KeyCode::Tab | KeyCode::Down => self.focus = Focus::LinksList,
                KeyCode::Enter => {
                    let text = self.search_input.trim();
                    if !text.is_empty() {
                        self.popup = Some(Popup::AddEntry {
                            text: text.to_string(),
                            selected_folder: ROOT_FOLDER_ID,
                        });
                    }
                }
                _ => {}
            },
            Focus::LinksList => match key {
                KeyCode::Tab => self.focus = Focus::SearchInput,
                KeyCode::Up => self.move_cursor(-1),
                KeyCode::Down => self.move_cursor(1),
                KeyCode::Enter => {
                    if let Some(idx) = self.links_cursor {
                        match self.flat_items[idx].kind {
                            FlatItemKind::Folder => {
                                let id = self.flat_items[idx].id;
                                self.toggle_folder_collapsed(id);
                            }
                            FlatItemKind::Entry => {
                                let text = self.flat_items[idx].name.clone();
                                if let Err(e) = self.open_entry(&text) {
                                    self.popup = Some(Popup::Alert {
                                        message: e.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
                KeyCode::Char(' ') => {
                    if let Some(idx) = self.links_cursor {
                        let id = self.flat_items[idx].id;
                        if self.selected_item_ids.contains(&id) {
                            self.selected_item_ids.remove(&id);
                        } else {
                            self.selected_item_ids.insert(id);
                        }
                    }
                }
                KeyCode::Char('n') => {
                    self.popup = Some(Popup::NewFolder {
                        name: "New Folder".to_string(),
                        selected_parent: ROOT_FOLDER_ID,
                    });
                }
                KeyCode::Char('d') | KeyCode::Delete => {
                    self.delete_selected();
                }
                KeyCode::Char('m') => {
                    self.init_move_selected();
                }
                KeyCode::Char('r') => {
                    self.init_rename_selected();
                }
                KeyCode::Char('e') => {
                    let default = dirs::home_dir()
                        .map(|p| p.join("epanel-export.json"))
                        .unwrap_or_else(|| PathBuf::from("epanel-export.json"));
                    self.popup = Some(Popup::ExportJSON {
                        path: default.to_string_lossy().into_owned(),
                    });
                }
                KeyCode::Char('i') => {
                    self.popup = Some(Popup::ImportJSON {
                        path: String::new(),
                    });
                }
                KeyCode::Right => {
                    if let Some(idx) = self.links_cursor {
                        let id = self.flat_items[idx].id;
                        if matches!(self.flat_items[idx].kind, FlatItemKind::Folder) {
                            self.expand_folder(id);
                        }
                    }
                }
                KeyCode::Left => {
                    if let Some(idx) = self.links_cursor {
                        let id = self.flat_items[idx].id;
                        if matches!(self.flat_items[idx].kind, FlatItemKind::Folder) {
                            self.collapse_folder(id);
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn handle_notes(&mut self, key: KeyCode) {
        let old_len = self.notes_text.len();
        match key {
            KeyCode::Char(c) => self.notes_text.push(c),
            KeyCode::Enter => self.notes_text.push('\n'),
            KeyCode::Backspace => {
                self.notes_text.pop();
            }
            _ => {}
        }
        if self.notes_text.len() != old_len {
            self.data_changed();
        }
        self.update_notes_cursor();
    }

    fn update_notes_cursor(&mut self) {
        let lines: Vec<&str> = self.notes_text.split('\n').collect();
        let y = if self.notes_text.ends_with('\n') {
            lines.len()
        } else {
            lines.len().saturating_sub(1)
        };
        let x = if self.notes_text.ends_with('\n') {
            0
        } else {
            lines.last().map(|l| l.chars().count()).unwrap_or(0)
        };
        self.notes_cursor = (x, y);
    }

    fn handle_settings(&mut self, key: KeyCode) {
        match self.focus {
            Focus::SettingsLinksPath => match key {
                KeyCode::Char(c) => self.settings_links_path.push(c),
                KeyCode::Backspace => {
                    self.settings_links_path.pop();
                }
                KeyCode::Tab => self.focus = Focus::SettingsNotesPath,
                _ => {}
            },
            Focus::SettingsNotesPath => match key {
                KeyCode::Char(c) => self.settings_notes_path.push(c),
                KeyCode::Backspace => {
                    self.settings_notes_path.pop();
                }
                KeyCode::BackTab => self.focus = Focus::SettingsLinksPath,
                #[cfg(target_os = "macos")]
                KeyCode::Tab => self.focus = Focus::SettingsSafariSync,
                #[cfg(not(target_os = "macos"))]
                KeyCode::Tab => self.focus = Focus::SettingsSaveButton,
                _ => {}
            },
            #[cfg(target_os = "macos")]
            Focus::SettingsSafariSync => match key {
                KeyCode::BackTab => self.focus = Focus::SettingsNotesPath,
                KeyCode::Tab => self.focus = Focus::SettingsSaveButton,
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.safari_sync_enabled = !self.safari_sync_enabled;
                    if self.safari_sync_enabled {
                        self.safari_permission_warned = false;
                        // Verify we can read Safari bookmarks before enabling
                        match self.import_safari() {
                            Ok((folders, reading_list)) => {
                                self.move_existing_content_to_original_folder();
                                self.apply_safari_sync(folders, reading_list);
                                self.start_safari_sync();
                            }
                            Err(msg) => {
                                self.safari_sync_enabled = false;
                                let message = if msg.contains("Full Disk Access") {
                                    format!("{}\n\nPress Enter to open System Settings.", msg)
                                } else {
                                    msg
                                };
                                self.popup = Some(Popup::Alert { message });
                            }
                        }
                    } else {
                        self.sync_manager = None;
                    }
                }
                _ => {}
            },
            Focus::SettingsSaveButton => match key {
                #[cfg(target_os = "macos")]
                KeyCode::BackTab => self.focus = Focus::SettingsSafariSync,
                #[cfg(not(target_os = "macos"))]
                KeyCode::BackTab => self.focus = Focus::SettingsNotesPath,
                KeyCode::Tab => self.focus = Focus::SettingsLinksPath,
                KeyCode::Enter => {
                    if let Err(e) = self.save() {
                        self.popup = Some(Popup::Alert {
                            message: format!("Save failed: {}", e),
                        });
                    } else {
                        self.message = Some("Saved successfully".to_string());
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    #[cfg(target_os = "macos")]
    pub fn sync_safari_on_startup(&mut self) {
        if !self.safari_sync_enabled {
            return;
        }
        match self.import_safari() {
            Ok((folders, reading_list)) => {
                self.apply_safari_sync(folders, reading_list);
                if let Ok(meta) = fs::metadata(expand_tilde(&self.safari_sync_path)) {
                    if let Ok(modified) = meta.modified() {
                        self.last_safari_writeback = Some(modified);
                    }
                }
            }
            Err(e) => {
                self.message = Some(format!("Safari sync failed: {}", e));
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub fn start_safari_sync(&mut self) {
        if let Some(ref tx) = self.sync_tx {
            let path = expand_tilde(&self.safari_sync_path);
            let path_clone = path.clone();
            let tx = tx.clone();
            if let Ok(manager) = crate::safari_sync::SafariSyncManager::new(&path, move || {
                if let Ok((folders, reading_list)) = crate::safari_sync::parse_safari_plist(&path_clone) {
                    let _ = tx.send((folders, reading_list));
                }
            }) {
                self.sync_manager = Some(manager);
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn import_safari(&self) -> Result<(Vec<Folder>, Folder), String> {
        let path = expand_tilde(&self.safari_sync_path);
        if !path.exists() {
            return Err(format!(
                "Safari bookmarks file not found at:\n{}\n\nOn newer macOS versions the file may be in a different location (e.g. ~/Library/Containers/com.apple.Safari/Data/Library/Safari/Bookmarks.plist).",
                self.safari_sync_path
            ));
        }
        match fs::read(&path) {
            Err(e) => {
                let hint = if e.kind() == std::io::ErrorKind::PermissionDenied {
                    "\n\nThis usually means Full Disk Access is required.\n1. Quit your terminal app completely\n2. Add it to System Settings > Privacy & Security > Full Disk Access\n3. Restart the terminal and epanel"
                } else {
                    ""
                };
                Err(format!("Cannot read Safari bookmarks: {}{}", e, hint))
            }
            Ok(_) => {
                crate::safari_sync::parse_safari_plist(&path)
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub fn apply_safari_sync(&mut self, bookmark_folders: Vec<Folder>, reading_list: Folder) {
        self.data_changed();
        // Capture collapsed state
        let mut collapsed_state: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
        fn capture_state(folder: &Folder, state: &mut std::collections::HashMap<String, bool>) {
            state.insert(folder.name.clone(), folder.is_collapsed);
            for sub in &folder.subfolders {
                capture_state(sub, state);
            }
        }
        for sub in &self.data.root_folder.subfolders {
            capture_state(sub, &mut collapsed_state);
        }

        fn restore_state(folder: &mut Folder, state: &std::collections::HashMap<String, bool>) {
            if let Some(&collapsed) = state.get(&folder.name) {
                folder.is_collapsed = collapsed;
            }
            for sub in &mut folder.subfolders {
                restore_state(sub, state);
            }
        }

        // Replace synced bookmark folders
        for mut safari_folder in bookmark_folders {
            restore_state(&mut safari_folder, &collapsed_state);
            if let Some(idx) = self.data.root_folder.subfolders.iter().position(|f| f.name == safari_folder.name) {
                self.data.root_folder.subfolders[idx] = safari_folder;
            } else if !safari_folder.entries.is_empty() || !safari_folder.subfolders.is_empty() {
                self.data.root_folder.subfolders.push(safari_folder);
            }
        }

        // Replace Reading List
        let mut rl = reading_list;
        rl.name = "Reading List".to_string();
        restore_state(&mut rl, &collapsed_state);
        if let Some(idx) = self.data.root_folder.subfolders.iter().position(|f| f.name == "Reading List") {
            self.data.root_folder.subfolders[idx] = rl;
        } else if !rl.entries.is_empty() {
            self.data.root_folder.subfolders.insert(0, rl);
        }

        self.last_sync_date = Some(Utc::now());
        self.rebuild_flat_items();
    }

    #[cfg(target_os = "macos")]
    fn move_existing_content_to_original_folder(&mut self) {
        let has_content = !self.data.root_folder.entries.is_empty() || !self.data.root_folder.subfolders.is_empty();
        if !has_content {
            return;
        }
        if self.data.root_folder.subfolders.iter().any(|f| f.name == "my_original_epanel") {
            return;
        }
        let original = Folder {
            name: "my_original_epanel".to_string(),
            entries: std::mem::take(&mut self.data.root_folder.entries),
            subfolders: std::mem::take(&mut self.data.root_folder.subfolders),
            ..Folder::new("")
        };
        self.data.root_folder.subfolders.push(original);
    }

    fn handle_popup_key(&mut self, key: KeyCode, popup: Popup) {
        match popup {
            Popup::AddEntry { text, mut selected_folder } => match key {
                KeyCode::Up | KeyCode::Down => {
                    let folders = self.flattened_folder_choices(None);
                    if let Some(idx) = folders.iter().position(|(id, _, _)| *id == selected_folder) {
                        let new_idx = match key {
                            KeyCode::Up => idx.saturating_sub(1),
                            _ => (idx + 1).min(folders.len() - 1),
                        };
                        selected_folder = folders[new_idx].0;
                        self.popup = Some(Popup::AddEntry { text, selected_folder });
                    }
                }
                KeyCode::Enter => {
                    let entry = Entry::new(text);
                    let entry_id = entry.id;
                    self.add_entry_to_folder(entry, selected_folder);
                    self.search_input.clear();
                    self.search_expanded_folders.clear();
                    self.selected_item_ids = [entry_id].into_iter().collect();
                    self.popup = None;
                    self.rebuild_flat_items();
                }
                KeyCode::Esc => self.popup = None,
                _ => {}
            },
            Popup::NewFolder { mut name, mut selected_parent } => match key {
                KeyCode::Char(c) => {
                    name.push(c);
                    self.popup = Some(Popup::NewFolder { name, selected_parent });
                }
                KeyCode::Backspace => {
                    name.pop();
                    self.popup = Some(Popup::NewFolder { name, selected_parent });
                }
                KeyCode::Up | KeyCode::Down => {
                    let folders = self.flattened_folder_choices(None);
                    if let Some(idx) = folders.iter().position(|(id, _, _)| *id == selected_parent) {
                        let new_idx = match key {
                            KeyCode::Up => idx.saturating_sub(1),
                            _ => (idx + 1).min(folders.len() - 1),
                        };
                        selected_parent = folders[new_idx].0;
                        self.popup = Some(Popup::NewFolder { name, selected_parent });
                    }
                }
                KeyCode::Enter => {
                    if !name.trim().is_empty() {
                        self.create_folder(name.trim().to_string(), selected_parent);
                        self.popup = None;
                        self.rebuild_flat_items();
                    }
                }
                KeyCode::Esc => self.popup = None,
                _ => {}
            },
            Popup::RenameFolder { folder_id, mut name } => match key {
                KeyCode::Char(c) => {
                    name.push(c);
                    self.popup = Some(Popup::RenameFolder { folder_id, name });
                }
                KeyCode::Backspace => {
                    name.pop();
                    self.popup = Some(Popup::RenameFolder { folder_id, name });
                }
                KeyCode::Enter => {
                    if !name.trim().is_empty() {
                        self.modify_folder(folder_id, |f| f.name = name.trim().to_string());
                        self.popup = None;
                        self.rebuild_flat_items();
                    }
                }
                KeyCode::Esc => self.popup = None,
                _ => {}
            },
            Popup::MoveItem { item_id, mut selected_folder, is_folder } => match key {
                KeyCode::Up | KeyCode::Down => {
                    let excluded = if is_folder {
                        let mut ex = HashSet::new();
                        ex.insert(item_id);
                        if let Some(folder) = self.find_folder(item_id) {
                            fn collect_ids(folder: &Folder, set: &mut HashSet<Uuid>) {
                                set.insert(folder.id);
                                for sub in &folder.subfolders {
                                    collect_ids(sub, set);
                                }
                            }
                            for sub in &folder.subfolders {
                                collect_ids(sub, &mut ex);
                            }
                        }
                        Some(ex)
                    } else {
                        None
                    };
                    let folders = self.flattened_folder_choices(excluded.as_ref());
                    if let Some(idx) = folders.iter().position(|(id, _, _)| *id == selected_folder) {
                        let new_idx = match key {
                            KeyCode::Up => idx.saturating_sub(1),
                            _ => (idx + 1).min(folders.len() - 1),
                        };
                        selected_folder = folders[new_idx].0;
                        self.popup = Some(Popup::MoveItem { item_id, selected_folder, is_folder });
                    }
                }
                KeyCode::Enter => {
                    if is_folder {
                        self.move_folder(item_id, selected_folder);
                    } else {
                        let ids: Vec<_> = if self.selected_item_ids.len() > 1 {
                            self.selected_item_ids.iter().copied().filter(|id| self.find_entry(*id).is_some()).collect()
                        } else {
                            vec![item_id]
                        };
                        for id in ids {
                            self.move_entry(id, selected_folder);
                        }
                    }
                    self.selected_item_ids.clear();
                    self.popup = None;
                    self.rebuild_flat_items();
                }
                KeyCode::Esc => self.popup = None,
                _ => {}
            },
            Popup::ConfirmDeleteSelected { .. } => match key {
                KeyCode::Char('y') | KeyCode::Enter => {
                    let ids: Vec<_> = self.selected_item_ids.iter().copied().collect();
                    for id in ids {
                        self.delete_folder(id);
                        self.delete_entry(id);
                    }
                    self.selected_item_ids.clear();
                    self.popup = None;
                    self.rebuild_flat_items();
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.popup = None;
                }
                _ => {}
            },
            Popup::Alert { ref message } => match key {
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') => {
                    if message.contains("System Settings") {
                        let _ = std::process::Command::new("open")
                            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
                            .spawn();
                    }
                    self.popup = None;
                }
                _ => {}
            },
            Popup::Help => match key {
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') | KeyCode::Char('?') | KeyCode::Char('q') => {
                    self.popup = None;
                }
                _ => {}
            },
            Popup::ExportJSON { mut path } => match key {
                KeyCode::Char(c) => {
                    path.push(c);
                    self.popup = Some(Popup::ExportJSON { path });
                }
                KeyCode::Backspace => {
                    path.pop();
                    self.popup = Some(Popup::ExportJSON { path });
                }
                KeyCode::Enter => {
                    if !path.trim().is_empty() {
                        match self.export_json(path.trim()) {
                            Ok(_) => self.message = Some(format!("Exported to {}", path.trim())),
                            Err(e) => self.popup = Some(Popup::Alert { message: format!("Export failed: {}", e) }),
                        }
                        if self.popup.is_some() && matches!(self.popup, Some(Popup::ExportJSON { .. })) {
                            self.popup = None;
                        }
                    }
                }
                KeyCode::Esc => self.popup = None,
                _ => {}
            },
            Popup::ImportJSON { mut path } => match key {
                KeyCode::Char(c) => {
                    path.push(c);
                    self.popup = Some(Popup::ImportJSON { path });
                }
                KeyCode::Backspace => {
                    path.pop();
                    self.popup = Some(Popup::ImportJSON { path });
                }
                KeyCode::Enter => {
                    if !path.trim().is_empty() {
                        let p = path.trim().to_string();
                        self.popup = Some(Popup::ConfirmImportJSON { path: p });
                    }
                }
                KeyCode::Esc => self.popup = None,
                _ => {}
            },
            Popup::ConfirmImportJSON { path } => match key {
                KeyCode::Char('y') | KeyCode::Enter => {
                    match self.import_json(&path) {
                        Ok(_) => {
                            self.search_input.clear();
                            self.search_expanded_folders.clear();
                            self.selected_item_ids.clear();
                            self.rebuild_flat_items();
                            self.message = Some("Imported successfully".to_string());
                        }
                        Err(e) => self.popup = Some(Popup::Alert { message: format!("Import failed: {}", e) }),
                    }
                    if self.popup.is_some() && matches!(self.popup, Some(Popup::ConfirmImportJSON { .. })) {
                        self.popup = None;
                    }
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.popup = None;
                }
                _ => {}
            },
        }
    }

    // -----------------------------------------------------------------------
    // Tree structure helpers
    // -----------------------------------------------------------------------

    pub fn rebuild_flat_items(&mut self) {
        self.flat_items.clear();
        let filter = self.search_input.trim().to_lowercase();
        let is_searching = !filter.is_empty();

        // Root-level subfolders
        for sub in &self.data.root_folder.subfolders {
            if is_searching {
                let matches = sub.name.to_lowercase().contains(&filter)
                    || has_matching_descendants(sub, &filter);
                if !matches {
                    continue;
                }
            }
            self.flat_items.push(FlatItem {
                id: sub.id,
                kind: FlatItemKind::Folder,
                depth: 0,
                name: sub.name.clone(),
                is_collapsed: sub.is_collapsed,
            });
            let expand = if is_searching {
                !sub.is_collapsed
                    || sub.name.to_lowercase().contains(&filter)
                    || has_matching_descendants(sub, &filter)
                    || self.search_expanded_folders.contains(&sub.id)
            } else {
                !sub.is_collapsed
            };
            if expand {
                flatten(sub, 0, &mut self.flat_items, &filter, is_searching, &self.search_expanded_folders);
            }
        }

        // Root-level entries
        for entry in &self.data.root_folder.entries {
            if is_searching && !entry.text.to_lowercase().contains(&filter) {
                continue;
            }
            self.flat_items.push(FlatItem {
                id: entry.id,
                kind: FlatItemKind::Entry,
                depth: 0,
                name: entry.text.clone(),
                is_collapsed: false,
            });
        }

        if self.flat_items.is_empty() {
            self.links_cursor = None;
        } else if self.links_cursor.is_none() || self.links_cursor.unwrap() >= self.flat_items.len() {
            self.links_cursor = Some(0);
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.flat_items.is_empty() {
            self.links_cursor = None;
            return;
        }
        let cur = self.links_cursor.unwrap_or(0);
        let new = if delta < 0 {
            cur.saturating_sub((-delta) as usize)
        } else {
            (cur + delta as usize).min(self.flat_items.len() - 1)
        };
        self.links_cursor = Some(new);
    }

    fn toggle_folder_collapsed(&mut self, id: Uuid) {
        if self.search_input.trim().is_empty() {
            self.modify_folder(id, |f| f.is_collapsed = !f.is_collapsed);
        } else {
            if self.search_expanded_folders.contains(&id) {
                self.search_expanded_folders.remove(&id);
            } else {
                self.search_expanded_folders.insert(id);
            }
        }
        self.rebuild_flat_items();
    }

    fn expand_folder(&mut self, id: Uuid) {
        if self.search_input.trim().is_empty() {
            self.modify_folder(id, |f| f.is_collapsed = false);
        } else {
            self.search_expanded_folders.insert(id);
        }
        self.rebuild_flat_items();
    }

    fn collapse_folder(&mut self, id: Uuid) {
        if self.search_input.trim().is_empty() {
            self.modify_folder(id, |f| f.is_collapsed = true);
        } else {
            self.search_expanded_folders.remove(&id);
        }
        self.rebuild_flat_items();
    }

    // -----------------------------------------------------------------------
    // Data modification helpers
    // -----------------------------------------------------------------------

    fn modify_folder(&mut self, id: Uuid, mut modifier: impl FnMut(&mut Folder)) {
        fn modify_in(folder: &mut Folder, id: Uuid, modifier: &mut dyn FnMut(&mut Folder)) -> bool {
            if folder.id == id {
                modifier(folder);
                return true;
            }
            for sub in &mut folder.subfolders {
                if modify_in(sub, id, modifier) {
                    return true;
                }
            }
            false
        }
        modify_in(&mut self.data.root_folder, id, &mut modifier);
        self.data_changed();
    }

    fn find_folder(&self, id: Uuid) -> Option<&Folder> {
        fn find_in(folder: &Folder, id: Uuid) -> Option<&Folder> {
            if folder.id == id {
                return Some(folder);
            }
            for sub in &folder.subfolders {
                if let Some(f) = find_in(sub, id) {
                    return Some(f);
                }
            }
            None
        }
        find_in(&self.data.root_folder, id)
    }

    fn find_entry(&self, id: Uuid) -> Option<&Entry> {
        fn find_in(folder: &Folder, id: Uuid) -> Option<&Entry> {
            if let Some(e) = folder.entries.iter().find(|e| e.id == id) {
                return Some(e);
            }
            for sub in &folder.subfolders {
                if let Some(e) = find_in(sub, id) {
                    return Some(e);
                }
            }
            None
        }
        find_in(&self.data.root_folder, id)
    }

    fn add_entry_to_folder(&mut self, entry: Entry, folder_id: Uuid) {
        self.modify_folder(folder_id, |f| f.entries.push(entry.clone()));
    }

    fn create_folder(&mut self, name: String, parent_id: Uuid) {
        self.modify_folder(parent_id, |f| f.subfolders.push(Folder::new(name.clone())));
    }

    fn delete_entry(&mut self, id: Uuid) {
        fn remove_from(folder: &mut Folder, id: Uuid) -> bool {
            if let Some(idx) = folder.entries.iter().position(|e| e.id == id) {
                folder.entries.remove(idx);
                return true;
            }
            for sub in &mut folder.subfolders {
                if remove_from(sub, id) {
                    return true;
                }
            }
            false
        }
        if remove_from(&mut self.data.root_folder, id) {
            self.data_changed();
        }
    }

    fn delete_folder(&mut self, id: Uuid) {
        fn remove_from(folder: &mut Folder, id: Uuid) -> bool {
            if let Some(idx) = folder.subfolders.iter().position(|f| f.id == id) {
                folder.subfolders.remove(idx);
                return true;
            }
            for sub in &mut folder.subfolders {
                if remove_from(sub, id) {
                    return true;
                }
            }
            false
        }
        if remove_from(&mut self.data.root_folder, id) {
            self.data_changed();
        }
    }

    fn move_entry(&mut self, entry_id: Uuid, to_folder_id: Uuid) {
        fn remove_from(folder: &mut Folder, id: Uuid) -> Option<Entry> {
            if let Some(idx) = folder.entries.iter().position(|e| e.id == id) {
                return Some(folder.entries.remove(idx));
            }
            for sub in &mut folder.subfolders {
                if let Some(e) = remove_from(sub, id) {
                    return Some(e);
                }
            }
            None
        }
        if let Some(entry) = remove_from(&mut self.data.root_folder, entry_id) {
            self.modify_folder(to_folder_id, |f| f.entries.push(entry.clone()));
        }
    }

    fn move_folder(&mut self, folder_id: Uuid, to_parent_id: Uuid) {
        if folder_id == to_parent_id {
            return;
        }
        // Prevent moving into a descendant
        if let Some(target) = self.find_folder(to_parent_id) {
            fn is_descendant(folder: &Folder, ancestor_id: Uuid) -> bool {
                if folder.id == ancestor_id {
                    return true;
                }
                for sub in &folder.subfolders {
                    if is_descendant(sub, ancestor_id) {
                        return true;
                    }
                }
                false
            }
            if is_descendant(target, folder_id) {
                return;
            }
        }
        fn remove_from(folder: &mut Folder, id: Uuid) -> Option<Folder> {
            if let Some(idx) = folder.subfolders.iter().position(|f| f.id == id) {
                return Some(folder.subfolders.remove(idx));
            }
            for sub in &mut folder.subfolders {
                if let Some(f) = remove_from(sub, id) {
                    return Some(f);
                }
            }
            None
        }
        if let Some(folder) = remove_from(&mut self.data.root_folder, folder_id) {
            self.modify_folder(to_parent_id, |f| f.subfolders.push(folder.clone()));
        }
    }

    fn open_entry(&self, text: &str) -> Result<()> {
        if text.contains("://") {
            #[cfg(target_os = "macos")]
            {
                std::process::Command::new("open").arg(text).spawn()?;
            }
            #[cfg(target_os = "linux")]
            {
                std::process::Command::new("xdg-open").arg(text).spawn()?;
            }
        } else {
            let path = Path::new(text);
            if path.exists() {
                #[cfg(target_os = "macos")]
                {
                    std::process::Command::new("open").arg(path).spawn()?;
                }
                #[cfg(target_os = "linux")]
                {
                    std::process::Command::new("xdg-open").arg(path).spawn()?;
                }
            } else {
                anyhow::bail!("Path not found: {}", text);
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Selection / actions
    // -----------------------------------------------------------------------

    fn delete_selected(&mut self) {
        if self.selected_item_ids.is_empty() {
            if let Some(idx) = self.links_cursor {
                self.selected_item_ids.insert(self.flat_items[idx].id);
            }
        }
        if self.selected_item_ids.is_empty() {
            return;
        }

        let mut entry_count = 0usize;
        let mut folder_count = 0usize;
        let mut subfolder_count = 0usize;
        let mut has_non_empty = false;

        for &id in &self.selected_item_ids {
            if let Some(folder) = self.find_folder(id) {
                folder_count += 1;
                let ec = count_entries(folder);
                entry_count += ec;
                subfolder_count += folder.subfolders.len();
                if ec > 0 || !folder.subfolders.is_empty() {
                    has_non_empty = true;
                }
            } else if self.find_entry(id).is_some() {
                entry_count += 1;
            }
        }

        if has_non_empty {
            self.popup = Some(Popup::ConfirmDeleteSelected {
                entry_count,
                folder_count,
                subfolder_count,
            });
            return;
        }

        let ids: Vec<_> = self.selected_item_ids.iter().copied().collect();
        for id in ids {
            self.delete_folder(id);
            self.delete_entry(id);
        }
        self.selected_item_ids.clear();
        self.rebuild_flat_items();
    }

    fn init_move_selected(&mut self) {
        if self.selected_item_ids.is_empty() {
            if let Some(idx) = self.links_cursor {
                let id = self.flat_items[idx].id;
                let is_folder = matches!(self.flat_items[idx].kind, FlatItemKind::Folder);
                self.popup = Some(Popup::MoveItem {
                    item_id: id,
                    selected_folder: ROOT_FOLDER_ID,
                    is_folder,
                });
            }
        } else if self.selected_item_ids.len() == 1 {
            let id = *self.selected_item_ids.iter().next().unwrap();
            let is_folder = self.find_folder(id).is_some();
            self.popup = Some(Popup::MoveItem {
                item_id: id,
                selected_folder: ROOT_FOLDER_ID,
                is_folder,
            });
        } else {
            // Multi-move entries only
            let first = *self.selected_item_ids.iter().next().unwrap();
            self.popup = Some(Popup::MoveItem {
                item_id: first,
                selected_folder: ROOT_FOLDER_ID,
                is_folder: false,
            });
        }
    }

    fn init_rename_selected(&mut self) {
        if let Some(idx) = self.links_cursor {
            let id = self.flat_items[idx].id;
            if let Some(folder) = self.find_folder(id) {
                self.popup = Some(Popup::RenameFolder {
                    folder_id: id,
                    name: folder.name.clone(),
                });
            }
        }
    }

    fn export_json(&self, path: impl AsRef<Path>) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.data)?;
        fs::write(path, json)?;
        Ok(())
    }

    fn import_json(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let content = fs::read_to_string(path)?;
        let data: EPanelData = serde_json::from_str(&content)?;
        self.data = data;
        self.ensure_valid_root();
        self.notes_text = self.data.notes.clone();
        self.data_changed();
        Ok(())
    }

    pub fn help_sections(&self) -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
        #[allow(unused_mut)]
        let mut settings_items = vec![
            ("Tab", "Navigate inputs"),
            ("Enter", "Save settings"),
        ];
        #[cfg(target_os = "macos")]
        settings_items.push(("Space / Enter", "Toggle Safari sync"));

        vec![
            (
                "Global",
                vec![
                    ("F1 / F2 / F3", "Switch tabs (Links / Notes / Settings)"),
                    ("?", "Show this help menu"),
                    ("Esc", "Clear search (Links tab)"),
                    ("Ctrl+C", "Quit application"),
                ],
            ),
            (
                "Links Tab",
                vec![
                    ("Tab", "Focus search / list"),
                    ("↑ / ↓", "Navigate list"),
                    ("→ / ←", "Expand / collapse folder"),
                    ("Enter", "Open entry or toggle folder"),
                    ("Space", "Select / deselect item"),
                    ("n", "New folder"),
                    ("r", "Rename folder"),
                    ("m", "Move selected item(s)"),
                    ("d / Delete", "Delete selected item(s)"),
                    ("e", "Export JSON"),
                    ("i", "Import JSON"),
                ],
            ),
            (
                "Notes Tab",
                vec![
                    ("Type normally", "Edit notes"),
                ],
            ),
            (
                "Settings Tab",
                settings_items,
            ),
            (
                "About",
                vec![
                    ("Author", "Al Biheiri <al@forgottheaddress.com>"),
                    ("Website", "http://www.abiheiri.com"),
                ],
            ),
        ]
    }

    pub fn flattened_folder_choices(&self, excluded: Option<&HashSet<Uuid>>) -> Vec<(Uuid, String, usize)> {
        let mut result = vec![(ROOT_FOLDER_ID, "/".to_string(), 0)];
        fn collect(folder: &Folder, depth: usize, result: &mut Vec<(Uuid, String, usize)>, excluded: Option<&HashSet<Uuid>>) {
            for sub in &folder.subfolders {
                if let Some(ex) = excluded {
                    if ex.contains(&sub.id) {
                        continue;
                    }
                }
                result.push((sub.id, sub.name.clone(), depth + 1));
                collect(sub, depth + 1, result, excluded);
            }
        }
        collect(&self.data.root_folder, 0, &mut result, excluded);
        result
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn has_matching_descendants(folder: &Folder, filter: &str) -> bool {
    if folder.entries.iter().any(|e| e.text.to_lowercase().contains(filter)) {
        return true;
    }
    folder
        .subfolders
        .iter()
        .any(|f| f.name.to_lowercase().contains(filter) || has_matching_descendants(f, filter))
}

fn flatten(
    folder: &Folder,
    depth: usize,
    flat: &mut Vec<FlatItem>,
    filter: &str,
    is_searching: bool,
    expanded: &HashSet<Uuid>,
) {
    for sub in &folder.subfolders {
        if is_searching {
            let matches = sub.name.to_lowercase().contains(filter)
                || has_matching_descendants(sub, filter);
            if !matches {
                continue;
            }
        }
        flat.push(FlatItem {
            id: sub.id,
            kind: FlatItemKind::Folder,
            depth: depth + 1,
            name: sub.name.clone(),
            is_collapsed: sub.is_collapsed,
        });
        let should_expand = if is_searching {
            !sub.is_collapsed
                || sub.name.to_lowercase().contains(filter)
                || has_matching_descendants(sub, filter)
                || expanded.contains(&sub.id)
        } else {
            !sub.is_collapsed
        };
        if should_expand {
            flatten(sub, depth + 1, flat, filter, is_searching, expanded);
        }
    }

    for entry in &folder.entries {
        if is_searching && !entry.text.to_lowercase().contains(filter) {
            continue;
        }
        flat.push(FlatItem {
            id: entry.id,
            kind: FlatItemKind::Entry,
            depth: depth + 1,
            name: entry.text.clone(),
            is_collapsed: false,
        });
    }
}

fn count_entries(folder: &Folder) -> usize {
    let mut count = folder.entries.len();
    for sub in &folder.subfolders {
        count += count_entries(sub);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn json_roundtrip() {
        let dir = TempDir::new().unwrap();
        let cfg = dir.path().join("config");

        let mut app = App::new();
        app.config_dir = cfg.clone();
        app.data.root_folder.entries.push(Entry::new("https://example.com".to_string()));
        app.data.notes = "Hello notes".to_string();
        app.notes_text = app.data.notes.clone();
        app.settings_links_path = dir.path().join("links").to_string_lossy().into_owned();
        app.settings_notes_path = dir.path().join("notes").to_string_lossy().into_owned();
        app.save().unwrap();

        let data_path = dir.path().join("links/epanel.json");
        assert!(data_path.exists());
        assert!(cfg.join("settings.txt").exists());

        let mut app2 = App::new();
        app2.config_dir = cfg;
        app2.load().unwrap();

        assert_eq!(app2.data.root_folder.entries.len(), 1);
        assert_eq!(app2.data.root_folder.entries[0].text, "https://example.com");
        assert_eq!(app2.data.notes, "Hello notes");
    }

    #[test]
    fn search_filter_and_expand() {
        let mut app = App::new();
        let mut folder = Folder::new("Docs");
        folder.entries.push(Entry::new("rust book".to_string()));
        folder.entries.push(Entry::new("swift guide".to_string()));
        app.data.root_folder.subfolders.push(folder);
        app.rebuild_flat_items();
        assert_eq!(app.flat_items.len(), 3); // folder + 2 entries

        app.search_input = "rust".to_string();
        app.rebuild_flat_items();
        // folder + matching entry
        assert_eq!(app.flat_items.len(), 2);
    }

    #[test]
    fn move_entry_between_folders() {
        let mut app = App::new();
        let entry = Entry::new("link1".to_string());
        let entry_id = entry.id;
        app.data.root_folder.entries.push(entry);

        let folder = Folder::new("F1");
        let folder_id = folder.id;
        app.data.root_folder.subfolders.push(folder);

        app.move_entry(entry_id, folder_id);
        assert!(app.data.root_folder.entries.is_empty());
        assert_eq!(app.data.root_folder.subfolders[0].entries.len(), 1);
        assert_eq!(app.data.root_folder.subfolders[0].entries[0].text, "link1");
    }
}
