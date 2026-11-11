//! Boot, export/import generations, subscriptions, and overrides.

use super::*;

pub(super) fn export_write_failure_text(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::PermissionDenied => {
            "The calendar could not be exported because writing there was denied."
        }
        std::io::ErrorKind::NotFound => {
            "The chosen location no longer exists — nothing was exported."
        }
        _ => "The calendar could not be written to the chosen location.",
    }
}

pub(super) fn export_omission_summary(
    omissions: &[crate::ics_export::ExportOmission],
) -> Option<String> {
    if omissions.is_empty() {
        return None;
    }
    let mut labels: Vec<&'static str> = Vec::new();
    for omission in omissions {
        let label = omission.field.label();
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    let events = omissions
        .iter()
        .map(|o| o.id)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    Some(crate::l10n::format(
        "calendar-export-omissions",
        &[
            (
                "fields",
                labels
                    .iter()
                    .map(crate::l10n::tr)
                    .collect::<Vec<_>>()
                    .join(" / ")
                    .into(),
            ),
            ("count", (events as i64).into()),
        ],
    ))
}

// Generation identities never wrap or reuse zero.
pub(super) fn checked_next_generation(current: u64) -> Option<u64> {
    current.checked_add(1)
}

pub(super) fn allocate_generation(counter: &std::sync::atomic::AtomicU64) -> Option<u64> {
    use std::sync::atomic::Ordering;
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let next = checked_next_generation(current)?;
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return Some(next),
            Err(observed) => current = observed,
        }
    }
}

pub(super) fn next_export_generation() -> Option<u64> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    allocate_generation(&COUNTER)
}

pub(super) fn next_import_generation() -> Option<u64> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    allocate_generation(&COUNTER)
}

pub(super) fn import_refusal(
    error: enzui_core::blocking_task::BlockingTaskError,
) -> crate::ics_import::ImportWorkerRefusal {
    use crate::ics_import::ImportWorkerRefusal;
    use enzui_core::blocking_task::BlockingTaskError;
    match error {
        BlockingTaskError::Busy => ImportWorkerRefusal::Busy,
        BlockingTaskError::Cancelled => ImportWorkerRefusal::Cancelled,
        BlockingTaskError::Panicked => ImportWorkerRefusal::Panicked,
        _ => ImportWorkerRefusal::Shutdown,
    }
}

pub(super) fn export_refusal_result(
    error: enzui_core::blocking_task::BlockingTaskError,
) -> crate::ics_export::ExportResult {
    use enzui_core::blocking_task::BlockingTaskError;
    match error {
        BlockingTaskError::Busy => crate::ics_export::ExportResult::Busy,
        _ => crate::ics_export::ExportResult::OwnerUnavailable,
    }
}

