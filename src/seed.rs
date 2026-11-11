//! Demo calendars and events with dates relative to the supplied day.

use enzui_core::civil_date::Date;

use crate::model::{Calendar, Event, MinuteOfDay, Recurrence};

#[must_use]
pub(crate) fn seed(today: Date) -> (Vec<Calendar>, Vec<Event>, u64) {
    let calendars = vec![
        cal("cal.home", "Home", "iCloud", "blue", true),
        cal("cal.work", "Work", "iCloud", "magenta", true),
        cal(
            "cal.reminders",
            "Scheduled Reminders",
            "Other",
            "teal",
            true,
        ),
        cal("cal.birthdays", "Birthdays", "Other", "gray", true),
        cal("cal.holidays", "US Holidays", "Other", "purple", true),
        cal("cal.siri", "Siri Suggestions", "Other", "yellow", false),
    ];

    let mut id = 1u64;
    let mut next = || {
        let v = id;
        id += 1;
        v
    };

    let mut events = vec![
        timed(
            next(),
            "cal.home",
            "Yoga",
            "",
            today.add_days(-2),
            7,
            0,
            8,
            0,
        ),
        timed(
            next(),
            "cal.home",
            "Coffee with Alex",
            "Blue Bottle",
            today,
            9,
            30,
            10,
            30,
        ),
        timed(next(), "cal.work", "1:1 with Sam", "", today, 11, 0, 12, 0),
        timed(
            next(),
            "cal.reminders",
            "Prescription refill",
            "",
            today,
            11,
            15,
            11,
            45,
        ),
        timed(
            next(),
            "cal.home",
            "Grocery pickup",
            "Whole Foods",
            today,
            11,
            30,
            12,
            15,
        ),
        timed(
            next(),
            "cal.work",
            "Work on screenshots",
            "",
            today,
            14,
            0,
            17,
            15,
        ),
        timed(
            next(),
            "cal.work",
            "Standup",
            "",
            today.add_days(1),
            10,
            0,
            10,
            30,
        ),
        timed(
            next(),
            "cal.work",
            "Design review",
            "Zoom",
            today.add_days(3),
            14,
            0,
            15,
            0,
        ),
        all_day(next(), "cal.holidays", "US Holiday", today.add_days(3)),
        all_day(
            next(),
            "cal.birthdays",
            "Jamie's birthday",
            today.add_days(1),
        ),
        Event {
            end_date: today.add_days(7),
            ..all_day(next(), "cal.home", "Trip to Lisbon", today.add_days(1))
        },
    ];

    if let Some(e) = events.iter_mut().find(|e| e.title == "Design review") {
        e.recurrence = Recurrence::Weekly;
        e.alert = Some(15);
        e.invitees = "team@example.com".into();
        e.notes = "Bring the layout comparisons.".into();
        e.url = "https://example.com/review".into();
    }
    if let Some(e) = events.iter_mut().find(|e| e.title == "Yoga") {
        e.color_category = Some("green".into());
    }
    if let Some(e) = events.iter_mut().find(|e| e.title == "Grocery pickup") {
        e.color_category = Some("orange".into());
    }

    (calendars, events, id)
}

fn cal(id: &str, name: &str, section: &str, color: &str, visible: bool) -> Calendar {
    Calendar {
        id: id.into(),
        name: name.into(),
        section: section.into(),
        color: color.into(),
        visible,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Demo events specify independent start and end clock components"
)]
fn timed(
    id: u64,
    cal: &str,
    title: &str,
    location: &str,
    date: Date,
    sh: i32,
    sm: i32,
    eh: i32,
    em: i32,
) -> Event {
    Event {
        title: title.into(),
        location: location.into(),
        start_minute: MinuteOfDay::hm(sh, sm),
        end_minute: MinuteOfDay::hm(eh, em),
        ..Event::new(id, cal, date)
    }
}

fn all_day(id: u64, cal: &str, title: &str, date: Date) -> Event {
    Event {
        title: title.into(),
        all_day: true,
        ..Event::new(id, cal, date)
    }
}
