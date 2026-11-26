//! Durable activation for the next reminder due after Calendar exits.

use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use enzui_core::blocking_task::{BlockingTaskError, BlockingTaskOwner};
use enzui_services::durable_activation::systemd::{self, SystemdUserBackend};
use enzui_services::durable_activation::{
    ActivationInvocation, ActivationRequest, ClaimOutcome, DesktopApplicationId,
    DurableActivationScheduler, JobId, MissedActivationPolicy,
};
use iced::Task;

use crate::reminders::{
    MINUTES_PER_DAY, MISSED_WINDOW_MINUTES, ReminderKey, abs_minute, occurrence_utc_minute,
};

#[path = "reminder_zone_watch.rs"]
mod zone_watch;
use crate::state::AppState;
pub(crate) use zone_watch::run_if_requested;

const APPLICATION_ID: &str = crate::package::APP_ID;
const NEXT_REMINDER_JOB: &str = "next-reminder";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DesiredReminder {
    key: ReminderKey,
    instant: SystemTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FailureKind {
    Setup,
    Invocation,
    Claim,
    Reconcile,
    Schedule,
    Worker,
}

#[derive(Clone, Debug)]
pub(crate) struct Failure {
    kind: FailureKind,
    diagnostic: String,
}

impl Failure {
    fn new(kind: FailureKind, diagnostic: impl Into<String>) -> Self {
        Self {
            kind,
            diagnostic: diagnostic.into(),
        }
    }

    #[must_use]
    pub(crate) fn user_summary(&self) -> &'static str {
        match self.kind {
            FailureKind::Invocation | FailureKind::Claim => {
                "This reminder activation could not be verified. Calendar opened without delivering it."
            }
            FailureKind::Setup
            | FailureKind::Reconcile
            | FailureKind::Schedule
            | FailureKind::Worker => {
                "Background reminders are unavailable. Reminders will still appear while Calendar is open."
            }
        }
    }

    #[must_use]
    pub(crate) fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

type Scheduler = DurableActivationScheduler<SystemdUserBackend>;
type SharedScheduler = Arc<Mutex<Scheduler>>;

#[derive(Clone)]
pub(crate) struct BootState {
    scheduler: Option<SharedScheduler>,
    claimed: Option<ReminderKey>,
    invocation_seen: bool,
    failure: Option<Failure>,
}

impl BootState {
    #[must_use]
    pub(crate) fn disabled() -> Self {
        Self {
            scheduler: None,
            claimed: None,
            invocation_seen: false,
            failure: None,
        }
    }
}

impl std::fmt::Debug for BootState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BootState")
            .field("scheduler_available", &self.scheduler.is_some())
            .field("claimed", &self.claimed)
            .field("invocation_seen", &self.invocation_seen)
            .field("failure", &self.failure)
            .finish()
    }
}

#[must_use]
pub(crate) fn boot(arguments: impl IntoIterator<Item = String>) -> BootState {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    let invocation = ActivationInvocation::parse(arguments.iter().map(String::as_str));
    let invocation_seen = !matches!(&invocation, Ok(None));
    let mut scheduler = match native_scheduler() {
        Ok(scheduler) => scheduler,
        Err(failure) => {
            return BootState {
                scheduler: None,
                claimed: None,
                invocation_seen,
                failure: Some(failure),
            };
        }
    };
    let mut claimed = None;
    let mut failure = None;
    match invocation {
        Ok(Some(invocation)) => match scheduler.claim(&invocation, SystemTime::now()) {
            Ok(ClaimOutcome::Deliver(delivery)) => {
                claimed = parse_claimed_key(&delivery.arguments);
                if claimed.is_none() {
                    failure = Some(Failure::new(
                        FailureKind::Claim,
                        "claimed reminder payload was invalid",
                    ));
                }
            }
            Ok(ClaimOutcome::Refused(reason)) => {
                failure = Some(Failure::new(
                    FailureKind::Claim,
                    format!("durable reminder claim refused: {reason:?}"),
                ));
            }
            Err(error) => {
                failure = Some(Failure::new(FailureKind::Claim, error.to_string()));
            }
        },
        Ok(None) => {}
        Err(error) => {
            failure = Some(Failure::new(
                FailureKind::Invocation,
                format!("invalid durable reminder invocation: {error:?}"),
            ));
        }
    }
    if let Err(error) = scheduler.reconcile()
        && failure.is_none()
    {
        failure = Some(Failure::new(FailureKind::Reconcile, error.to_string()));
    }
    BootState {
        scheduler: Some(Arc::new(Mutex::new(scheduler))),
        claimed,
        invocation_seen,
        failure,
    }
}

