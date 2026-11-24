//! Writes the stored VEVENT subset of iCalendar with CRLF endings and UTF-8-safe line folding.

use std::fmt::Write as _;

use enzui_core::civil_date::{Date, Weekday};

use crate::model::{Event, MINUTES_PER_DAY, Recurrence};

pub(crate) const SUGGESTED_FILENAME: &str = "calendar.ics";

const PRODID: &str = "-//enzUI//calendar-app//EN";

fn serialize_events<'a>(
    events: impl Iterator<Item = &'a Event>,
    stamp: Date,
) -> (String, Vec<ExportOmission>) {
    let events: Vec<&Event> = events.collect();
    let mut lines: Vec<String> = Vec::new();
    let mut omissions: Vec<ExportOmission> = Vec::new();
    lines.push("BEGIN:VCALENDAR".to_owned());
    lines.push("VERSION:2.0".to_owned());
    lines.push(format!("PRODID:{PRODID}"));
    lines.push("CALSCALE:GREGORIAN".to_owned());
    for event in &events {
        write_vevent(&mut lines, event, &events, stamp, &mut omissions);
    }
    lines.push("END:VCALENDAR".to_owned());

    let mut out = String::new();
    for line in &lines {
        out.push_str(&fold_line(line));
        out.push_str("\r\n");
    }
    (out, omissions)
}

