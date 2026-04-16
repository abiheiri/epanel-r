//! Application state and logic for epanel.
//!
//! A TUI panel for managing links and notes.
//!
//! Author: Al Biheiri <al@forgottheaddress.com>

use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::KeyCode;

pub const VERSION: &str = match option_env!("EPANEL_VERSION") {
    Some(v) => v,
    None => "v0.0.1",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentTab {
    Links,
    Notes,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    LinksInput,
    LinksButton,
    LinksList,
    NotesText,
    SettingsLinksPath,
    SettingsNotesPath,
    SettingsSaveButton,
}

pub struct App {
    pub current_tab: CurrentTab,
    pub focus: Focus,
    pub links_input: String,
    pub links_items: Vec<String>,
    pub links_selected: Option<usize>,
    pub notes_text: String,
    pub settings_links_path: String,
    pub settings_notes_path: String,
}

impl App {
    pub fn new() -> Self {
        let config_dir = Self::config_dir();
        Self {
            current_tab: CurrentTab::Links,
            focus: Focus::LinksInput,
            links_input: String::new(),
            links_items: Vec::new(),
            links_selected: None,
            notes_text: String::new(),
            settings_links_path: config_dir.join("links.txt").to_string_lossy().into_owned(),
            settings_notes_path: config_dir.join("notes.txt").to_string_lossy().into_owned(),
        }
    }

    fn config_dir() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join("epanel"))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn load(&mut self) -> Result<()> {
        let config_dir = Self::config_dir();
        let settings_path = config_dir.join("settings.txt");
        if fs::metadata(&settings_path).is_ok() {
            let content = fs::read_to_string(&settings_path)?;
            for line in content.lines() {
                if let Some((key, val)) = line.split_once('=') {
                    match key.trim() {
                        "links_path" => self.settings_links_path = val.trim().to_string(),
                        "notes_path" => self.settings_notes_path = val.trim().to_string(),
                        _ => {}
                    }
                }
            }
        }

        if fs::metadata(&self.settings_links_path).is_ok() {
            let content = fs::read_to_string(&self.settings_links_path)?;
            self.links_items = content.lines().map(|s| s.to_string()).collect();
        }

        if fs::metadata(&self.settings_notes_path).is_ok() {
            self.notes_text = fs::read_to_string(&self.settings_notes_path)?;
        }

        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(Self::config_dir())?;
        fs::write(&self.settings_links_path, self.links_items.join("\n"))?;
        fs::write(&self.settings_notes_path, &self.notes_text)?;
        let settings = format!(
            "links_path={}\nnotes_path={}\n",
            self.settings_links_path, self.settings_notes_path
        );
        let settings_path = Self::config_dir().join("settings.txt");
        fs::write(&settings_path, settings)?;
        Ok(())
    }

    pub fn handle_key(&mut self, key: KeyCode, modifiers: crossterm::event::KeyModifiers) -> bool {
        if key == KeyCode::Esc || (modifiers.contains(crossterm::event::KeyModifiers::CONTROL) && key == KeyCode::Char('c')) {
            return true;
        }

        match key {
            KeyCode::Char('1') => {
                self.current_tab = CurrentTab::Links;
                self.focus = Focus::LinksInput;
                return false;
            }
            KeyCode::Char('2') => {
                self.current_tab = CurrentTab::Notes;
                self.focus = Focus::NotesText;
                return false;
            }
            KeyCode::Char('3') => {
                self.current_tab = CurrentTab::Settings;
                self.focus = Focus::SettingsLinksPath;
                return false;
            }
            _ => {}
        }

        match self.current_tab {
            CurrentTab::Links => {
                self.handle_links(key);
            }
            CurrentTab::Notes => {
                self.handle_notes(key);
            }
            CurrentTab::Settings => {
                self.handle_settings(key);
            }
        }
        false
    }

    fn handle_links(&mut self, key: KeyCode) {
        match self.focus {
            Focus::LinksInput => match key {
                KeyCode::Char(c) => self.links_input.push(c),
                KeyCode::Backspace => {
                    self.links_input.pop();
                }
                KeyCode::Tab => self.focus = Focus::LinksButton,
                KeyCode::Enter => {
                    if !self.links_input.trim().is_empty() {
                        self.links_items.push(self.links_input.trim().to_string());
                        self.links_input.clear();
                    }
                }
                _ => {}
            },
            Focus::LinksButton => match key {
                KeyCode::Tab => self.focus = Focus::LinksList,
                KeyCode::BackTab => self.focus = Focus::LinksInput,
                KeyCode::Enter => {
                    if !self.links_input.trim().is_empty() {
                        self.links_items.push(self.links_input.trim().to_string());
                        self.links_input.clear();
                        self.focus = Focus::LinksInput;
                    }
                }
                _ => {}
            },
            Focus::LinksList => match key {
                KeyCode::Tab => self.focus = Focus::LinksInput,
                KeyCode::BackTab => self.focus = Focus::LinksButton,
                KeyCode::Up => {
                    if let Some(sel) = self.links_selected {
                        if sel > 0 {
                            self.links_selected = Some(sel - 1);
                        }
                    } else if !self.links_items.is_empty() {
                        self.links_selected = Some(self.links_items.len() - 1);
                    }
                }
                KeyCode::Down => {
                    if let Some(sel) = self.links_selected {
                        if sel + 1 < self.links_items.len() {
                            self.links_selected = Some(sel + 1);
                        }
                    } else if !self.links_items.is_empty() {
                        self.links_selected = Some(0);
                    }
                }
                KeyCode::Delete => {
                    if let Some(sel) = self.links_selected {
                        self.links_items.remove(sel);
                        if self.links_items.is_empty() {
                            self.links_selected = None;
                        } else if sel >= self.links_items.len() {
                            self.links_selected = Some(self.links_items.len() - 1);
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn handle_notes(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char(c) => self.notes_text.push(c),
            KeyCode::Enter => self.notes_text.push('\n'),
            KeyCode::Backspace => {
                self.notes_text.pop();
            }
            _ => {}
        }
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
                KeyCode::Tab => self.focus = Focus::SettingsSaveButton,
                _ => {}
            },
            Focus::SettingsSaveButton => match key {
                KeyCode::BackTab => self.focus = Focus::SettingsNotesPath,
                KeyCode::Tab => self.focus = Focus::SettingsLinksPath,
                KeyCode::Enter => {
                    let _ = self.save();
                }
                _ => {}
            },
            _ => {}
        }
    }
}
