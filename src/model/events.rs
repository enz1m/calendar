//! Event identities, recurrence queries, and UTC endpoint anchors.

use super::*;

#[derive(Debug, Clone, PartialEq, Hash)]
pub(crate) struct Event {
    pub id: u64,
    pub calendar_id: String,
    pub title: String,
    pub location: String,
    pub notes: String,
    pub url: String,
    pub invitees: String,
    pub all_day: bool,
    pub date: Date,
    pub end_date: Date,
    pub start_minute: MinuteOfDay,
    pub end_minute: MinuteOfDay,
    pub recurrence: Recurrence,
    pub recurrence_until: Option<Date>,
    pub recurrence_count: Option<u32>,
    pub recurrence_interval: u32,
    pub exceptions: Vec<Date>,
    pub alert: Option<i32>,
    pub color_category: Option<String>,
    pub tz_anchor: TzAnchor,
    pub recurrence_weekdays: Vec<Weekday>,
    pub recurrence_raw: Option<String>,
    pub uid: Option<String>,
    pub recurrence_id: Option<Date>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub(crate) enum TzAnchor {
    #[default]
    Floating,
    FixedUtc {
        start_minutes: i32,
        end_minutes: i32,
    },
}

impl TzAnchor {
    #[must_use]
    pub(crate) fn offsets(self) -> Option<(i32, i32)> {
        match self {
            TzAnchor::Floating => None,
            TzAnchor::FixedUtc {
                start_minutes,
                end_minutes,
            } => Some((start_minutes, end_minutes)),
        }
    }

    #[must_use]
    pub(crate) fn is_anchored(self) -> bool {
        matches!(self, TzAnchor::FixedUtc { .. })
    }
}

impl Event {
    #[must_use]
    pub(crate) fn new(id: u64, calendar_id: impl Into<String>, date: Date) -> Self {
        Self {
            id,
            calendar_id: calendar_id.into(),
            title: String::new(),
            location: String::new(),
            notes: String::new(),
            url: String::new(),
            invitees: String::new(),
            all_day: false,
            date,
            end_date: date,
            start_minute: MinuteOfDay::hm(9, 0),
            end_minute: MinuteOfDay::hm(10, 0),
            recurrence: Recurrence::None,
            recurrence_until: None,
            recurrence_count: None,
            recurrence_interval: 1,
            exceptions: Vec::new(),
            alert: None,
            color_category: None,
            tz_anchor: TzAnchor::Floating,
            recurrence_weekdays: Vec::new(),
            recurrence_raw: None,
            uid: None,
            recurrence_id: None,
        }
    }

    #[must_use]
    pub(crate) fn duration_minutes(&self) -> i32 {
        let days = i64::from((self.end_date.to_days() - self.date.to_days()).max(0));
        let minutes = days * i64::from(MINUTES_PER_DAY)
            + i64::from(self.end_minute.get() - self.start_minute.get());
        i32::try_from(minutes.max(0)).unwrap_or(i32::MAX)
    }

    pub(crate) fn set_duration_minutes(&mut self, duration: i32) {
        let per_day = i64::from(MINUTES_PER_DAY);
        let start = i64::from(self.start_minute.get());
        let total = start + i64::from(duration.max(0));
        let mut days = total.div_euclid(per_day);
        let mut minute = total.rem_euclid(per_day);
        // A midnight end belongs to the prior day at 24:00 and must not create another day segment.
        if minute == 0 && total > start {
            days -= 1;
            minute = per_day;
        }
        self.end_date = self.date.add_days(i32::try_from(days).unwrap_or(i32::MAX));
        self.end_minute = MinuteOfDay::new(i32::try_from(minute).unwrap_or(MINUTES_PER_DAY));
    }

    #[must_use]
    pub(crate) fn span_days(&self) -> i32 {
        (self.end_date.to_days() - self.date.to_days()).max(0)
    }

