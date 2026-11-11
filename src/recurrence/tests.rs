use super::*;
use crate::model::{Event, Recurrence};
use enzui_core::civil_date::Date;

fn d(year: i32, month: u32, day: u32) -> Date {
    Date::new(year, month, day).unwrap()
}

#[test]
fn calendar_recurrence_skips_unrepresentable_dates() {
    for (base, frequency, hit, miss) in [
        (
            d(2026, 1, 31),
            Recurrence::Monthly,
            d(2026, 3, 31),
            d(2026, 2, 28),
        ),
        (
            d(2024, 2, 29),
            Recurrence::Yearly,
            d(2028, 2, 29),
            d(2025, 3, 1),
        ),
    ] {
        let event = Event {
            recurrence: frequency,
            ..Event::new(1, "cal.home", base)
        };
        assert!(event.occurs_on(hit));
        assert!(!event.occurs_on(miss));
    }
}

#[test]
fn expansion_obeys_interval_count_window_and_exceptions() {
    let event = Event {
        recurrence: Recurrence::Daily,
        recurrence_interval: 2,
        recurrence_count: Some(4),
        exceptions: vec![d(2026, 1, 3)],
        ..Event::new(1, "cal.home", d(2026, 1, 1))
    };
    assert_eq!(
        occurrences_in_range(&event, d(2026, 1, 2), d(2026, 2, 1)),
        vec![d(2026, 1, 5), d(2026, 1, 7)]
    );
    assert_eq!(next_on_or_after(&event, d(2026, 1, 2)), Some(d(2026, 1, 5)));
    assert_eq!(next_on_or_after(&event, d(2026, 1, 8)), None);

    let mut state = crate::state::AppState::empty(d(2026, 1, 1), 600);
    state.events.push(Event {
        end_date: d(2026, 1, 2),
        ..event
    });
    for (day, expected_starts) in [(4, vec![]), (6, vec![5]), (8, vec![7]), (9, vec![])] {
        let date = d(2026, 1, day);
        assert_eq!(state.has_events_on(date), !expected_starts.is_empty());
        assert_eq!(
            state
                .events_on(date)
                .iter()
                .map(|event| event.date.day())
                .collect::<Vec<_>>(),
            expected_starts
        );
    }
    state.events[0].recurrence_until = Some(d(2026, 1, 5));
    assert!(!state.has_events_on(d(2026, 1, 7)));
    state.toggle_calendar("cal.home");
    assert!(!state.has_events_on(d(2026, 1, 6)));
}
