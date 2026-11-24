//! Import selection and cancellation over a scripted native chooser.

use super::*;
use crate::picker::tests::ScriptedBackend;
use crate::support::drain_task;
use enzui_widgets::file_chooser::PortalResponse;

fn test_app() -> App {
    let mut app = App::test_new().0;
    app.state = AppState::empty(Date::new(2026, 5, 13).unwrap(), 680);
    app
}

fn pump_chooser(app: &mut App) -> Task<Message> {
    let wake = app.update(Message::PortalTick);
    let mut outcomes = drain_task(wake);
    assert_eq!(
        outcomes.len(),
        1,
        "native chooser emits one tagged completion"
    );
    app.update(outcomes.pop().unwrap())
}

const ICS: &str = "BEGIN:VCALENDAR\r\n\
    VERSION:2.0\r\n\
    BEGIN:VEVENT\r\n\
    DTSTART:20260610T090000\r\n\
    DTEND:20260610T100000\r\n\
    UID:chosen@example.invalid\r\n\
    SUMMARY:Chosen-file event\r\n\
    END:VEVENT\r\n\
    END:VCALENDAR\r\n";

fn temp_ics(content: &str, tag: &str) -> (std::path::PathBuf, String) {
    let mut path = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    path.push(format!("calendar-import-{tag}-{nanos}.ics"));
    std::fs::write(&path, content).expect("write temp ics");
    let uri = format!("file://{}", path.to_string_lossy());
    (path, uri)
}

#[test]
fn import_cancellation_stale_reply_destination_and_uid_deduplication() {
    let (path, uri) = temp_ics(ICS, "cal");
    let mut app = test_app();
    app.picker = picker::Picker::with_backend(Box::new(ScriptedBackend::replying(
        PortalResponse::Success,
        vec![uri],
    )));
    // A second destination distinguishes the chosen calendar from a first-visible fallback.
    let chosen = app.state.add_calendar();

    let _ = app.update(Message::ImportEvents);
    drop(pump_chooser(&mut app));
    drive_bounded_worker(&mut app, &path);
    let parsed = match &app.state.overlay {
        Overlay::ImportDestination(parsed) => parsed.clone(),
        other => panic!("expected destination choice, got {other:?}"),
    };
    app.step(Message::CloseOverlay);
    assert!(app.state.events.is_empty());
    app.state.overlay = Overlay::ImportDestination(parsed.clone());
    app.step(Message::ChooseImportDestination(chosen.clone()));
    app.state.overlay = Overlay::ImportDestination(parsed);
    app.step(Message::ChooseImportDestination(chosen.clone()));
    assert_eq!(
        app.state.events.len(),
        1,
        "UID re-import must not duplicate an event"
    );

    let ev = app
        .state
        .events
        .iter()
        .find(|e| e.title == "Chosen-file event")
        .expect("imported");
    assert_eq!(
        ev.calendar_id, chosen,
        "the event lands in the chosen calendar"
    );
    let _ = std::fs::remove_file(path);
}

fn drive_bounded_worker(app: &mut App, path: &std::path::Path) {
    let generation = match app.state.overlay {
        Overlay::Importing { generation } => generation,
        ref other => panic!("expected active import, got {other:?}"),
    };
    app.step(Message::CloseOverlay);
    app.step(Message::ImportFinished {
        generation,
        outcome: crate::ics_import::run_import(path.to_path_buf()),
    });
    assert!(app.state.events.is_empty());
    assert_eq!(app.state.overlay, Overlay::None);
    drop(app.begin_bounded_import(path.to_path_buf()));
    let generation = app.pending_import_generation;
    app.step(Message::ImportFinished {
        generation,
        outcome: crate::ics_import::run_import(path.to_path_buf()),
    });
}

#[test]
fn unavailable_portal_uses_native_fallback_and_cancel_can_reopen_without_importing() {
    use enzui_widgets::file_chooser::FallbackEvent;
    let mut app = test_app();
    app.picker = picker::Picker::with_backend(Box::new(ScriptedBackend::unavailable()));
    app.step(Message::ImportEvents);
    assert!(
        app.picker.is_pending(),
        "unavailable portal must leave an interactive chooser"
    );
    let old_generation = app.picker.generation();
    let cancelled = app.update(Message::ChooserEvent(old_generation, FallbackEvent::Cancel));
    assert!(!app.picker.is_pending());
    let late = drain_task(cancelled);
    app.step(Message::ImportEvents);
    assert!(app.picker.is_pending());
    for message in late {
        app.step(message);
    }
    assert!(
        app.picker.is_pending(),
        "an old cancellation cannot retire the new chooser"
    );
    app.step(Message::ChooserEvent(old_generation, FallbackEvent::Accept));
    assert!(
        app.picker.is_pending(),
        "an old chooser press cannot accept the new request"
    );
    let generation = app.picker.generation();
    app.step(Message::ChooserEvent(generation, FallbackEvent::Cancel));
    assert!(!app.picker.is_pending());
    assert!(app.state.events.is_empty());
    assert_eq!(app.state.overlay, Overlay::None);
}

