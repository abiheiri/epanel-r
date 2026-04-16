//! Safari bookmarks sync for macOS.
//!
//! Watches Safari's Bookmarks.plist and bidirectionally syncs
//! bookmarks and reading list with epanel's folder structure.
//!
//! Author: Al Biheiri <al@forgottheaddress.com>

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use notify::{Event, RecursiveMode, Watcher};
use plist::Value;
use uuid::Uuid;

use crate::app::{Entry, Folder};

#[derive(Debug)]
pub struct SafariSyncManager {
    #[allow(dead_code)]
    path: PathBuf,
    _watcher: notify::RecommendedWatcher,
}

impl SafariSyncManager {
    pub fn new(
        path: impl AsRef<Path>,
        on_change: impl Fn() + Send + 'static,
    ) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let (tx, rx) = mpsc::channel::<()>();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                match event.kind {
                    notify::EventKind::Modify(_) | notify::EventKind::Remove(_) => {
                        let _ = tx.send(());
                    }
                    _ => {}
                }
            }
        })?;

        watcher.watch(&path, RecursiveMode::NonRecursive)?;

        let watch_path = path.clone();
        std::thread::spawn(move || {
            let mut last_modified: Option<std::time::SystemTime> = None;
            loop {
                match rx.recv_timeout(Duration::from_secs(30)) {
                    Ok(()) => {
                        std::thread::sleep(Duration::from_millis(500));
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }

                if let Ok(meta) = fs::metadata(&watch_path) {
                    if let Ok(modified) = meta.modified() {
                        if last_modified == Some(modified) {
                            continue;
                        }
                        last_modified = Some(modified);
                        on_change();
                    }
                }
            }
        });

        Ok(Self {
            path,
            _watcher: watcher,
        })
    }
}

pub fn parse_safari_plist(path: impl AsRef<Path>) -> Result<(Vec<Folder>, Folder), String> {
    let value: Value = plist::from_file(&path).map_err(|e| format!("Plist parse error: {}", e))?;
    let dict = value.into_dictionary().ok_or("Root plist value is not a dictionary")?;
    let children = dict.get("Children").ok_or("Missing 'Children' key in root dictionary")?.as_array().ok_or("'Children' is not an array")?;

    let mut bookmark_folders = Vec::new();
    let mut reading_list = Folder::new("Reading List");

    for child in children {
        let child_dict = child.as_dictionary().ok_or("Child item is not a dictionary")?;
        let title = child_dict
            .get("Title")
            .and_then(|v| v.as_string())
            .unwrap_or("");
        let bookmark_type = child_dict
            .get("WebBookmarkType")
            .and_then(|v| v.as_string())
            .unwrap_or("");

        if bookmark_type != "WebBookmarkTypeList" {
            continue;
        }
        let empty: Vec<Value> = Vec::new();
        let sub_children = child_dict
            .get("Children")
            .and_then(|v| v.as_array())
            .unwrap_or(&empty);

        if title == "com.apple.ReadingList" {
            reading_list = convert_children(sub_children);
            reading_list.name = "Reading List".to_string();
        } else {
            let mut subfolder = convert_children(sub_children);
            subfolder.name = match title {
                "BookmarksBar" => "Favorites".to_string(),
                "BookmarksMenu" => "Bookmarks Menu".to_string(),
                _ => {
                    if title.is_empty() {
                        "Untitled Folder".to_string()
                    } else {
                        title.to_string()
                    }
                }
            };
            bookmark_folders.push(subfolder);
        }
    }

    Ok((bookmark_folders, reading_list))
}

fn convert_children(children: &[Value]) -> Folder {
    let mut folder = Folder::new("");
    for child in children {
        let Some(child_dict) = child.as_dictionary() else {
            continue;
        };
        let bookmark_type = child_dict
            .get("WebBookmarkType")
            .and_then(|v| v.as_string())
            .unwrap_or("");

        if bookmark_type == "WebBookmarkTypeLeaf" {
            if let Some(url) = child_dict
                .get("URLString")
                .and_then(|v| v.as_string())
                .or_else(|| child_dict.get("URL").and_then(|v| v.as_string()))
            {
                if !url.is_empty() {
                    folder.entries.push(Entry::new(url.to_string()));
                }
            }
        } else if bookmark_type == "WebBookmarkTypeList" {
            let title = child_dict
                .get("Title")
                .and_then(|v| v.as_string())
                .unwrap_or("");
            if let Some(sub_children) = child_dict.get("Children").and_then(|v| v.as_array()) {
                let mut subfolder = convert_children(sub_children);
                subfolder.name = if title.is_empty() {
                    "Untitled Folder".to_string()
                } else {
                    title.to_string()
                };
                folder.subfolders.push(subfolder);
            }
        }
    }
    folder
}

