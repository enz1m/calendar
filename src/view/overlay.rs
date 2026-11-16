//! Editor, search, settings, confirmation and progress surfaces.

use enzui_core::civil_date::{Date, YearMonth};
use enzui_core::semantics::HeadingLevel;
use enzui_theme::{SizingTokens, SpaceToken, StrokeToken};
use enzui_widgets::button::{ButtonKind, ButtonSize, enz_button};
use enzui_widgets::capped_dialog_body::enz_capped_dialog_body;
use enzui_widgets::card::CardTone;
use enzui_widgets::color_swatch::{enz_color_swatch, enz_color_swatch_none};
use enzui_widgets::combo_box::enz_combo_box;
use enzui_widgets::date_picker::enz_date_picker;
use enzui_widgets::enz_text::{TextSize, TextTone, enz_text};
use enzui_widgets::form_layout::enz_form_layout;
use enzui_widgets::list_row_surface::enz_list_row_surface;
use enzui_widgets::modal_host::SurfacePlacement;
use enzui_widgets::modal_surface::{
    CLOSE_CONTROL_SIZE, CLOSE_INSET, close_inset, enz_modal_surface,
};
use enzui_widgets::preferences_group::enz_preferences_group;
use enzui_widgets::preferences_row::enz_preferences_row;
use enzui_widgets::scroll_view::enz_scroll_view;
use enzui_widgets::search_field::enz_search_field;
use enzui_widgets::settings_dialog::{
    DialogSize, EnzSettingsPresentation, SettingsPage, enz_settings_dialog,
};
use enzui_widgets::shortcuts_dialog::enz_shortcuts_dialog;
use enzui_widgets::stepper::enz_stepper;
use enzui_widgets::text_field::{TextFieldSize, enz_text_field};
use enzui_widgets::time_field::enz_time_field;
use enzui_widgets::toggle::enz_toggle;
use enzui_widgets::tooltip::enz_tooltip;
use iced::widget::{Space, column, container, row};
use iced::{Alignment, Border, Element, Length};

const FORM_LABEL_COL_W_PX: f32 = 90.0;
const FORM_VALUE_COL_W_PX: f32 = 176.0;

const SEARCH_VIEWPORT_H_PX: f32 = 260.0;

const SEARCH_STRIPE_W_PX: f32 = 3.0;

const SEARCH_STRIPE_H_PX: f32 = 20.0;

const DIALOG_CONTENT_MAX_H_PX: f32 = 600.0;

use crate::message::Message;
use crate::model::{
    ALERT_CHOICES, COLOR_KEYS, Event, Recurrence, RecurrenceEndKind, TimeFormatPref, alert_label,
    date_label, palette_color,
};
use crate::state::{
    AppState, DateField, MenuTarget, Overlay, PendingImport, PendingRecurrenceChoice,
    RecurrenceActionKind, SettingsTab,
};

#[must_use]
pub(crate) fn editor_date_text(date: enzui_core::civil_date::Date) -> String {
    crate::l10n::date(date, false)
}

fn date_picker_row<'a>(
    app: &'a AppState,
    label: &'static str,
    field: DateField,
    value: Date,
) -> Element<'a, Message> {
    let state = app.editor_pickers.get(field);
    let caption = container(
        enz_text(crate::l10n::tr(label))
            .size(TextSize::Caption)
            .tone(TextTone::Muted),
    )
    .width(Length::Fixed(FORM_LABEL_COL_W_PX));
    let picker = enz_date_picker(Some(value), state, move |ev| {
        Message::EditorDatePicker(field, ev)
    })
    // The picker retains its accessible name while the surrounding row supplies its visual caption.
    .label(crate::l10n::tr(label))
    .label_visible(false)
    .value_text(editor_date_text(value))
    .today(Some(app.today))
    .id(state.trigger_id())
    .width(Length::Fixed(FORM_VALUE_COL_W_PX));

    row![caption, picker]
        .mirrored(crate::l10n::rtl())
        .spacing(SpaceToken::Xs.px())
        .align_y(Alignment::Center)
        .into()
}

