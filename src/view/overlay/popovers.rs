//! Popover surfaces, context menus, and their models.

use super::*;
use iced::widget::column;

pub(crate) fn popover(app: &AppState) -> Option<(iced::Point, Element<'_, Message>)> {
    match &app.overlay {
        Overlay::Details { id, on, at } => {
            let e = app.event(*id)?;
            Some((*at, details_popover(app, e, *on)))
        }
        Overlay::QuickAdd(q) => Some((q.at, quick_add_popover(app, &q.draft))),
        _ => None,
    }
}

pub(super) fn delete_confirm<'a>(
    app: &'a AppState,
    pending: &'a crate::state::PendingDelete,
) -> Element<'a, Message> {
    let title = if pending.title.trim().is_empty() {
        crate::l10n::tr("Delete this event?")
    } else {
        crate::l10n::format(
            "Delete “{}”?",
            &[("arg0", (pending.title.trim()).to_owned().into())],
        )
    };
    enzui_widgets::confirm_dialog::enz_confirm_dialog(
        title,
        crate::l10n::tr("The event will be removed from this calendar. This cannot be undone."),
        &app.delete_confirm_state,
        Message::DeleteConfirmEvent,
    )
    .confirm_label(crate::l10n::tr("Delete"))
    .cancel_label(crate::l10n::tr("Cancel"))
    .destructive(true)
    .into_surface()
}

fn details_popover<'a>(app: &'a AppState, e: &'a Event, on: Option<Date>) -> Element<'a, Message> {
    let id = e.id;
    let title = enz_text(if e.title.trim().is_empty() {
        crate::l10n::tr("New Event")
    } else {
        e.title.clone()
    })
    .size(TextSize::Headline);
    let title: Element<'a, Message> = if crate::ics_export::is_unsupported_for_export(e)
        || e.uid
            .as_deref()
            .is_none_or(|uid| !crate::ics_safety::safe_uid(uid))
    {
        title.into_element()
    } else {
        let event = e.clone();
        let stamp = app.today;
        enzui_widgets::file_drag::enz_file_drag_source_with(title, move |cancel| {
            let (directory, path) = crate::ics_export::prepare_event_drag(&event, stamp, &cancel)?;
            Ok(enzui_widgets::file_drag::PreparedFiles::owned(
                vec![path],
                directory,
            ))
        })
        .key((&e.calendar_id, e.id))
        .allows_captured_press(true)
        .into()
    };
    let mut lines = column![title].spacing(SpaceToken::Xxs.px());

    let when = match on {
        Some(date) => format!(
            "{} · {}",
            date_label(date),
            e.time_label(&app.time_format())
        ),
        None => e.time_label(&app.time_format()),
    };
    lines = lines.push(enz_text(when).size(TextSize::Callout).tone(TextTone::Muted));

    if let Some(cal) = app.calendars.iter().find(|c| c.id == e.calendar_id) {
        lines = lines.push(
            enz_text(cal.name.clone())
                .size(TextSize::Caption)
                .tone(TextTone::Subtle),
        );
    }

    if !e.location.trim().is_empty() {
        lines = lines.push(
            enz_text(e.location.clone())
                .size(TextSize::Caption)
                .tone(TextTone::Subtle),
        );
    }

    let delete_msg = match on {
        Some(date) => Message::DeleteOccurrence { id, on: date },
        None => Message::RequestDeleteEvent(id),
    };

    let edit_msg = match on {
        Some(date) => Message::OpenOccurrenceEditor { id, on: date },
        None => Message::OpenEditor(id),
    };
    let actions = row![
        enz_button(crate::l10n::tr("Delete"), delete_msg)
            .id(iced::advanced::widget::Id::new("calendar-details-delete"))
            .kind(ButtonKind::Destructive)
            .size(ButtonSize::Sm),
        Space::new().width(Length::Fill).height(Length::Shrink),
        enz_button(crate::l10n::tr("Edit"), edit_msg)
            .id(iced::advanced::widget::Id::new("calendar-details-edit"))
            .kind(ButtonKind::Accent)
            .size(ButtonSize::Sm),
    ]
    .mirrored(crate::l10n::rtl())
    .align_y(Alignment::Center);

    let body = column![lines, actions].spacing(SpaceToken::Sm.px());
    enzui_widgets::card::enz_card(body)
        .padding(SpaceToken::Sm.px())
        .tone(CardTone::Elevated)
        .width_length(Length::Fixed(POPOVER_W_PX))
        .into()
}

