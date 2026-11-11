//! Personal calendar with shared Day and Week schedule surfaces.

#![deny(unconditional_recursion)]

mod calendar_dialogs;
mod calendars;
mod l10n;
mod package;

#[cfg(feature = "center")]
mod background;
mod boot;
#[cfg(feature = "center")]
mod center;
mod ics;
mod ics_export;
mod ics_import;
mod ics_safety;
mod launch;
mod layout;
mod message;
mod modal_focus;
mod model;
mod notify;
mod persistence;
mod persistence_runtime;
mod picker;
mod recurrence;
mod reminder_activation;
mod reminders;
#[cfg(feature = "center")]
mod search;
mod seed;
mod shortcuts;
mod state;
mod sysclock;
mod time_entry;
mod view;

#[cfg(test)]
mod close_tests;
#[cfg(test)]
mod persistence_failure;

use std::sync::Arc;

use enzui_core::civil_date::{Date, Weekday};
use enzui_core::theme_file::integration::ThemeIntegration;
use enzui_core::time_of_day::HourCycle;
use enzui_widgets::app_shell::{DesktopApplicationIdentity, ShellLayout, ShellWindowState};
use enzui_widgets::nav_split::NavSplitState;
use enzui_widgets::schedule::ScheduleEvent as SchedEvent;
use enzui_widgets::title_bar_gesture::WindowCommand;
use enzui_widgets::toast::ToastTone;
use enzui_widgets::toast_host::ToastEvent;
use iced::{Element, Subscription, Task};

use message::Message;
use modal_focus::dismiss_overlay;
use model::{Event, MinuteOfDay, View};
use state::{
    AppState, CloseState, MenuTarget, Overlay, PendingRecurrenceChoice, RecurrenceActionKind,
};

fn main() -> std::process::ExitCode {
    l10n::app_localizer().scope(start)
}

fn start() -> std::process::ExitCode {
    if let Some(result) = package::command() {
        return result;
    }
    enzui_core::crash_report::CrashReporter::for_app("calendar-app", env!("CARGO_PKG_VERSION"));

    #[cfg(feature = "center")]
    {
        use enzui_agent_app::LaunchMode;
        match background::launch_mode() {
            Ok(LaunchMode::ExportManifest) => return background::export_manifest(),
            Ok(LaunchMode::Background) => {
                return match background::run_background() {
                    Ok(Some(app)) => run_gui(Some(app)),
                    Ok(None) => std::process::ExitCode::SUCCESS,
                    Err(error) => {
                        eprintln!("calendar-app: background mode: {error}");
                        std::process::ExitCode::FAILURE
                    }
                };
            }
            Ok(LaunchMode::Gui) => {}
            Err(error) => {
                eprintln!("calendar-app: {error}");
                return std::process::ExitCode::from(2);
            }
        }
    }
    run_gui(None)
}

