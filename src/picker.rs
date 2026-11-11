//! Tagged native file chooser with the framework's in-app fallback.

use crate::Message;
#[cfg(test)]
use enzui_widgets::file_chooser::PortalBackend;
use enzui_widgets::file_chooser::{
    ChooserOptions, ChooserSession, FallbackEvent, FileFilter, FileResult, PortalOperation,
    PortalOutcome, PortalWake,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    Import,
    Export,
}

mod parent;
pub(crate) use parent::ParentEvent;

pub(crate) struct Picker {
    session: ChooserSession<Purpose>,
    generation: u64,
    parent: Option<parent::PendingParent>,
    export_pool: Option<enzui_widgets::export_pool::ExportPoolOwner>,
    #[cfg(test)]
    scripted: bool,
}
impl Default for Picker {
    fn default() -> Self {
        Self::new()
    }
}
impl Picker {
    pub(crate) fn new() -> Self {
        Self::from_session(ChooserSession::new())
    }
    fn from_session(session: ChooserSession<Purpose>) -> Self {
        Self {
            session: session.with_fallback(),
            generation: 0,
            parent: None,
            export_pool: None,
            #[cfg(test)]
            scripted: false,
        }
    }
    #[cfg(test)]
    pub(crate) fn with_backend(backend: Box<dyn PortalBackend>) -> Self {
        let mut picker = Self::from_session(ChooserSession::with_backend(backend));
        picker.scripted = true;
        picker
    }
    pub(crate) fn is_pending(&self) -> bool {
        self.parent.is_some() || self.session.is_pending()
    }
    pub(crate) fn start(&mut self) -> Result<iced::Task<Message>, PortalOutcome<FileResult>> {
        if self.is_pending() {
            return Ok(iced::Task::none());
        }
        self.begin(
            Purpose::Import,
            PortalOperation::OpenFile,
            ChooserOptions::new()
                .title(crate::l10n::tr("Import events from an .ics file"))
                .filter(
                    FileFilter::new(crate::l10n::tr("iCalendar files"))
                        .glob("*.ics")
                        .mime("text/calendar"),
                ),
        )
    }
    pub(crate) fn start_save(
        &mut self,
        suggested_name: &str,
    ) -> Result<iced::Task<Message>, PortalOutcome<FileResult>> {
        if self.is_pending() {
            return Ok(iced::Task::none());
        }
        self.begin(
            Purpose::Export,
            PortalOperation::SaveFile,
            ChooserOptions::new()
                .title(crate::l10n::tr("Export events to an .ics file"))
                .current_name(suggested_name)
                .filter(
                    FileFilter::new(crate::l10n::tr("iCalendar files"))
                        .glob("*.ics")
                        .mime("text/calendar"),
                ),
        )
    }
    pub(crate) fn completion_subscription(&self) -> iced::Subscription<PortalWake> {
        self.session.subscription()
    }
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
    pub(crate) fn accepts(&self, generation: u64) -> bool {
        self.generation == generation
    }
    pub(crate) fn cancel_pending(&mut self) -> bool {
        let parenting = self.cancel_parent();
        self.generation = self.generation.saturating_add(1);
        self.session
            .cancel(enzui_widgets::file_chooser::CancelReason::Application)
            .is_some()
            || parenting
    }
    pub(crate) fn pump(&mut self) -> iced::Task<Message> {
        let generation = self.generation;
        self.session
            .pump(move |purpose, outcome| Message::ChooserFinished(generation, purpose, outcome))
    }
    pub(crate) fn handle(&mut self, event: FallbackEvent) -> iced::Task<Message> {
        let generation = self.generation;
        self.session.handle(event, move |purpose, outcome| {
            Message::ChooserFinished(generation, purpose, outcome)
        })
    }
    pub(crate) fn view<'a>(
        &'a self,
        content: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        let generation = self.generation();
        self.session.view(content, move |event| {
            Message::ChooserEvent(generation, event)
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    //! Scripted portal backend for reducer lifecycle tests.

    use enzui_widgets::file_chooser::{
        BackendError, BackendReply, CancelReason, ChooserOptions, ParentHandle, PortalBackend,
        PortalOperation, PortalResponse, RequestId,
    };

    pub(crate) struct ScriptedBackend {
        reply: Option<BackendReply>,
        start_error: Option<BackendError>,
        next_id: u64,
    }

    impl ScriptedBackend {
        pub(crate) fn unavailable() -> Self {
            Self {
                start_error: Some(BackendError::Unavailable),
                ..Self::silent()
            }
        }
        pub(crate) fn silent() -> Self {
            Self {
                reply: None,
                start_error: None,
                next_id: 0,
            }
        }

        pub(crate) fn replying(response: PortalResponse, uris: Vec<String>) -> Self {
            Self {
                reply: Some(BackendReply { response, uris }),
                ..Self::silent()
            }
        }
    }

    impl PortalBackend for ScriptedBackend {
        fn start(&mut self, operation: PortalOperation) -> Result<RequestId, BackendError> {
            self.start_with_options(operation, &ChooserOptions::new(), &ParentHandle::none())
        }

        fn start_with_options(
            &mut self,
            _operation: PortalOperation,
            _options: &ChooserOptions,
            _parent: &ParentHandle,
        ) -> Result<RequestId, BackendError> {
            if let Some(e) = self.start_error {
                return Err(e);
            }
            self.next_id += 1;
            Ok(RequestId::new(self.next_id))
        }

        fn poll_reply(&mut self, _id: RequestId) -> Option<BackendReply> {
            self.reply.take()
        }

        fn close(&mut self, _id: RequestId, _reason: CancelReason) {}

        fn describe(&self) -> String {
            "scripted (in-process, no D-Bus)".to_owned()
        }
    }
}