#[derive(Debug, Clone)]
pub(crate) struct ExportOmission {
    pub id: u64,
    pub field: OmittedField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OmittedField {
    Url,
    Attendee,
    Uid,
}

impl OmittedField {
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            OmittedField::Url => "web link",
            OmittedField::Attendee => "attendee",
            OmittedField::Uid => "identity",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SkippedExport {
    pub title: String,
}

pub(crate) fn is_unsupported_for_export(e: &Event) -> bool {
    e.tz_anchor.is_anchored() && e.recurrence != Recurrence::None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportResult {
    Wrote,
    NothingWritten,
    WriteFailed(std::io::ErrorKind),
    Busy,
    OwnerUnavailable,
}

#[derive(Debug, Clone)]
pub(crate) struct ExportDisclosure {
    pub path_display: String,
    pub skipped: Vec<SkippedExport>,
    pub omissions: Vec<ExportOmission>,
}

impl ExportDisclosure {
    #[must_use]
    pub(crate) fn none() -> Self {
        Self {
            path_display: String::new(),
            skipped: Vec::new(),
            omissions: Vec::new(),
        }
    }
}

#[must_use]
pub(crate) fn serialize_calendar_checked(
    events: &[Event],
    stamp: Date,
) -> (String, Vec<SkippedExport>, Vec<ExportOmission>) {
    let mut skipped = Vec::new();
    let writable: Vec<&Event> = events
        .iter()
        .filter(|e| {
            if is_unsupported_for_export(e) {
                skipped.push(SkippedExport {
                    title: e.title.clone(),
                });
                false
            } else {
                true
            }
        })
        .collect();
    let (document, omissions) = serialize_events(writable.into_iter(), stamp);
    (document, skipped, omissions)
}

fn shares_series(a: &Event, b: &Event) -> bool {
    a.uid.is_some() && a.uid == b.uid
}

fn write_vevent(
    lines: &mut Vec<String>,
    e: &Event,
    exported: &[&Event],
    stamp: Date,
    omissions: &mut Vec<ExportOmission>,
) {
    lines.push("BEGIN:VEVENT".to_owned());
    // Export never mints a UID: doing so would change identity on every export.
    match e
        .uid
        .as_deref()
        .filter(|u| crate::ics_safety::safe_uid(u))
        .map(str::to_owned)
    {
        Some(uid) => lines.push(format!("UID:{uid}")),
        None => omissions.push(ExportOmission {
            id: e.id,
            field: OmittedField::Uid,
        }),
    }
    // RECURRENCE-ID names the replaced series occurrence in the series DTSTART value type.
    if let Some(rid) = e.recurrence_id {
        let series = exported
            .iter()
            .find(|s| s.recurrence_id.is_none() && shares_series(s, e))
            .copied()
            .unwrap_or(e);
        if series.all_day {
            lines.push(format!("RECURRENCE-ID;VALUE=DATE:{}", fmt_date(rid)));
        } else {
            let (start_offset, _) = series.tz_anchor.offsets().unwrap_or((0, 0));
            lines.push(format!(
                "RECURRENCE-ID:{}",
                fmt_datetime(
                    rid,
                    series.start_minute.get() - start_offset,
                    series.tz_anchor.is_anchored()
                )
            ));
        }
    }
    lines.push(format!("DTSTAMP:{}T000000Z", fmt_date(stamp)));
    lines.push(format!("SUMMARY:{}", escape_text(&e.title)));

    if e.all_day {
        lines.push(format!("DTSTART;VALUE=DATE:{}", fmt_date(e.date)));
        // RFC 5545 all-day DTEND is exclusive, while the domain end date is inclusive.
        if e.end_date > e.date {
            lines.push(format!(
                "DTEND;VALUE=DATE:{}",
                fmt_date(e.end_date.add_days(1))
            ));
        }
    } else {
        // Each endpoint must reverse its own stored offset to preserve DST-straddling instants.
        let (start_offset, end_offset) = e.tz_anchor.offsets().unwrap_or((0, 0));
        let anchored = e.tz_anchor.is_anchored();
        lines.push(format!(
            "DTSTART:{}",
            fmt_datetime(e.date, e.start_minute.get() - start_offset, anchored)
        ));
        lines.push(format!(
            "DTEND:{}",
            fmt_datetime(e.end_date, e.end_minute.get() - end_offset, anchored)
        ));
    }

    if !e.location.is_empty() {
        lines.push(format!("LOCATION:{}", escape_text(&e.location)));
    }
    if !e.notes.is_empty() {
        lines.push(format!("DESCRIPTION:{}", escape_text(&e.notes)));
    }
    if !e.url.is_empty() {
        // Invalid URI properties are omitted and disclosed, never escaped into a different value.
        if crate::ics_safety::safe_uri(&e.url) {
            lines.push(format!("URL:{}", e.url));
        } else {
            omissions.push(ExportOmission {
                id: e.id,
                field: OmittedField::Url,
            });
        }
    }
    let mut bad_attendee = false;
    for who in e
        .invitees
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if crate::ics_safety::safe_cal_address(who) {
            lines.push(format!("ATTENDEE:mailto:{who}"));
        } else {
            bad_attendee = true;
        }
    }
    if bad_attendee {
        omissions.push(ExportOmission {
            id: e.id,
            field: OmittedField::Attendee,
        });
    }

    let occurrence_minute = (!e.all_day
        && !e.tz_anchor.is_anchored()
        && e.recurrence != Recurrence::None
        && e.recurrence_raw.is_none())
    .then_some(e.start_minute.get());
    if let Some(rrule) = rrule_line(e, occurrence_minute) {
        lines.push(rrule);
    }
    // An exported override replaces its occurrence, so EXDATE must not also remove it.
    let exdates: Vec<Date> = e
        .exceptions
        .iter()
        .copied()
        .filter(|&date| {
            !exported
                .iter()
                .any(|o| o.recurrence_id == Some(date) && shares_series(o, e))
        })
        .collect();
    if !exdates.is_empty() {
        let dates = exdates
            .iter()
            .map(|&date| fmt_recurrence_date(date, occurrence_minute))
            .collect::<Vec<_>>()
            .join(",");
        let property = if occurrence_minute.is_some() {
            "EXDATE"
        } else {
            "EXDATE;VALUE=DATE"
        };
        lines.push(format!("{property}:{dates}"));
    }
    if let Some(minutes) = e.alert {
        write_valarm(lines, e, minutes);
    }

    lines.push("END:VEVENT".to_owned());
}

fn write_valarm(lines: &mut Vec<String>, e: &Event, minutes: i32) {
    let description = if e.title.trim().is_empty() {
        "Reminder".to_owned()
    } else {
        escape_text(&e.title)
    };
    lines.push("BEGIN:VALARM".to_owned());
    lines.push("ACTION:DISPLAY".to_owned());
    lines.push(format!("DESCRIPTION:{description}"));
    // Alert lead times are negative durations relative to START.
    lines.push(format!("TRIGGER;RELATED=START:-PT{}M", minutes.max(0)));
    lines.push("END:VALARM".to_owned());
}

fn byday_code(w: Weekday) -> &'static str {
    match w {
        Weekday::Monday => "MO",
        Weekday::Tuesday => "TU",
        Weekday::Wednesday => "WE",
        Weekday::Thursday => "TH",
        Weekday::Friday => "FR",
        Weekday::Saturday => "SA",
        Weekday::Sunday => "SU",
    }
}

fn rrule_line(e: &Event, occurrence_minute: Option<i32>) -> Option<String> {
    if e.recurrence == Recurrence::None {
        return None;
    }
    // Unsupported raw recurrence rules remain byte-preserved until a confirmed rule edit.
    if let Some(raw) = &e.recurrence_raw {
        return Some(format!("RRULE:{raw}"));
    }
    let freq = match e.recurrence {
        Recurrence::None => return None,
        Recurrence::Daily => "DAILY",
        Recurrence::Weekly => "WEEKLY",
        Recurrence::Monthly => "MONTHLY",
        Recurrence::Yearly => "YEARLY",
    };
    let mut rule = format!("RRULE:FREQ={freq}");
    if e.recurrence_interval > 1 {
        let _ = write!(rule, ";INTERVAL={}", e.recurrence_interval);
    }
    if e.recurrence == Recurrence::Weekly && !e.recurrence_weekdays.is_empty() {
        let mut days = e.recurrence_weekdays.clone();
        days.sort_by_key(|w| w.index());
        days.dedup();
        let codes = days
            .iter()
            .map(|w| byday_code(*w))
            .collect::<Vec<_>>()
            .join(",");
        let _ = write!(rule, ";BYDAY={codes}");
    }
    // UNTIL takes precedence over COUNT if both are present in recovered data.
    if let Some(until) = e.recurrence_until {
        let _ = write!(
            rule,
            ";UNTIL={}",
            fmt_recurrence_date(until, occurrence_minute)
        );
    } else if let Some(count) = e.recurrence_count {
        let _ = write!(rule, ";COUNT={}", count.max(1));
    }
    Some(rule)
}

fn fmt_recurrence_date(date: Date, occurrence_minute: Option<i32>) -> String {
    match occurrence_minute {
        Some(minute) => fmt_datetime(date, minute, false),
        None => fmt_date(date),
    }
}

fn fmt_date(d: Date) -> String {
    format!("{:04}{:02}{:02}", d.year(), d.month(), d.day())
}

fn fmt_datetime(date: Date, minute: i32, anchored: bool) -> String {
    let day_shift = minute.div_euclid(MINUTES_PER_DAY);
    let in_day = minute.rem_euclid(MINUTES_PER_DAY);
    let date = date.add_days(day_shift);
    let hh = in_day / 60;
    let mm = in_day % 60;
    let suffix = if anchored { "Z" } else { "" };
    format!("{}T{hh:02}{mm:02}00{suffix}", fmt_date(date))
}

fn escape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {} // CR is a line-break artefact, dropped
            other => out.push(other),
        }
    }
    out
}