pub fn writeback_safari_plist(path: impl AsRef<Path>, root_folder: &Folder) -> Result<()> {
    let path = path.as_ref();
    let existing: Value = plist::from_file(path)?;
    let mut existing_dict = existing.into_dictionary().unwrap_or_default();
    let existing_children = existing_dict
        .get("Children")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut url_to_leaf: HashMap<String, plist::Dictionary> = HashMap::new();
    collect_leaves(&existing_children, &mut url_to_leaf);

    let mut title_to_folder: HashMap<String, plist::Dictionary> = HashMap::new();
    collect_folders(&existing_children, &mut title_to_folder);

    let mut new_children: Vec<Value> = Vec::new();

    // Preserve proxy entries
    for child in &existing_children {
        if let Some(dict) = child.as_dictionary() {
            if dict
                .get("WebBookmarkType")
                .and_then(|v| v.as_string())
                == Some("WebBookmarkTypeProxy")
            {
                new_children.push(child.clone());
            }
        }
    }

    let favorites_folder = root_folder
        .subfolders
        .iter()
        .find(|f| f.name == "Favorites");
    let bookmarks_menu_folder = root_folder
        .subfolders
        .iter()
        .find(|f| f.name == "Bookmarks Menu");
    let reading_list_folder = root_folder
        .subfolders
        .iter()
        .find(|f| f.name == "Reading List");

    let special_names: std::collections::HashSet<&str> =
        ["Favorites", "Bookmarks Menu", "Reading List", "my_original_epanel"]
            .iter()
            .copied()
            .collect();
    let other_folders: Vec<&Folder> = root_folder
        .subfolders
        .iter()
        .filter(|f| !special_names.contains(f.name.as_str()))
        .collect();

    // BookmarksBar (Favorites)
    if let Some(fav) = favorites_folder {
        new_children.push(Value::Dictionary(build_safari_folder(
            fav,
            "BookmarksBar",
            &url_to_leaf,
            &title_to_folder,
        )));
    } else if let Some(existing) = existing_children.iter().find(|c| {
        c.as_dictionary()
            .and_then(|d| d.get("Title"))
            .and_then(|v| v.as_string())
            == Some("BookmarksBar")
    }) {
        new_children.push(existing.clone());
    }

    // BookmarksMenu
    let bm_entries: Vec<Entry> = bookmarks_menu_folder
        .map(|f| f.entries.clone())
        .unwrap_or_default()
        .into_iter()
        .chain(root_folder.entries.clone())
        .collect();
    let bm_subfolders: Vec<Folder> = bookmarks_menu_folder
        .map(|f| f.subfolders.clone())
        .unwrap_or_default()
        .into_iter()
        .chain(other_folders.into_iter().cloned())
        .collect();
    let merged_bm = Folder {
        name: "Bookmarks Menu".to_string(),
        entries: bm_entries,
        subfolders: bm_subfolders,
        ..Folder::new("")
    };
    new_children.push(Value::Dictionary(build_safari_folder(
        &merged_bm,
        "BookmarksMenu",
        &url_to_leaf,
        &title_to_folder,
    )));

    // Reading List
    if let Some(rl) = reading_list_folder {
        let existing_rl = existing_children.iter().find(|c| {
            c.as_dictionary()
                .and_then(|d| d.get("Title"))
                .and_then(|v| v.as_string())
                == Some("com.apple.ReadingList")
        });
        new_children.push(Value::Dictionary(build_safari_reading_list(
            rl,
            &url_to_leaf,
            existing_rl,
        )));
    } else if let Some(existing) = existing_children.iter().find(|c| {
        c.as_dictionary()
            .and_then(|d| d.get("Title"))
            .and_then(|v| v.as_string())
            == Some("com.apple.ReadingList")
    }) {
        new_children.push(existing.clone());
    }

    existing_dict.insert("Children".to_string(), Value::Array(new_children));

    let mut buf = Vec::new();
    plist::to_writer_binary(&mut buf, &Value::Dictionary(existing_dict))?;

    let p: &Path = path.as_ref();
    let temp_path = p.with_extension("tmp");
    fs::write(&temp_path, buf)?;
    fs::rename(&temp_path, path)?;

    Ok(())
}

