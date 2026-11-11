//! Focus ownership for the shared modal host.

use enzui_core::modal::DismissCause;
use enzui_widgets::modal_host::{ModalFocusLifecycle, ModalHostEvent};
use iced::Task;

use crate::state::{AppState, Overlay, OverlayFocusKind, SettingsInvoker};
use crate::{App, Message};

fn settings_invoker_id(invoker: SettingsInvoker) -> iced::advanced::widget::Id {
    match invoker {
        SettingsInvoker::InlineGear => crate::view::overlay::settings_button_id(),
    }
}

impl App {
    pub(crate) fn sync_overlay_focus(&mut self) -> Task<Message> {
        let desired = self.state.overlay.focus_kind();
        let current = self.state.overlay_focus.as_ref().map(|(kind, _)| *kind);
        if desired == current {
            return Task::none();
        }
        let mut tasks = Vec::new();
        if let Some((_, lifecycle)) = self.state.overlay_focus.take() {
            tasks.push(lifecycle.close_for_host::<Message>().into_task());
        }
        if let Some(kind) = desired {
            let invoker = (kind == OverlayFocusKind::Settings)
                .then(|| self.state.settings_invoker.map(settings_invoker_id))
                .flatten();
            let (lifecycle, operation) =
                ModalFocusLifecycle::open::<Message>(crate::view::OVERLAY_SCOPE_KEY, invoker);
            self.state.overlay_focus = Some((kind, lifecycle));
            tasks.push(iced::advanced::widget::operate(operation));
        }
        Task::batch(tasks)
    }

    pub(crate) fn reduce_overlay_host(&mut self, event: ModalHostEvent) -> Task<Message> {
        let Some((_, lifecycle)) = self.state.overlay_focus.as_ref() else {
            return Task::none();
        };
        let outcome = lifecycle.reduce::<Message>(event);
        if outcome.dismissed().is_some() {
            self.state.overlay_focus = None;
            clear_overlay(&mut self.state);
        }
        outcome.into_task()
    }
}

fn clear_overlay(state: &mut AppState) {
    state.overlay = Overlay::None;
    state.settings_invoker = None;
    state.editing_occurrence = None;
    state.pending_recurrence_edit = None;
}

pub(crate) fn dismiss_overlay(state: &mut AppState) -> Task<Message> {
    let owner = state.overlay_focus.take();
    clear_overlay(state);
    owner.map_or_else(Task::none, |(_, lifecycle)| {
        lifecycle
            .reduce::<Message>(ModalHostEvent::Dismissed(DismissCause::CloseButton))
            .into_task()
    })
}
