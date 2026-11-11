//! Calendar for agents with or without its window, behind `center`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use enzui_agent::center::background::Disposition;
use enzui_agent::center::task_host::{TaskHost, TaskHostError};
use enzui_agent_app::{
    AgentProcess, ApplicationIntent, BackgroundApp, BackgroundExit, DEFAULT_IDLE_EXIT, LaunchMode,
};

use super::*;

pub(crate) use enzui_agent_app::DesktopClaim;

pub(crate) const IDLE_EXIT: Duration = DEFAULT_IDLE_EXIT;

pub(crate) fn launch_mode() -> Result<LaunchMode, String> {
    LaunchMode::from_args(std::env::args_os().skip(1)).map_err(|error| error.to_string())
}

pub(crate) fn export_manifest() -> std::process::ExitCode {
    enzui_agent_app::print_manifest(&crate::center::registry(), crate::center::metadata())
}

pub(crate) fn claim_for_gui() -> Result<DesktopClaim, enzui_agent_app::ApplicationServiceError> {
    enzui_agent_app::claim_desktop(crate::center::CENTER_APP_ID, ApplicationIntent::Activate)
}

pub(crate) fn run_background() -> Result<Option<App>, String> {
    let Some(service) = enzui_agent_app::claim_background(crate::center::CENTER_APP_ID)? else {
        return Ok(None);
    };
    let tasks =
        Arc::new(TaskHost::spawn().map_err(|error| format!("task host did not start: {error}"))?);
    let (mut app, boot) = App::new_windowless();
    app.agent = AgentProcess::windowless(Some(service), Arc::clone(&tasks));
    app.serve_search();
    match enzui_agent_app::run_background(Background(app), boot, &tasks, IDLE_EXIT)
        .map_err(|error| error.to_string())?
    {
        BackgroundExit::Present(Background(app)) => Ok(Some(app)),
        BackgroundExit::Idle(Background(mut app)) | BackgroundExit::Exit(Background(mut app)) => {
            app.shutdown_center();
            Ok(None)
        }
    }
}

impl App {
    pub(crate) fn present_task(&mut self) -> Task<Message> {
        let (_launch, deferred) = self.agent.begin_presenting();
        deferred
    }

    pub(crate) fn application_wake(&mut self) -> Task<Message> {
        let calls = self.agent.take_application_calls();
        let launched = !calls.is_empty();
        let searches = self.agent.take_searches();
        if !launched && searches.is_empty() {
            return Task::none();
        }
        let mut tasks: Vec<Task<Message>> = searches
            .iter()
            .flat_map(|activation| crate::search::messages_for(&activation.request))
            .map(|message| self.update(message))
            .collect();
        let contexts = calls
            .into_iter()
            .map(|call| call.context)
            .chain(searches.into_iter().map(|search| search.context));
        for mut context in contexts {
            tasks.push(iced::window::latest().then(move |window| {
                match (window, context.take()) {
                    (Some(window), Some(context)) => context
                        .activate(window, Instant::now())
                        .map(|_| Message::ApplicationWake),
                    (Some(window), None) => iced::window::gain_focus(window),
                    _ => Task::none(),
                }
            }));
        }
        Task::batch(tasks)
    }

    pub(crate) fn serve_search(&mut self) {
        self.search_index.refresh(&self.state);
        if let Err(error) = self.agent.serve_search(self.search_index.provider()) {
            eprintln!("calendar-app: Search provider unavailable: {error}");
        }
    }

    pub(crate) fn agent_subscription(&self) -> Subscription<Message> {
        self.agent
            .subscription(|_| Message::ApplicationWake, Message::BackgroundTaskRefused)
    }

    fn calendar_busy(&self) -> bool {
        matches!(self.state.overlay, Overlay::Booting { .. }) || self.pending_save.is_some()
    }
}

struct Background(App);

impl std::fmt::Debug for Background {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Background").field(&self.0.agent).finish()
    }
}

impl BackgroundApp for Background {
    type Message = Message;

    fn update(&mut self, message: Message) -> Task<Message> {
        self.0.update(message)
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([self.0.center_subscription(), self.0.agent_subscription()])
    }

    fn disposition(&self) -> Disposition {
        let app = &self.0;
        // Loading state without a connector is pending application work, not a disconnected broker.
        if app.center.is_none() && app.calendar_busy() {
            return Disposition::Working;
        }
        enzui_agent_app::disposition(app.center.as_ref(), &app.agent, app.calendar_busy())
    }

    fn wake_at(&self) -> Option<Instant> {
        self.0
            .center
            .as_ref()
            .and_then(enzui_agent_app::deadline_instant)
    }

    fn wake(&mut self) -> Task<Message> {
        if let Some(host) = self.0.center.as_mut() {
            let _ended = host.poll_ended();
        }
        Task::none()
    }

    fn refused(&mut self, error: TaskHostError) -> Task<Message> {
        self.0
            .update(Message::BackgroundTaskRefused(error.to_string()))
    }
}
