//! System-local clock readings and per-date UTC offset resolution through chrono.

use chrono::{Datelike, Local, LocalResult, NaiveDate, Offset, TimeZone, Timelike};
use enzui_core::civil_date::Date;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalInstant {
    Utc(i64),
    Ambiguous(i64),
    Unresolvable,
}

#[must_use]
pub(crate) fn local_minute_to_utc(local_abs_minute: i64) -> LocalInstant {
    let Ok(day) = i32::try_from(local_abs_minute.div_euclid(1440)) else {
        return LocalInstant::Unresolvable;
    };
    let minute_of_day = local_abs_minute.rem_euclid(1440);
    let date = Date::from_days(day);
    let Some(naive_date) = NaiveDate::from_ymd_opt(date.year(), date.month(), date.day()) else {
        return LocalInstant::Unresolvable;
    };
    let Some(naive) =
        naive_date.and_hms_opt((minute_of_day / 60) as u32, (minute_of_day % 60) as u32, 0)
    else {
        return LocalInstant::Unresolvable;
    };
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(value) => LocalInstant::Utc(value.timestamp().div_euclid(60)),
        LocalResult::Ambiguous(first, second) => {
            LocalInstant::Ambiguous(first.timestamp().min(second.timestamp()).div_euclid(60))
        }
        LocalResult::None => LocalInstant::Unresolvable,
    }
}

#[must_use]
pub(crate) fn local_now() -> (Date, i32) {
    let now = Local::now();
    let date = Date::new(now.year(), now.month(), now.day()).unwrap_or_else(|| Date::from_days(0));
    let minute = now.hour() as i32 * 60 + now.minute() as i32;
    (date, minute)
}

#[must_use]
pub(crate) fn local_offset_at_utc(utc_date: Date, utc_minute: i32) -> i32 {
    let Some(naive_date) =
        chrono::NaiveDate::from_ymd_opt(utc_date.year(), utc_date.month(), utc_date.day())
    else {
        return 0;
    };
    let hour = utc_minute.div_euclid(60);
    let minute = utc_minute.rem_euclid(60);
    if !(0..=23).contains(&hour) {
        return 0;
    }
    let Some(naive) = naive_date.and_hms_opt(hour as u32, minute as u32, 0) else {
        return 0;
    };
    let local = naive.and_utc().with_timezone(&Local);
    local.offset().fix().local_minus_utc() / 60
}

enum BoundedContent {
    Admitted(Vec<u8>),
    Degraded,
}

#[must_use]
pub(crate) fn zone_fingerprint() -> u64 {
    use std::os::unix::ffi::OsStrExt;
    let tz = std::env::var_os("TZ");
    let tz_override = tz.as_deref().map(|value| bounded_bytes(value.as_bytes()));
    let symlink_target = std::fs::read_link(LOCALTIME_PATH)
        .ok()
        .map(|target| bounded_bytes(target.as_os_str().as_bytes()));
    let content = read_bounded_localtime();
    let (today, now_minute) = local_now();
    let current_offset = local_offset_at_utc(today, now_minute);
    let fallback_offsets = match &content {
        BoundedContent::Degraded => sample_fallback_offsets(today),
        BoundedContent::Admitted(_) => Vec::new(),
    };
    fold_zone_fingerprint(
        tz_override.as_deref(),
        symlink_target.as_deref(),
        &content,
        current_offset,
        &fallback_offsets,
    )
}

fn fold_zone_fingerprint(
    tz_override: Option<&[u8]>,
    symlink_target: Option<&[u8]>,
    content: &BoundedContent,
    current_offset: i32,
    fallback_offsets: &[i32],
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    tz_override.hash(&mut hasher);
    symlink_target.hash(&mut hasher);
    current_offset.hash(&mut hasher);
    match content {
        BoundedContent::Admitted(bytes) => {
            0u8.hash(&mut hasher);
            bytes.hash(&mut hasher);
        }
        BoundedContent::Degraded => {
            1u8.hash(&mut hasher);
            fallback_offsets.hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn read_bounded_localtime() -> BoundedContent {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(LOCALTIME_PATH) else {
        return BoundedContent::Degraded;
    };
    let mut bytes = Vec::new();
    if file
        .take(LOCALTIME_CONTENT_CAP as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return BoundedContent::Degraded;
    }
    if bytes.len() > LOCALTIME_CONTENT_CAP {
        return BoundedContent::Degraded;
    }
    BoundedContent::Admitted(bytes)
}

fn sample_fallback_offsets(today: Date) -> Vec<i32> {
    let mut offsets = Vec::with_capacity(DEGRADED_HORIZON_MONTHS as usize);
    let (mut year, mut month) = (today.year(), today.month());
    for _ in 0..DEGRADED_HORIZON_MONTHS {
        if let Some(first) = Date::new(year, month, 1) {
            offsets.push(local_offset_at_utc(first, 12 * 60));
        }
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
    }
    offsets
}

fn bounded_bytes(value: &[u8]) -> Vec<u8> {
    value[..value.len().min(IDENTITY_BYTES_CAP)].to_vec()
}

const LOCALTIME_PATH: &str = "/etc/localtime";

const LOCALTIME_CONTENT_CAP: usize = 64 * 1024;

const IDENTITY_BYTES_CAP: usize = 4 * 1024;

const DEGRADED_HORIZON_MONTHS: u32 = 26;
