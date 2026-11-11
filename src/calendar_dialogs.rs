//! Calendar metadata editing keeps Settings mounted beneath its confirmation.

use enzui_core::modal::DismissCause;
use enzui_widgets::modal_host::DialogEvent;
use iced::Task;

use crate::{App, Message, state::Overlay};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CalendarDialog {
    CalendarEditor,
    CalendarRemoval,
}

impl App {
    pub(crate) fn sync_calendar_dialogs(&mut self) -> Task<Message> {
        use CalendarDialog::{CalendarEditor, CalendarRemoval};
        let state = &mut self.state;
        if state.overlay != Overlay::Settings {
            state.calendar_editor = None;
        }
        let Some(draft) = &state.calendar_editor else {
            return if state.overlay == Overlay::Settings {
                state
                    .calendar_dialogs
                    .dismiss(&CalendarEditor, DismissCause::DefaultAction)
                    .1
            } else {
                let _ = state.calendar_dialogs.close(&CalendarEditor);
                Task::none()
            };
        };
        let mut tasks = Vec::new();
        if !state.calendar_dialogs.is_open(&CalendarEditor) {
            tasks.push(state.calendar_dialogs.open(
                CalendarEditor,
                Some(iced::advanced::widget::Id::from(format!(
                    "calendar-edit-{}",
                    draft.id
                ))),
            ));
        }
        if draft.removing {
            if !state.calendar_dialogs.is_open(&CalendarRemoval) {
                tasks.push(state.calendar_dialogs.open(
                    CalendarRemoval,
                    Some(iced::advanced::widget::Id::new("calendar-name")),
                ));
            }
        } else {
            tasks.push(
                state
                    .calendar_dialogs
                    .dismiss(&CalendarRemoval, DismissCause::CancelAction)
                    .1,
            );
        }
        Task::batch(tasks)
    }

    pub(crate) fn calendar_dialog_event(
        &mut self,
        event: DialogEvent<CalendarDialog>,
    ) -> Task<Message> {
        let (closed, focus) = self.state.calendar_dialogs.update(event);
        if closed.contains(&CalendarDialog::CalendarEditor) {
            self.state.calendar_editor = None;
        } else if closed.contains(&CalendarDialog::CalendarRemoval)
            && let Some(draft) = self.state.calendar_editor.as_mut()
        {
            draft.removing = false;
            draft.error = None;
        }
        focus
    }
}