pub(crate) const WEEK_START_LABELS: &[&str] = &[
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

pub(crate) const TIME_FORMAT_LABELS: &[&str] = &[
    TimeFormatPref::System.label(),
    TimeFormatPref::TwentyFourHour.label(),
    TimeFormatPref::TwelveHour.label(),
];

#[must_use]
pub(crate) fn time_format_index(pref: TimeFormatPref) -> usize {
    TimeFormatPref::ALL
        .iter()
        .position(|p| *p == pref)
        .unwrap_or(0)
}

pub(crate) const RECURRENCE_END_LABELS: &[&str] = &[
    RecurrenceEndKind::Never.label(),
    RecurrenceEndKind::OnDate.label(),
    RecurrenceEndKind::AfterCount.label(),
];

#[must_use]
pub(crate) fn week_start_index(day: enzui_core::civil_date::Weekday) -> usize {
    day.index()
}

#[must_use]
pub(crate) fn modal_host_label(overlay: &Overlay) -> Option<&'static str> {
    match overlay {
        Overlay::Settings => Some(SETTINGS_TITLE),
        Overlay::Shortcuts => Some("Keyboard Shortcuts"),
        Overlay::None
        | Overlay::Editor(_)
        | Overlay::Details { .. }
        | Overlay::QuickAdd(_)
        | Overlay::Search
        | Overlay::RecurrencePrompt(_)
        | Overlay::DeleteConfirm(_)
        | Overlay::DayEvents(_)
        | Overlay::ReminderAlert { .. }
        | Overlay::ImportDestination(_)
        | Overlay::RetireRecurrenceConfirm(_)
        | Overlay::Importing { .. }
        | Overlay::Booting { .. } => None,
    }
}

mod calendar_settings;
#[path = "overlay/editor.rs"]
mod editor;
#[path = "overlay/popovers.rs"]
mod popovers;
#[path = "overlay/settings.rs"]
mod settings;

use day_events::day_events;
use editor::editor;
use popovers::delete_confirm;
pub(crate) use popovers::{
    CONTEXT_MENU_LABEL, event_row_id, menu_invoker_id, menu_items, month_day_cell_id, popover,
    settings_button_id, sidebar_calendar_row_id, year_day_cell_id,
};
pub(crate) use settings::settings;

#[path = "overlay/day_events.rs"]
mod day_events;

pub(crate) fn calendar_dialog(
    app: &AppState,
    key: crate::calendar_dialogs::CalendarDialog,
) -> Element<'_, Message> {
    use crate::calendar_dialogs::CalendarDialog;
    let Some(draft) = &app.calendar_editor else {
        return iced::widget::Space::new().into();
    };
    let removing = key == CalendarDialog::CalendarRemoval;
    enz_modal_surface(calendar_settings::editor(app, draft, removing))
        .width(Length::Fixed(520.0))
        .on_dismiss(move || {
            Message::Calendar(if removing {
                crate::calendars::Action::Back
            } else {
                crate::calendars::Action::Cancel
            })
        })
        .into()
}

pub(crate) fn overlay<'a>(
    app: &'a AppState,
    theme: &enzui_theme::EnzTheme,
) -> Option<Element<'a, Message>> {
    match &app.overlay {
        Overlay::None => None,
        Overlay::Editor(e) => Some(editor(app, e, theme)),
        Overlay::Search => Some(search(app)),
        Overlay::Shortcuts => Some(shortcuts()),
        Overlay::Settings => Some(settings(app)),
        Overlay::DayEvents(date) => Some(day_events(app, *date)),
        Overlay::RecurrencePrompt(p) => Some(recurrence_prompt(p)),
        Overlay::DeleteConfirm(p) => Some(delete_confirm(app, p)),
        Overlay::ImportDestination(p) => Some(import_destination(app, p)),
        Overlay::Importing { .. } => Some(importing()),
        Overlay::Booting { .. } => Some(booting()),
        Overlay::RetireRecurrenceConfirm(e) => Some(retire_recurrence_confirm(e)),
        Overlay::ReminderAlert { title, when, .. } => Some(reminder_alert(title, when)),
        Overlay::Details { .. } | Overlay::QuickAdd(_) => None,
    }
}

fn shortcuts() -> Element<'static, Message> {
    enz_shortcuts_dialog::<Message>(true, crate::shortcuts::shortcut_entries())
        .title(crate::l10n::tr("Keyboard Shortcuts"))
        .into_surface(Some(Box::new(|| Message::CloseOverlay)))
}

fn dialog<'a>(
    title: impl Into<String>,
    body: Element<'a, Message>,
    footer: Option<Element<'a, Message>>,
    width: f32,
) -> Element<'a, Message> {
    dialog_surface(
        title,
        body,
        footer,
        width,
        Some(|| Message::CloseOverlay),
        None,
    )
}

