// SPDX-License-Identifier: GPL-3.0-only

//! Events from calendars on disk: every `.ics` under the places calendars
//! keep them (Evolution's store, vdirsyncer's, our own folder). Enough of
//! iCalendar to show what's on a day: start, end, summary, and the common
//! repeats.

use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use chrono::{Datelike, Duration as Days, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};
use cosmic::iced::Subscription;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub date: NaiveDate,
    /// None for an all-day event.
    pub time: Option<NaiveTime>,
    pub summary: String,
}

impl Event {
    pub fn when(&self) -> String {
        match self.time {
            Some(time) => time.format("%H:%M").to_string(),
            None => "All day".to_owned(),
        }
    }
}

/// Events by day.
pub type Calendar = BTreeMap<NaiveDate, Vec<Event>>;

fn dirs() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));
    vec![
        format!("{data}/evolution/calendar").into(),
        format!("{data}/calendars").into(),
        format!("{home}/.calendars").into(),
        format!("{data}/cabin/calendars").into(),
    ]
}

fn ics_files(dir: &std::path::Path, out: &mut Vec<PathBuf>, depth: u8) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < 4 && path.file_name().is_none_or(|n| n != "trash") {
                ics_files(&path, out, depth + 1);
            }
        } else if path.extension().is_some_and(|e| e == "ics") {
            out.push(path);
        }
    }
}

/// Unfolded lines: a line starting with a space continues the one before.
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix([' ', '\t']) {
            if let Some(last) = lines.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        lines.push(line.to_owned());
    }
    lines
}

/// A DTSTART or DTEND value: a date, or a date and time (local, UTC or in a zone).
fn parse_when(params: &str, value: &str) -> Option<(NaiveDate, Option<NaiveTime>)> {
    let value = value.trim();
    if params.contains("VALUE=DATE") || value.len() == 8 {
        return NaiveDate::parse_from_str(value, "%Y%m%d").ok().map(|d| (d, None));
    }
    let utc = value.ends_with('Z');
    let value = value.trim_end_matches('Z');
    let naive = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
    let local = if utc {
        chrono::Local.from_utc_datetime(&naive)
    } else if let Some(tzid) = params.split(';').find_map(|p| p.strip_prefix("TZID=")) {
        match tzid.parse::<chrono_tz::Tz>() {
            Ok(tz) => tz.from_local_datetime(&naive).single().map(|t| t.with_timezone(&chrono::Local)).unwrap_or_else(|| chrono::Local.from_utc_datetime(&naive)),
            Err(_) => chrono::Local.from_local_datetime(&naive).single()?,
        }
    } else {
        chrono::Local.from_local_datetime(&naive).single()?
    };
    Some((local.date_naive(), Some(local.time())))
}