fn quick_add_popover<'a>(app: &'a AppState, draft: &'a Event) -> Element<'a, Message> {
    let title_field = enz_text_field(
        crate::l10n::tr("Title"),
        crate::l10n::tr("Event title"),
        &draft.title,
        Message::QuickAddTitle,
    )
    .id(quick_add_title_id())
    .on_submit(Message::QuickAddSave);

    let fmt = app.time_format();
    let drafts = &app.time_drafts;
    let mut start_field = enz_time_field(
        &drafts.start,
        draft.start_minute.to_time_of_day(),
        Message::QuickAddStartTime,
    )
    .format(fmt.clone())
    .aria_label(crate::l10n::tr("Start time"))
    .id(quick_add_start_time_id());
    if let Some(err) = &drafts.start_error {
        start_field = start_field.error(err.clone());
    }
    let mut end_field = enz_time_field(
        &drafts.end,
        draft.end_minute.to_time_of_day(),
        Message::QuickAddEndTime,
    )
    .format(fmt)
    .aria_label(crate::l10n::tr("End time"))
    .id(quick_add_end_time_id());
    if let Some(err) = &drafts.end_error {
        end_field = end_field.error(err.clone());
    }
    let start_el: Element<'_, Message> = start_field.into();
    let end_el: Element<'_, Message> = end_field.into();
    let time_row = row![
        start_el,
        enz_text(crate::l10n::tr("to"))
            .size(TextSize::Caption)
            .tone(TextTone::Muted),
        end_el,
    ]
    .mirrored(crate::l10n::rtl())
    .spacing(SpaceToken::Sm.px())
    .align_y(Alignment::Center);
    let duration_row = enz_text(crate::time_entry::duration_label(draft))
        .size(TextSize::Caption)
        .tone(TextTone::Subtle);

    let actions = row![
        enz_button(
            crate::l10n::tr("More details"),
            Message::QuickAddMoreDetails
        )
        .id(iced::advanced::widget::Id::new(
            "calendar-quick-add-details"
        ))
        .kind(ButtonKind::Quiet)
        .size(ButtonSize::Sm),
        Space::new().width(Length::Fill).height(Length::Shrink),
        enz_button(crate::l10n::tr("Add"), Message::QuickAddSave)
            .id(iced::advanced::widget::Id::new("calendar-quick-add-save"))
            .kind(ButtonKind::Accent)
            .size(ButtonSize::Sm),
    ]
    .mirrored(crate::l10n::rtl())
    .align_y(Alignment::Center);

    let body = column![
        enz_text(crate::l10n::tr("Quick add event"))
            .size(TextSize::Caption)
            .tone(TextTone::Subtle),
        title_field,
        time_row,
        duration_row,
        actions,
    ]
    .spacing(SpaceToken::Sm.px());

    enzui_widgets::card::enz_card(body)
        .padding(SpaceToken::Sm.px())
        .tone(CardTone::Elevated)
        .width_length(Length::Fixed(POPOVER_W_PX))
        .into()
}

use enzui_widgets::context_menu_host::ContextMenuItem;

fn menu_item(label: impl Into<String>, message: Message) -> ContextMenuItem<Message> {
    let icon = match &message {
        Message::OpenOccurrenceEditor { .. } | Message::OpenEditor(_) => "pencil",
        Message::DuplicateEvent(_) => "copy",
        Message::DeleteOccurrence { .. } | Message::RequestDeleteEvent(_) => "trash-2",
        Message::NewEventAt(..) => "plus",
        Message::OpenDayView(_) | Message::JumpTo(_) => "calendar",
        Message::ToggleCalendar(_) => "eye",
        _ => "calendar",
    };
    ContextMenuItem::command(message, label).icon(icon)
}