fn dialog_surface<'a>(
    title: impl Into<String>,
    body: Element<'a, Message>,
    footer: Option<Element<'a, Message>>,
    width: f32,
    dismiss: Option<fn() -> Message>,
    background: Option<iced::Color>,
) -> Element<'a, Message> {
    let inset = close_inset(enzui_theme::text_defaults::shape_radius(|s| s.dialog));
    let head = enzui_widgets::title_bar::enz_title_bar()
        .title(crate::l10n::tr(title.into()))
        .title_heading(HeadingLevel::Page)
        .height(CLOSE_CONTROL_SIZE)
        .padding_x(0.0)
        .tone(if background.is_some() {
            enzui_widgets::surface::SurfaceTone::Surface
        } else {
            enzui_widgets::surface::SurfaceTone::Elevated
        })
        .into_element();

    let head = container(head).padding(iced::Padding {
        right: if dismiss.is_some() {
            CLOSE_CONTROL_SIZE + inset
        } else {
            0.0
        },
        ..iced::Padding::ZERO
    });

    let below = footer.into_iter().collect();
    let content = enz_capped_dialog_body(
        vec![head.into()],
        body,
        below,
        SpaceToken::Md.px(),
        DIALOG_CONTENT_MAX_H_PX,
    );
    let surface = enz_modal_surface(content.into_element())
        .width(Length::Fixed(width))
        .padding(SpaceToken::Md.px())
        .padding_top(inset);
    let surface = match background {
        Some(color) => surface.background(color),
        None => surface,
    };
    match dismiss {
        Some(dismiss) => surface.on_dismiss(dismiss).into(),
        None => surface.into(),
    }
}

pub(crate) const SEARCH_FIELD_ID: &str = "calendar-search-field";

fn search(app: &AppState) -> Element<'_, Message> {
    let hits = app.search(&app.search_query);
    let mut body = column![
        enz_search_field(&app.search_query, Message::SearchChanged)
            .placeholder(crate::l10n::tr("Search events…"))
            .size(TextFieldSize::Sm)
            .focus_on_mount(true)
            .id(iced::advanced::widget::Id::new(SEARCH_FIELD_ID))
    ]
    .spacing(SpaceToken::Sm.px());
    if app.search_query.trim().is_empty() {
        body = body.push(
            enz_text(crate::l10n::tr(
                "Type to search event titles, locations, and notes.",
            ))
            .size(TextSize::Caption)
            .tone(TextTone::Subtle),
        );
    } else if hits.is_empty() {
        body = body.push(
            enz_text(crate::l10n::format(
                "No events match \"{}\"",
                &[("arg0", (app.search_query.trim()).to_owned().into())],
            ))
            .size(TextSize::Caption)
            .tone(TextTone::Muted),
        );
    } else {
        let mut list = column![].spacing(SpaceToken::Custom(3.0).px());
        for e in hits.iter().take(50) {
            let color = e.color(&app.calendars);
            let date_line = crate::view::search_result_date_label(e, app.today);
            let content = row![
                container(
                    Space::new()
                        .width(Length::Fixed(SEARCH_STRIPE_W_PX))
                        .height(Length::Fixed(SEARCH_STRIPE_H_PX))
                )
                .style(move |_| container::Style {
                    background: Some(color.into()),
                    border: Border {
                        radius: 1.5.into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                }),
                column![
                    enz_text(e.title.clone()).size(TextSize::Footnote),
                    enz_text(format!(
                        "{} · {}",
                        date_line,
                        e.time_label(&app.time_format())
                    ))
                    .size(TextSize::Caption2)
                    .tone(TextTone::Muted),
                ]
                .spacing(StrokeToken::Hairline.px()),
            ]
            .mirrored(crate::l10n::rtl())
            .spacing(SpaceToken::Space3.px())
            .align_y(Alignment::Center);
            list = list.push(
                enz_list_row_surface(content, Message::JumpToEvent(e.id))
                    .label(e.title.clone())
                    .width(Length::Fill),
            );
        }
        body = body.push(enz_scroll_view(list).height(Length::Fixed(SEARCH_VIEWPORT_H_PX)));
    }
    dialog(crate::l10n::tr("Search"), body.into(), None, 440.0)
}

pub(crate) const SETTINGS_TITLE: &str = "Settings";

pub(crate) const SETTINGS_ID_PREFIX: &str = "calendar-app-settings";

#[must_use]
pub(crate) fn settings_presentation(app: &AppState) -> EnzSettingsPresentation {
    enzui_widgets::settings_dialog::enz_settings_presentation_for(app.window_size())
}

