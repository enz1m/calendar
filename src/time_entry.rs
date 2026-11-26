//! Editable time drafts for event editors and quick-add popovers.

use enzui_core::schedule::format_duration;
use enzui_core::time_of_day::{TimeFormat, TimeOfDay};
use enzui_widgets::time_field::TimeFieldEvent;

use crate::model::{Event, MINUTES_PER_DAY, MinuteOfDay};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TimeDrafts {
    pub format: Option<TimeFormat>,
    pub start: String,
    pub end: String,
    pub start_error: Option<String>,
    pub end_error: Option<String>,
}

impl TimeDrafts {
    #[must_use]
    pub(crate) fn seeded(event: &Event, fmt: &TimeFormat) -> Self {
        Self {
            format: Some(fmt.clone()),
            start: event.start_minute.format(fmt),
            end: event.end_minute.format(fmt),
            start_error: None,
            end_error: None,
        }
    }

    pub(crate) fn reseed(&mut self, event: &Event, fmt: &TimeFormat) {
        self.start = event.start_minute.format(fmt);
        self.end = event.end_minute.format(fmt);
    }
}

#[must_use]
pub(crate) fn refusal_hint(fmt: &TimeFormat) -> String {
    crate::l10n::format(
        "Enter a time like {}",
        &[(
            "arg0",
            fmt.format(TimeOfDay::hm(14, 5).expect("14:05 is a time"))
                .into(),
        )],
    )
}

// Time-field commits discard seconds because event times have minute precision.
fn minute_of(time: TimeOfDay) -> MinuteOfDay {
    let hour = i32::try_from(time.hour()).unwrap_or(0);
    let minute = i32::try_from(time.minute()).unwrap_or(0);
    MinuteOfDay::hm(hour, minute)
}

pub(crate) fn apply_start(
    event: &mut Event,
    drafts: &mut TimeDrafts,
    fmt: &TimeFormat,
    outcome: TimeFieldEvent,
) {
    match outcome {
        TimeFieldEvent::Drafted(text) => drafts.start = text,
        TimeFieldEvent::Committed(time) => {
            let duration = event.duration_minutes();
            event.start_minute = minute_of(time);
            event.set_duration_minutes(duration);
            drafts.reseed(event, fmt);
            drafts.start_error = None;
            drafts.end_error = None;
        }
        TimeFieldEvent::Refused(_) => drafts.start_error = Some(refusal_hint(fmt)),
    }
}

pub(crate) fn apply_end(
    event: &mut Event,
    drafts: &mut TimeDrafts,
    fmt: &TimeFormat,
    outcome: TimeFieldEvent,
) {
    match outcome {
        TimeFieldEvent::Drafted(text) => drafts.end = text,
        TimeFieldEvent::Committed(time) => {
            event.end_minute = minute_of(time);
            if event.end_date == event.date && event.end_minute.get() <= event.start_minute.get() {
                event.end_date = event.date.add_days(1);
            }
            drafts.reseed(event, fmt);
            drafts.start_error = None;
            drafts.end_error = None;
        }
        TimeFieldEvent::Refused(_) => drafts.end_error = Some(refusal_hint(fmt)),
    }
}

// Display the end-of-day bound as midnight instead of clamping it to 23:59.
#[must_use]
pub(crate) fn work_hour_text(minute: i32, fmt: &TimeFormat) -> String {
    if minute >= MINUTES_PER_DAY {
        fmt.format(TimeOfDay::MIDNIGHT)
    } else {
        MinuteOfDay::new(minute).format(fmt)
    }
}

// Midnight denotes 24:00 for an end bound and 00:00 for a start bound.
#[must_use]
pub(crate) fn parse_work_hour(text: &str, fmt: &TimeFormat, end: bool) -> Option<i32> {
    let minute = minute_of(fmt.parse(text).ok()?).get();
    Some(if end && minute == 0 {
        MINUTES_PER_DAY
    } else {
        minute
    })
}

#[must_use]
pub(crate) fn duration_label(event: &Event) -> String {
    format_duration(event.duration_minutes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use enzui_core::time_of_day::HourCycle;

    fn h12() -> TimeFormat {
        TimeFormat::new(HourCycle::H12)
    }

    #[test]
    fn desktop_hour_cycle_updates_labels_without_reinterpreting_open_drafts() {
        use crate::model::TimeFormatPref;
        use crate::state::{AppState, Overlay};
        use enzui_core::desktop_settings::{DesktopSettings, with_settings};

        let date = enzui_core::civil_date::Date::new(2026, 11, 26).unwrap();
        let mut state = AppState::empty(date, 0);
        let desktop = |clock_24h| DesktopSettings {
            clock_24h: Some(clock_24h),
            ..DesktopSettings::default()
        };
        let mut event = Event::new(1, "cal.home", date);
        event.start_minute = MinuteOfDay::hm(17, 30);
        with_settings(desktop(false), || {
            assert_eq!(state.time_format().cycle(), HourCycle::H12);
            state.set_overlay(Overlay::Editor(Box::new(event)));
        });
        with_settings(desktop(true), || {
            assert_eq!(state.time_format().cycle(), HourCycle::H23);
            let parsed = state.entry_time_format().parse(&state.time_drafts.start).unwrap();
            assert_eq!(parsed, TimeOfDay::hm(17, 30).unwrap());
            state.settings.time_format = TimeFormatPref::TwelveHour;
            assert_eq!(state.time_format().cycle(), HourCycle::H12);
        });
    }

    #[test]
    fn typed_working_hours_keep_their_minutes() {
        assert_eq!(parse_work_hour("8:30 AM", &h12(), false), Some(8 * 60 + 30));
        assert_eq!(parse_work_hour("5:45 PM", &h12(), true), Some(17 * 60 + 45));
        assert_eq!(parse_work_hour("soon", &h12(), false), None);
    }

    #[test]
    fn midnight_is_the_end_of_the_working_day() {
        assert_eq!(
            parse_work_hour("12:00 AM", &h12(), true),
            Some(MINUTES_PER_DAY)
        );
        assert_eq!(parse_work_hour("12:00 AM", &h12(), false), Some(0));
        assert_eq!(
            work_hour_text(MINUTES_PER_DAY, &h12()),
            work_hour_text(0, &h12())
        );
    }
}
