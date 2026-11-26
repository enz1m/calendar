use super::*;
use crate::{
    App,
    message::Message,
    model::{Event, MinuteOfDay},
    state::{AppState, Overlay},
};
use enzui_core::civil_date::Date;

fn app() -> App {
    let date = Date::new(2026, 5, 13).unwrap();
    let mut app = App::test_new().0;
    app.state = AppState::empty(date, 580);
    app.state.events.push(Event {
        title: "Standup".into(),
        start_minute: MinuteOfDay::hm(10, 0),
        alert: Some(15),
        recurrence: crate::model::Recurrence::Weekly,
        ..Event::new(1, "cal.home", date)
    });
    app.state.reminders.last_checked = Some(abs_minute(date, 580));
    app
}

#[test]
fn missed_reminders_catch_up_once_despite_backward_ticks() {
    let mut app = app();
    let date = app.state.today;
    app.step(Message::MinuteTick(date, 605));
    assert!(matches!(
        app.state.overlay,
        Overlay::ReminderAlert { event_id: 1, .. }
    ));
    app.step(Message::DismissReminder);
    app.step(Message::MinuteTick(date, 580));
    app.step(Message::MinuteTick(date, 606));
    assert_eq!(app.state.overlay, Overlay::None);
}

#[test]
fn deleting_a_snoozed_occurrence_cancels_its_reminder() {
    let mut app = app();
    let date = app.state.today;
    app.step(Message::MinuteTick(date, 585));
    assert!(matches!(app.state.overlay, Overlay::ReminderAlert { .. }));
    app.step(Message::SnoozeReminder);
    assert_eq!(app.state.reminders.snoozed.len(), 1);
    app.step(Message::DeleteOccurrence { id: 1, on: date });
    app.step(Message::RecurrenceChoiceThis);
    app.step(Message::MinuteTick(date, 590));
    assert_eq!(app.state.overlay, Overlay::None);
    assert!(app.state.event(1).unwrap().occurs_on(date.add_days(7)));
}

#[test]
fn utc_snoozes_follow_elapsed_minutes_through_folds_and_gaps() {
    let cases = [
        (Date::new(2026, 11, 1).unwrap(), 6 * 60 + 25, 85, 90),
        (Date::new(2026, 11, 1).unwrap(), 5 * 60 + 58, 118, 63),
        (Date::new(2026, 3, 8).unwrap(), 6 * 60 + 58, 118, 183),
    ];
    for (date, start_utc, start_local, due_local) in cases {
        let key = ReminderKey {
            event_id: 1,
            on: date,
        };
        let mut state = AppState::empty(date, start_local);
        state.events.push(Event {
            alert: Some(0),
            ..Event::new(key.event_id, "cal.home", date)
        });
        state.reminders.mark_fired(key);
        let start = abs_minute(date, start_utc);
        crate::sysclock::TEST_UTC_MINUTE.set(Some(start));
        snooze(&mut state, key);
        assert_eq!(state.reminders.snoozed_until(key), Some(start + 5));

        crate::sysclock::TEST_UTC_MINUTE.set(Some(start + 4));
        assert!(!check_alerts(&mut state));
        state.now_minute = due_local;
        crate::sysclock::TEST_UTC_MINUTE.set(Some(start + 5));
        assert!(matches!(apply_claimed(&mut state, key), Claim::Applied));
        state.overlay = Overlay::None;
        state.reminders.snooze(key, start + 5);
        assert!(
            check_alerts(&mut state),
            "due snooze at {date:?} {due_local}"
        );
        assert!(state.reminders.snoozed.is_empty());
    }
}