#[must_use]
pub(crate) fn settings_placement(app: &AppState) -> SurfacePlacement {
    match settings_presentation(app) {
        EnzSettingsPresentation::Sheet => SurfacePlacement::BottomEdge,
        EnzSettingsPresentation::Dialog | EnzSettingsPresentation::Adaptive => {
            SurfacePlacement::Centred
        }
    }
}

#[must_use]
pub(crate) fn overlay_placement(app: &AppState) -> SurfacePlacement {
    match app.overlay {
        Overlay::Settings => settings_placement(app),
        _ => SurfacePlacement::Centred,
    }
}

#[must_use]
pub(crate) fn recurrence_prompt_title(kind: RecurrenceActionKind) -> &'static str {
    match kind {
        RecurrenceActionKind::Delete => "Delete Recurring Event",
        RecurrenceActionKind::Edit => "Edit Recurring Event",
    }
}

fn recurrence_prompt<'a>(p: &'a PendingRecurrenceChoice) -> Element<'a, Message> {
    let title = recurrence_prompt_title(p.kind);
    let body = column![
        enz_text(p.title.clone()).size(TextSize::Headline),
        enz_text(crate::l10n::tr(
            "This is a recurring event. What would you like to change?"
        ))
        .size(TextSize::Callout)
        .tone(TextTone::Muted),
        enz_button(crate::l10n::tr("This Event"), Message::RecurrenceChoiceThis)
            .id(iced::advanced::widget::Id::new("calendar-recurrence-this"))
            .kind(ButtonKind::Secondary)
            .size(ButtonSize::Md)
            .width(Length::Fill),
        enz_button(
            crate::l10n::tr("This and Following"),
            Message::RecurrenceChoiceFollowing
        )
        .id(iced::advanced::widget::Id::new(
            "calendar-recurrence-following"
        ))
        .kind(ButtonKind::Secondary)
        .size(ButtonSize::Md)
        .width(Length::Fill),
        enz_button(crate::l10n::tr("All Events"), Message::RecurrenceChoiceAll)
            .id(iced::advanced::widget::Id::new("calendar-recurrence-all"))
            .kind(ButtonKind::Accent)
            .size(ButtonSize::Md)
            .width(Length::Fill),
    ]
    .spacing(SpaceToken::Sm.px());
    let footer = row![
        Space::new().width(Length::Fill).height(Length::Shrink),
        enz_button(crate::l10n::tr("Cancel"), Message::CloseOverlay)
            .id(iced::advanced::widget::Id::new("calendar-overlay-cancel"))
            .kind(ButtonKind::Quiet)
            .size(ButtonSize::Sm),
    ]
    .mirrored(crate::l10n::rtl());
    dialog(title, body.into(), Some(footer.into()), 400.0)
}

fn import_destination<'a>(app: &'a AppState, pending: &'a PendingImport) -> Element<'a, Message> {
    let count = pending.parsed.events.len();
    let mut body = column![
        enz_text(crate::l10n::count("calendar-import-count", count))
            .size(TextSize::Callout)
            .tone(TextTone::Muted),
    ]
    .spacing(SpaceToken::Sm.px());
    for cal in &app.calendars {
        body = body.push(
            enz_button(
                cal.name.clone(),
                Message::ChooseImportDestination(cal.id.clone()),
            )
            .id(enzui_core::identity::derive_id(
                "calendar.import.destination",
                &cal.id,
            ))
            .kind(ButtonKind::Secondary)
            .size(ButtonSize::Md)
            .width(Length::Fill),
        );
    }
    let footer = row![
        Space::new().width(Length::Fill).height(Length::Shrink),
        enz_button(crate::l10n::tr("Cancel"), Message::CloseOverlay)
            .id(iced::advanced::widget::Id::new("calendar-overlay-cancel"))
            .kind(ButtonKind::Quiet)
            .size(ButtonSize::Sm),
    ]
    .mirrored(crate::l10n::rtl());
    dialog(
        crate::l10n::tr("Import Events"),
        body.into(),
        Some(footer.into()),
        400.0,
    )
}

