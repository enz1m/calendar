//! Recurrence and alarm parsing: RRULE, VALARM, durations, and text.

use super::parse_date_prefix;
use super::{Date, Prop, Recurrence, Weekday};

pub(super) struct ParsedRrule {
    pub(super) freq: Recurrence,
    pub(super) until: Option<Date>,
    pub(super) count: Option<u32>,
    pub(super) interval: u32,
    pub(super) weekdays: Vec<Weekday>,
    pub(super) unsupported: Vec<String>,
}

pub(super) fn parse_byday_token(tok: &str) -> Option<(Option<i32>, Weekday)> {
    let tok = tok.trim();
    if tok.len() < 2 {
        return None;
    }
    let (num, day) = tok.split_at_checked(tok.len() - 2)?;
    let weekday = match day.to_ascii_uppercase().as_str() {
        "MO" => Weekday::Monday,
        "TU" => Weekday::Tuesday,
        "WE" => Weekday::Wednesday,
        "TH" => Weekday::Thursday,
        "FR" => Weekday::Friday,
        "SA" => Weekday::Saturday,
        "SU" => Weekday::Sunday,
        _ => return None,
    };
    let ordinal = if num.is_empty() {
        None
    } else {
        Some(num.parse::<i32>().ok()?)
    };
    Some((ordinal, weekday))
}

pub(super) fn parse_rrule(value: &str) -> ParsedRrule {
    let mut freq = Recurrence::None;
    let mut until = None;
    let mut count = None;
    let mut interval = 1u32;
    let mut byday: Option<Vec<String>> = None;
    let mut unsupported: Vec<String> = Vec::new();
    let mut schedule_altering = false;
    for part in value.split(';') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let val = v.trim();
        if key.eq_ignore_ascii_case("FREQ") {
            freq = match val.to_ascii_uppercase().as_str() {
                "DAILY" => Recurrence::Daily,
                "WEEKLY" => Recurrence::Weekly,
                "MONTHLY" => Recurrence::Monthly,
                "YEARLY" => Recurrence::Yearly,
                _ => Recurrence::None,
            };
        } else if key.eq_ignore_ascii_case("UNTIL") {
            until = parse_date_prefix(val);
        } else if key.eq_ignore_ascii_case("COUNT") {
            count = val.parse::<u32>().ok().map(|n| n.max(1));
        } else if key.eq_ignore_ascii_case("INTERVAL") {
            if let Ok(n) = val.parse::<u32>() {
                interval = n.max(1);
            }
        } else if key.eq_ignore_ascii_case("BYDAY") {
            byday = Some(val.split(',').map(|t| t.trim().to_owned()).collect());
        } else if key.eq_ignore_ascii_case("WKST") {
            // Non-Monday WKST changes BYDAY interval alignment and must remain an unsupported raw
            // rule.
            if !val.eq_ignore_ascii_case("MO") {
                unsupported.push("WKST".to_owned());
                schedule_altering = true;
            }
        } else if key
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("X-"))
        {
            // Non-standard extensions are retained but do not change the supported occurrence
            // expansion.
            unsupported.push(key.to_ascii_uppercase());
        } else if !key.is_empty() {
            unsupported.push(key.to_ascii_uppercase());
            schedule_altering = true;
        }
    }

    let mut weekdays = Vec::new();
    if let Some(tokens) = byday {
        let parsed: Vec<Option<(Option<i32>, Weekday)>> =
            tokens.iter().map(|t| parse_byday_token(t)).collect();
        let ordinal_or_unreadable = parsed
            .iter()
            .any(|p| p.is_none() || matches!(p, Some((Some(_), _))));
        if freq == Recurrence::Weekly && !ordinal_or_unreadable {
            weekdays = parsed
                .into_iter()
                .filter_map(|p| p.map(|(_, weekday)| weekday))
                .collect();
        } else {
            unsupported.push("BYDAY".to_owned());
            schedule_altering = true;
        }
    }
    // An unsupported schedule-altering part prevents trustworthy partial BYDAY expansion.
    if schedule_altering {
        weekdays.clear();
    }

    if until.is_some() {
        count = None;
    }
    ParsedRrule {
        freq,
        until,
        count,
        interval,
        weekdays,
        unsupported,
    }
}

pub(super) fn parse_valarm(lines: &[String]) -> Option<i32> {
    for line in lines {
        let Some(p) = Prop::parse(line) else { continue };
        if !p.name.eq_ignore_ascii_case("TRIGGER") {
            continue;
        }
        // The minutes-before-start model cannot represent RELATED=END alarms.
        if p.param("RELATED")
            .is_some_and(|v| v.eq_ignore_ascii_case("END"))
        {
            return None;
        }
        return parse_duration(p.value).and_then(i32::checked_neg);
    }
    None
}

pub(super) fn parse_duration(value: &str) -> Option<i32> {
    let mut s = value.trim();
    let neg = s.starts_with('-');
    s = s.trim_start_matches(['-', '+']);
    let s = s.strip_prefix('P')?;

    let mut total: i64 = 0;
    let mut in_time = false;
    let mut num = String::new();
    let mut saw_unit = false;
    for c in s.chars() {
        if c == 'T' {
            in_time = true;
            continue;
        }
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let n: i64 = num.parse().ok()?;
        num.clear();
        let mult = match (c, in_time) {
            ('W', false) => 7 * 24 * 60,
            ('D', false) => 24 * 60,
            ('H', true) => 60,
            ('M', true) => 1,
            ('S', true) => 0, // sub-minute rounded away
            _ => return None,
        };
        total = total.checked_add(n.checked_mul(mult)?)?;
        saw_unit = true;
    }
    if !saw_unit {
        return None;
    }
    let minutes = if neg { -total } else { total };
    i32::try_from(minutes).ok()
}

pub(super) fn unescape_text(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some('t' | 'T') => out.push('\t'),
            Some(',') => out.push(','),
            Some(';') => out.push(';'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}
