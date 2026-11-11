use super::*;

#[test]
fn event_create_edit_cancel_and_confirmed_removal_survive_relaunch() {
    use enzui_core::modal::DismissCause;
    use enzui_widgets::confirm_dialog::ConfirmDialogEvent;
    let dir = TempDir::new("event-lifecycle");
    let path = dir.state_file();
    let mut app = empty_app_at(&path);
    assert!(app.state.events.is_empty());
    let id = create_event(&mut app, "Design review");
    app.step(Message::OpenEditor(id));
    app.step(Message::EditTitle("Revised review".into()));
    app.step(Message::SaveEvent);
    let event = app
        .state
        .events
        .iter_mut()
        .find(|event| event.id == id)
        .unwrap();
    event.notes = "Résumé and comparisons".into();
    event.location = "Room 3".into();
    event.url = "https://example.invalid/agenda".into();
    event.invitees = "team@example.invalid".into();
    event.recurrence = Recurrence::Weekly;
    event.recurrence_count = Some(4);
    event.exceptions = vec![today().add_days(7)];
    event.alert = Some(15);
    app.step(Message::SetFirstDayOfWeek(0));
    let expected = app.state.event(id).unwrap().clone();
    drop(app);
    let mut app = empty_app_at(&path);
    assert_eq!(app.state.event(id), Some(&expected));
    assert_eq!(app.state.settings.first_day_of_week, Weekday::Monday);
    app.step(Message::OpenEditor(id));
    app.step(Message::EditTitle("Discard this edit".into()));
    app.step(Message::CloseOverlay);
    assert_eq!(app.state.event(id), Some(&expected));
    app.step(Message::RequestDeleteEvent(id));
    app.step(Message::DeleteConfirmEvent(ConfirmDialogEvent::Resolved(
        DismissCause::CancelAction,
    )));
    assert_eq!(app.state.event(id), Some(&expected));
    app.step(Message::RequestDeleteEvent(id));
    app.step(Message::DeleteConfirmEvent(ConfirmDialogEvent::Resolved(
        DismissCause::DefaultAction,
    )));
    drop(app);
    assert!(
        app_at(&path).state.events.is_empty(),
        "an empty saved calendar must not be reseeded"
    );
}

#[test]
fn calendar_management_metadata_and_safe_removal_survive_relaunch() {
    use crate::{calendars::Action, state::SettingsInvoker};
    let dir = TempDir::new("calendar-management");
    let path = dir.state_file();
    let mut app = empty_app_at(&path);
    create_event(&mut app, "Calendar migration fixture");
    app.step(Message::OpenSettings(SettingsInvoker::InlineGear));
    app.step(Message::SelectSettingsPage("calendars".into()));
    let source = app.state.calendars[0].id.clone();
    app.step(Message::Calendar(Action::Edit(source.clone())));
    app.step(Message::Calendar(Action::Name("Discarded name".into())));
    app.step(Message::Calendar(Action::Cancel));
    assert_ne!(app.state.calendars[0].name, "Discarded name");
    app.step(Message::AddCalendar);
    let destination = app.state.calendars.last().unwrap().id.clone();
    let before = app.state.events.clone();
    app.step(Message::Calendar(Action::Edit(destination.clone())));
    app.step(Message::Calendar(Action::Name("Kept events".into())));
    app.step(Message::Calendar(Action::Color("purple".into())));
    app.step(Message::Calendar(Action::Save));
    app.step(Message::Calendar(Action::Edit(source.clone())));
    app.step(Message::Calendar(Action::RequestRemoval));
    app.step(Message::Calendar(Action::ConfirmRemoval));
    assert_eq!(
        app.state.events, before,
        "populated removal needs a destination"
    );
    assert!(
        app.state
            .calendars
            .iter()
            .any(|calendar| calendar.id == source)
    );
    app.step(Message::Calendar(Action::Destination(destination.clone())));
    app.step(Message::Calendar(Action::ConfirmRemoval));
    drop(app);
    let reopened = app_at(&path);
    let retained = reopened
        .state
        .calendars
        .iter()
        .find(|c| c.id == destination)
        .unwrap();
    assert_eq!(
        (&*retained.name, &*retained.color),
        ("Kept events", "purple")
    );
    assert!(!reopened.state.calendars.iter().any(|c| c.id == source));
    let expected: Vec<_> = before
        .into_iter()
        .map(|mut event| {
            if event.calendar_id == source {
                event.calendar_id = destination.clone();
            }
            event
        })
        .collect();
    assert_eq!(reopened.state.events, expected);
    let mut reopened = reopened;
    reopened.step(Message::OpenSettings(SettingsInvoker::InlineGear));
    reopened.step(Message::SelectSettingsPage("calendars".into()));
    reopened.step(Message::Calendar(Action::Edit(destination)));
    reopened.step(Message::Calendar(Action::RequestRemoval));
    reopened.step(Message::Calendar(Action::ConfirmRemoval));
    assert_eq!(
        reopened.state.calendars.len(),
        1,
        "the last calendar cannot be removed"
    );
}