fn collect_leaves(children: &[Value], map: &mut HashMap<String, plist::Dictionary>) {
    for child in children {
        let Some(dict) = child.as_dictionary() else {
            continue;
        };
        let bm_type = dict
            .get("WebBookmarkType")
            .and_then(|v| v.as_string())
            .unwrap_or("");
        if bm_type == "WebBookmarkTypeLeaf" {
            if let Some(url) = dict.get("URLString").and_then(|v| v.as_string()) {
                let normalized = url.to_lowercase();
                map.insert(normalized, dict.clone());
            }
        } else if bm_type == "WebBookmarkTypeList" {
            if let Some(sub) = dict.get("Children").and_then(|v| v.as_array()) {
                collect_leaves(sub, map);
            }
        }
    }
}

fn collect_folders(children: &[Value], map: &mut HashMap<String, plist::Dictionary>) {
    for child in children {
        let Some(dict) = child.as_dictionary() else {
            continue;
        };
        if dict
            .get("WebBookmarkType")
            .and_then(|v| v.as_string())
            == Some("WebBookmarkTypeList")
        {
            if let Some(title) = dict.get("Title").and_then(|v| v.as_string()) {
                map.insert(title.to_string(), dict.clone());
            }
            if let Some(sub) = dict.get("Children").and_then(|v| v.as_array()) {
                collect_folders(sub, map);
            }
        }
    }
}

fn build_safari_folder(
    folder: &Folder,
    safari_title: &str,
    url_to_leaf: &HashMap<String, plist::Dictionary>,
    title_to_folder: &HashMap<String, plist::Dictionary>,
) -> plist::Dictionary {
    let existing = title_to_folder.get(safari_title);
    let mut dict = plist::Dictionary::new();
    dict.insert(
        "WebBookmarkType".to_string(),
        Value::String("WebBookmarkTypeList".to_string()),
    );
    dict.insert("Title".to_string(), Value::String(safari_title.to_string()));
    dict.insert(
        "WebBookmarkUUID".to_string(),
        Value::String(
            existing
                .and_then(|d| d.get("WebBookmarkUUID"))
                .and_then(|v| v.as_string())
                .map(|s| s.to_string())
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
        ),
    );

    if let Some(existing) = existing {
        for (key, value) in existing.iter() {
            if key != "Children" && !dict.contains_key(key) {
                dict.insert(key.clone(), value.clone());
            }
        }
    }

    let mut children: Vec<Value> = Vec::new();
    for sub in &folder.subfolders {
        children.push(Value::Dictionary(build_safari_subfolder(
            sub,
            url_to_leaf,
            title_to_folder,
        )));
    }
    for entry in &folder.entries {
        children.push(Value::Dictionary(build_safari_leaf(entry, url_to_leaf)));
    }
    if !children.is_empty() {
        dict.insert("Children".to_string(), Value::Array(children));
    }

    dict
}

fn build_safari_subfolder(
    folder: &Folder,
    url_to_leaf: &HashMap<String, plist::Dictionary>,
    title_to_folder: &HashMap<String, plist::Dictionary>,
) -> plist::Dictionary {
    let existing = title_to_folder.get(&folder.name);
    let mut dict = plist::Dictionary::new();
    dict.insert(
        "WebBookmarkType".to_string(),
        Value::String("WebBookmarkTypeList".to_string()),
    );
    dict.insert("Title".to_string(), Value::String(folder.name.clone()));
    dict.insert(
        "WebBookmarkUUID".to_string(),
        Value::String(
            existing
                .and_then(|d| d.get("WebBookmarkUUID"))
                .and_then(|v| v.as_string())
                .map(|s| s.to_string())
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
        ),
    );

    if let Some(existing) = existing {
        for (key, value) in existing.iter() {
            if key != "Children" && !dict.contains_key(key) {
                dict.insert(key.clone(), value.clone());
            }
        }
    }

    let mut children: Vec<Value> = Vec::new();
    for sub in &folder.subfolders {
        children.push(Value::Dictionary(build_safari_subfolder(
            sub,
            url_to_leaf,
            title_to_folder,
        )));
    }
    for entry in &folder.entries {
        children.push(Value::Dictionary(build_safari_leaf(entry, url_to_leaf)));
    }
    if !children.is_empty() {
        dict.insert("Children".to_string(), Value::Array(children));
    }

    dict
}