    pub(crate) fn shift_wallclock(&mut self, start_delta: i32, end_delta: i32) {
        if start_delta == 0 && end_delta == 0 {
            return;
        }
        let per_day = i64::from(MINUTES_PER_DAY);
        let shift = |date: Date, minute: i32, delta: i32| -> (Date, i32) {
            let total = i64::from(date.to_days()) * per_day + i64::from(minute) + i64::from(delta);
            (
                Date::from_days(i32::try_from(total.div_euclid(per_day)).unwrap_or(i32::MAX)),
                total.rem_euclid(per_day) as i32,
            )
        };
        let (new_date, new_start) = shift(self.date, self.start_minute.get(), start_delta);
        let (new_end_date, new_end) = shift(self.end_date, self.end_minute.get(), end_delta);
        let carry = new_date.to_days() - self.date.to_days();
        self.date = new_date;
        self.start_minute = MinuteOfDay::new(new_start);
        self.end_date = new_end_date;
        self.end_minute = MinuteOfDay::new(new_end);
        if let Some(until) = self.recurrence_until {
            self.recurrence_until = Some(until.add_days(carry));
        }
        for exception in &mut self.exceptions {
            *exception = exception.add_days(carry);
        }
    }

    pub(crate) fn anchor_utc_to_local(&mut self, start_offset: i32, end_offset: i32) {
        self.shift_wallclock(start_offset, end_offset);
        self.tz_anchor = TzAnchor::FixedUtc {
            start_minutes: start_offset,
            end_minutes: end_offset,
        };
    }

    pub(crate) fn reproject_fixed_utc(&mut self, offset_at: impl Fn(Date, i32) -> i32) {
        let TzAnchor::FixedUtc {
            start_minutes,
            end_minutes,
        } = self.tz_anchor
        else {
            return;
        };
        self.shift_wallclock(-start_minutes, -end_minutes);
        let new_start = offset_at(self.date, self.start_minute.get());
        let new_end = offset_at(self.end_date, self.end_minute.get());
        self.anchor_utc_to_local(new_start, new_end);
    }

    #[must_use]
    pub(crate) fn import_signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.title.hash(&mut h);
        self.date.to_days().hash(&mut h);
        self.end_date.to_days().hash(&mut h);
        self.all_day.hash(&mut h);
        self.start_minute.get().hash(&mut h);
        self.end_minute.get().hash(&mut h);
        (self.recurrence as u8).hash(&mut h);
        h.finish()
    }

    pub(crate) fn color(&self, calendars: &[Calendar]) -> Color {
        if let Some(key) = &self.color_category {
            return palette_color(key);
        }
        calendars
            .iter()
            .find(|c| c.id == self.calendar_id)
            .map_or_else(|| palette_color(""), |c| palette_color(&c.color))
    }

    #[must_use]
    pub(crate) fn time_label(&self, fmt: &TimeFormat) -> String {
        if self.all_day {
            crate::l10n::tr("All day")
        } else {
            format!(
                "{} – {}",
                self.start_minute.format(fmt),
                self.end_minute.format(fmt)
            )
        }
    }

    #[must_use]
    pub(crate) fn occurs_on(&self, date: Date) -> bool {
        crate::recurrence::occurs_on(self, date)
    }

    #[must_use]
    pub(crate) fn recurrence_end_kind(&self) -> RecurrenceEndKind {
        if self.recurrence_until.is_some() {
            RecurrenceEndKind::OnDate
        } else if self.recurrence_count.is_some() {
            RecurrenceEndKind::AfterCount
        } else {
            RecurrenceEndKind::Never
        }
    }

    #[must_use]
    pub(crate) fn next_occurrence(&self, from: Date) -> Option<Date> {
        crate::recurrence::next_on_or_after(self, from)
    }

    #[must_use]
    pub(crate) fn occurrence_on(&self, date: Date) -> Self {
        let end_date = date.add_days(self.span_days());
        Self {
            date,
            end_date,
            ..self.clone()
        }
    }

    #[must_use]
    pub(crate) fn occurrence_starts_intersecting(&self, start: Date, end: Date) -> Vec<Date> {
        if end < start {
            return Vec::new();
        }
        let first_start = start.add_days(-self.span_days());
        crate::recurrence::occurrences_in_range(self, first_start, end)
            .into_iter()
            .filter(|date| date.add_days(self.span_days()) >= start)
            .collect()
    }

    #[must_use]
    pub(crate) fn occurrences_intersecting(&self, start: Date, end: Date) -> Vec<Self> {
        self.occurrence_starts_intersecting(start, end)
            .into_iter()
            .map(|date| self.occurrence_on(date))
            .collect()
    }
}
