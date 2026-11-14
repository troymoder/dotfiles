// SPDX-License-Identifier: GPL-3.0-only

//! Apps' icons, from their desktop entries: PipeWire and notifications name
//! the app, or its binary, and the entry names the icon. And the apps
//! themselves, for the overview's dock and search: listed, matched to their
//! windows, found by name, started.

use std::{collections::HashMap, sync::Mutex};

struct Entry {
    id: String,
    name: String,
    /// The name as written, to show.
    title: String,
    wm_class: String,
    exec: String,
    icon: String,
    /// The whole Exec line.
    command: String,
    keywords: String,
    generic: String,
    comment: String,
    terminal: bool,
    hidden: bool,
    /// The entry's own actions ("New Incognito Window"): name and command.
    actions: Vec<(String, String)>,
}

/// A desktop entry's actions, in the order its `Actions` key lists them:
/// each `[Desktop Action id]` group's Name and Exec.
fn actions(text: &str) -> Vec<(String, String)> {
    let mut listed: Vec<String> = Vec::new();
    let mut groups: HashMap<String, (String, String)> = HashMap::new();
    let mut group = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            group = name.to_owned();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if group == "Desktop Entry" && key == "Actions" {
            listed = value
                .split(';')
                .filter(|a| !a.is_empty())
                .map(str::to_owned)
                .collect();
        } else if let Some(id) = group.strip_prefix("Desktop Action ") {
            let entry = groups.entry(id.to_owned()).or_default();
            match key {
                "Name" => entry.0 = value.to_owned(),
                "Exec" => entry.1 = value.to_owned(),
                _ => {}
            }
        }
    }
    listed
        .iter()
        .filter_map(|id| groups.remove(id))
        .filter(|(name, exec)| !name.is_empty() && !exec.is_empty())
        .collect()
}

