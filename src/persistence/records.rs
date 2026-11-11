//! Versioned calendar records and compatibility conversions.

use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PersistedState {
    pub version: u32,
    #[serde(default, deserialize_with = "bounded::calendars")]
    pub calendars: Vec<CalendarRec>,
    #[serde(default, deserialize_with = "bounded::events")]
    pub events: Vec<EventRec>,
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub settings: SettingsRec,
    #[serde(default, deserialize_with = "bounded::hidden_categories")]
    pub hidden_categories: Vec<String>,
    #[serde(default)]
    pub reminders: RemindersRec,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct RemindersRec {
    #[serde(default, deserialize_with = "bounded::fired_vec")]
    pub fired: Vec<FiredRec>,
    #[serde(default, deserialize_with = "bounded::snoozed_vec")]
    pub snoozed: Vec<SnoozeRec>,
    #[serde(default, deserialize_with = "bounded::short_option_string")]
    pub last_checked_date: Option<String>,
    #[serde(default)]
    pub last_checked_minute: i32,
    #[serde(default, deserialize_with = "bounded::snoozed_vec")]
    pub reconciliation_debt: Vec<SnoozeRec>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct FiredRec {
    #[serde(default)]
    pub event_id: u64,
    #[serde(default, deserialize_with = "bounded::short_string")]
    pub on: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct SnoozeRec {
    #[serde(default)]
    pub event_id: u64,
    #[serde(default, deserialize_with = "bounded::short_string")]
    pub on: String,
    #[serde(default, deserialize_with = "bounded::short_string")]
    pub until_date: String,
    #[serde(default)]
    pub until_minute: i32,
    #[serde(default)]
    pub until_utc_minute: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct CalendarRec {
    #[serde(default, deserialize_with = "bounded::short_string")]
    pub id: String,
    #[serde(default, deserialize_with = "bounded::medium_string")]
    pub name: String,
    #[serde(default, deserialize_with = "bounded::medium_string")]
    pub section: String,
    #[serde(default, deserialize_with = "bounded::medium_string")]
    pub color: String,
    #[serde(default)]
    pub visible: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct EventRec {
    #[serde(default)]
    pub id: u64,
    #[serde(default, deserialize_with = "bounded::short_string")]
    pub calendar_id: String,
    #[serde(default, deserialize_with = "bounded::text_field")]
    pub title: String,
    #[serde(default, deserialize_with = "bounded::text_field")]
    pub location: String,
    #[serde(default, deserialize_with = "bounded::text_field")]
    pub notes: String,
    #[serde(default, deserialize_with = "bounded::text_field")]
    pub url: String,
    #[serde(default, deserialize_with = "bounded::invitees_string")]
    pub invitees: String,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default, deserialize_with = "bounded::short_string")]
    pub date: String,
    #[serde(default, deserialize_with = "bounded::short_option_string")]
    pub end_date: Option<String>,
    #[serde(default)]
    pub start_minute: i32,
    #[serde(default)]
    pub end_minute: i32,
    #[serde(default, deserialize_with = "bounded::short_string")]
    pub recurrence: String,
    #[serde(default, deserialize_with = "bounded::short_option_string")]
    pub recurrence_until: Option<String>,
    #[serde(default)]
    pub recurrence_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence_interval: Option<u32>,
    #[serde(default, deserialize_with = "bounded::exceptions")]
    pub exceptions: Vec<String>,
    #[serde(default)]
    pub alert: Option<i32>,
    #[serde(default, deserialize_with = "bounded::short_option_string")]
    pub color_category: Option<String>,
    #[serde(default)]
    pub tz_offset_minutes: Option<i32>,
    #[serde(default)]
    pub end_tz_offset_minutes: Option<i32>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "bounded::recurrence_weekdays"
    )]
    pub recurrence_weekdays: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "bounded::text_option_string"
    )]
    pub recurrence_raw: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "bounded::short_option_string"
    )]
    pub uid: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "bounded::short_option_string"
    )]
    pub recurrence_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SettingsRec {
    #[serde(
        default = "default_first_day",
        deserialize_with = "bounded::short_string"
    )]
    pub first_day_of_week: String,
    #[serde(
        default = "default_time_format",
        deserialize_with = "bounded::short_string"
    )]
    pub time_format: String,
    #[serde(default)]
    pub work_start_minute: i32,
    #[serde(default)]
    pub work_end_minute: i32,
    #[serde(default)]
    pub show_week_numbers: bool,
    #[serde(default)]
    pub show_secondary_tz: bool,
    #[serde(default)]
    pub secondary_tz_offset_minutes: i32,
}

