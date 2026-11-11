//! Export settlement preserves the current destination across stale replies.

use super::*;
use crate::ics_export::{ExportDisclosure, ExportResult};

fn test_app() -> App {
    let mut app = App::test_new().0;
    app.state = AppState::empty(Date::new(2026, 5, 13).unwrap(), 680);
    app
}

fn intent(generation: u64, destination: &str) -> PendingExport {
    PendingExport {
        events: Vec::new(),
        stamp: Date::new(2026, 5, 13).unwrap(),
        destination: Some(std::path::PathBuf::from(destination)),
        generation,
        phase: ExportPhase::InFlight,
        announced: false,
    }
}

#[test]
fn a_stale_first_export_completion_cannot_settle_a_second_destination() {
    let mut app = test_app();
    app.pending_export = Some(intent(9, "/second/calendar.ics"));

    let _ = app.update(Message::ExportSettled {
        generation: 4,
        result: ExportResult::Wrote,
        disclosure: ExportDisclosure {
            path_display: "/first/calendar.ics".to_owned(),
            skipped: Vec::new(),
            omissions: Vec::new(),
        },
    });

    let retained = app
        .pending_export
        .as_ref()
        .expect("a stale completion must not consume the live export intent");
    assert_eq!(
        retained.generation, 9,
        "the live generation must be untouched by a stale completion"
    );
    assert_eq!(
        retained.destination.as_deref(),
        Some(std::path::Path::new("/second/calendar.ics")),
        "the second destination must survive the stale first completion"
    );

    let _ = app.update(Message::ExportSettled {
        generation: 9,
        result: ExportResult::Wrote,
        disclosure: ExportDisclosure::none(),
    });
    assert!(
        app.pending_export.is_none(),
        "the live generation's completion clears the intent"
    );
}