#[derive(Default)]
struct Raw {
    start: Option<(NaiveDate, Option<NaiveTime>)>,
    summary: String,
    rrule: Option<String>,
    exdates: Vec<NaiveDate>,
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push(' '),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The repeats of an event within a window, as plain as iCalendar's common
/// ones go: FREQ with INTERVAL, COUNT or UNTIL, BYDAY for weekly ones.
fn expand(raw: &Raw, from: NaiveDate, to: NaiveDate, out: &mut Calendar) {
    let Some((start, time)) = raw.start else {
        return;
    };
    let mut push = |date: NaiveDate| {
        if date >= from && date <= to && !raw.exdates.contains(&date) {
            out.entry(date).or_default().push(Event { date, time, summary: raw.summary.clone() });
        }
    };
    let Some(rule) = &raw.rrule else {
        push(start);
        return;
    };
    let field = |key: &str| rule.split(';').find_map(|p| p.strip_prefix(key).and_then(|v| v.strip_prefix('=')).map(str::to_owned));
    let freq = field("FREQ").unwrap_or_default();
    let interval: i64 = field("INTERVAL").and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
    let count: Option<usize> = field("COUNT").and_then(|v| v.parse().ok());
    let until: Option<NaiveDate> = field("UNTIL").and_then(|v| parse_when("", &v)).map(|(d, _)| d);
    let bydays: Vec<Weekday> = field("BYDAY")
        .map(|v| {
            v.split(',')
                .filter_map(|d| match d.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-' || c == '+') {
                    "MO" => Some(Weekday::Mon),
                    "TU" => Some(Weekday::Tue),
                    "WE" => Some(Weekday::Wed),
                    "TH" => Some(Weekday::Thu),
                    "FR" => Some(Weekday::Fri),
                    "SA" => Some(Weekday::Sat),
                    "SU" => Some(Weekday::Sun),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let mut made = 0usize;
    let mut date = start;
    // Walk day by day for weekly rules with days, else by the rule's step.
    for _ in 0..5000 {
        if date > to || until.is_some_and(|u| date > u) || count.is_some_and(|c| made >= c) {
            break;
        }
        let hit = match freq.as_str() {
            "WEEKLY" if !bydays.is_empty() => bydays.contains(&date.weekday()) && ((date - start).num_days() / 7) % interval == 0,
            "WEEKLY" => (date - start).num_days() % (7 * interval) == 0,
            "DAILY" => (date - start).num_days() % interval == 0,
            "MONTHLY" => date.day() == start.day() && ((date.year() - start.year()) as i64 * 12 + (date.month() as i64 - start.month() as i64)) % interval == 0,
            "YEARLY" => date.day() == start.day() && date.month() == start.month() && (date.year() - start.year()) as i64 % interval == 0,
            _ => date == start,
        };
        if hit {
            made += 1;
            push(date);
            if freq.is_empty() {
                break;
            }
        }
        date += Days::days(1);
    }
}

fn parse(text: &str, from: NaiveDate, to: NaiveDate, out: &mut Calendar) {
    let mut current: Option<Raw> = None;
    for line in unfold(text) {
        if line == "BEGIN:VEVENT" {
            current = Some(Raw::default());
            continue;
        }
        if line == "END:VEVENT" {
            if let Some(raw) = current.take() {
                if !raw.summary.is_empty() {
                    expand(&raw, from, to, out);
                }
            }
            continue;
        }
        let Some(raw) = current.as_mut() else {
            continue;
        };
        let Some((head, value)) = line.split_once(':') else {
            continue;
        };
        let (name, params) = head.split_once(';').map_or((head, ""), |(n, p)| (n, p));
        match name {
            "DTSTART" => raw.start = parse_when(params, value),
            "SUMMARY" => raw.summary = unescape(value),
            "RRULE" => raw.rrule = Some(value.to_owned()),
            "EXDATE" => {
                for v in value.split(',') {
                    if let Some((d, _)) = parse_when(params, v) {
                        raw.exdates.push(d);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Everything on the calendars from a month back to a year on, by day.
pub fn load() -> Calendar {
    let today = chrono::Local::now().date_naive();
    let (from, to) = (today - Days::days(60), today + Days::days(400));
    let mut files = Vec::new();
    for dir in dirs() {
        ics_files(&dir, &mut files, 0);
    }
    let mut calendar = Calendar::new();
    for file in files {
        if let Ok(text) = std::fs::read_to_string(&file) {
            parse(&text, from, to, &mut calendar);
        }
    }
    for events in calendar.values_mut() {
        events.sort_by_key(|e| e.time);
    }
    calendar
}

pub fn subscription() -> Subscription<Calendar> {
    Subscription::run(|| super::watch(Duration::from_secs(600), super::no_changes, || async { Some(tokio::task::spawn_blocking(load).await.unwrap_or_default()) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_and_weekly() {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20261113\r\nSUMMARY:Invoice \r\n due\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;TZID=Africa/Johannesburg:20261102T093000\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let mut cal = Calendar::new();
        parse(text, NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(), NaiveDate::from_ymd_opt(2026, 11, 30).unwrap(), &mut cal);
        assert_eq!(cal[&NaiveDate::from_ymd_opt(2026, 11, 13).unwrap()][0].summary, "Invoice due");
        assert_eq!(cal[&NaiveDate::from_ymd_opt(2026, 11, 13).unwrap()][0].when(), "All day");
        let standups: usize = cal.values().flatten().filter(|e| e.summary == "Standup").count();
        assert_eq!(standups, 9);
        assert!(cal.contains_key(&NaiveDate::from_ymd_opt(2026, 11, 4).unwrap()));
        assert!(!cal.contains_key(&NaiveDate::from_ymd_opt(2026, 11, 3).unwrap()));
    }
}