pub(super) fn run_export(
    events: &[model::Event],
    stamp: Date,
    path: &std::path::Path,
) -> (
    crate::ics_export::ExportResult,
    crate::ics_export::ExportDisclosure,
) {
    use crate::ics_export::{ExportDisclosure, ExportResult};
    let (document, skipped, omissions) =
        crate::ics_export::serialize_calendar_checked(events, stamp);
    let disclosure = ExportDisclosure {
        path_display: path.to_string_lossy().into_owned(),
        skipped,
        omissions,
    };
    if disclosure.skipped.len() == events.len() {
        return (ExportResult::NothingWritten, disclosure);
    }
    match persistence::write_serialized(path, &document) {
        Ok(()) => (ExportResult::Wrote, disclosure),
        Err(error) => (ExportResult::WriteFailed(error.kind()), disclosure),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExportPhase {
    AwaitingDestination,
    InFlight,
    Waiting,
    Exhausted,
}

pub(super) struct PendingExport {
    pub(super) events: Vec<model::Event>,
    pub(super) stamp: Date,
    pub(super) destination: Option<std::path::PathBuf>,
    pub(super) generation: u64,
    pub(super) phase: ExportPhase,
    pub(super) announced: bool,
}

pub(super) fn today_and_now() -> (Date, i32) {
    if let Ok(s) = std::env::var("ENZUI_CALENDAR_TODAY") {
        let parts: Vec<&str> = s.trim().split('-').collect();
        if parts.len() == 3
            && let (Ok(y), Ok(m), Ok(d)) = (
                parts[0].parse::<i32>(),
                parts[1].parse::<u32>(),
                parts[2].parse::<u32>(),
            )
            && let Some(date) = Date::new(y, m, d)
        {
            let now = std::env::var("ENZUI_CALENDAR_NOW")
                .ok()
                .and_then(|v| v.trim().parse::<i32>().ok())
                .unwrap_or(11 * 60 + 20);
            return (date, now);
        }
    }
    crate::sysclock::local_now()
}

pub(super) fn first_run_policy() -> persistence::FirstRun {
    if std::env::var_os("ENZUI_CALENDAR_TODAY").is_some() {
        persistence::FirstRun::Demo
    } else {
        persistence::FirstRun::Empty
    }
}

pub(super) fn system_hour_cycle() -> HourCycle {
    match std::env::var("ENZUI_CALENDAR_CLOCK").as_deref() {
        Ok("24h") => HourCycle::H23,
        Ok("12h") => HourCycle::H12,
        _ => crate::l10n::localizer()
            .locale()
            .hour_cycle()
            .unwrap_or(HourCycle::H23),
    }
}

pub(super) fn boot_worker_refusal_text(
    error: enzui_core::blocking_task::BlockingTaskError,
) -> String {
    use enzui_core::blocking_task::BlockingTaskError;
    match error {
        BlockingTaskError::Busy => {
            "Boot is still working; try opening again in a moment.".to_owned()
        }
        BlockingTaskError::Cancelled => "Opening your calendar file was cancelled.".to_owned(),
        BlockingTaskError::Shutdown => "The calendar boot worker is unavailable.".to_owned(),
        BlockingTaskError::Panicked => "Opening your calendar file failed unexpectedly.".to_owned(),
        _ => "The calendar boot worker is unavailable.".to_owned(),
    }
}

pub(super) fn default_boot_worker() -> Arc<enzui_core::blocking_task::BlockingTaskOwner> {
    use enzui_core::blocking_task::{BlockingTaskOwner, DEFAULT_QUEUE_CAPACITY};
    #[cfg(test)]
    {
        Arc::new(BlockingTaskOwner::unavailable(DEFAULT_QUEUE_CAPACITY))
    }
    #[cfg(not(test))]
    match BlockingTaskOwner::with_defaults("calendar-app-boot") {
        Ok(owner) => Arc::new(owner),
        Err(_) => Arc::new(BlockingTaskOwner::unavailable(DEFAULT_QUEUE_CAPACITY)),
    }
}

pub(super) fn default_persist_worker() -> Arc<enzui_core::blocking_task::BlockingTaskOwner> {
    use enzui_core::blocking_task::{BlockingTaskOwner, DEFAULT_QUEUE_CAPACITY};
    match BlockingTaskOwner::with_defaults("calendar-app-persist") {
        Ok(owner) => Arc::new(owner),
        // An unavailable worker must settle submissions so retained save debt remains retryable.
        Err(_) => Arc::new(BlockingTaskOwner::unavailable(DEFAULT_QUEUE_CAPACITY)),
    }
}

pub(super) fn minute_message(_now: std::time::Instant) -> Message {
    let (today, now) = today_and_now();
    Message::MinuteTick(today, now)
}

pub(super) fn minute_tick() -> Subscription<Message> {
    iced::time::every(std::time::Duration::from_secs(60)).map(minute_message)
}

pub(super) fn zone_message(_now: std::time::Instant) -> Message {
    Message::ZoneObserved(crate::sysclock::zone_fingerprint())
}

pub(super) fn zone_tick() -> Subscription<Message> {
    iced::time::every(std::time::Duration::from_secs(60)).map(zone_message)
}

pub(super) fn boot_accepts(message: &Message) -> bool {
    // Activation queued before subscriptions start must be admitted during boot.
    #[cfg(feature = "center")]
    if matches!(message, Message::Center(_) | Message::ApplicationWake) {
        return true;
    }
    matches!(
        message,
        Message::Theme(_)
            | Message::InstanceWake
            | Message::WindowResized(_, _)
            | Message::MinuteTick(_, _)
            | Message::CloseWindow(_)
            | Message::WindowClosed(_)
            | Message::Window(_)
            | Message::BootFinished { .. }
            | Message::PersistenceFinished { .. }
            | Message::RetryPersistence
            | Message::ReminderActivationFinished(..)
            | Message::CloseDrainTick
    )
}

pub(super) fn classify_event(event: &iced::Event, status: iced::event::Status) -> Option<Message> {
    let captured = matches!(status, iced::event::Status::Captured);
    match event {
        iced::Event::Window(iced::window::Event::Resized(size)) => {
            Some(Message::WindowResized(size.width, size.height))
        }
        // Captured keys belong to the focused widget and cannot trigger application shortcuts.
        iced::Event::Keyboard(key_event) if !captured => {
            shortcuts::dispatch(key_event).map(Message::Shortcut)
        }
        _ => None,
    }
}

pub(super) fn persist_notice_from_origin(
    origin: &persistence::Origin,
) -> Option<state::PersistNotice> {
    match origin {
        persistence::Origin::FreshStart | persistence::Origin::Restored => None,
        persistence::Origin::RecoveredFromCorrupt(moved) => {
            Some(state::PersistNotice::Quarantined(moved.clone()))
        }
        persistence::Origin::ReadOnlyFutureVersion(version) => {
            Some(state::PersistNotice::ReadOnlyFuture(*version))
        }
        persistence::Origin::RefusedBoot(refusal) => {
            Some(state::PersistNotice::BootRefused(*refusal))
        }
        // Persistence-root refusals disclose no path, errno, or capacity details.
        persistence::Origin::PersistenceRootUnavailable(refusal) => {
            Some(state::PersistNotice::PersistenceRootUnavailable(*refusal))
        }
    }
}

pub(super) fn apply_startup_overrides(state: &mut AppState) {
    if let Ok(v) = std::env::var("ENZUI_CALENDAR_VIEW")
        && let Some(view) = View::ALL
            .iter()
            .copied()
            .find(|x| x.label().eq_ignore_ascii_case(v.trim()))
    {
        state.view = view;
    }
    if let Ok(v) = std::env::var("ENZUI_CALENDAR_OVERLAY") {
        match v.trim().to_ascii_lowercase().as_str() {
            "settings" => state.overlay = state::Overlay::Settings,
            "settings-weekstart" | "week-start" => {
                state.settings_menu_open = Some(state::SettingsMenu::WeekStart);
                state.overlay = state::Overlay::Settings;
            }
            "settings-calendars" | "calendars" => {
                state.settings_tab = state::SettingsTab::Calendars;
                state.settings_page_open = true;
                state.overlay = state::Overlay::Settings;
            }
            "dayevents" | "day-events" => {
                state.overlay = state::Overlay::DayEvents(state.today);
            }
            "editor" | "editor-new" => {
                let minute = state.settings.work_start_minute;
                let date = state.focus;
                if let Some(draft) = state.try_draft_at(date, minute) {
                    state.set_overlay(state::Overlay::Editor(Box::new(draft)));
                } else {
                    state.refuse_id_exhaustion();
                }
            }
            "editor-weekly" => {
                if let Some(ev) = state
                    .events
                    .iter()
                    .find(|e| e.recurrence != model::Recurrence::None)
                    .cloned()
                {
                    state.set_overlay(state::Overlay::Editor(Box::new(ev)));
                }
            }
            "reminder" => {
                if let Some(ev) = state.events.iter().find(|e| e.alert.is_some()).cloned() {
                    state.overlay = state::Overlay::ReminderAlert {
                        event_id: ev.id,
                        on: state.today,
                        title: ev.title.clone(),
                        when: reminders::reminder_when(
                            &reminders::Due {
                                key: reminders::ReminderKey {
                                    event_id: ev.id,
                                    on: state.today,
                                },
                                title: ev.title.clone(),
                                start_minute: ev.start_minute.get(),
                                all_day: ev.all_day,
                                at: reminders::abs_minute(state.today, state.now_minute),
                                from_snooze: false,
                            },
                            &state.time_format(),
                        ),
                    };
                }
            }
            "recurrence" => {
                if let Some(ev) = state
                    .events
                    .iter()
                    .find(|e| e.recurrence != model::Recurrence::None)
                    .cloned()
                {
                    state.overlay = state::Overlay::RecurrencePrompt(Box::new(
                        state::PendingRecurrenceChoice {
                            event_id: ev.id,
                            occurrence: ev.date.add_days(7),
                            kind: state::RecurrenceActionKind::Delete,
                            title: ev.title,
                        },
                    ));
                }
            }
            "details" => {
                let today = state.today;
                if let Some(ev) = state
                    .events
                    .iter()
                    .find(|e| e.date == today && !e.all_day)
                    .or_else(|| state.events.first())
                    .cloned()
                {
                    open_details_overlay(
                        state,
                        ev.id,
                        Some(today),
                        Some(iced::Point::new(420.0, 300.0)),
                    );
                }
            }
            "search" => {
                state.overlay = state::Overlay::Search;
                state.search_query = CAPTURE_SEARCH_QUERY.into();
            }
            "quickadd" | "quick-add" => {
                let minute = state.settings.work_start_minute;
                let date = state.focus;
                if let Some(draft) = state.try_draft_at(date, minute) {
                    state.set_overlay(state::Overlay::QuickAdd(Box::new(state::QuickAdd {
                        at: iced::Point::new(420.0, 300.0),
                        draft: Box::new(draft),
                    })));
                } else {
                    state.refuse_id_exhaustion();
                }
            }
            _ => {}
        }
        state.sync_settings_presentation();
    }
    if let Ok(value) = std::env::var("ENZUI_CALENDAR_CONTEXT") {
        apply_startup_context(state, &value);
    }
}

pub(super) fn apply_startup_context(state: &mut AppState, value: &str) {
    let at = iced::Point::new(40.0, 120.0);
    let target = match value.trim().to_ascii_lowercase().as_str() {
        "event" => state.events.first().map(|event| MenuTarget::Event {
            id: event.id,
            on: Some(state.today),
        }),
        "slot" => Some(MenuTarget::Slot {
            date: state.today,
            minute: state.settings.work_start_minute,
        }),
        "day" => Some(MenuTarget::Day(state.today)),
        "calendar" => state
            .calendars
            .first()
            .map(|calendar| MenuTarget::CalendarRow(calendar.id.clone())),
        "miniday" => Some(MenuTarget::MiniDay(state.today)),
        _ => None,
    };
    if let Some(target) = target {
        open_menu(state, target, at);
    }
}
