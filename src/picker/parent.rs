//! Request-scoped native chooser parents and cancellation.

use super::*;
use enzui_core::portal::cancel::PortalCancel;
use enzui_parent_lease::Handoff;
use enzui_widgets::file_chooser::CancelReason;
use enzui_widgets::file_chooser::ChooserParent;
use enzui_widgets::portal_parent::{ParentExport, export_parent, export_parent_task};
use iced::{Task, window};

#[derive(Debug, Clone)]
pub(crate) enum ParentEvent {
    Window(Option<window::Id>),
    Handle(window::Id, ParentExport),
    Export(
        window::Id,
        Handoff<enzui_core::blocking_task::BlockingTaskSettlement<enzui_wayland::ParentAdoption>>,
    ),
    Live(window::Id, Handoff<ChooserParent>, bool),
}

pub(super) struct PendingParent {
    tag: Purpose,
    operation: PortalOperation,
    options: ChooserOptions,
    window: Option<window::Id>,
    cancel: PortalCancel,
}

impl Picker {
    pub(super) fn begin(
        &mut self,
        tag: Purpose,
        operation: PortalOperation,
        options: ChooserOptions,
    ) -> Result<Task<Message>, PortalOutcome<FileResult>> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(PortalOutcome::Unavailable)?;
        #[cfg(test)]
        if self.scripted {
            self.session.start(tag, operation, options)?;
            return Ok(Task::none());
        }
        self.parent = Some(PendingParent {
            tag,
            operation,
            options,
            window: None,
            cancel: PortalCancel::new(),
        });
        let generation = self.generation;
        Ok(window::latest()
            .map(move |id| Message::ChooserParent(generation, ParentEvent::Window(id))))
    }

    pub(crate) fn parent_event(&mut self, generation: u64, event: ParentEvent) -> Task<Message> {
        if !self.accepts(generation) || self.parent.is_none() {
            return Task::none();
        }
        match event {
            ParentEvent::Window(Some(window)) => {
                if let Some(pending) = self.parent.as_mut() {
                    pending.window = Some(window);
                }
                window::run(window, |handle| {
                    handle
                        .window_handle()
                        .map_or(ParentExport::Unsupported, |handle| {
                            export_parent(&handle.as_raw())
                        })
                })
                .map(move |handle| {
                    Message::ChooserParent(generation, ParentEvent::Handle(window, handle))
                })
            }
            ParentEvent::Handle(window, ParentExport::Ready(handle)) => {
                Self::verify_parent(generation, window, ChooserParent::X11 { window, handle })
            }
            ParentEvent::Handle(window, ParentExport::NeedsWaylandExport) => {
                let pool = self.export_pool.get_or_insert_with(|| {
                    enzui_widgets::export_pool::ExportPoolOwner::start().unwrap_or_else(|_| {
                        enzui_widgets::export_pool::ExportPoolOwner::unavailable()
                    })
                });
                let Some(pending) = self.parent.as_ref() else {
                    return Task::none();
                };
                export_parent_task(
                    &pool.dispatcher(),
                    generation,
                    window,
                    pending.cancel.clone(),
                    std::time::Duration::from_secs(2),
                )
                .map(move |result| {
                    Message::ChooserParent(generation, ParentEvent::Export(window, result))
                })
            }
            ParentEvent::Export(window, carrier) => {
                let Some(settlement) = carrier.take() else {
                    return Task::none();
                };
                match settlement.into_result() {
                    Ok(enzui_wayland::ParentAdoption::Parented { export, .. }) => {
                        Self::verify_parent(
                            generation,
                            window,
                            ChooserParent::Wayland { window, export },
                        )
                    }
                    _ => self.parent_failure(),
                }
            }
            ParentEvent::Live(window, carrier, live) => {
                if !live {
                    return self.parent_failure();
                }
                let Some(parent) = carrier.take() else {
                    return Task::none();
                };
                let Some(pending) = self.parent.take() else {
                    return Task::none();
                };
                match self.session.start_parented(
                    pending.tag,
                    pending.operation,
                    pending.options,
                    parent,
                    |candidate| live && candidate == window,
                ) {
                    Ok(()) => Task::none(),
                    Err(outcome) => {
                        Task::done(Message::ChooserFinished(generation, pending.tag, outcome))
                    }
                }
            }
            ParentEvent::Window(None) | ParentEvent::Handle(_, ParentExport::Unsupported) => {
                self.parent_failure()
            }
        }
    }

    fn verify_parent(generation: u64, window: window::Id, parent: ChooserParent) -> Task<Message> {
        let carrier = Handoff::new(parent);
        window::latest().map(move |latest| {
            Message::ChooserParent(
                generation,
                ParentEvent::Live(window, carrier.clone(), latest == Some(window)),
            )
        })
    }

    fn parent_failure(&mut self) -> Task<Message> {
        let Some(pending) = self.parent.take() else {
            return Task::none();
        };
        pending.cancel.cancel(CancelReason::Application);
        Task::done(Message::ChooserFinished(
            self.generation,
            pending.tag,
            PortalOutcome::ParentUnavailable,
        ))
    }

    pub(super) fn cancel_parent(&mut self) -> bool {
        self.parent.take().is_some_and(|pending| {
            pending.cancel.cancel(CancelReason::Application);
            true
        })
    }

    pub(crate) fn parent_closed(&mut self, window: window::Id) {
        if self
            .parent
            .as_ref()
            .is_some_and(|pending| pending.window.is_none() || pending.window == Some(window))
        {
            self.cancel_pending();
        }
        if self.session.parent_closed(window).is_some() {
            self.generation = self.generation.saturating_add(1);
        }
    }
}

impl Drop for Picker {
    fn drop(&mut self) {
        self.cancel_parent();
    }
}
