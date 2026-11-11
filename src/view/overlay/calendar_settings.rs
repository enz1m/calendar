use super::*;
use crate::calendars::{Action, Editor};
use iced::widget::column;

pub(super) fn editor<'a>(
    app: &'a AppState,
    draft: &'a Editor,
    removing: bool,
) -> Element<'a, Message> {
    let count = app
        .events
        .iter()
        .filter(|event| event.calendar_id == draft.id)
        .count();
    let title = if removing {
        "Remove calendar"
    } else {
        "Edit calendar"
    };
    let heading = enzui_widgets::title_bar::enz_title_bar()
        .title(crate::l10n::tr(title))
        .title_heading(HeadingLevel::Page)
        .tone(enzui_widgets::surface::SurfaceTone::Elevated)
        .into_element();
    let heading = container(heading).padding(iced::Padding {
        right: CLOSE_CONTROL_SIZE + CLOSE_INSET,
        ..iced::Padding::ZERO
    });
    let mut content = column![heading].spacing(SpaceToken::Md.px());
    if removing {
        content = content.push(
            enz_text(crate::l10n::format(
                "Remove “{}”?",
                &[("arg0", (draft.name).as_str().into())],
            ))
            .size(TextSize::Body),
        );
        if count > 0 {
            content = content.push(
                enz_text(crate::l10n::count("calendar-keep-count", count)).size(TextSize::Body),
            );
            let destinations: Vec<_> = app
                .calendars
                .iter()
                .filter(|c| c.id != draft.id)
                .map(|c| {
                    (
                        format!("{} · {}", c.name, crate::l10n::tr(&c.color)),
                        Message::Calendar(Action::Destination(c.id.clone())),
                    )
                })
                .collect();
            let selected = draft
                .destination
                .as_ref()
                .and_then(|id| app.calendars.iter().find(|c| &c.id == id))
                .map_or_else(|| crate::l10n::tr("Choose a calendar"), |c| c.name.clone());
            content = content.push(
                enz_preferences_row(crate::l10n::tr("Move events to"))
                    .value_menu(
                        selected,
                        draft.destination_open,
                        Message::Calendar(Action::ToggleDestination),
                        Message::Calendar(Action::CloseDestination),
                        destinations,
                    )
                    .into_element(),
            );
        } else {
            content = content.push(
                enz_text(crate::l10n::tr("This calendar has no events.")).size(TextSize::Body),
            );
        }
        content = content.push(
            row![
                enz_button(crate::l10n::tr("Back"), Message::Calendar(Action::Back))
                    .kind(ButtonKind::Quiet)
                    .id(iced::advanced::widget::Id::new("calendar-removal-back")),
                enz_button(
                    crate::l10n::tr(if count > 0 {
                        "Move events and remove"
                    } else {
                        "Remove calendar"
                    }),
                    Message::Calendar(Action::ConfirmRemoval)
                )
                .id(iced::advanced::widget::Id::new("calendar-remove-confirm"))
                .kind(ButtonKind::Destructive)
                .enabled(app.calendars.len() > 1 && (count == 0 || draft.destination.is_some())),
            ]
            .mirrored(crate::l10n::rtl())
            .spacing(SpaceToken::Sm.px()),
        );
    } else {
        content = content.push(
            enz_text_field(
                crate::l10n::tr("Calendar name"),
                crate::l10n::tr("Name"),
                &draft.name,
                |name| Message::Calendar(Action::Name(name)),
            )
            .id(iced::advanced::widget::Id::new("calendar-name"))
            .on_submit(Message::Calendar(Action::Save)),
        );
        content = content.push(
            enz_combo_box(
                crate::l10n::tr("Color"),
                crate::model::CALENDAR_COLOR_KEYS
                    .iter()
                    .map(crate::l10n::tr)
                    .collect(),
                None,
                |_| Message::Calendar(Action::Color(String::new())),
            )
            .selected_index(
                crate::model::CALENDAR_COLOR_KEYS
                    .iter()
                    .position(|key| *key == draft.color),
            )
            .on_select_index(|index| {
                Message::Calendar(Action::Color(
                    crate::model::CALENDAR_COLOR_KEYS[index].into(),
                ))
            }),
        );
        content = content.push(
            row![
                enz_button(crate::l10n::tr("Cancel"), Message::Calendar(Action::Cancel))
                    .id(iced::advanced::widget::Id::new("calendar-edit-cancel"))
                    .kind(ButtonKind::Quiet),
                enz_button(crate::l10n::tr("Save"), Message::Calendar(Action::Save))
                    .id(iced::advanced::widget::Id::new("calendar-edit-save")),
            ]
            .mirrored(crate::l10n::rtl())
            .spacing(SpaceToken::Sm.px()),
        );
        content = content.push(enz_preferences_group(
            enz_preferences_row(crate::l10n::tr("Remove this calendar"))
                .subtitle(crate::l10n::tr(if app.calendars.len() > 1 {
                    "Events will be kept in another calendar of your choice."
                } else {
                    "Keep at least one calendar for your events."
                }))
                .action(
                    crate::l10n::tr("Remove…"),
                    Message::Calendar(Action::RequestRemoval),
                )
                .enabled(app.calendars.len() > 1)
                .into_element(),
        ));
    }
    if let Some(error) = draft.error {
        content = content.push(
            enz_text(crate::l10n::tr(error))
                .size(TextSize::Body)
                .tone(TextTone::Danger),
        );
    }
    content.into()
}
