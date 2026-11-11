//! Off-thread startup admission for calendar state and reminder activation.

use std::path::PathBuf;

use crate::persistence::{self, Origin, Persist};
use crate::state::AppState;

pub(crate) struct BootOutcome {
    pub(crate) state: AppState,
    pub(crate) persist: Persist,
    pub(crate) origin: Origin,
    pub(crate) reminder_activation: crate::reminder_activation::BootState,
    pub(crate) system_hour_cycle: Option<enzui_core::time_of_day::HourCycle>,
}

type BootResult = Result<BootOutcome, enzui_core::blocking_task::BlockingTaskError>;

// Cloned messages share one delivery; native presentation owners must never be cloned.
#[derive(Debug, Clone)]
pub(crate) struct BootDelivery(std::sync::Arc<std::sync::Mutex<Option<BootResult>>>);

impl BootDelivery {
    pub(crate) fn new(result: BootResult) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(Some(result))))
    }

    pub(crate) fn take(&self) -> Option<BootResult> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

impl std::fmt::Debug for BootOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BootOutcome")
            .field("origin", &self.origin)
            .field("events", &self.state.events.len())
            .field("calendars", &self.state.calendars.len())
            .field("persist_enabled", &self.persist.is_enabled())
            .field("reminder_activation", &self.reminder_activation)
            .field("system_hour_cycle", &self.system_hour_cycle)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BootInputs {
    pub(crate) path: PathBuf,
    pub(crate) today: enzui_core::civil_date::Date,
    pub(crate) now_minute: i32,
    pub(crate) first_run: persistence::FirstRun,
}

#[must_use]
pub(crate) fn run_boot(inputs: BootInputs) -> BootOutcome {
    let (state, persist, origin) = persistence::boot_at(
        &inputs.path,
        inputs.today,
        inputs.now_minute,
        inputs.first_run,
    );
    BootOutcome {
        state,
        persist,
        origin,
        reminder_activation: crate::reminder_activation::BootState::disabled(),
        system_hour_cycle: None,
    }
}

#[must_use]
pub(crate) fn run_production_boot(
    inputs: BootInputs,
    arguments: impl IntoIterator<Item = String>,
) -> BootOutcome {
    let mut outcome = run_boot(inputs);
    outcome.reminder_activation = crate::reminder_activation::boot(arguments);
    outcome.system_hour_cycle = Some(crate::system_hour_cycle());
    outcome
}
