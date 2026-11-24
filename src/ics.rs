//! Reads the stored VEVENT subset of iCalendar, unfolding lines and unescaping TEXT values.

use enzui_core::civil_date::{Date, Weekday};

use crate::model::{Event, MinuteOfDay, Recurrence};

pub(crate) const MAX_IMPORT_WARNINGS: usize = 8;
const MAX_WARNING_FIELD_BYTES: usize = 64;
pub(crate) const MAX_IMPORT_SUMMARY_BYTES: usize = 384;

// Sanitize feedback copies without changing imported event data.
fn safe_feedback_fragment(value: &str, fallback: &str) -> String {
    let mut safe = String::new();
    let mut truncated = false;
    let content_budget = MAX_WARNING_FIELD_BYTES.saturating_sub('…'.len_utf8());

    for ch in value.chars() {
        let bidi_control = matches!(
            ch,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        );
        if ch.is_control() || bidi_control || matches!(ch, '\u{2028}' | '\u{2029}') {
            continue;
        }
        if safe.len() + ch.len_utf8() > content_budget {
            truncated = true;
            break;
        }
        safe.push(ch);
    }

    let trimmed_len = safe.trim_end().len();
    safe.truncate(trimmed_len);
    let leading = safe.len() - safe.trim_start().len();
    if leading > 0 {
        safe.drain(..leading);
    }
    if safe.is_empty() {
        return fallback.to_owned();
    }
    if truncated {
        safe.push('…');
    }
    safe
}

pub(crate) fn bounded_feedback(prefix: &str, details: &str, suffix: &str) -> String {
    let translated_prefix = crate::l10n::tr(prefix);
    let translated_details = crate::l10n::tr(details);
    let translated_suffix = crate::l10n::tr(suffix);
    let (prefix, details, suffix) = (
        translated_prefix.as_str(),
        translated_details.as_str(),
        translated_suffix.as_str(),
    );
    let mut full = String::with_capacity(prefix.len() + details.len() + suffix.len());
    full.push_str(prefix);
    full.push_str(details);
    full.push_str(suffix);
    if full.len() <= MAX_IMPORT_SUMMARY_BYTES {
        return full;
    }

    let fixed = prefix.len() + suffix.len();
    if fixed >= MAX_IMPORT_SUMMARY_BYTES {
        return truncate_with_ellipsis(&full, MAX_IMPORT_SUMMARY_BYTES);
    }
    let detail_budget = MAX_IMPORT_SUMMARY_BYTES - fixed;
    let bounded_details = truncate_with_ellipsis(details, detail_budget);
    let mut bounded = String::with_capacity(MAX_IMPORT_SUMMARY_BYTES);
    bounded.push_str(prefix);
    bounded.push_str(&bounded_details);
    bounded.push_str(suffix);
    bounded
}

fn truncate_with_ellipsis(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    if max_bytes < '…'.len_utf8() {
        return String::new();
    }
    let mut end = max_bytes - '…'.len_utf8();
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut bounded = value.get(..end).unwrap_or_default().to_owned();
    bounded.push('…');
    bounded
}