#[must_use]
pub(crate) fn menu_items(app: &AppState, target: &MenuTarget) -> Vec<ContextMenuItem<Message>> {
    match target {
        MenuTarget::Event { id, on } => {
            let delete_msg = match on {
                Some(date) => Message::DeleteOccurrence { id: *id, on: *date },
                None => Message::RequestDeleteEvent(*id),
            };
            let edit_msg = match on {
                Some(date) => Message::OpenOccurrenceEditor { id: *id, on: *date },
                None => Message::OpenEditor(*id),
            };
            vec![
                menu_item(crate::l10n::tr("Edit"), edit_msg),
                menu_item(crate::l10n::tr("Duplicate"), Message::DuplicateEvent(*id)),
                menu_item(crate::l10n::tr("Delete"), delete_msg).destructive(true),
            ]
        }
        MenuTarget::Slot { date, minute } => {
            vec![menu_item(
                crate::l10n::tr("New Event here"),
                Message::NewEventAt(*date, *minute),
            )]
        }
        MenuTarget::Day(date) => vec![
            menu_item(
                crate::l10n::tr("New Event"),
                Message::NewEventAt(*date, app.settings.work_start_minute),
            ),
            menu_item(
                crate::l10n::tr("Open this day"),
                Message::OpenDayView(*date),
            )
            .enabled(!already_showing_day(app, *date)),
        ],
        MenuTarget::CalendarRow(id) => {
            let visible = app
                .calendars
                .iter()
                .find(|c| c.id == *id)
                .is_some_and(|c| c.visible);
            vec![
                menu_item(
                    crate::l10n::tr(if visible { "Hide" } else { "Show" }),
                    Message::ToggleCalendar(id.clone()),
                )
                .icon(if visible { "eye-off" } else { "eye" }),
            ]
        }
        MenuTarget::MiniDay(date) => vec![
            menu_item(crate::l10n::tr("Jump to date"), Message::JumpTo(*date))
                .enabled(!already_showing(app, *date)),
            menu_item(
                crate::l10n::tr("New Event"),
                Message::NewEventAt(*date, app.settings.work_start_minute),
            ),
        ],
    }
}

#[must_use]
fn already_showing(app: &AppState, date: Date) -> bool {
    app.focus == date && app.mini_month() == enzui_core::civil_date::YearMonth::of(date)
}

#[must_use]
fn already_showing_day(app: &AppState, date: Date) -> bool {
    app.view == crate::model::View::Day && already_showing(app, date)
}

#[must_use]
pub(crate) fn menu_invoker_id(target: &MenuTarget) -> Option<iced::advanced::widget::Id> {
    match target {
        MenuTarget::CalendarRow(id) => Some(sidebar_calendar_row_id(id)),
        MenuTarget::Event { id, on } => on.map(|date| event_row_id(*id, date)),
        MenuTarget::Day(date) => Some(month_day_cell_id(*date)),
        MenuTarget::MiniDay(_) | MenuTarget::Slot { .. } => None,
    }
}

#[must_use]
pub(crate) fn month_day_cell_id(date: Date) -> iced::advanced::widget::Id {
    enzui_core::identity::derive_id("calendar.month.day", &format!("{}", date.to_days()))
}

#[must_use]
pub(crate) fn year_day_cell_id(month: YearMonth, date: Date) -> iced::advanced::widget::Id {
    enzui_core::identity::derive_id(
        "calendar.year.day",
        &format!("{}-{}:{}", month.year(), month.month(), date.to_days()),
    )
}

#[must_use]
pub(crate) fn settings_button_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("calendar.titlebar.settings")
}

#[must_use]
pub(crate) fn sidebar_calendar_row_id(calendar_id: &str) -> iced::advanced::widget::Id {
    enzui_core::identity::derive_id("calendar.sidebar", calendar_id)
}

#[must_use]
pub(crate) fn event_row_id(id: u64, date: Date) -> iced::advanced::widget::Id {
    enzui_core::identity::derive_id("calendar.event", &format!("{id}@{}", date.to_days()))
}

pub(crate) const CONTEXT_MENU_LABEL: &str = "Actions";
