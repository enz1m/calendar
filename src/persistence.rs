//! Persisting the user's calendars, events and settings across launches.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use enzui_core::app_data::{AppDataError, AppDataResolver, AppId};
use enzui_core::atomic_state::open_state_file_nofollow;
use enzui_core::civil_date::{Date, Weekday};
use enzui_core::persist::{FileBackend, PersistError, RawByteLimit};
use enzui_core::quarantine::{
    ExpectedSourceIdentity, QuarantineOutcome, quarantine_sibling_expected,
};
use enzui_core::secure_dir::ensure_private_dir;
use enzui_core::state_authority::{PublishCustody, PublishError, StateAuthority};
use enzui_core::tempstage::verify_path_names_file;
use serde::{Deserialize, Serialize};

use crate::model::{Calendar, Event, MinuteOfDay, Recurrence, Settings};
use crate::reminders::{self, ReminderKey, ReminderLog, Snoozed};
use crate::state::AppState;

mod bounded;

pub(crate) const FORMAT_VERSION: u32 = 2;

pub(crate) const CLOSE_DEADLINE_SUMMARY: &str =
    "Closing is taking longer than expected. Retry to finish saving before quitting.";

pub(crate) const PUBLISH_NOT_DURABLE_SUMMARY: &str = "Your last change was published, but the file's durability could not be confirmed. Retry to \
     write it again.";

pub(crate) const ZONE_AUTHORITY_EXHAUSTED_SUMMARY: &str = "Time-zone updates are paused after too many changes this session; the calendar kept its last \
     known zone. Restart to resume.";

pub(crate) const ZONE_SCHEDULER_RETRY_SUMMARY: &str =
    "A time-zone update could not reschedule reminders yet. Retry to finish applying it.";

pub(crate) const APP_ID: &str = "calendar-app";
pub(crate) const STATE_FILE_NAME: &str = "state.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootRefusal {
    NoDataHome,
    Unusable,
}

impl RootRefusal {
    #[must_use]
    pub(crate) const fn user_summary(self) -> &'static str {
        match self {
            Self::NoDataHome => "This system has no private data folder for Calendar to use.",
            Self::Unusable => "Calendar's data folder can't be used safely on this system.",
        }
    }
}

pub(crate) fn resolve_state_location(resolver: &AppDataResolver) -> Result<PathBuf, RootRefusal> {
    let app = AppId::new(APP_ID).map_err(|_| RootRefusal::Unusable)?;
    match resolver.ensure_tightening(&app) {
        Ok(dir) => dir.join(STATE_FILE_NAME).map_err(|_| RootRefusal::Unusable),
        Err(AppDataError::NoDataHome { .. }) => Err(RootRefusal::NoDataHome),
        Err(AppDataError::Create { .. } | AppDataError::InvalidAppId { .. }) => {
            Err(RootRefusal::Unusable)
        }
    }
}

pub(crate) const MAX_STATE_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_STATE_BODY_BYTES_U64: u64 = 4 * 1024 * 1024;

const STATE_FILE_LIMIT: RawByteLimit =
    match RawByteLimit::for_body(STATE_DESTINATION, FORMAT_VERSION, MAX_STATE_BODY_BYTES_U64) {
        Ok(limit) => limit,
        Err(_) => panic!("calendar state limit exceeds the framework ceiling"),
    };

fn state_file_backend(path: impl Into<PathBuf>) -> FileBackend {
    FileBackend::new(path, STATE_FILE_LIMIT)
}

pub(crate) const MAX_CALENDARS: usize = 512;
pub(crate) const MAX_EVENTS: usize = 50_000;
pub(crate) const MAX_HIDDEN_CATEGORIES: usize = 128;
pub(crate) const MAX_REMINDERS_FIRED: usize = 100_000;
pub(crate) const MAX_REMINDERS_SNOOZED: usize = 1_000;
pub(crate) const MAX_EVENT_EXCEPTIONS: usize = 512;
pub(crate) const MAX_EVENT_TEXT_FIELD_BYTES: usize = 16 * 1024;
pub(crate) const MAX_EVENT_INVITEES_BYTES: usize = 8 * 1024;
pub(crate) const MAX_CALENDAR_ID_BYTES: usize = 256;
pub(crate) const MAX_CALENDAR_TEXT_BYTES: usize = 1_024;

#[path = "persistence/load.rs"]
mod load;
#[path = "persistence/persist.rs"]
mod persist;
#[path = "persistence/queue.rs"]
mod queue;
#[path = "persistence/records.rs"]
mod records;

pub(crate) use load::PersistenceRefusal;
use load::{
    ReadIdentity, admit_state_file, serialize_pretty_classified, validate_bounded, validate_ids,
};
#[cfg(test)]
pub(crate) use load::{load_from, save_to};
pub(crate) use persist::boot_at;
use queue::STATE_DESTINATION;
pub(crate) use queue::{
    FirstRun, Origin, PendingSave, Persist, PersistenceCompletion, PersistenceQueue,
    SharedAuthority, WorkerRefusal, retire_expected, write_serialized,
};
pub(crate) use records::{CalendarRec, EventRec, PersistedState, restore, snapshot};
use records::{FiredRec, SnoozeRec};

#[cfg(test)]
mod tests;
