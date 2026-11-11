//! State format, file admission, persistence, and relaunch checks.

use crate::support::TempDir;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use enzui_core::civil_date::{Date, Weekday};

use super::*;
use crate::message::Message;
use crate::model::Recurrence;

#[path = "tests/admission_tests.rs"]
mod admission_tests;
#[path = "tests/quarantine_tests.rs"]
mod quarantine_tests;
#[path = "tests/roundtrip_tests.rs"]
mod roundtrip_tests;

fn owner_only_file(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .expect("set owner-only mode");
}

fn today() -> Date {
    Date::new(2026, 5, 13).expect("valid fixture date")
}

const NOW: i32 = 11 * 60 + 20;

fn app_at(path: &Path) -> crate::App {
    crate::App::start(boot_at(path, today(), NOW, FirstRun::Demo)).0
}

fn empty_app_at(path: &Path) -> crate::App {
    crate::App::start(boot_at(path, today(), NOW, FirstRun::Empty)).0
}

fn create_event(app: &mut crate::App, title: &str) -> u64 {
    app.step(Message::NewEvent);
    app.step(Message::EditTitle(title.to_owned()));
    app.step(Message::SaveEvent);
    app.state
        .events
        .iter()
        .find(|e| e.title == title)
        .unwrap_or_else(|| panic!("the app must hold the event it just saved: {title}"))
        .id
}