fn default_first_day() -> String {
    weekday_key(Settings::default().first_day_of_week).to_owned()
}

fn default_time_format() -> String {
    time_format_key(Settings::default().time_format).to_owned()
}

fn time_format_key(p: crate::model::TimeFormatPref) -> &'static str {
    match p {
        crate::model::TimeFormatPref::System => "system",
        crate::model::TimeFormatPref::TwentyFourHour => "24h",
        crate::model::TimeFormatPref::TwelveHour => "12h",
    }
}

fn time_format_from_key(s: &str) -> crate::model::TimeFormatPref {
    crate::model::TimeFormatPref::ALL
        .into_iter()
        .find(|p| time_format_key(*p).eq_ignore_ascii_case(s.trim()))
        .unwrap_or_default()
}

impl Default for SettingsRec {
    fn default() -> Self {
        settings_to_rec(&Settings::default())
    }
}

pub(super) fn weekday_key(w: Weekday) -> &'static str {
    match w {
        Weekday::Monday => "monday",
        Weekday::Tuesday => "tuesday",
        Weekday::Wednesday => "wednesday",
        Weekday::Thursday => "thursday",
        Weekday::Friday => "friday",
        Weekday::Saturday => "saturday",
        Weekday::Sunday => "sunday",
    }
}

pub(super) fn weekday_from_key(s: &str) -> Weekday {
    Weekday::ALL
        .into_iter()
        .find(|w| weekday_key(*w).eq_ignore_ascii_case(s.trim()))
        .unwrap_or_else(|| Settings::default().first_day_of_week)
}

pub(super) fn recurrence_key(r: Recurrence) -> &'static str {
    match r {
        Recurrence::None => "none",
        Recurrence::Daily => "daily",
        Recurrence::Weekly => "weekly",
        Recurrence::Monthly => "monthly",
        Recurrence::Yearly => "yearly",
    }
}

pub(super) fn recurrence_from_key(s: &str) -> Recurrence {
    Recurrence::ALL
        .into_iter()
        .find(|r| recurrence_key(*r).eq_ignore_ascii_case(s.trim()))
        .unwrap_or(Recurrence::None)
}

