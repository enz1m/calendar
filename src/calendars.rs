//! Local calendar editing and removal without deleting events.

use crate::{
    Message,
    model::CALENDAR_COLOR_KEYS,
    state::{AppState, Overlay, SettingsTab},
};
use iced::Task;

#[derive(Clone, Debug)]
pub(crate) enum Action {
    Edit(String),
    Name(String),
    Color(String),
    Destination(String),
    ToggleDestination,
    CloseDestination,
    Save,
    Cancel,
    RequestRemoval,
    ConfirmRemoval,
    Back,
}

#[derive(Clone, Debug)]
pub(crate) struct Editor {
    pub id: String,
    pub name: String,
    pub color: String,
    pub destination: Option<String>,
    pub destination_open: bool,
    pub removing: bool,
    pub error: Option<&'static str>,
}

pub(crate) fn reduce(state: &mut AppState, action: Action) -> Task<Message> {
    if state.overlay != Overlay::Settings || state.settings_tab != SettingsTab::Calendars {
        return Task::none();
    }
    match action {
        Action::Edit(id) => {
            let Some(calendar) = state.calendars.iter().find(|c| c.id == id) else {
                return Task::none();
            };
            state.calendar_editor = Some(Editor {
                id,
                name: calendar.name.clone(),
                color: calendar.color.clone(),
                destination: None,
                destination_open: false,
                removing: false,
                error: None,
            });
            return Task::none();
        }
        Action::Cancel => {
            state.calendar_editor = None;
            return Task::none();
        }

        action => {
            let Some(draft) = state.calendar_editor.as_mut() else {
                return Task::none();
            };
            match action {
                Action::Name(value) => {
                    draft.name = value;
                    draft.error = None;
                }
                Action::Color(value) => {
                    draft.color = value;
                    draft.error = None;
                }
                Action::Destination(id) => {
                    draft.destination = Some(id);
                    draft.destination_open = false;
                    draft.error = None;
                }
                Action::ToggleDestination => draft.destination_open = !draft.destination_open,
                Action::CloseDestination => draft.destination_open = false,
                Action::RequestRemoval => {
                    draft.removing = true;
                    draft.error = None;
                    return Task::none();
                }
                Action::Back => {
                    draft.removing = false;
                    draft.error = None;
                    return Task::none();
                }
                Action::Save => {
                    let name = draft.name.trim();
                    if name.is_empty()
                        || name.chars().count() > 120
                        || name.chars().any(char::is_control)
                    {
                        draft.error = Some("Use a name of 1–120 characters without line breaks.");
                        return Task::none();
                    }
                    if !CALENDAR_COLOR_KEYS.contains(&draft.color.as_str()) {
                        draft.error = Some("Choose one of the calendar colors.");
                        return Task::none();
                    }
                    let Some(calendar) = state.calendars.iter_mut().find(|c| c.id == draft.id)
                    else {
                        state.calendar_editor = None;
                        return Task::none();
                    };
                    name.clone_into(&mut calendar.name);
                    calendar.color.clone_from(&draft.color);
                    state.calendar_editor = None;
                    state.publish_toast(
                        "Calendar updated",
                        enzui_widgets::toast::ToastTone::Success,
                    );
                    return Task::none();
                }
                Action::ConfirmRemoval => {
                    if !draft.removing {
                        return Task::none();
                    }
                    if state.calendars.len() <= 1 {
                        draft.error = Some("Keep at least one calendar for your events.");
                        return Task::none();
                    }
                    if !state.calendars.iter().any(|c| c.id == draft.id) {
                        state.calendar_editor = None;
                        return Task::none();
                    }
                    let populated = state
                        .events
                        .iter()
                        .any(|event| event.calendar_id == draft.id);
                    if populated
                        && !draft.destination.as_ref().is_some_and(|id| {
                            id != &draft.id && state.calendars.iter().any(|c| &c.id == id)
                        })
                    {
                        draft.error = Some("Choose another calendar to keep these events in.");
                        return Task::none();
                    }
                    for event in &mut state.events {
                        if event.calendar_id == draft.id {
                            event.calendar_id = draft
                                .destination
                                .clone()
                                .expect("populated calendars require a destination");
                        }
                    }
                    state.calendars.retain(|c| c.id != draft.id);
                    let next_focus = draft
                        .destination
                        .clone()
                        .or_else(|| state.calendars.first().map(|c| c.id.clone()));
                    state.calendar_editor = None;
                    state.publish_toast(
                        "Calendar removed; your events were kept",
                        enzui_widgets::toast::ToastTone::Success,
                    );
                    return next_focus.map_or_else(Task::none, |id| focus_calendar(&id));
                }
                Action::Edit(_) | Action::Cancel => unreachable!(),
            }
        }
    }
    Task::none()
}

fn focus_calendar(id: &str) -> Task<Message> {
    focus_control(&format!("calendar-edit-{id}"))
}

fn focus_control(id: &str) -> Task<Message> {
    iced::advanced::widget::operate(enzui_core::focus::focus_and_reveal_id(
        iced::advanced::widget::Id::from(id.to_owned()),
    ))
}