const GOOGLE_SERIES_WITH_OVERRIDES: &str = "BEGIN:VCALENDAR\r\n\
    PRODID:-//Google Inc//Google Calendar 70.9054//EN\r\n\
    VERSION:2.0\r\n\
    CALSCALE:GREGORIAN\r\n\
    METHOD:PUBLISH\r\n\
    X-WR-CALNAME:Home\r\n\
    BEGIN:VEVENT\r\n\
    DTSTART:20261203T100000\r\n\
    DTEND:20261203T103000\r\n\
    DTSTAMP:20261124T120000Z\r\n\
    UID:standup@google.com\r\n\
    RECURRENCE-ID:20261202T090000\r\n\
    SEQUENCE:1\r\n\
    STATUS:CONFIRMED\r\n\
    SUMMARY:Standup (moved)\r\n\
    END:VEVENT\r\n\
    BEGIN:VEVENT\r\n\
    DTSTART:20261201T090000\r\n\
    DTEND:20261201T093000\r\n\
    RRULE:FREQ=DAILY;COUNT=5\r\n\
    DTSTAMP:20261124T120000Z\r\n\
    UID:standup@google.com\r\n\
    SEQUENCE:0\r\n\
    STATUS:CONFIRMED\r\n\
    SUMMARY:Standup\r\n\
    END:VEVENT\r\n\
    BEGIN:VEVENT\r\n\
    DTSTART;VALUE=DATE:20261201\r\n\
    DTEND;VALUE=DATE:20261202\r\n\
    RRULE:FREQ=WEEKLY;BYDAY=TU\r\n\
    DTSTAMP:20261124T120000Z\r\n\
    UID:bins@google.com\r\n\
    STATUS:CONFIRMED\r\n\
    SUMMARY:Bins out\r\n\
    END:VEVENT\r\n\
    BEGIN:VEVENT\r\n\
    DTSTART;VALUE=DATE:20261208\r\n\
    DTEND;VALUE=DATE:20261209\r\n\
    DTSTAMP:20261124T120000Z\r\n\
    UID:bins@google.com\r\n\
    RECURRENCE-ID;VALUE=DATE:20261208\r\n\
    STATUS:CANCELLED\r\n\
    SUMMARY:Bins out\r\n\
    END:VEVENT\r\n\
    BEGIN:VEVENT\r\n\
    DTSTART:20261201T120000\r\n\
    DTEND:20261201T130000\r\n\
    DTSTAMP:20261124T120000Z\r\n\
    UID:lunch@google.com\r\n\
    STATUS:CANCELLED\r\n\
    SUMMARY:Lunch\r\n\
    END:VEVENT\r\n\
    END:VCALENDAR\r\n";

#[test]
fn recurrence_overrides_replace_their_series_occurrence_across_import_and_export() {
    let mut state = AppState::empty(Date::new(2026, 11, 24).unwrap(), 680);
    let parsed = crate::ics::parse_ics(GOOGLE_SERIES_WITH_OVERRIDES, "cal.home").unwrap();
    let report = state.commit_import(parsed, "cal.home");
    assert_eq!(report.added, 3);

    let titles_on = |state: &AppState, day: u32| {
        state
            .events_on(Date::new(2026, 12, day).unwrap())
            .into_iter()
            .map(|event| event.title)
            .collect::<Vec<_>>()
    };
    assert_eq!(titles_on(&state, 1), ["Bins out", "Standup"]);
    assert_eq!(titles_on(&state, 2), Vec::<String>::new());
    assert_eq!(titles_on(&state, 3), ["Standup", "Standup (moved)"]);
    assert_eq!(titles_on(&state, 8), Vec::<String>::new());
    assert_eq!(titles_on(&state, 15), ["Bins out"]);

    let exported =
        crate::ics_export::serialize_calendar(&state.events, Date::new(2026, 11, 24).unwrap());
    assert!(exported.contains("RECURRENCE-ID:20261202T090000\r\n"));
    assert!(!exported.contains("EXDATE:20261202T090000"));
    assert!(exported.contains("EXDATE;VALUE=DATE:20261208\r\n"));

    let mut reimported = AppState::empty(Date::new(2026, 11, 24).unwrap(), 680);
    reimported.commit_import(
        crate::ics::parse_ics(&exported, "cal.home").unwrap(),
        "cal.home",
    );
    for day in [1, 2, 3, 8, 15] {
        assert_eq!(titles_on(&reimported, day), titles_on(&state, day));
    }
}