fn date_from_iso(s: &str) -> Option<Date> {
    let (year_month, day) = s.trim().rsplit_once('-')?;
    let (year, month) = year_month.rsplit_once('-')?;
    Date::new(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
}

fn date_to_iso(d: Date) -> String {
    d.to_string()
}

fn calendar_to_rec(c: &Calendar) -> CalendarRec {
    CalendarRec {
        id: c.id.clone(),
        name: c.name.clone(),
        section: c.section.clone(),
        color: c.color.clone(),
        visible: c.visible,
    }
}

fn rec_to_calendar(r: &CalendarRec) -> Option<Calendar> {
    if r.id.trim().is_empty() {
        return None;
    }
    Some(Calendar {
        id: r.id.clone(),
        name: r.name.clone(),
        section: r.section.clone(),
        color: r.color.clone(),
        visible: r.visible,
    })
}

fn event_to_rec(e: &Event) -> EventRec {
    EventRec {
        id: e.id,
        calendar_id: e.calendar_id.clone(),
        title: e.title.clone(),
        location: e.location.clone(),
        notes: e.notes.clone(),
        url: e.url.clone(),
        invitees: e.invitees.clone(),
        all_day: e.all_day,
        date: date_to_iso(e.date),
        end_date: Some(date_to_iso(e.end_date)),
        start_minute: e.start_minute.get(),
        end_minute: e.end_minute.get(),
        recurrence: recurrence_key(e.recurrence).to_owned(),
        recurrence_until: e.recurrence_until.map(date_to_iso),
        recurrence_count: e.recurrence_count,
        recurrence_interval: (e.recurrence_interval > 1).then_some(e.recurrence_interval),
        exceptions: e.exceptions.iter().copied().map(date_to_iso).collect(),
        alert: e.alert,
        color_category: e.color_category.clone(),
        tz_offset_minutes: e.tz_anchor.offsets().map(|(start, _)| start),
        end_tz_offset_minutes: e.tz_anchor.offsets().map(|(_, end)| end),
        recurrence_weekdays: e
            .recurrence_weekdays
            .iter()
            .map(|w| weekday_code(*w).to_owned())
            .collect(),
        recurrence_raw: e.recurrence_raw.clone(),
        uid: e.uid.clone(),
        recurrence_id: e.recurrence_id.map(date_to_iso),
    }
}

fn weekday_code(w: enzui_core::civil_date::Weekday) -> &'static str {
    use enzui_core::civil_date::Weekday;
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

fn weekday_from_code(code: &str) -> Option<enzui_core::civil_date::Weekday> {
    use enzui_core::civil_date::Weekday;
    match code.trim().to_ascii_uppercase().as_str() {
        "MO" => Some(Weekday::Monday),
        "TU" => Some(Weekday::Tuesday),
        "WE" => Some(Weekday::Wednesday),
        "TH" => Some(Weekday::Thursday),
        "FR" => Some(Weekday::Friday),
        "SA" => Some(Weekday::Saturday),
        "SU" => Some(Weekday::Sunday),
        _ => None,
    }
}

fn rec_to_event(r: &EventRec) -> Option<Event> {
    let date = date_from_iso(&r.date)?;
    let end_date = r
        .end_date
        .as_deref()
        .and_then(date_from_iso)
        .filter(|end| *end >= date)
        .unwrap_or(date);
    Some(Event {
        id: r.id,
        calendar_id: r.calendar_id.clone(),
        title: r.title.clone(),
        location: r.location.clone(),
        notes: r.notes.clone(),
        url: r.url.clone(),
        invitees: r.invitees.clone(),
        all_day: r.all_day,
        date,
        end_date,
        start_minute: MinuteOfDay::new(r.start_minute),
        end_minute: MinuteOfDay::new(r.end_minute),
        recurrence: recurrence_from_key(&r.recurrence),
        recurrence_until: r.recurrence_until.as_deref().and_then(date_from_iso),
        // A recovered zero count is floored to one to avoid silently erasing the series.
        recurrence_count: r.recurrence_count.map(|n| n.max(1)),
        // Legacy zero or absent recurrence intervals mean every period, never an empty series.
        recurrence_interval: r.recurrence_interval.unwrap_or(1).max(1),
        exceptions: r
            .exceptions
            .iter()
            .filter_map(|s| date_from_iso(s))
            .collect(),
        alert: r.alert,
        color_category: r.color_category.clone(),
        // Legacy records without an end offset inherit the start offset before versioned migration.
        tz_anchor: match r.tz_offset_minutes {
            None => crate::model::TzAnchor::Floating,
            Some(start_minutes) => crate::model::TzAnchor::FixedUtc {
                start_minutes,
                end_minutes: r.end_tz_offset_minutes.unwrap_or(start_minutes),
            },
        },
        recurrence_weekdays: r
            .recurrence_weekdays
            .iter()
            .filter_map(|c| weekday_from_code(c))
            .collect(),
        recurrence_raw: r.recurrence_raw.clone(),
        uid: r.uid.clone(),
        recurrence_id: r.recurrence_id.as_deref().and_then(date_from_iso),
    })
}

fn coarsen_last_checked(minute: i64) -> i64 {
    minute.div_euclid(60) * 60
}

fn reminders_to_rec(log: &ReminderLog) -> RemindersRec {
    let (last_checked_date, last_checked_minute) = match log.last_checked.map(coarsen_last_checked)
    {
        Some(m) => {
            let date = Date::from_days(
                i32::try_from(m.div_euclid(reminders::MINUTES_PER_DAY)).unwrap_or_default(),
            );
            (
                Some(date_to_iso(date)),
                i32::try_from(m.rem_euclid(reminders::MINUTES_PER_DAY)).unwrap_or_default(),
            )
        }
        None => (None, 0),
    };
    RemindersRec {
        fired: log
            .fired
            .iter()
            .map(|k| FiredRec {
                event_id: k.event_id,
                on: date_to_iso(k.on),
            })
            .collect(),
        snoozed: log.snoozed.iter().map(|s| snooze_to_rec(s, true)).collect(),
        last_checked_date,
        last_checked_minute,
        reconciliation_debt: log
            .reconciliation_debt
            .iter()
            .map(|s| snooze_to_rec(s, false))
            .collect(),
    }
}

fn snooze_to_rec(s: &Snoozed, native: bool) -> SnoozeRec {
    let until_date = Date::from_days(
        i32::try_from(s.until_utc.div_euclid(reminders::MINUTES_PER_DAY)).unwrap_or_default(),
    );
    SnoozeRec {
        event_id: s.key.event_id,
        on: date_to_iso(s.key.on),
        until_date: date_to_iso(until_date),
        until_minute: i32::try_from(s.until_utc.rem_euclid(reminders::MINUTES_PER_DAY))
            .unwrap_or_default(),
        until_utc_minute: native.then_some(s.until_utc),
    }
}

fn rec_to_reminders(r: &RemindersRec) -> ReminderLog {
    let mut log = ReminderLog {
        fired: r
            .fired
            .iter()
            .filter_map(|f| {
                date_from_iso(&f.on).map(|on| ReminderKey {
                    event_id: f.event_id,
                    on,
                })
            })
            .collect(),
        snoozed: Vec::new(),
        last_checked: r
            .last_checked_date
            .as_deref()
            .and_then(date_from_iso)
            .map(|d| reminders::abs_minute(d, r.last_checked_minute)),
        reconciliation_debt: Vec::new(),
    };
    // Unresolvable legacy snooze times remain durable reconciliation debt instead of guessed
    // instants.
    for s in r.snoozed.iter().chain(r.reconciliation_debt.iter()) {
        migrate_snooze_rec(s, &mut log.snoozed, &mut log.reconciliation_debt);
    }
    log
}

fn migrate_snooze_rec(s: &SnoozeRec, snoozed: &mut Vec<Snoozed>, debt: &mut Vec<Snoozed>) {
    let Some(on) = date_from_iso(&s.on) else {
        return;
    };
    let key = ReminderKey {
        event_id: s.event_id,
        on,
    };
    if let Some(until_utc) = s.until_utc_minute {
        snoozed.push(Snoozed { key, until_utc });
        return;
    }
    let Some(until_date) = date_from_iso(&s.until_date) else {
        return;
    };
    let local_abs = reminders::abs_minute(until_date, s.until_minute);
    match crate::sysclock::local_minute_to_utc(local_abs) {
        crate::sysclock::LocalInstant::Utc(until_utc) => snoozed.push(Snoozed { key, until_utc }),
        // Ambiguous folds and gaps retain the original local deadline for later reconciliation;
        // debt does not fire.
        crate::sysclock::LocalInstant::Ambiguous(_)
        | crate::sysclock::LocalInstant::Unresolvable => debt.push(Snoozed {
            key,
            until_utc: local_abs,
        }),
    }
}

fn settings_to_rec(s: &Settings) -> SettingsRec {
    SettingsRec {
        first_day_of_week: weekday_key(s.first_day_of_week).to_owned(),
        time_format: time_format_key(s.time_format).to_owned(),
        work_start_minute: s.work_start_minute,
        work_end_minute: s.work_end_minute,
        show_week_numbers: s.show_week_numbers,
        show_secondary_tz: s.show_secondary_tz,
        secondary_tz_offset_minutes: s.secondary_tz_offset_minutes,
    }
}

fn rec_to_settings(r: &SettingsRec) -> Settings {
    Settings {
        first_day_of_week: weekday_from_key(&r.first_day_of_week),
        time_format: time_format_from_key(&r.time_format),
        work_start_minute: r.work_start_minute,
        work_end_minute: r.work_end_minute,
        show_week_numbers: r.show_week_numbers,
        show_secondary_tz: r.show_secondary_tz,
        secondary_tz_offset_minutes: r.secondary_tz_offset_minutes,
    }
}

#[must_use]
pub(crate) fn snapshot(state: &AppState) -> PersistedState {
    PersistedState {
        version: FORMAT_VERSION,
        calendars: state.calendars.iter().map(calendar_to_rec).collect(),
        events: state.events.iter().map(event_to_rec).collect(),
        next_id: state.next_id,
        settings: settings_to_rec(&state.settings),
        hidden_categories: state.hidden_categories.clone(),
        reminders: reminders_to_rec(&state.reminders),
    }
}

#[must_use]
pub(crate) fn restore(today: Date, now_minute: i32, doc: &PersistedState) -> AppState {
    let calendars: Vec<Calendar> = doc.calendars.iter().filter_map(rec_to_calendar).collect();
    let mut events: Vec<Event> = doc.events.iter().filter_map(rec_to_event).collect();
    // Version-one UTC digits migrate endpoint by endpoint; floating events remain unchanged.
    if doc.version < FORMAT_VERSION {
        for event in &mut events {
            if event.tz_anchor.is_anchored() {
                let start_offset =
                    crate::sysclock::local_offset_at_utc(event.date, event.start_minute.get());
                let end_offset =
                    crate::sysclock::local_offset_at_utc(event.end_date, event.end_minute.get());
                event.anchor_utc_to_local(start_offset, end_offset);
                // An unsupported legacy anchored recurrence is reduced to its correctly placed base
                // occurrence.
                if event.recurrence != Recurrence::None {
                    event.recurrence = Recurrence::None;
                    event.recurrence_until = None;
                    event.recurrence_count = None;
                    event.exceptions.clear();
                }
            }
        }
    }
    // Zone reprojection rekeys fired marks by date while UTC snooze deadlines remain unchanged.
    let mut date_moves: Vec<(u64, Date, Date)> = Vec::new();
    for event in &mut events {
        if !event.tz_anchor.is_anchored() {
            continue;
        }
        let before_date = event.date;
        event.reproject_fixed_utc(crate::sysclock::local_offset_at_utc);
        if event.date != before_date {
            date_moves.push((event.id, before_date, event.date));
        }
    }
    let mut reminders = rec_to_reminders(&doc.reminders);
    for (event_id, from, to) in date_moves {
        reminders.remap_occurrence(event_id, from, to);
    }
    // Recovered allocators are bounded above the retained maximum; u64::MAX keeps the exhausted
    // sentinel.
    let max_id = events.iter().map(|e| e.id).max();
    let next_id = match max_id {
        Some(u64::MAX) => 0,
        Some(m) => doc.next_id.max(m + 1),
        None => doc.next_id.max(1),
    };
    AppState::with_data(
        today,
        now_minute,
        calendars,
        events,
        next_id,
        rec_to_settings(&doc.settings),
        doc.hidden_categories.clone(),
        reminders,
    )
}