fn fold_line(line: &str) -> String {
    // The RFC 5545 folded-line byte limit includes the continuation space.
    const LIMIT: usize = 75;
    if line.len() <= LIMIT {
        return line.to_owned();
    }
    let mut out = String::with_capacity(line.len() + line.len() / LIMIT + 1);
    let mut budget = LIMIT;
    for c in line.chars() {
        let need = c.len_utf8();
        if need > budget {
            out.push_str("\r\n ");
            budget = LIMIT - 1; // the leading space is one octet
        }
        out.push(c);
        budget -= need;
    }
    out
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[must_use]
pub(crate) fn serialize_calendar(events: &[Event], stamp: Date) -> String {
    serialize_events(events.iter(), stamp).0
}

pub(crate) fn prepare_event_drag(
    event: &Event,
    stamp: Date,
    cancel: &enzui_core::cancellation::CancelToken,
) -> Result<(tempfile::TempDir, std::path::PathBuf), String> {
    if cancel.is_terminal() {
        return Err("event export cancelled".into());
    }
    let (document, skipped, omissions) =
        serialize_calendar_checked(std::slice::from_ref(event), stamp);
    if !skipped.is_empty() || !omissions.is_empty() {
        return Err("this event cannot be exported without losing data".into());
    }
    let directory = tempfile::Builder::new()
        .prefix("enz-calendar-drag-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let path = directory.path().join("Event.ics");
    enzui_core::user_file::save_with(
        &path,
        enzui_core::user_file::SavePolicy::CreateNew,
        |output| {
            for bytes in document.as_bytes().chunks(64 * 1024) {
                if cancel.is_terminal() {
                    return Err(std::io::Error::other("event export cancelled"));
                }
                output.write_all(bytes)?;
            }
            Ok(())
        },
    )
    .map_err(|error| error.to_string())?;
    Ok((directory, path))
}