pub(crate) fn bounded_warning_feedback(
    prefix: &str,
    warnings: &[String],
    omitted_warnings: usize,
) -> String {
    let translated_prefix = crate::l10n::tr(prefix);
    let prefix = translated_prefix.as_str();
    let total = warnings.len().saturating_add(omitted_warnings);
    for displayed in (1..=warnings.len()).rev() {
        let omitted = total.saturating_sub(displayed);
        let suffix = if omitted == 0 {
            String::new()
        } else {
            format!(
                " · {}",
                crate::l10n::count("calendar-omitted-warnings", omitted)
            )
        };
        let details = warnings[..displayed].join(" ");
        let mut summary = String::with_capacity(prefix.len() + details.len() + suffix.len());
        summary.push_str(prefix);
        summary.push_str(&details);
        summary.push_str(&suffix);
        if summary.len() <= MAX_IMPORT_SUMMARY_BYTES {
            return summary;
        }
    }

    let suffix = crate::l10n::count("calendar-omitted-warnings", total);
    bounded_feedback(prefix, "", &suffix)
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ParsedIcs {
    pub events: Vec<Event>,
    pub cancelled_occurrences: Vec<(String, Date)>,
    pub cancelled_events: usize,
    pub warnings: Vec<String>,
    pub omitted_warnings: usize,
}

impl ParsedIcs {
    fn push_warning(&mut self, warning: String) {
        if self.warnings.len() < MAX_IMPORT_WARNINGS {
            self.warnings.push(warning);
        } else {
            self.omitted_warnings = self.omitted_warnings.saturating_add(1);
        }
    }
}

pub(crate) fn parse_ics(text: &str, default_calendar_id: &str) -> Result<ParsedIcs, String> {
    let unfolded = unfold_lines(text);

    if !unfolded.iter().any(|l| eq_prop(l, "BEGIN", "VCALENDAR")) {
        return Err("Not an iCalendar file (no BEGIN:VCALENDAR).".to_owned());
    }

    let tzdb = TzDb::from_lines(&unfolded);
    let mut out = ParsedIcs::default();

    let mut i = 0;
    while i < unfolded.len() {
        if eq_prop(&unfolded[i], "BEGIN", "VEVENT") {
            match find_end(&unfolded, i + 1, "VEVENT") {
                Some(end) => {
                    match parse_vevent(&unfolded[i + 1..end], default_calendar_id, &tzdb) {
                        Ok((ev, warnings, cancelled)) => {
                            match (cancelled, &ev.uid, ev.recurrence_id) {
                                (false, ..) => out.events.push(ev),
                                (true, Some(uid), Some(date)) => {
                                    out.cancelled_occurrences.push((uid.clone(), date));
                                }
                                (true, ..) => out.cancelled_events += 1,
                            }
                            for warning in warnings {
                                out.push_warning(warning);
                            }
                        }
                        Err(w) => out.push_warning(w),
                    }
                    i = end + 1;
                    continue;
                }
                None => {
                    out.push_warning("Unterminated VEVENT block.".to_owned());
                    break;
                }
            }
        }
        i += 1;
    }

    if out.events.is_empty() && out.cancelled_occurrences.is_empty() && out.cancelled_events == 0 {
        let Some(details) = out.warnings.first() else {
            return Err("No events found in this file.".to_owned());
        };
        let omitted_from_copy = out
            .warnings
            .len()
            .saturating_sub(1)
            .saturating_add(out.omitted_warnings);
        let suffix = if omitted_from_copy == 0 {
            String::new()
        } else {
            format!(
                " · {}",
                crate::l10n::count("calendar-omitted-warnings", omitted_from_copy)
            )
        };
        return Err(bounded_feedback(
            "No events found in this file. ",
            details,
            &suffix,
        ));
    }

    Ok(out)
}

fn unfold_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        match line.chars().next() {
            Some(' ' | '\t') if !out.is_empty() => {
                if let Some(previous) = out.last_mut() {
                    previous.push_str(
                        line.strip_prefix(' ')
                            .or_else(|| line.strip_prefix('\t'))
                            .unwrap_or(line),
                    );
                }
            }
            Some(_) => out.push(line.to_owned()),
            None => {}
        }
    }
    out
}

fn find_end(lines: &[String], from: usize, block: &str) -> Option<usize> {
    (from..lines.len()).find(|&j| eq_prop(&lines[j], "END", block))
}

pub(super) struct Prop<'a> {
    pub(super) name: &'a str,
    pub(super) params: Vec<(&'a str, &'a str)>,
    pub(super) value: &'a str,
}

impl<'a> Prop<'a> {
    pub(super) fn parse(line: &'a str) -> Option<Prop<'a>> {
        // Quoted RFC 5545 parameter values can contain colons.
        let colon = first_unquoted_colon(line)?;
        let (head, rest) = line.split_at(colon);
        let value = rest.strip_prefix(':')?;
        let mut parts = head.split(';');
        let name = parts.next()?.trim();
        let params = parts
            .filter_map(|p| p.split_once('='))
            .map(|(k, v)| (k.trim(), v.trim().trim_matches('"')))
            .collect();
        Some(Prop {
            name,
            params,
            value,
        })
    }

    pub(super) fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| *v)
    }
}

