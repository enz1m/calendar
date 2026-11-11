//! Ordinary desktop launch intents use the same bounded import path as the chooser.

use enzui_core::activation::ApplicationIntent;
use enzui_widgets::toast::ToastTone;
use iced::Task;

use crate::{App, Message, state::Overlay};

impl App {
    pub(crate) fn accept_launch(&mut self, intent: ApplicationIntent) {
        let ApplicationIntent::Open(uris) = intent else {
            return;
        };
        if uris.len() != 1 {
            self.state
                .publish_toast("Choose one calendar file to import.", ToastTone::Warning);
            return;
        }
        let Some(path) = uris
            .first()
            .and_then(|uri| enzui_core::portal::uri::file_uri_to_path(uri).ok())
        else {
            self.state.publish_toast(
                "That is not a local file — nothing to import.",
                ToastTone::Warning,
            );
            return;
        };
        if self.pending_open.is_some() {
            self.state.publish_toast(
                "An import is already waiting. Try opening the file again later.",
                ToastTone::Warning,
            );
            return;
        }
        self.pending_open = Some(path);
    }

    pub(crate) fn instance_wake(&mut self) -> Task<Message> {
        let Some(instance) = self.instance.as_mut() else {
            return Task::none();
        };
        let (intents, raise) = instance.drain(None);
        for intent in intents {
            self.accept_launch(intent);
        }
        raise
    }

    pub(crate) fn open_deferred_launch(&mut self) -> Task<Message> {
        if self.state.overlay != Overlay::None || !self.state.close_state.is_idle() {
            return Task::none();
        }
        self.pending_open
            .take()
            .map_or_else(Task::none, |path| self.begin_bounded_import(path))
    }
}
