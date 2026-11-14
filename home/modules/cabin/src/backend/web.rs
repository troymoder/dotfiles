// SPDX-License-Identifier: GPL-3.0-only

//! Web searches from the overview's search: a shortcut, a space, the query
//! ("g rust iced" is Google for "rust iced"). The shortcuts are
//! `data/web-shortcuts.ron` (once pop-launcher's web plugin config, same
//! format), read in at build time.

use std::sync::LazyLock;

/// A search the query asks for: whose, and where it goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Search {
    /// The engine's name and what's searched for ("Google: rust iced").
    pub label: String,
    pub url: String,
    /// The engine's icon: a name in Papirus' apps.
    pub icon: Option<String>,
}

/// An engine the query names, before its own query: picked, the search
/// becomes its shortcut and a space, ready for one.
#[derive(Clone, Debug, PartialEq)]
pub struct Engine {
    pub name: String,
    pub icon: Option<String>,
    /// What the search becomes ("google ").
    pub shortcut: String,
}

#[derive(Debug, PartialEq)]
struct Rule {
    matches: Vec<String>,
    /// Each engine's name, its URL up to the query (no scheme), its icon.
    queries: Vec<(String, String, Option<String>)>,
}

static RULES: LazyLock<Vec<Rule>> =
    LazyLock::new(|| parse(include_str!("../../data/web-shortcuts.ron")));

/// The searches `query` asks for, if it starts with a shortcut and a
/// space and has something after it.
pub fn searches(query: &str) -> Vec<Search> {
    let Some((prefix, rest)) = query.trim_start().split_once(' ') else {
        return Vec::new();
    };
    let rest = rest.trim();
    if rest.is_empty() {
        return Vec::new();
    }
    RULES
        .iter()
        .filter(|rule| rule.matches.iter().any(|m| m.eq_ignore_ascii_case(prefix)))
        .flat_map(|rule| &rule.queries)
        .map(|(name, url, icon)| Search {
            label: format!("{name}: {rest}"),
            url: format!("https://{url}{}", encode(rest)),
            icon: icon.clone(),
        })
        .collect()
}

/// The engines a word names, by a shortcut or the engine's name starting
/// with it ("goo", "g", "yt", "wiki"); none once there's a space, when
/// `searches` takes over.
pub fn engines(query: &str) -> Vec<Engine> {
    let word = query.trim().to_lowercase();
    if word.is_empty() || word.contains(' ') {
        return Vec::new();
    }
    let word = word.as_str();
    RULES
        .iter()
        .flat_map(|rule| {
            let keyword = rule
                .matches
                .iter()
                .find(|m| m.to_lowercase().starts_with(&word));
            rule.queries.iter().filter_map(move |(name, _, icon)| {
                let by_name = name.to_lowercase().starts_with(&word);
                let shortcut =
                    keyword.or_else(|| by_name.then(|| rule.matches.first()).flatten())?;
                Some(Engine {
                    name: name.clone(),
                    icon: icon.clone(),
                    shortcut: format!("{shortcut} "),
                })
            })
        })
        .collect()
}

/// The rules in the shortcuts file: each `matches: [...]` with the
/// `queries: [(name: ..., query: ...)]` after it. Only as much RON as that
/// file uses: quoted strings, no escapes, `//` comments.
fn parse(text: &str) -> Vec<Rule> {
    let text: String = text
        .lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    text.split("matches:")
        .skip(1)
        .filter_map(|chunk| {
            let (list, rest) = bracketed(chunk)?;
            let matches = quoted(list);
            let (queries, _) = bracketed(rest.split_once("queries:")?.1)?;
            let pairs = queries
                .split("name:")
                .skip(1)
                .filter_map(|q| {
                    let name = quoted(q).into_iter().next()?;
                    let url = quoted(q.split_once("query:")?.1).into_iter().next()?;
                    let icon = q
                        .split_once("icon:")
                        .and_then(|(_, rest)| quoted(rest).into_iter().next());
                    Some((name, url, icon))
                })
                .collect();
            Some(Rule {
                matches,
                queries: pairs,
            })
        })
        .collect()
}

/// What's between the first `[` and the `]` closing it, and what follows.
fn bracketed(text: &str) -> Option<(&str, &str)> {
    let start = text.find('[')?;
    let mut depth = 0;
    for (i, c) in text[start..].char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    let end = start + i;
                    return Some((&text[start + 1..end], &text[end + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// The quoted strings in `text`, in order.
fn quoted(text: &str) -> Vec<String> {
    text.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// `text` as a URL's query value: unreserved characters kept, the rest
/// percent-encoded (a space as `%20`).
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shortcuts_file_reads() {
        assert!(RULES.len() >= 5, "{:?}", *RULES);
        let google = RULES
            .iter()
            .find(|r| r.matches.contains(&"g".to_owned()))
            .unwrap();
        assert_eq!(google.matches, ["g", "gs", "google"]);
        assert_eq!(
            google.queries,
            [(
                "Google".to_owned(),
                "google.com/search?q=".to_owned(),
                Some("google".to_owned())
            )]
        );
    }

    #[test]
    fn a_shortcut_and_a_query_make_a_search() {
        assert_eq!(
            searches("g rust iced"),
            [Search {
                label: "Google: rust iced".into(),
                url: "https://google.com/search?q=rust%20iced".into(),
                icon: Some("google".into()),
            }]
        );
        assert_eq!(
            searches("YT lofi")[0].url,
            "https://youtube.com/results?search_query=lofi"
        );
        assert_eq!(
            searches("nix c++ & co")[0].url,
            "https://search.nixos.org/packages?query=c%2B%2B%20%26%20co"
        );
        // Named but nothing to search for: an engine, not a search.
        let named: Vec<String> = engines("goo").into_iter().map(|e| e.shortcut).collect();
        assert_eq!(named, ["google "]);
        assert_eq!(engines("yt")[0].name, "YouTube");
        assert_eq!(engines("wiki")[0].shortcut, "wiki ");
        // By the engine's name when no shortcut starts so: "duck" is ddg.
        assert_eq!(engines("duck")[0].shortcut, "ddg ");
        assert!(engines("g rust").is_empty());
        assert!(engines("zzz").is_empty());
        // No query, no shortcut, or not one: nothing.
        assert!(searches("g").is_empty());
        assert!(searches("g   ").is_empty());
        assert!(searches("chromium").is_empty());
        assert!(searches("gx foo").is_empty());
    }
}
