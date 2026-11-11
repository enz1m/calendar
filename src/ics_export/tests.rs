//! Export round trips through the Calendar reader.

use super::*;
use crate::ics::parse_ics;
use crate::model::{Event, MinuteOfDay, Recurrence, TzAnchor};
use enzui_core::civil_date::{Date, Weekday};

fn d(y: i32, m: u32, day: u32) -> Date {
    Date::new(y, m, day).expect("valid test date")
}

const STAMP: fn() -> Date = || d(2025, 6, 1);

fn round_trip(event: Event) -> Event {
    let (owner, path) = prepare_event_drag(
        &event,
        STAMP(),
        &enzui_core::cancellation::CancelToken::new(),
    )
    .expect("native event export");
    let text = std::fs::read_to_string(&path).expect("exported event bytes");
    drop(owner);
    assert!(!path.exists(), "temporary exports retire with their owner");
    let parsed = parse_ics(&text, "cal.dest").expect("re-import of exported .ics");
    assert_eq!(parsed.events.len(), 1);
    parsed.events.into_iter().next().unwrap()
}

#[test]
fn round_trip_preserves_a_full_timed_recurring_event() {
    let base = Event {
        title: "Design review".to_owned(),
        uid: Some("review@example.invalid".into()),
        location: "Zoom".to_owned(),
        notes: "Bring the layout comparisons.".to_owned(),
        url: "https://example.com/review".to_owned(),
        invitees: "team@example.com".to_owned(),
        start_minute: MinuteOfDay::hm(14, 0),
        end_minute: MinuteOfDay::hm(15, 0),
        recurrence: Recurrence::Weekly,
        recurrence_until: Some(d(2025, 12, 31)),
        recurrence_weekdays: vec![Weekday::Wednesday, Weekday::Friday],
        exceptions: vec![d(2025, 7, 3), d(2025, 8, 7)],
        alert: Some(15),
        tz_anchor: TzAnchor::Floating,
        ..Event::new(7, "cal.work", d(2025, 6, 4))
    };

    let mut missing_identity = base.clone();
    missing_identity.uid = None;
    assert!(
        prepare_event_drag(
            &missing_identity,
            STAMP(),
            &enzui_core::cancellation::CancelToken::new()
        )
        .is_err()
    );
    let cancelled = enzui_core::cancellation::CancelToken::new();
    cancelled.cancel(enzui_core::cancellation::CancelCause::Cancelled);
    assert!(prepare_event_drag(&base, STAMP(), &cancelled).is_err());
    let mut expected = base.clone();
    let got = round_trip(base);
    expected.id = got.id;
    expected.calendar_id.clone_from(&got.calendar_id);
    assert_eq!(got, expected);
}

#[test]
fn a_single_fixed_utc_event_round_trips_both_instants_and_duration_with_asymmetric_offsets() {
    // Distinct endpoint offsets distinguish DST-straddling imports from a single-offset conversion.
    let base = Event {
        title: "Cross-boundary call".to_owned(),
        uid: Some("cross-boundary@example.invalid".into()),
        start_minute: MinuteOfDay::hm(14, 0),
        end_minute: MinuteOfDay::hm(15, 30),
        tz_anchor: TzAnchor::FixedUtc {
            start_minutes: 120,
            end_minutes: 60,
        },
        ..Event::new(11, "cal.work", d(2025, 6, 4))
    };

    let got = round_trip(base);
    assert_eq!(
        (got.date, got.start_minute.get(), got.end_minute.get()),
        (d(2025, 6, 4), 720, 870)
    );
    assert_eq!(got.duration_minutes(), 150);
    assert!(got.tz_anchor.is_anchored());
}

#[test]
fn a_multi_day_all_day_event_keeps_its_inclusive_end() {
    let base = Event {
        title: "Trip to Lisbon".to_owned(),
        uid: Some("trip@example.invalid".into()),
        all_day: true,
        end_date: d(2025, 6, 8),
        ..Event::new(3, "cal.home", d(2025, 6, 2))
    };
    let text = serialize_calendar(std::slice::from_ref(&base), STAMP());
    assert!(
        text.contains("DTEND;VALUE=DATE:20250609"),
        "multi-day all-day DTEND must be inclusive-end + 1; got:\n{text}"
    );
    let got = round_trip(base);
    assert!(got.all_day);
    assert_eq!(got.date, d(2025, 6, 2));
    assert_eq!(got.end_date, d(2025, 6, 8));
}