fn entries() -> Vec<Entry> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data_home =
        std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));
    let data_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());
    let dirs = std::iter::once(data_home)
        .chain(data_dirs.split(':').map(str::to_owned))
        .chain([
            format!("{home}/.nix-profile/share"),
            "/run/current-system/sw/share".to_owned(),
            "/etc/profiles/per-user".to_owned()
                + &format!("/{}/share", std::env::var("USER").unwrap_or_default()),
        ]);
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
                id: path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase(),
                name: String::new(),
                title: String::new(),
                wm_class: String::new(),
                exec: String::new(),
                icon: String::new(),
                command: String::new(),
                keywords: String::new(),
                generic: String::new(),
                comment: String::new(),
                terminal: false,
                hidden: false,
                actions: actions(&text),
            };
            for line in text
                .lines()
                .skip_while(|l| l.trim() != "[Desktop Entry]")
                .skip(1)
                .take_while(|l| !l.starts_with('['))
            {
                match line.split_once('=') {
                    Some(("Name", value)) => {
                        entry.title = value.trim().to_owned();
                        entry.name = entry.title.to_lowercase();
                    }
                    Some(("StartupWMClass", value)) => entry.wm_class = value.trim().to_lowercase(),
                    Some(("Icon", value)) => entry.icon = value.trim().to_owned(),
                    Some(("Keywords", value)) => entry.keywords = value.trim().to_lowercase(),
                    Some(("GenericName", value)) => entry.generic = value.trim().to_lowercase(),
                    Some(("Comment", value)) => entry.comment = value.trim().to_owned(),
                    Some(("Terminal", value)) => entry.terminal = value.trim() == "true",
                    Some(("NoDisplay" | "Hidden", value)) => {
                        entry.hidden |= value.trim() == "true";
                    }
                    Some(("Type", value)) => entry.hidden |= value.trim() != "Application",
                    Some(("Exec", value)) => {
                        entry.command = value.trim().to_owned();
                        entry.exec = value
                            .split_whitespace()
                            .next()
                            .and_then(|e| e.rsplit('/').next())
                            .unwrap_or_default()
                            .to_lowercase();
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

/// The icon of the app called `app`, run as `binary` if known.
pub fn icon(app: &str, binary: Option<&str>) -> Option<String> {
    find(app, binary).map(|(_, icon)| icon)
}

/// The name and icon of the app called `app` (its app id will do), run as
/// `binary` if known, from its desktop entry; looked up once per name.
pub fn find(app: &str, binary: Option<&str>) -> Option<(String, String)> {
    static ENTRIES: Mutex<Option<Vec<Entry>>> = Mutex::new(None);
    static FOUND: Mutex<Option<HashMap<String, Option<(String, String)>>>> = Mutex::new(None);
    let key = format!("{app}\u{0}{}", binary.unwrap_or_default());
    if let Some(found) = FOUND.lock().ok()?.get_or_insert_default().get(&key) {
        return found.clone();
    }
    let app = app.trim().to_lowercase();
    let binary = binary
        .map(|b| b.trim().to_lowercase())
        .filter(|b| !b.is_empty());
    let dashed = app.replace(' ', "-");
    let found = {
        let mut entries = ENTRIES.lock().ok()?;
        let entries = entries.get_or_insert_with(self::entries);
        entries
            .iter()
            .find(|e| e.name == app)
            .or_else(|| {
                entries
                    .iter()
                    .find(|e| e.wm_class == app || e.id == dashed || e.id == app)
            })
            .or_else(|| {
                entries
                    .iter()
                    .find(|e| e.id.rsplit('.').next() == Some(&dashed))
            })
            .or_else(|| {
                binary.as_ref().and_then(|b| {
                    entries
                        .iter()
                        .find(|e| &e.exec == b || e.id == *b || e.id.rsplit('.').next() == Some(b))
                })
            })
            .map(|e| (e.title.clone(), e.icon.clone()))
    };
    FOUND
        .lock()
        .ok()?
        .get_or_insert_default()
        .insert(key, found.clone());
    found
}

/// An app the overview's dock and search show, and can start.
#[derive(Clone, Debug, PartialEq)]
pub struct App {
    /// Its desktop entry's name, lower case (`com.mitchellh.ghostty`).
    pub id: String,
    pub name: String,
    pub icon: String,
    /// What it is, from its entry's comment ("Access the Internet").
    pub about: String,
    command: String,
    terminal: bool,
    wm_class: String,
    /// Lower case: what a search looks in besides the name.
    keywords: String,
    /// Its own actions ("New Incognito Window"): name and command.
    pub actions: Vec<(String, String)>,
}

/// Every app a person would start: shown entries with a command, the
/// first of each id (the user's own entries come first).
pub fn all() -> Vec<App> {
    let mut seen = std::collections::HashSet::new();
    entries()
        .into_iter()
        .filter(|e| seen.insert(e.id.clone()) && !e.hidden && !e.command.is_empty())
        .map(|e| App {
            keywords: format!("{} {} {}", e.keywords, e.generic, e.exec),
            id: e.id,
            name: e.title,
            icon: e.icon,
            about: e.comment,
            command: e.command,
            terminal: e.terminal,
            wm_class: e.wm_class,
            actions: e.actions,
        })
        .collect()
}

impl App {
    /// An app by name alone, for other modules' tests.
    #[cfg(test)]
    pub fn test(name: &str) -> Self {
        App {
            id: name.to_lowercase(),
            name: name.into(),
            icon: String::new(),
            about: String::new(),
            command: String::new(),
            terminal: false,
            wm_class: String::new(),
            keywords: String::new(),
            actions: Vec::new(),
        }
    }

    /// Whether a window with this app id is this app's.
    pub fn owns(&self, app_id: &str) -> bool {
        let app_id = app_id.to_lowercase();
        self.id == app_id
            || self.wm_class == app_id
            || self.id.rsplit('.').next() == Some(app_id.as_str())
            || app_id.rsplit('.').next() == Some(self.id.as_str())
    }

    /// Started, as its desktop entry says (a terminal one in Ghostty).
    pub fn launch(&self) {
        self.run(&self.command);
    }

    /// One of its own actions, by its place in `actions`.
    pub fn launch_action(&self, index: usize) {
        if let Some((_, exec)) = self.actions.get(index) {
            self.run(exec);
        }
    }

    fn run(&self, exec: &str) {
        let mut args = exec_args(exec);
        if self.terminal {
            args.splice(0..0, ["ghostty".to_owned(), "-e".to_owned()]);
        }
        let Some((program, rest)) = args.split_first() else {
            return tracing::warn!("{} has no command", self.id);
        };
        let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
        super::spawn(program, &rest, &[]);
    }
}

/// An Exec line's words: quoting undone, field codes (`%U`, `%f`…) dropped
/// as nothing is passed, `%%` a `%`.
fn exec_args(exec: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut word = String::new();
    let (mut quoted, mut started) = (false, false);
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            '%' => match chars.next() {
                Some('%') => word.push('%'),
                Some(_) | None => {}
            },
            c if c.is_whitespace() && !quoted => {
                if started || !word.is_empty() {
                    args.push(std::mem::take(&mut word));
                }
                started = false;
            }
            c => word.push(c),
        }
    }
    if started || !word.is_empty() {
        args.push(word);
    }
    args
}

/// The apps pinned to the dock, by id: COSMIC's own list (the dotfiles set
/// it), so it stays the one list.
pub fn favorites() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let config = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"));
    std::fs::read_to_string(format!(
        "{config}/cosmic/com.system76.CosmicAppList/v1/favorites"
    ))
    .map(|text| {
        text.split('"')
            .skip(1)
            .step_by(2)
            .map(|id| id.to_lowercase())
            .collect()
    })
    .unwrap_or_default()
}