fn first_unquoted_colon(line: &str) -> Option<usize> {
    let mut in_quote = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quote = !in_quote,
            ':' if !in_quote => return Some(i),
            _ => {}
        }
    }
    None
}

fn eq_prop(line: &str, key: &str, value: &str) -> bool {
    Prop::parse(line).is_some_and(|p| {
        p.name.eq_ignore_ascii_case(key) && p.value.trim().eq_ignore_ascii_case(value)
    })
}

// A fixed-UTC anchor cannot represent seasonal transition rules.
struct TzDb {
    zones: Vec<(String, i32)>,
}

impl TzDb {
    fn from_lines(lines: &[String]) -> Self {
        let mut zones = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            if eq_prop(&lines[i], "BEGIN", "VTIMEZONE")
                && let Some(end) = find_end(lines, i + 1, "VTIMEZONE")
            {
                if let Some((id, offset)) = parse_vtimezone(&lines[i + 1..end]) {
                    zones.push((id, offset));
                }
                i = end + 1;
                continue;
            }
            i += 1;
        }
        Self { zones }
    }

    fn offset(&self, tzid: &str) -> Option<i32> {
        self.zones
            .iter()
            .find(|(id, _)| id == tzid)
            .map(|(_, off)| *off)
    }
}

fn parse_vtimezone(lines: &[String]) -> Option<(String, i32)> {
    let mut tzid: Option<String> = None;
    let mut offset: Option<i32> = None;
    let mut has_from = false;
    let mut has_to = false;
    for line in lines {
        let Some(p) = Prop::parse(line) else { continue };
        if p.name.eq_ignore_ascii_case("TZID") {
            tzid = Some(p.value.trim().to_owned());
        } else if p.name.eq_ignore_ascii_case("TZOFFSETTO")
            || p.name.eq_ignore_ascii_case("TZOFFSETFROM")
        {
            let candidate = parse_utc_offset(p.value.trim())?;
            if offset.is_some_and(|known| known != candidate) {
                return None;
            }
            offset = Some(candidate);
            has_from |= p.name.eq_ignore_ascii_case("TZOFFSETFROM");
            has_to |= p.name.eq_ignore_ascii_case("TZOFFSETTO");
        }
    }
    if !has_from || !has_to {
        return None;
    }
    Some((tzid?, offset?))
}

