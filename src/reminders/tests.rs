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