/// The apps matching `query`, best first: a name starting with it, then a
/// word of the name starting with it, then a name holding it, then its
/// keywords, generic name or command holding it.
pub fn search<'a>(apps: &'a [App], query: &str) -> Vec<&'a App> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(u8, &App)> = apps
        .iter()
        .filter_map(|app| {
            let name = app.name.to_lowercase();
            let rank = if name.starts_with(&query) {
                0
            } else if name.split_whitespace().any(|w| w.starts_with(&query)) {
                1
            } else if name.contains(&query) {
                2
            } else if app.keywords.contains(&query) {
                3
            } else {
                return None;
            };
            Some((rank, app))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    found.into_iter().map(|(_, app)| app).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str, keywords: &str) -> App {
        App {
            id: id.into(),
            name: name.into(),
            icon: String::new(),
            about: String::new(),
            command: String::new(),
            terminal: false,
            wm_class: String::new(),
            keywords: keywords.into(),
            actions: Vec::new(),
        }
    }

    #[test]
    fn actions_come_in_their_listed_order() {
        let entry = "[Desktop Entry]\nName=Chromium\nExec=chromium %U\nActions=new-window;new-private-window;gone;\n\n\
                     [Desktop Action new-private-window]\nName=New Incognito Window\nExec=chromium --incognito\n\n\
                     [Desktop Action new-window]\nName=New Window\nExec=chromium\n\n\
                     [Desktop Action unlisted]\nName=Hidden\nExec=x\n";
        assert_eq!(
            actions(entry),
            [
                ("New Window".to_owned(), "chromium".to_owned()),
                (
                    "New Incognito Window".to_owned(),
                    "chromium --incognito".to_owned()
                ),
            ]
        );
        assert!(actions("[Desktop Entry]\nName=Plain\n").is_empty());
    }

    #[test]
    fn exec_lines_come_apart_as_the_spec_says() {
        assert_eq!(exec_args("code %F"), ["code"]);
        assert_eq!(
            exec_args("ghostty --gtk-single-instance=true"),
            ["ghostty", "--gtk-single-instance=true"]
        );
        assert_eq!(
            exec_args(r#""/opt/My App/run" --name "a \"b\"" %U"#),
            ["/opt/My App/run", "--name", "a \"b\""]
        );
        assert_eq!(exec_args("printf 100%%"), ["printf", "100%"]);
        assert_eq!(exec_args("env A=1  app"), ["env", "A=1", "app"]);
    }

    #[test]
    fn windows_find_their_apps() {
        assert!(app("code", "Visual Studio Code", "").owns("code"));
        assert!(app("com.mitchellh.ghostty", "Ghostty", "").owns("com.mitchellh.ghostty"));
        assert!(app("org.gnome.nautilus", "Files", "").owns("nautilus"));
        assert!(!app("code", "Visual Studio Code", "").owns("chromium-browser"));
    }

    /// Prints the dock's favorites as found, and what a few searches give.
    #[test]
    #[ignore = "reads this machine's desktop entries"]
    fn lists_the_apps() {
        let apps = all();
        println!("{} apps", apps.len());
        for id in favorites() {
            let found = apps.iter().find(|a| a.id == id);
            println!("favorite {id}: {:?}", found.map(|a| (&a.name, &a.icon)));
        }
        for query in ["fir", "term", "set", "code"] {
            let names: Vec<&str> = search(&apps, query)
                .iter()
                .take(5)
                .map(|a| a.name.as_str())
                .collect();
            println!("{query:>5}: {names:?}");
        }
        assert!(!apps.is_empty());
    }

    #[test]
    fn search_puts_the_likeliest_first() {
        let apps = [
            app("a", "Visual Studio Code", "editor"),
            app("b", "Code Editor", ""),
            app("c", "Barcode Reader", ""),
            app("d", "Chromium", "browser web"),
        ];
        let names = |q| -> Vec<&str> { search(&apps, q).iter().map(|a| a.name.as_str()).collect() };
        assert_eq!(
            names("code"),
            ["Code Editor", "Visual Studio Code", "Barcode Reader"]
        );
        assert_eq!(names("web"), ["Chromium"]);
        assert_eq!(names("CHRO"), ["Chromium"]);
        assert!(names("  ").is_empty());
        assert!(names("zzz").is_empty());
    }
}