fn build_safari_leaf(
    entry: &Entry,
    url_to_leaf: &HashMap<String, plist::Dictionary>,
) -> plist::Dictionary {
    let normalized = entry.text.to_lowercase();
    if let Some(existing) = url_to_leaf.get(&normalized) {
        return existing.clone();
    }
    let mut dict = plist::Dictionary::new();
    dict.insert(
        "WebBookmarkType".to_string(),
        Value::String("WebBookmarkTypeLeaf".to_string()),
    );
    dict.insert(
        "WebBookmarkUUID".to_string(),
        Value::String(Uuid::new_v4().to_string()),
    );
    dict.insert("URLString".to_string(), Value::String(entry.text.clone()));
    let mut uri_dict = plist::Dictionary::new();
    uri_dict.insert("title".to_string(), Value::String(entry.text.clone()));
    dict.insert("URIDictionary".to_string(), Value::Dictionary(uri_dict));
    dict
}

fn build_safari_reading_list(
    folder: &Folder,
    url_to_leaf: &HashMap<String, plist::Dictionary>,
    existing_rl: Option<&Value>,
) -> plist::Dictionary {
    let existing = existing_rl.and_then(|v| v.as_dictionary());
    let mut dict = plist::Dictionary::new();
    dict.insert(
        "WebBookmarkType".to_string(),
        Value::String("WebBookmarkTypeList".to_string()),
    );
    dict.insert(
        "Title".to_string(),
        Value::String("com.apple.ReadingList".to_string()),
    );
    dict.insert(
        "WebBookmarkUUID".to_string(),
        Value::String(
            existing
                .and_then(|d| d.get("WebBookmarkUUID"))
                .and_then(|v| v.as_string())
                .map(|s| s.to_string())
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
        ),
    );

    if let Some(existing) = existing {
        for (key, value) in existing.iter() {
            if key != "Children" && !dict.contains_key(key) {
                dict.insert(key.clone(), value.clone());
            }
        }
    }

    let mut children: Vec<Value> = Vec::new();
    for entry in &folder.entries {
        let normalized = entry.text.to_lowercase();
        if let Some(existing) = url_to_leaf.get(&normalized) {
            children.push(Value::Dictionary(existing.clone()));
        } else {
            let mut child = plist::Dictionary::new();
            child.insert(
                "WebBookmarkType".to_string(),
                Value::String("WebBookmarkTypeLeaf".to_string()),
            );
            child.insert(
                "WebBookmarkUUID".to_string(),
                Value::String(Uuid::new_v4().to_string()),
            );
            child.insert("URLString".to_string(), Value::String(entry.text.clone()));
            let mut uri_dict = plist::Dictionary::new();
            uri_dict.insert("title".to_string(), Value::String(entry.text.clone()));
            child.insert("URIDictionary".to_string(), Value::Dictionary(uri_dict));

            let mut rl_dict = plist::Dictionary::new();
            rl_dict.insert(
                "DateAdded".to_string(),
                Value::Date(
                    plist::Date::from_xml_format(
                        &entry.date.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                    )
                    .unwrap_or_else(|_| {
                        plist::Date::from_xml_format("1970-01-01T00:00:00Z").unwrap()
                    }),
                ),
            );
            rl_dict.insert("PreviewText".to_string(), Value::String("".to_string()));
            child.insert("ReadingList".to_string(), Value::Dictionary(rl_dict));

            let mut rl_ns = plist::Dictionary::new();
            rl_ns.insert("neverFetchMetadata".to_string(), Value::Boolean(false));
            child.insert("ReadingListNonSync".to_string(), Value::Dictionary(rl_ns));

            children.push(Value::Dictionary(child));
        }
    }
    if !children.is_empty() {
        dict.insert("Children".to_string(), Value::Array(children));
    }

    dict
}