fn parse_utc_offset(s: &str) -> Option<i32> {
    let bytes = s.as_bytes();
    if !matches!(bytes.len(), 5 | 7) || !bytes[1..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let sign = match bytes[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hh: i32 = s.get(1..3)?.parse().ok()?;
    let mm: i32 = s.get(3..5)?.parse().ok()?;
    // Minute-resolution offsets cannot represent a nonzero seconds component.
    let ss: i32 = if bytes.len() == 7 {
        s.get(5..7)?.parse().ok()?
    } else {
        0
    };
    if hh > 23 || mm > 59 || ss != 0 || (sign < 0 && hh == 0 && mm == 0) {
        return None;
    }
    Some(sign * (hh * 60 + mm))
}

#[derive(Clone, Copy)]
struct IcsDateTime {
    date: Date,
    // Minutes after midnight; None denotes an all-day date.
    minute: Option<i32>,
    // Minutes east of UTC; None denotes floating local time.
    utc_offset: Option<i32>,
}

fn parse_vevent(
    lines: &[String],
    calendar_id: &str,
    tz: &TzDb,
) -> Result<(Event, Vec<String>, bool), String> {
    let mut title = String::new();
    let mut location = String::new();
    let mut notes = String::new();
    let mut url = String::new();
    let mut invitees: Vec<String> = Vec::new();
    let mut start: Option<IcsDateTime> = None;
    let mut end: Option<IcsDateTime> = None;
    let mut end_supplied = false;
    let mut duration_minutes: Option<i32> = None;
    let mut all_day = false;
    let mut recurrence = Recurrence::None;
    let mut recurrence_until: Option<Date> = None;
    let mut recurrence_count: Option<u32> = None;
    let mut recurrence_interval: u32 = 1;
    let mut unsupported_rrule: Vec<String> = Vec::new();
    let mut recurrence_weekdays: Vec<Weekday> = Vec::new();
    let mut recurrence_raw: Option<String> = None;
    let mut uid: Option<String> = None;
    let mut recurrence_id_date: Option<Date> = None;
    let mut exceptions: Vec<Date> = Vec::new();
    let mut cancelled = false;
    let mut alert: Option<i32> = None;
    // An unresolved TZID must refuse the event instead of silently changing it to floating time.
    let mut unresolved_zone: Option<String> = None;
    let (mut url_rejected, mut attendee_rejected, mut uid_rejected) = (false, false, false);

    let mut i = 0;
    while i < lines.len() {
        if eq_prop(&lines[i], "BEGIN", "VALARM") {
            if let Some(end_i) = find_end(lines, i + 1, "VALARM") {
                if let Some(m) = parse_valarm(&lines[i + 1..end_i]) {
                    alert = Some(m);
                }
                i = end_i + 1;
                continue;
            }
            break;
        }
        let Some(p) = Prop::parse(&lines[i]) else {
            i += 1;
            continue;
        };
        match p.name.to_ascii_uppercase().as_str() {
            "SUMMARY" => title = unescape_text(p.value),
            "LOCATION" => location = unescape_text(p.value),
            "DESCRIPTION" => notes = unescape_text(p.value),
            // URI properties are not TEXT and cannot be unescaped or escaped without changing their
            // meaning.
            "URL" => {
                let value = p.value.trim();
                if crate::ics_safety::safe_uri(value) {
                    value.clone_into(&mut url);
                } else if !value.is_empty() {
                    url_rejected = true;
                }
            }
            "ATTENDEE" => {
                let who = p
                    .value
                    .trim()
                    .trim_start_matches("mailto:")
                    .trim_start_matches("MAILTO:");
                if crate::ics_safety::safe_cal_address(who) {
                    invitees.push(who.to_owned());
                } else if !who.is_empty() {
                    attendee_rejected = true;
                }
            }
            "DTSTART" => {
                if unresolved_zone.is_none() {
                    unresolved_zone = unresolved_tzid(&p, tz).map(str::to_owned);
                }
                start = parse_datetime(&p, tz);
                if let Some(dt) = start {
                    all_day = dt.minute.is_none();
                }
            }
            "DTEND" => {
                end_supplied = true;
                if unresolved_zone.is_none() {
                    unresolved_zone = unresolved_tzid(&p, tz).map(str::to_owned);
                }
                end = parse_datetime(&p, tz);
            }
            "DURATION" => duration_minutes = parse_duration(p.value).map(|m| m.max(0)),
            "RRULE" => {
                let parsed = parse_rrule(p.value);
                recurrence = parsed.freq;
                recurrence_until = parsed.until;
                recurrence_count = parsed.count;
                recurrence_interval = parsed.interval;
                recurrence_weekdays = parsed.weekdays;
                unsupported_rrule = parsed.unsupported;
                // Unsupported recurrence text round-trips unchanged until a confirmed rule edit.
                if !unsupported_rrule.is_empty() {
                    recurrence_raw = Some(p.value.trim().to_owned());
                }
            }
            "UID" => {
                let value = p.value.trim();
                if crate::ics_safety::safe_uid(value) {
                    uid = Some(value.to_owned());
                } else if !value.is_empty() {
                    uid_rejected = true;
                }
            }
            "RECURRENCE-ID" => {
                recurrence_id_date = parse_date_prefix(p.value.trim());
            }
            "STATUS" => cancelled = p.value.trim().eq_ignore_ascii_case("CANCELLED"),
            "EXDATE" => {
                for part in p.value.split(',') {
                    if let Some(d) = parse_date_prefix(part.trim()) {
                        exceptions.push(d);
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }

    let Some(start) = start else {
        let safe_title = safe_feedback_fragment(&title, "untitled event");
        return Err(crate::l10n::format(
            "Skipped an event with a missing or invalid start date/time (\"{safe_title}\").",
            &[("safe_title", (safe_title).as_str().into())],
        ));
    };

    if end_supplied && end.is_none() {
        let safe_title = safe_feedback_fragment(&title, "untitled event");
        return Err(crate::l10n::format(
            "Skipped \"{safe_title}\": its end date/time is invalid.",
            &[("safe_title", (safe_title).as_str().into())],
        ));
    }

    if let Some(zone) = &unresolved_zone {
        let safe_title = safe_feedback_fragment(&title, "untitled event");
        let safe_zone = safe_feedback_fragment(zone, "an undeclared zone");
        return Err(crate::l10n::format(
            "Skipped \"{safe_title}\": its time zone \"{safe_zone}\" is missing or requires unsupported daylight-saving transitions, so its exact time could not be resolved.",
            &[
                ("safe_title", (safe_title).as_str().into()),
                ("safe_zone", (safe_zone).as_str().into()),
            ],
        ));
    }

    if start.utc_offset.is_some() && recurrence != Recurrence::None {
        let safe_title = safe_feedback_fragment(&title, "untitled event");
        return Err(crate::l10n::format(
            "Skipped a recurring event with a fixed time zone (UTC or TZID), which isn't supported yet (\"{safe_title}\").",
            &[("safe_title", (safe_title).as_str().into())],
        ));
    }

    let (start_date, start_min, tz_offset) = place(start);

    // RFC 5545 DATE ends are exclusive; the domain stores inclusive end dates.
    let has_all_day_extent = all_day && (end.is_some() || duration_minutes.is_some());
    let (raw_end_date, end_min) = if let Some(e) = end {
        let (d, m, _) = place(e);
        (d, m)
    } else if let Some(dur) = duration_minutes {
        add_minutes(start_date, start_min.unwrap_or(0), dur, all_day)
    } else if all_day {
        (start_date, None)
    } else {
        add_minutes(start_date, start_min.unwrap_or(0), 60, false)
    };
    let end_date = if has_all_day_extent && raw_end_date > start_date {
        raw_end_date.add_days(-1)
    } else {
        raw_end_date.max(start_date)
    };

    if Date::new(start_date.year(), start_date.month(), start_date.day()).is_none()
        || Date::new(end_date.year(), end_date.month(), end_date.day()).is_none()
    {
        let safe_title = safe_feedback_fragment(&title, "untitled event");
        return Err(crate::l10n::format(
            "Skipped \"{safe_title}\": its normalized dates exceed the supported calendar.",
            &[("safe_title", (safe_title).as_str().into())],
        ));
    }

    let (start_minute, end_minute) = if all_day {
        (
            MinuteOfDay::hm(0, 0),
            MinuteOfDay::new(crate::model::MINUTES_PER_DAY),
        )
    } else {
        let s = start_min.unwrap_or(0);
        let e = end_min.unwrap_or(s + 60);
        let e = if end_date == start_date { e.max(s) } else { e };
        (MinuteOfDay::new(s), MinuteOfDay::new(e))
    };

    let safe_title = safe_feedback_fragment(&title, "untitled event");
    let mut warnings: Vec<String> = Vec::new();
    if !unsupported_rrule.is_empty() {
        warnings.push(crate::l10n::format("Imported \"{safe_title}\" but ignored an unsupported repeat rule ({}); editing it may change how it repeats.", &[("safe_title", (safe_title).as_str().into()),("arg0", (safe_feedback_fragment(&unsupported_rrule.join(", "), "unrecognized parts")).as_str().into())]));
    }
    warnings.extend(crate::ics_safety::import_omission_warnings(
        &safe_title,
        url_rejected,
        attendee_rejected,
        uid_rejected,
    ));

    Ok((
        Event {
            id: 0,
            calendar_id: calendar_id.to_owned(),
            title,
            location,
            notes,
            url,
            invitees: invitees.join(", "),
            all_day,
            date: start_date,
            end_date,
            start_minute,
            end_minute,
            recurrence,
            recurrence_until,
            recurrence_count,
            recurrence_interval,
            exceptions,
            alert,
            color_category: None,
            // Resolve each anchored endpoint independently when importing to the system timezone.
            tz_anchor: if tz_offset.is_some() {
                crate::model::TzAnchor::FixedUtc {
                    start_minutes: 0,
                    end_minutes: 0,
                }
            } else {
                crate::model::TzAnchor::Floating
            },
            recurrence_weekdays,
            recurrence_raw,
            uid,
            recurrence_id: recurrence_id_date,
        },
        warnings,
        cancelled,
    ))
}

fn place(dt: IcsDateTime) -> (Date, Option<i32>, Option<i32>) {
    let Some(min) = dt.minute else {
        return (dt.date, None, None);
    };
    let Some(off) = dt.utc_offset else {
        return (dt.date, Some(min), None);
    };
    let total = i64::from(dt.date.to_days()) * i64::from(crate::model::MINUTES_PER_DAY)
        + i64::from(min)
        - i64::from(off);
    let day = total.div_euclid(i64::from(crate::model::MINUTES_PER_DAY));
    let minute = total.rem_euclid(i64::from(crate::model::MINUTES_PER_DAY)) as i32;
    (Date::from_days(day as i32), Some(minute), Some(0))
}

fn add_minutes(date: Date, minute: i32, delta: i32, all_day: bool) -> (Date, Option<i32>) {
    if all_day {
        let days = (delta / crate::model::MINUTES_PER_DAY).max(0);
        return (date.add_days(days), None);
    }
    let total = i64::from(date.to_days()) * i64::from(crate::model::MINUTES_PER_DAY)
        + i64::from(minute)
        + i64::from(delta);
    let day = total.div_euclid(i64::from(crate::model::MINUTES_PER_DAY)) as i32;
    let m = total.rem_euclid(i64::from(crate::model::MINUTES_PER_DAY)) as i32;
    (Date::from_days(day), Some(m))
}

fn parse_datetime(p: &Prop<'_>, tz: &TzDb) -> Option<IcsDateTime> {
    let v = p.value.trim();
    let is_date = p
        .param("VALUE")
        .is_some_and(|x| x.eq_ignore_ascii_case("DATE"))
        || (v.len() == 8 && v.bytes().all(|b| b.is_ascii_digit()));

    if is_date {
        if v.len() != 8 {
            return None;
        }
        let date = parse_date_prefix(v)?;
        return Some(IcsDateTime {
            date,
            minute: None,
            utc_offset: None,
        });
    }

    let (body, is_utc) = match v.strip_suffix('Z') {
        Some(rest) => (rest, true),
        None => (v, false),
    };
    if body.len() != 15 || body.as_bytes().get(8) != Some(&b'T') {
        return None;
    }
    let date = parse_date_prefix(body.get(..8)?)?;
    let clock = body.get(9..)?;
    if !clock.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let hh: i32 = clock.get(..2)?.parse().ok()?;
    let mm: i32 = clock.get(2..4)?.parse().ok()?;
    let ss: i32 = clock.get(4..6)?.parse().ok()?;
    if !(0..=23).contains(&hh) || !(0..=59).contains(&mm) || !(0..=60).contains(&ss) {
        return None;
    }
    let minute = hh * 60 + mm;

    let utc_offset = if is_utc {
        Some(0)
    } else {
        p.param("TZID").and_then(|id| tz.offset(id))
    };
    Some(IcsDateTime {
        date,
        minute: Some(minute),
        utc_offset,
    })
}

fn unresolved_tzid<'p>(p: &'p Prop<'_>, tz: &TzDb) -> Option<&'p str> {
    let v = p.value.trim();
    let is_date = p
        .param("VALUE")
        .is_some_and(|x| x.eq_ignore_ascii_case("DATE"))
        || (v.len() == 8 && v.bytes().all(|b| b.is_ascii_digit()));
    if is_date || v.ends_with('Z') {
        return None;
    }
    let id = p.param("TZID")?;
    if tz.offset(id).is_some() {
        return None;
    }
    Some(id)
}

pub(super) fn parse_date_prefix(s: &str) -> Option<Date> {
    if !s.get(..8)?.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let y: i32 = s.get(0..4)?.parse().ok()?;
    let m: u32 = s.get(4..6)?.parse().ok()?;
    let d: u32 = s.get(6..8)?.parse().ok()?;
    Date::new(y, m, d)
}

#[path = "ics/recurrence.rs"]
mod recurrence;

use recurrence::{parse_duration, parse_rrule, parse_valarm, unescape_text};

#[cfg(test)]
mod tests;