fn native_scheduler() -> Result<Scheduler, Failure> {
    // Host systemd cannot launch an executable or read units inside the application sandbox.
    if enzui_core::portal::sandbox::is_sandboxed(
        &enzui_core::portal::sandbox::SandboxEvidence::from_host(),
    ) {
        return Err(Failure::new(
            FailureKind::Setup,
            "native systemd reminder activation is unavailable in an application sandbox",
        ));
    }
    let application = DesktopApplicationId::new(APPLICATION_ID)
        .map_err(|error| Failure::new(FailureKind::Setup, format!("{error:?}")))?;
    systemd::from_env(application)
        .map_err(|error| Failure::new(FailureKind::Setup, error.to_string()))
}

#[derive(Clone, Debug)]
pub(crate) struct SyncCompletion {
    generation: u64,
    desired: Option<DesiredReminder>,
    result: Result<(), Failure>,
}

impl SyncCompletion {
    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) const fn is_success(&self) -> bool {
        self.result.is_ok()
    }
}

pub(crate) struct Owner {
    scheduler: Option<SharedScheduler>,
    generation: u64,
    pending: Option<(u64, Option<DesiredReminder>)>,
    applied: Option<DesiredReminder>,
    applied_known: bool,
    enabled: bool,
    failure_announced: bool,
    latest_generation: Arc<AtomicU64>,
}

impl Owner {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            scheduler: None,
            generation: 0,
            pending: None,
            applied: None,
            applied_known: false,
            enabled: false,
            failure_announced: false,
            latest_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn install(
        &mut self,
        boot: BootState,
    ) -> (Option<ReminderKey>, bool, Option<Failure>) {
        self.enabled = boot.scheduler.is_some() || boot.failure.is_some();
        self.scheduler = boot.scheduler;
        self.failure_announced = boot.failure.is_some();
        (boot.claimed, boot.invocation_seen, boot.failure)
    }

    pub(crate) fn dispatch(
        &mut self,
        desired: Option<DesiredReminder>,
        worker: &Arc<BlockingTaskOwner>,
    ) -> Option<Task<SyncCompletion>> {
        let scheduler = Arc::clone(self.scheduler.as_ref()?);
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, pending)| *pending == desired)
        {
            return None;
        }
        if self.pending.is_none() && self.applied_known && self.applied == desired {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.latest_generation.store(generation, Ordering::Release);
        self.pending = Some((generation, desired.clone()));
        let submitted_desired = desired.clone();
        let latest_generation = Arc::clone(&self.latest_generation);
        let blocking = worker.submit(generation, move |context| {
            if context.is_cancelled() {
                return Err(Failure::new(
                    FailureKind::Worker,
                    "durable reminder synchronization was cancelled",
                ));
            }
            sync_scheduler_if_current(
                &scheduler,
                submitted_desired,
                &latest_generation,
                generation,
            )
        });
        Some(Task::future(async move {
            let settlement = blocking.await;
            let generation = settlement.generation();
            let result = match settlement.into_result() {
                Ok(result) => result,
                Err(error) => Err(worker_failure(error)),
            };
            SyncCompletion {
                generation,
                desired,
                result,
            }
        }))
    }

    pub(crate) fn settle(&mut self, completion: SyncCompletion) -> Option<Failure> {
        if !self
            .pending
            .as_ref()
            .is_some_and(|(generation, _)| *generation == completion.generation)
        {
            return None;
        }
        self.pending = None;
        match completion.result {
            Ok(()) => {
                self.applied = completion.desired;
                self.applied_known = true;
                self.failure_announced = false;
                None
            }
            Err(failure) if !self.failure_announced => {
                self.failure_announced = true;
                Some(failure)
            }
            Err(_) => None,
        }
    }

    pub(crate) fn pending_generation(&self) -> Option<u64> {
        self.pending.as_ref().map(|(generation, _)| *generation)
    }

    pub(crate) fn note_failure(&mut self) -> bool {
        std::mem::replace(&mut self.failure_announced, true)
    }
}

