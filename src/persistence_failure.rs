//! Failed-save recovery and vetoable close.

use crate::support::TempDir;
use std::path::Path;

use enzui_core::civil_date::Date;

use crate::message::Message;
use crate::persistence::{self, PersistenceQueue};
use crate::state::PersistNotice;

fn today() -> Date {
    Date::new(2026, 8, 19).expect("valid fixture date")
}

fn app_at(path: &Path, queue: PersistenceQueue) -> crate::App {
    let doc = crate::close_tests::document(2);
    persistence::save_to(path, &doc).expect("seed state file");
    crate::App::start_with_queue(
        persistence::boot_at(path, today(), 10 * 60, persistence::FirstRun::Empty),
        queue,
    )
    .0
}

#[test]
fn failed_event_save_vetoes_close_until_retry_publishes_the_event() {
    let dir = TempDir::new("event-retry");
    let path = dir.state_file();
    let queue = PersistenceQueue::default();
    let mut app = app_at(&path, queue.clone());
    let requested = iced::window::Id::unique();
    app.shell_window.bind(requested);
    let old_bytes = std::fs::read(&path).expect("seed bytes");
    let old_saved = app.persist.last_saved().clone();
    queue.inject_write_error(std::io::ErrorKind::PermissionDenied, "permission denied");

    app.step(Message::NewEvent);
    app.step(Message::EditTitle("Newest unsaved event".into()));
    app.step(Message::SaveEvent);

    assert_eq!(std::fs::read(&path).expect("old state survives"), old_bytes);
    assert_eq!(
        app.persist.last_saved(),
        &old_saved,
        "failed I/O cannot advance the durable baseline"
    );
    let pending = app
        .pending_save
        .as_ref()
        .expect("newest snapshot remains debt");
    assert!(
        pending
            .document()
            .events
            .iter()
            .any(|event| event.title == "Newest unsaved event")
    );
    let failed_close = app.update(Message::Window(
        enzui_widgets::title_bar_gesture::WindowCommand::Close,
    ));
    assert_eq!(
        crate::close_tests::CloseDriver::new(failed_close)
            .pump(&mut app, std::time::Duration::from_secs(2)),
        0,
        "failed final save must emit no window close"
    );
    assert!(app.state.close_state.is_vetoed());
    assert_eq!(app.state.close_state.window(), Some(requested));
    assert!(app.pending_save.is_some());
    assert!(matches!(
        app.state.persist_notice.as_ref(),
        Some(PersistNotice::PersistenceFailed(_))
    ));

    queue.clear_injected_write_error();
    let retry = app.update(Message::RetryPersistence);
    assert_eq!(
        crate::close_tests::CloseDriver::new(retry)
            .pump(&mut app, std::time::Duration::from_secs(2)),
        1,
        "successful Retry must emit one Close for the bound window"
    );
    assert!(app.state.close_state.is_idle());
    assert!(app.pending_save.is_none());
    assert!(
        persistence::load_from(&path)
            .unwrap()
            .events
            .iter()
            .any(|event| event.title == "Newest unsaved event")
    );
}

#[test]
fn closing_the_failed_save_notice_hides_it_until_the_failure_changes() {
    let dir = TempDir::new("notice-hide");
    let path = dir.state_file();
    let queue = PersistenceQueue::default();
    let mut app = app_at(&path, queue.clone());
    queue.inject_write_error(std::io::ErrorKind::PermissionDenied, "permission denied");

    app.step(Message::NewEvent);
    app.step(Message::EditTitle("Unsaved event".into()));
    app.step(Message::SaveEvent);
    assert!(matches!(
        app.state.persist_notice.as_ref(),
        Some(PersistNotice::PersistenceFailed(_))
    ));

    // Closing the notice only hides it: the failure and its retained debt stay.
    app.step(Message::DismissPersistNotice);
    assert!(app.state.persist_notice_hidden);
    assert!(matches!(
        app.state.persist_notice.as_ref(),
        Some(PersistNotice::PersistenceFailed(_))
    ));
    assert!(app.pending_save.is_some());

    queue.clear_injected_write_error();
    app.step(Message::RetryPersistence);
    assert!(app.state.persist_notice.is_none());
    assert!(
        !app.state.persist_notice_hidden,
        "a settled failure forgets that its notice was closed"
    );
}
