// SPDX-License-Identifier: GPL-3.0-only

//! What the search finds besides the apps, the windows and the web, by the
//! prefixes pop-launcher's plugins took for COSMIC's launcher: `=` works a
//! sum out with qalc (so does a query that starts like one), `/` and `~/`
//! list a folder (Tab completes), `find` looks for a file under home with
//! fd, `recent` searches the files used lately, `run` and `t:` run a
//! command in a terminal and `:` in the background; `?` lists these.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// At most this many files.
const MAX: usize = 24;
/// fd isn't run for each letter typed: only once typing stops this long.
const FIND_AFTER: Duration = Duration::from_millis(200);
/// A program that takes longer than this is given up on.
const LIMIT: Duration = Duration::from_secs(3);

/// What the query asks for, by its prefix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode<'a> {
    /// The apps, windows and web searches; a sum's answer too if it
    /// starts like one.
    Plain,
    Calc(&'a str),
    /// A path, as typed (`~/Doc`).
    Files(&'a str),
    Find(&'a str),
    Recent(&'a str),
    /// A command, and whether in a terminal.
    Run(&'a str, bool),
    /// What follows the `?`.
    Help(&'a str),
}

pub fn mode(query: &str) -> Mode<'_> {
    let q = query.trim_start();
    let after = |prefix: &str| q.strip_prefix(prefix).map(str::trim);
    if let Some(rest) = after("=") {
        Mode::Calc(rest)
    } else if q.starts_with('/') || q.starts_with('~') {
        Mode::Files(q)
    } else if let Some(rest) = after("find ") {
        Mode::Find(rest)
    } else if let Some(rest) = after("recent ") {
        Mode::Recent(rest)
    } else if let Some(rest) = after("t:").or_else(|| after("run ")) {
        Mode::Run(rest, true)
    } else if let Some(rest) = after(":") {
        Mode::Run(rest, false)
    } else if let Some(rest) = after("?") {
        Mode::Help(rest)
    } else {
        Mode::Plain
    }
}

/// Whether a plain query starts like a sum ("2*3", "(4+5)/2", "10 km to mi").
pub fn sum_like(query: &str) -> bool {
    query
        .trim_start()
        .starts_with(|c: char| c.is_ascii_digit() || c == '(')
}

/// Whether two queries ask for the same kind of thing: what one found
/// can stand while the other's is looked up.
pub fn same_kind(a: &str, b: &str) -> bool {
    std::mem::discriminant(&mode(a)) == std::mem::discriminant(&mode(b))
}

/// Whether `lookup` has anything to do for the query, and how long to
/// wait for more typing before it does.
pub fn wait(query: &str) -> Option<Duration> {
    match mode(query) {
        Mode::Plain if sum_like(query) => Some(Duration::ZERO),
        Mode::Calc(sum) if !sum.is_empty() => Some(Duration::ZERO),
        Mode::Files(_) | Mode::Recent(_) => Some(Duration::ZERO),
        Mode::Find(name) if !name.is_empty() => Some(FIND_AFTER),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    /// A sum and its answer.
    Calc(String, String),
    /// A file, or a folder (true).
    File(PathBuf, bool),
}

/// What the query finds by running a program or reading the disk:
/// blocking, so off the main loop.
pub fn lookup(query: &str) -> Vec<Hit> {
    match mode(query) {
        Mode::Plain if sum_like(query) => calc(query.trim()),
        Mode::Calc(sum) => calc(sum),
        Mode::Files(path) => list(path),
        Mode::Find(name) => find(name),
        Mode::Recent(name) => recent(name),
        _ => Vec::new(),
    }
}

/// A sum worked out by qalc (Qalculate!: units, currencies, functions);
/// none if the answer only says the sum again ("2", "1 / 0").
fn calc(sum: &str) -> Vec<Hit> {
    if sum.is_empty() {
        return Vec::new();
    }
    let mut qalc = Command::new("qalc");
    // After a space, so a sum starting with "-" isn't read as an option.
    qalc.args(["-t", "-set", "maxdeci 9"])
        .arg(format!(" {sum}"));
    let Some(out) = output(qalc) else {
        return Vec::new();
    };
    let answer = out
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or_default();
    let bare = |s: &str| s.split_whitespace().collect::<String>();
    if answer.is_empty() || bare(answer) == bare(sum) {
        return Vec::new();
    }
    vec![Hit::Calc(sum.to_owned(), answer.to_owned())]
}

/// A folder's entries whose names start with what's typed after its last
/// `/` (any case; hidden ones only once that starts with a dot), folders
/// first.
fn list(typed: &str) -> Vec<Hit> {
    let full = expand(typed);
    let Some(cut) = full.rfind('/') else {
        return Vec::new();
    };
    let (folder, start) = (&full[..=cut], full[cut + 1..].to_lowercase());
    let Ok(read) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<(bool, String, PathBuf)> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let shown = !name.starts_with('.') || start.starts_with('.');
            (shown && name.to_lowercase().starts_with(&start)).then(|| {
                let path = e.path();
                (path.is_dir(), name, path)
            })
        })
        .collect();
    found.sort_by_key(|(dir, name, _)| (!dir, name.to_lowercase()));
    found
        .into_iter()
        .take(MAX)
        .map(|(dir, _, path)| Hit::File(path, dir))
        .collect()
}

/// Files and folders under home whose names hold `name` (any case), by
/// fd, which leaves out hidden and git-ignored ones.
fn find(name: &str) -> Vec<Hit> {
    let mut fd = Command::new("fd");
    fd.args(["--ignore-case", "--fixed-strings", "--absolute-path"])
        .args(["--max-results", &MAX.to_string(), "--"])
        .arg(name)
        .current_dir(home());
    output(fd).map_or_else(Vec::new, |out| {
        out.lines()
            .map(|line| PathBuf::from(line.trim_end_matches('/')))
            .map(|path| {
                let dir = path.is_dir();
                Hit::File(path, dir)
            })
            .collect()
    })
}

/// The files used lately (GTK's list, which COSMIC's apps keep too) whose
/// names hold `name` (any case), the latest first; gone ones left out.
fn recent(name: &str) -> Vec<Hit> {
    let data =
        std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{}/.local/share", home()));
    let Ok(xml) = std::fs::read_to_string(format!("{data}/recently-used.xbel")) else {
        return Vec::new();
    };
    let name = name.to_lowercase();
    let mut used = used(&xml);
    used.sort_by(|a, b| b.0.cmp(&a.0));
    used.into_iter()
        .map(|(_, path)| path)
        .filter(|path| {
            path.file_name()
                .is_some_and(|n| n.to_string_lossy().to_lowercase().contains(&name))
        })
        .filter(|path| path.exists())
        .take(MAX)
        .map(|path| {
            let dir = path.is_dir();
            Hit::File(path, dir)
        })
        .collect()
}

/// Each local file in a `recently-used.xbel`, with when it was last
/// changed (ISO 8601, so it sorts as text).
fn used(xml: &str) -> Vec<(String, PathBuf)> {
    xml.split("<bookmark ")
        .skip(1)
        .filter_map(|chunk| {
            let tag = format!(" {}", &chunk[..chunk.find('>')?]);
            let path = attribute(&tag, "href")?.strip_prefix("file://")?;
            let when = attribute(&tag, "modified")
                .or_else(|| attribute(&tag, "visited"))
                .unwrap_or_default();
            Some((when.to_owned(), PathBuf::from(decode(&unescape(path)))))
        })
        .collect()
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

/// XML's escapes undone.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// A URI's `%xx` escapes undone.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| text.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// `~` (alone or before a `/`) as the home folder.
fn expand(typed: &str) -> String {
    match typed.strip_prefix('~') {
        Some("") => format!("{}/", home()),
        Some(rest) if rest.starts_with('/') => format!("{}{rest}", home()),
        _ => typed.to_owned(),
    }
}

/// A path to show: under home as `~/…`.
pub fn tilde(path: &Path) -> String {
    let shown = path.to_string_lossy();
    match shown.strip_prefix(&home()) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => shown.into_owned(),
    }
}

/// What the search becomes when Tab completes it to a file: its path in
/// the form typed (`~/` kept), a folder's with a `/` to go on into it.
pub fn complete(typed: &str, path: &Path, dir: bool) -> String {
    let mut done = if typed.starts_with('~') {
        tilde(path)
    } else {
        path.to_string_lossy().into_owned()
    };
    if dir {
        done.push('/');
    }
    done
}

/// A command run in the home folder: in a terminal (kept open until
/// Enter, to read what it said) or in the background.
pub fn run(command: &str, terminal: bool) {
    // On a line of its own: a trailing comment can't swallow what follows.
    if terminal {
        let script = format!("{command}\nprintf '\\nPress Enter to close'; read _");
        super::spawn(
            "ghostty",
            &["--working-directory=home", "-e", "sh", "-c", &script],
            &[],
        );
    } else {
        super::spawn("sh", &["-c", &format!("cd\n{command}")], &[]);
    }
}

/// A program's output; none if it can't start, fails, or takes too long.
fn output(mut command: Command) -> Option<String> {
    let mut child = command
        .env_remove("LD_LIBRARY_PATH")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // Read alongside, so a full pipe can't hold the program up.
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + LIMIT;
    let ok = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let text = reader.join().ok()?;
    ok.then_some(text)
}

/// One of the things the search does besides the apps, as `?` lists it:
/// picked, the search becomes its prefix, ready for the rest.
pub struct Help {
    pub name: &'static str,
    pub syntax: &'static str,
    pub example: &'static str,
    pub prefix: &'static str,
    pub icon: &'static str,
}

pub const HELP: [Help; 6] = [
    Help {
        name: "Calculator with unit conversion",
        syntax: "= <expression>",
        example: "= 20 °C to °F",
        prefix: "= ",
        icon: "accessories-calculator",
    },
    Help {
        name: "File navigation, Tab completes",
        syntax: "{ / | ~/ }<path>",
        example: "~/Documents",
        prefix: "~/",
        icon: "folder",
    },
    Help {
        name: "File search",
        syntax: "find <filename>",
        example: "find invoice.pdf",
        prefix: "find ",
        icon: "system-search",
    },
    Help {
        name: "Recently opened files",
        syntax: "recent <filename>",
        example: "recent report",
        prefix: "recent ",
        icon: "document-open-recent",
    },
    Help {
        name: "Terminal or background commands",
        syntax: "{ run | t: } <command> in a terminal, : <command> in the background",
        example: "run htop",
        prefix: "run ",
        icon: "utilities-terminal",
    },
    Help {
        name: "Web search",
        syntax: "<shortcut> <keywords>; type an engine's name for its shortcut",
        example: "g rust iterators",
        prefix: "g ",
        icon: "system-search",
    },
];

/// The help whose names hold `word` (any case).
pub fn help(word: &str) -> impl Iterator<Item = &'static Help> {
    let word = word.to_lowercase();
    HELP.iter()
        .filter(move |h| h.name.to_lowercase().contains(&word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_pick_the_mode() {
        assert_eq!(mode("firefox"), Mode::Plain);
        assert_eq!(mode("= 2+2"), Mode::Calc("2+2"));
        assert_eq!(mode("~/Doc"), Mode::Files("~/Doc"));
        assert_eq!(mode("/etc"), Mode::Files("/etc"));
        assert_eq!(mode("find  a.pdf"), Mode::Find("a.pdf"));
        assert_eq!(mode("finder"), Mode::Plain);
        assert_eq!(mode("recent x"), Mode::Recent("x"));
        assert_eq!(mode("run htop"), Mode::Run("htop", true));
        assert_eq!(mode("t:htop"), Mode::Run("htop", true));
        assert_eq!(mode(": touch x"), Mode::Run("touch x", false));
        assert_eq!(mode("?file"), Mode::Help("file"));
        assert!(sum_like("2*3") && sum_like("(1)") && !sum_like("g 2"));
        assert!(same_kind("~/a", "/b") && !same_kind("~/a", "find a"));
        assert_eq!(wait("find a"), Some(FIND_AFTER));
        assert_eq!(wait("find "), None);
        assert_eq!(wait("chromium"), None);
        assert_eq!(help("file").count(), 3);
    }

    #[test]
    fn paths_expand_and_complete() {
        let home = home();
        assert_eq!(expand("~"), format!("{home}/"));
        assert_eq!(expand("~/a"), format!("{home}/a"));
        assert_eq!(expand("~a"), "~a");
        let docs = PathBuf::from(format!("{home}/Documents"));
        assert_eq!(complete("~/Do", &docs, true), "~/Documents/");
        assert_eq!(complete("/ho", &docs, false), format!("{home}/Documents"));
        assert_eq!(tilde(&docs), "~/Documents");
    }

    #[test]
    fn the_recent_list_reads_and_decodes() {
        let xml = r#"<xbel>
  <bookmark href="file:///home/t/My%20Notes.txt" added="2026-01-01T00:00:00Z" modified="2026-03-01T00:00:00Z">
  </bookmark>
  <bookmark href="https://example.com" modified="2026-04-01T00:00:00Z"></bookmark>
  <bookmark href="file:///home/t/a&amp;b.pdf" visited="2026-02-01T00:00:00Z"></bookmark>
</xbel>"#;
        assert_eq!(
            used(xml),
            [
                (
                    "2026-03-01T00:00:00Z".into(),
                    PathBuf::from("/home/t/My Notes.txt")
                ),
                (
                    "2026-02-01T00:00:00Z".into(),
                    PathBuf::from("/home/t/a&b.pdf")
                ),
            ]
        );
        assert_eq!(decode("%E2%82%AC%2"), "€%2");
    }

    /// Needs qalc and fd on PATH (`. ./run-dev.env`).
    #[test]
    #[ignore]
    fn looks_up_with_the_programs() {
        assert_eq!(lookup("= 2+2"), [Hit::Calc("2+2".into(), "4".into())]);
        assert_eq!(lookup("3*4"), [Hit::Calc("3*4".into(), "12".into())]);
        assert!(lookup("= 2").is_empty());
        let found = lookup("find cargo.toml");
        assert!(
            found
                .iter()
                .any(|h| matches!(h, Hit::File(p, false) if p.ends_with("Cargo.toml")))
        );
    }

    #[test]
    fn a_folder_lists_by_its_start() {
        let dir = std::env::temp_dir().join(format!("cabin-search-{}", std::process::id()));
        for sub in ["Docs", "downloads", ".hidden"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        std::fs::write(dir.join("data.txt"), "").unwrap();
        let names = |typed: &str| -> Vec<String> {
            list(typed)
                .into_iter()
                .map(|h| match h {
                    Hit::File(p, _) => p.file_name().unwrap().to_string_lossy().into_owned(),
                    Hit::Calc(..) => unreachable!(),
                })
                .collect()
        };
        let base = dir.to_string_lossy();
        assert_eq!(
            names(&format!("{base}/d")),
            ["Docs", "downloads", "data.txt"]
        );
        assert_eq!(names(&format!("{base}/.")), [".hidden"]);
        assert!(names(&format!("{base}/nothing/x")).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
