use super::*;
use crate::model::{Event, Recurrence};

fn weekly() -> (App, Date) {
    let date = Date::new(2026, 5, 13).unwrap();
    let mut app = App::test_new().0;
    app.state = AppState::empty(date, 680);
    app.state.events.push(Event {
        title: "Standup".into(),
        recurrence: Recurrence::Weekly,
        ..Event::new(1, "cal.home", date)
    });
    app.state.next_id = 2;
    (app, date)
}

#[test]
fn recurring_deletion_respects_the_chosen_scope() {
    for (choice, expected) in [
        (Message::RecurrenceChoiceThis, [true, false, true]),
        (Message::RecurrenceChoiceFollowing, [true, false, false]),
        (Message::RecurrenceChoiceAll, [false, false, false]),
    ] {
        let (mut app, date) = weekly();
        app.step(Message::DeleteOccurrence {
            id: 1,
            on: date.add_days(7),
        });
        assert!(matches!(app.state.overlay, Overlay::RecurrencePrompt(_)));
        assert!(app.state.event(1).is_some());
        app.step(choice);
        assert_eq!(
            [0, 7, 14].map(|days| app
                .state
                .events
                .iter()
                .any(|event| event.occurs_on(date.add_days(days)))),
            expected
        );
    }
}

#[test]
fn recurring_edits_preserve_earlier_occurrences() {
    for (choice, repeats) in [
        (Message::RecurrenceChoiceThis, false),
        (Message::RecurrenceChoiceFollowing, true),
    ] {
        let (mut app, date) = weekly();
        let on = date.add_days(7);
        app.step(Message::OpenOccurrenceEditor { id: 1, on });
        app.step(Message::EditTitle("Revised standup".into()));
        app.step(Message::SaveEvent);
        assert_eq!(app.state.event(1).unwrap().title, "Standup");
        app.step(choice);
        let original = app.state.event(1).unwrap();
        assert!(original.occurs_on(date));
        assert!(!original.occurs_on(on));
        let edited = app.state.events.iter().find(|event| event.id != 1).unwrap();
        assert_eq!(edited.title, "Revised standup");
        assert!(edited.occurs_on(on));
        assert_eq!(edited.occurs_on(on.add_days(7)), repeats);
    }
}

#[test]
fn paging_retires_occurrence_actions_without_reusing_their_identity() {
    let (mut app, date) = weekly();
    let first = crate::view::schedule::blocks_for_dates(&app.state, &[date])[0].id;
    for page in 1..=300 {
        let on = date.add_days(page * 7);
        let blocks = crate::view::schedule::blocks_for_dates(&app.state, &[on]);
        assert_eq!(
            app.state.schedule_occurrence(blocks[0].id.get()),
            Some((1, on))
        );
        assert_eq!(app.state.schedule_occurrence(first.get()), None);
    }
    let returned = crate::view::schedule::blocks_for_dates(&app.state, &[date])[0].id;
    assert_ne!(returned, first);
    drop(app.on_schedule(SchedEvent::BlockActivated {
        id: first,
        at: None,
    }));
    assert!(matches!(app.state.overlay, Overlay::None));
    drop(app.on_schedule(SchedEvent::BlockActivated {
        id: returned,
        at: None,
    }));
    assert!(
        matches!(app.state.overlay, Overlay::Details { id: 1, on: Some(on), .. } if on == date)
    );

    app.state.events[0].all_day = true;
    app.state.events[0].end_date = date.add_days(2);
    app.state.focus = date;
    drop(crate::view::month::month(&app.state));
    assert_eq!(
        app.state.schedule_occurrence(returned.get()),
        Some((1, date))
    );
    app.state.focus = date.add_months(1);
    drop(crate::view::month::month(&app.state));
    assert_eq!(app.state.schedule_occurrence(returned.get()), None);
}