// Cancelled import generations cannot admit late worker completions.
fn importing<'a>() -> Element<'a, Message> {
    let body = column![
        enz_text(crate::l10n::tr("Reading the selected calendar file…"))
            .size(TextSize::Callout)
            .tone(TextTone::Muted),
    ]
    .spacing(SpaceToken::Sm.px());
    let footer = row![
        Space::new().width(Length::Fill).height(Length::Shrink),
        enz_button(crate::l10n::tr("Cancel"), Message::CloseOverlay)
            .id(iced::advanced::widget::Id::new("calendar-overlay-cancel"))
            .kind(ButtonKind::Quiet)
            .size(ButtonSize::Sm),
    ]
    .mirrored(crate::l10n::rtl());
    dialog(
        crate::l10n::tr("Importing Events"),
        body.into(),
        Some(footer.into()),
        380.0,
    )
}

fn booting<'a>() -> Element<'a, Message> {
    let body = column![
        enz_text(crate::l10n::tr("Opening your calendar file…"))
            .size(TextSize::Callout)
            .tone(TextTone::Muted),
    ]
    .spacing(SpaceToken::Sm.px());
    dialog_surface(
        crate::l10n::tr("Opening Calendar"),
        body.into(),
        None,
        380.0,
        None,
        None,
    )
}

fn retire_recurrence_confirm(e: &Event) -> Element<'_, Message> {
    let title = if e.title.trim().is_empty() {
        "Change this repeat rule?".to_owned()
    } else {
        crate::l10n::format(
            "Change the repeat rule for “{}”?",
            &[("arg0", (e.title.trim()).to_owned().into())],
        )
    };
    let body = column![
        enz_text(
            crate::l10n::tr("This event uses an advanced repeat rule this app can't fully edit, so the original rule was kept exactly as written.")
        )
        .size(TextSize::Callout)
        .tone(TextTone::Muted),
        enz_text(
            crate::l10n::tr("Saving your change replaces it with a simpler rule the app can express. Any advanced parts of the original (for example BYSETPOS or extra weekdays) are dropped.")
        )
        .size(TextSize::Callout)
        .tone(TextTone::Muted),
    ]
    .spacing(SpaceToken::Sm.px());
    let footer = row![
        enz_button(crate::l10n::tr("Keep Original"), Message::CloseOverlay)
            .id(iced::advanced::widget::Id::new("calendar-overlay-cancel"))
            .kind(ButtonKind::Ghost)
            .size(ButtonSize::Sm),
        Space::new().width(Length::Fill).height(Length::Shrink),
        enz_button(
            crate::l10n::tr("Simplify & Save"),
            Message::ConfirmRetireRecurrence
        )
        .id(iced::advanced::widget::Id::new(
            "calendar-recurrence-simplify"
        ))
        .kind(ButtonKind::Accent)
        .size(ButtonSize::Sm),
    ]
    .mirrored(crate::l10n::rtl())
    .align_y(Alignment::Center);
    dialog(title, body.into(), Some(footer.into()), 420.0)
}

fn reminder_alert<'a>(title: &'a str, when: &'a str) -> Element<'a, Message> {
    let body = column![
        enz_text(title).size(TextSize::Headline),
        enz_text(when)
            .size(TextSize::Footnote)
            .tone(TextTone::Muted),
        row![
            enz_button(crate::l10n::tr("Snooze"), Message::SnoozeReminder)
                .id(iced::advanced::widget::Id::new("calendar-reminder-snooze"))
                .kind(ButtonKind::Secondary)
                .size(ButtonSize::Md)
                .width(Length::Fill),
            enz_button(crate::l10n::tr("Dismiss"), Message::DismissReminder)
                .id(iced::advanced::widget::Id::new("calendar-reminder-dismiss"))
                .kind(ButtonKind::Accent)
                .size(ButtonSize::Md)
                .width(Length::Fill),
        ]
        .mirrored(crate::l10n::rtl())
        .spacing(SpaceToken::Sm.px()),
    ]
    .spacing(SpaceToken::Sm.px());
    dialog(crate::l10n::tr("Reminder"), body.into(), None, 360.0)
}

pub(crate) const POPOVER_W_PX: f32 = 260.0;

pub(crate) fn editor_recurrence_count_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("calendar-recurrence-count")
}

#[must_use]
pub(crate) fn editor_start_time_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("calendar.editor.start-time")
}

#[must_use]
pub(crate) fn editor_end_time_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("calendar.editor.end-time")
}

#[must_use]
pub(crate) fn quick_add_start_time_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("calendar.quick-add.start-time")
}

#[must_use]
pub(crate) fn quick_add_end_time_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("calendar.quick-add.end-time")
}

#[must_use]
pub(crate) fn quick_add_title_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("calendar.quickadd.title")
}