fn run_gui(presented: Option<App>) -> std::process::ExitCode {
    #[cfg(feature = "center")]
    let mut service = None;
    #[cfg(feature = "center")]
    if presented.is_none() {
        match background::claim_for_gui() {
            Ok(background::DesktopClaim::Owner(owned)) => service = Some(owned),
            Ok(background::DesktopClaim::Forwarded) => return std::process::ExitCode::SUCCESS,
            Ok(background::DesktopClaim::Independent) => {}
            Err(error) => {
                // A refused handoff must never create another writer.
                eprintln!("calendar-app: could not reach the running instance: {error}");
                return std::process::ExitCode::FAILURE;
            }
        }
    }
    let mut instance = None;
    let mut launch = None;
    #[cfg(feature = "center")]
    let ordinary = presented.is_none() && service.is_none();
    #[cfg(not(feature = "center"))]
    let ordinary = true;
    if ordinary {
        use enzui_widgets::single_instance::{InstanceStart, SingleInstance};
        // A refused launch must not create an independent calendar writer.
        if enzui_core::activation::InboundActivationToken::from_env().is_err() {
            eprintln!("calendar-app: invalid startup activation token");
            return std::process::ExitCode::FAILURE;
        }
        match SingleInstance::start(package::APP_ID) {
            InstanceStart::Forwarded => return std::process::ExitCode::SUCCESS,
            InstanceStart::Owner(owner, intent) if owner.is_shared() => {
                instance = Some(owner);
                launch = Some(intent);
            }
            InstanceStart::Owner(_, _) => {
                eprintln!("calendar-app: could not establish single-writer ownership");
                return std::process::ExitCode::FAILURE;
            }
        }
    }
    let instance = std::cell::RefCell::new(instance);
    let launch = std::cell::RefCell::new(launch);
    #[cfg(feature = "center")]
    let pending = std::cell::RefCell::new(presented);
    #[cfg(not(feature = "center"))]
    let _ = presented;
    #[cfg(feature = "center")]
    let service = std::cell::RefCell::new(service);
    let boot = move || {
        #[cfg(feature = "center")]
        if let Some(mut app) = pending.borrow_mut().take() {
            let task = app.present_task();
            return (app, task);
        }
        let (mut app, task) = App::new();
        app.instance = instance.borrow_mut().take();
        if let Some(intent) = launch.borrow_mut().take() {
            app.accept_launch(intent);
        }
        #[cfg(feature = "center")]
        let app = {
            let mut app = app;
            app.agent = enzui_agent_app::AgentProcess::desktop(service.borrow_mut().take());
            app.serve_search();
            app
        };
        (app, task)
    };
    let result = {
        let identity = DesktopApplicationIdentity::for_package(&package::identity());
        let program = iced::application(boot, App::update, App::view)
            .title(|_: &App| crate::l10n::tr("Calendar"))
            .subscription(App::subscription)
            .theme(App::theme)
            .window(identity.window_settings(ShellLayout::TitlebarSidebarContent))
            // Compositor close requests must pass through the persistence drain before window
            // teardown.
            .exit_on_close_request(enzui_core::close_guard::EXIT_ON_CLOSE_REQUEST);
        enzui_widgets::app_setup::run_with_services(
            program,
            enzui_widgets::app_setup::AppServices::default().localizer(l10n::localizer()),
        )
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("calendar-app: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[path = "app_parts/app_shell.rs"]
mod app_shell;
#[path = "app_parts/editing.rs"]
mod editing;
#[path = "app_parts/reduce.rs"]
mod reduce;
#[path = "app_parts/reduce_state.rs"]
mod reducer;
#[path = "app_parts/startup.rs"]
mod startup;
#[path = "app_parts/tasks.rs"]
mod tasks;

pub(crate) use editing::{
    apply_edit_to_all_occurrences, apply_edit_to_following_occurrences,
    apply_edit_to_this_occurrence, edit, event_focus_after_removal, open_details_overlay,
    open_menu, recurrence_edited, restore_event_focus_task, restore_focus_task,
    retire_anchor_on_wallclock_change, stage_recurring_drag_edit, translate_move,
    wallclock_endpoints,
};
pub(crate) use startup::{
    ExportPhase, PendingExport, apply_startup_overrides, boot_accepts, classify_event,
    default_boot_worker, default_persist_worker, first_run_policy, import_refusal, minute_tick,
    next_export_generation, next_import_generation, persist_notice_from_origin, run_export,
    system_hour_cycle, today_and_now, zone_tick,
};
pub(crate) use startup::{
    boot_worker_refusal_text, export_omission_summary, export_refusal_result,
    export_write_failure_text,
};

struct App {
    state: AppState,
    themes: ThemeIntegration,
    shell_window: ShellWindowState,
    nav: NavSplitState,
    picker: picker::Picker,
    instance: Option<enzui_widgets::single_instance::SingleInstance>,
    pending_open: Option<std::path::PathBuf>,
    persist: persistence::Persist,
    persistence_queue: persistence::PersistenceQueue,
    // Window teardown waits for the latest retained save.
    pending_save: Option<persistence::PendingSave>,
    // The export snapshot survives chooser completion and Busy retries.
    pending_export: Option<PendingExport>,
    click_detector: enzui_core::click::ClickDetector,
    notifier: notify::Notifier,
    pending_import_generation: u64,
    pending_import_drop: Option<enzui_widgets::file_drop::FileDrop>,
    import_drop_epoch: u64,
    pending_boot_generation: u64,
    boot_worker: Arc<enzui_core::blocking_task::BlockingTaskOwner>,
    persist_worker: Arc<enzui_core::blocking_task::BlockingTaskOwner>,
    #[cfg(feature = "center")]
    center: Option<enzui_agent::center::CenterHost<crate::center::CenterCall>>,
    #[cfg(feature = "center")]
    center_requested: bool,
    #[cfg(feature = "center")]
    agent: enzui_agent_app::AgentProcess<Message>,
    #[cfg(feature = "center")]
    search_index: search::SearchIndex,
}

pub(crate) const CLOSE_DRAIN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

pub(crate) const CLOSE_DRAIN_TICK_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(100);

pub(crate) const CAPTURE_SEARCH_QUERY: &str = "re";

#[cfg(test)]
#[path = "main_tests/support.rs"]
mod support;

#[cfg(test)]
#[path = "main_tests/recurrence_prompt_tests.rs"]
mod recurrence_prompt_tests;

#[cfg(test)]
#[path = "main_tests/import_reducer_tests.rs"]
mod import_reducer_tests;

#[cfg(test)]
#[path = "main_tests/export_reducer_tests.rs"]
mod export_reducer_tests;