impl Default for Owner {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn stale_claim_failure() -> Failure {
    Failure::new(
        FailureKind::Claim,
        "claimed reminder no longer matches the persisted calendar",
    )
}

fn worker_failure(error: BlockingTaskError) -> Failure {
    Failure::new(FailureKind::Worker, error.to_string())
}

fn sync_scheduler_if_current(
    scheduler: &SharedScheduler,
    desired: Option<DesiredReminder>,
    latest_generation: &AtomicU64,
    generation: u64,
) -> Result<(), Failure> {
    if latest_generation.load(Ordering::Acquire) != generation {
        return Ok(());
    }
    let mut scheduler = scheduler.lock().unwrap_or_else(PoisonError::into_inner);
    let _lock = zone_watch::lock(&scheduler)
        .map_err(|error| Failure::new(FailureKind::Schedule, error.to_string()))?;
    if latest_generation.load(Ordering::Acquire) != generation {
        return Ok(());
    }
    let has_reminder = desired.is_some();
    sync_scheduler_locked(&mut scheduler, desired, None)?;
    if has_reminder {
        zone_watch::enable(scheduler.backend().unit_directory())
            .map_err(|error| Failure::new(FailureKind::Setup, error.to_string()))?;
    }
    Ok(())
}

fn sync_scheduler_locked(
    scheduler: &mut Scheduler,
    desired: Option<DesiredReminder>,
    earliest_wakeup: Option<SystemTime>,
) -> Result<(), Failure> {
    let job = JobId::new(NEXT_REMINDER_JOB)
        .map_err(|error| Failure::new(FailureKind::Schedule, format!("{error:?}")))?;
    match desired {
        Some(desired) => {
            let executable = std::env::current_exe()
                .map_err(|error| Failure::new(FailureKind::Schedule, error.to_string()))?;
            let request = ActivationRequest::new(
                job,
                executable,
                vec![
                    desired.key.event_id.to_string(),
                    desired.key.on.to_days().to_string(),
                ],
                earliest_wakeup.map_or(desired.instant, |wakeup| desired.instant.max(wakeup)),
                MissedActivationPolicy::CatchUpWithin(Duration::from_secs(
                    u64::try_from(MISSED_WINDOW_MINUTES)
                        .unwrap_or_default()
                        .saturating_mul(60),
                )),
            )
            .map_err(|error| Failure::new(FailureKind::Schedule, format!("{error:?}")))?;
            scheduler
                .schedule(request)
                .map(|_| ())
                .map_err(|error| Failure::new(FailureKind::Schedule, error.to_string()))
        }
        None => scheduler
            .cancel(&job)
            .map(|_| ())
            .map_err(|error| Failure::new(FailureKind::Schedule, error.to_string())),
    }
}

fn parse_claimed_key(arguments: &[String]) -> Option<ReminderKey> {
    let [event_id, day] = arguments else {
        return None;
    };
    Some(ReminderKey {
        event_id: event_id.parse().ok()?,
        on: enzui_core::civil_date::Date::from_days(day.parse().ok()?),
    })
}

pub(crate) fn desired(state: &AppState) -> Result<Option<DesiredReminder>, Failure> {
    let now = abs_minute(state.today, state.now_minute);
    let floor = crate::reminders::floor_of(now, &state.reminders);
    let utc_cutoff = crate::sysclock::utc_minute_now().saturating_sub(MISSED_WINDOW_MINUTES);
    let mut best: Option<DesiredReminder> = None;
    for snoozed in &state.reminders.snoozed {
        // A snooze whose occurrence no longer exists cannot own the scheduled reminder job.
        if snoozed.until_utc < utc_cutoff
            || !state.event(snoozed.key.event_id).is_some_and(|event| {
                event.alert.is_some() && state.is_visible(event) && event.occurs_on(snoozed.key.on)
            })
        {
            continue;
        }
        let instant = utc_minute_to_system_time(snoozed.until_utc)
            .ok_or_else(unrepresentable_reminder_time)?;
        keep_earliest(
            &mut best,
            DesiredReminder {
                key: snoozed.key,
                instant,
            },
        );
    }
    for event in state.events.iter().filter(|event| state.is_visible(event)) {
        let Some(lead) = event.alert.map(i64::from) else {
            continue;
        };
        let Ok(day) = i32::try_from((floor + lead).div_euclid(MINUTES_PER_DAY)) else {
            continue;
        };
        let mut occurrence = event.next_occurrence(enzui_core::civil_date::Date::from_days(day));
        while let Some(on) = occurrence {
            let key = ReminderKey {
                event_id: event.id,
                on,
            };
            let wall = abs_minute(on, event.start_minute.get()) - lead;
            if wall > floor && !state.reminders.has_fired(key) {
                let utc =
                    occurrence_utc_minute(event, on).ok_or_else(unrepresentable_reminder_time)?;
                if utc >= utc_cutoff {
                    let instant =
                        utc_minute_to_system_time(utc).ok_or_else(unrepresentable_reminder_time)?;
                    keep_earliest(&mut best, DesiredReminder { key, instant });
                    break;
                }
            }
            occurrence = event
                .next_occurrence(on.add_days(1))
                .filter(|next| *next > on);
        }
    }
    Ok(best)
}

fn unrepresentable_reminder_time() -> Failure {
    Failure::new(
        FailureKind::Schedule,
        "reminder wall-clock time is not representable in the local timezone",
    )
}

fn keep_earliest(best: &mut Option<DesiredReminder>, candidate: DesiredReminder) {
    if best
        .as_ref()
        .is_none_or(|current| (candidate.instant, candidate.key) < (current.instant, current.key))
    {
        *best = Some(candidate);
    }
}

fn utc_minute_to_system_time(minute: i64) -> Option<SystemTime> {
    let seconds = u64::try_from(minute).ok()?.checked_mul(60)?;
    UNIX_EPOCH.checked_add(Duration::from_secs(seconds))
}

#[cfg(test)]
#[cfg(target_os = "linux")]
#[path = "reminder_activation_route_tests.rs"]
mod route_tests;
