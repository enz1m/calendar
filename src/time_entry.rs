//! Editable time drafts for event editors and quick-add popovers.

use enzui_core::schedule::format_duration;
use enzui_core::time_of_day::{TimeFormat, TimeOfDay};
use enzui_widgets::time_field::TimeFieldEvent;

use crate::model::{Event, MinuteOfDay};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TimeDrafts {
    pub start: String,
    pub end: String,
    pub start_error: Option<String>,
    pub end_error: Option<String>,
}

impl TimeDrafts {
    #[must_use]
    pub(crate) fn seeded(event: &Event, fmt: &TimeFormat) -> Self {
        Self {
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

#[must_use]
pub(crate) fn duration_label(event: &Event) -> String {
    format_duration(event.duration_minutes())
}
