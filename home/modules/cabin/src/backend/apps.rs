// SPDX-License-Identifier: GPL-3.0-only

//! Apps' icons, from their desktop entries: PipeWire and notifications name
//! the app, or its binary, and the entry names the icon.

use std::{collections::HashMap, sync::Mutex};

struct Entry {
    id: String,
    name: String,
    wm_class: String,
    exec: String,
    icon: String,
}

fn entries() -> Vec<Entry> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));
    let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());
    let dirs = std::iter::once(data_home)
        .chain(data_dirs.split(':').map(str::to_owned))
        .chain([format!("{home}/.nix-profile/share"), "/run/current-system/sw/share".to_owned(), "/etc/profiles/per-user".to_owned() + &format!("/{}/share", std::env::var("USER").unwrap_or_default())]);
    let mut entries = Vec::new();
    for dir in dirs {
        let Ok(files) = std::fs::read_dir(format!("{dir}/applications")) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_none_or(|e| e != "desktop") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let mut entry = Entry {
                id: path.file_stem().unwrap_or_default().to_string_lossy().to_lowercase(),
                name: String::new(),
                wm_class: String::new(),
                exec: String::new(),
                icon: String::new(),
            };
            for line in text.lines().skip_while(|l| l.trim() != "[Desktop Entry]").skip(1).take_while(|l| !l.starts_with('[')) {
                match line.split_once('=') {
                    Some(("Name", value)) => entry.name = value.trim().to_lowercase(),
                    Some(("StartupWMClass", value)) => entry.wm_class = value.trim().to_lowercase(),
                    Some(("Icon", value)) => entry.icon = value.trim().to_owned(),
                    Some(("Exec", value)) => {
                        entry.exec = value.split_whitespace().next().and_then(|e| e.rsplit('/').next()).unwrap_or_default().to_lowercase();
                    }
                    _ => {}
                }
            }
            if !entry.icon.is_empty() {
                entries.push(entry);
            }
        }
    }
    entries
}

/// The icon of the app called `app`, run as `binary` if known; looked up
/// once per name.
pub fn icon(app: &str, binary: Option<&str>) -> Option<String> {
    static ENTRIES: Mutex<Option<Vec<Entry>>> = Mutex::new(None);
    static FOUND: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);
    let key = format!("{app}\u{0}{}", binary.unwrap_or_default());
    if let Some(found) = FOUND.lock().ok()?.get_or_insert_default().get(&key) {
        return found.clone();
    }
    let app = app.trim().to_lowercase();
    let binary = binary.map(|b| b.trim().to_lowercase()).filter(|b| !b.is_empty());
    let dashed = app.replace(' ', "-");
    let found = {
        let mut entries = ENTRIES.lock().ok()?;
        let entries = entries.get_or_insert_with(self::entries);
        entries
            .iter()
            .find(|e| e.name == app)
            .or_else(|| entries.iter().find(|e| e.wm_class == app || e.id == dashed || e.id == app))
            .or_else(|| entries.iter().find(|e| e.id.rsplit('.').next() == Some(&dashed)))
            .or_else(|| binary.as_ref().and_then(|b| entries.iter().find(|e| &e.exec == b || e.id == *b || e.id.rsplit('.').next() == Some(b))))
            .map(|e| e.icon.clone())
    };
    FOUND.lock().ok()?.get_or_insert_default().insert(key, found.clone());
    found
}
