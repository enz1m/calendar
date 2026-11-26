//! Event editor dialog and its chrome.

use super::*;
use enzui_widgets::stepper::enz_stepper_u32;

pub(super) fn editor<'a>(
    app: &'a AppState,
    e: &'a Event,
    theme: &enzui_theme::EnzTheme,
) -> Element<'a, Message> {
    let existing = app.event(e.id).is_some();

    let mut fields: Vec<Element<'a, Message>> = vec![
        enz_text_field(
            crate::l10n::tr("Title"),
            crate::l10n::tr("Event title"),
            &e.title,
            Message::EditTitle,
        )
        .into(),
    ];
    fields.push(
        enz_text_field(
            crate::l10n::tr("Location"),
            crate::l10n::tr("Add Location"),
            &e.location,
            Message::EditLocation,
        )
        .into(),
    );
    fields.push(enz_toggle(e.all_day, crate::l10n::tr("All Day"), Message::EditAllDay).into());

    // Picker Escape is consumed before the surrounding dialog can dismiss.
    fields.push(date_picker_row(app, "Starts", DateField::Start, e.date));
    fields.push(date_picker_row(app, "Ends", DateField::End, e.end_date));

    if !e.all_day {
        // Uncommitted time text is retained separately from the event's committed minute values.
        let fmt = app.entry_time_format();
        let drafts = &app.time_drafts;
        let starts_label: Element<'_, Message> = container(
            enz_text(crate::l10n::tr("Time"))
                .size(TextSize::Caption)
                .tone(TextTone::Muted),
        )
        .width(Length::Fixed(FORM_LABEL_COL_W_PX))
        .into();
        let to_label: Element<'_, Message> = container(
            enz_text(crate::l10n::tr("to"))
                .size(TextSize::Caption)
                .tone(TextTone::Muted),
        )
        .width(Length::Fixed(FORM_LABEL_COL_W_PX))
        .into();

        let mut start_field = enz_time_field(
            &drafts.start,
            e.start_minute.to_time_of_day(),
            Message::EditStartTime,
        )
        .format(fmt.clone())
        .aria_label(crate::l10n::tr("Start time"))
        .id(editor_start_time_id());
        if let Some(err) = &drafts.start_error {
            start_field = start_field.error(err.clone());
        }
        let mut end_field = enz_time_field(
            &drafts.end,
            e.end_minute.to_time_of_day(),
            Message::EditEndTime,
        )
        .format(fmt)
        .aria_label(crate::l10n::tr("End time"))
        .id(editor_end_time_id());
        if let Some(err) = &drafts.end_error {
            end_field = end_field.error(err.clone());
        }

        let duration_label: Element<'_, Message> = enz_text(crate::time_entry::duration_label(e))
            .size(TextSize::Caption)
            .tone(TextTone::Subtle)
            .into();

        let to_desktop: Element<'_, Message> = enz_text(crate::l10n::tr("to"))
            .size(TextSize::Caption)
            .tone(TextTone::Muted)
            .into();
        let start_el: Element<'_, Message> = start_field.into();
        let end_el: Element<'_, Message> = end_field.into();

        let time_row: Element<'_, Message> =
            if app.content_density() == crate::layout::Density::Phone {
                iced::widget::column![
                    row![starts_label, start_el]
                        .mirrored(crate::l10n::rtl())
                        .spacing(SpaceToken::Xs.px())
                        .align_y(Alignment::Center),
                    row![to_label, end_el]
                        .mirrored(crate::l10n::rtl())
                        .spacing(SpaceToken::Xs.px())
                        .align_y(Alignment::Center),
                    duration_label,
                ]
                .spacing(SpaceToken::Xs.px())
                .into()
            } else {
                row![
                    starts_label,
                    start_el,
                    to_desktop,
                    end_el,
                    Space::new()
                        .width(Length::Fixed(SpaceToken::Space5.px()))
                        .height(Length::Shrink),
                    duration_label,
                ]
                .mirrored(crate::l10n::rtl())
                .spacing(SpaceToken::Xs.px())
                .align_y(Alignment::Center)
                .into()
            };
        fields.push(time_row);
    }

    // Calendar selection carries stable identities so duplicate names remain distinct.
    let cals = &app.calendars;
    fields.push(
        enz_combo_box(
            crate::l10n::tr("Calendar"),
            cals.iter().map(|c| c.name.clone()).collect(),
            None,
            |_| Message::EditCalendar(String::new()),
        )
        .selected_index(cals.iter().position(|c| c.id == e.calendar_id))
        .on_select_index(move |index| Message::EditCalendar(cals[index].id.clone()))
        .into(),
    );

    fields.push(
        enz_combo_box(
            crate::l10n::tr("Repeat"),
            Recurrence::ALL
                .iter()
                .map(|r| crate::l10n::tr(r.label()))
                .collect(),
            None,
            |_| Message::EditRecurrence(Recurrence::None),
        )
        .selected_index(Recurrence::ALL.iter().position(|r| *r == e.recurrence))
        .on_select_index(|index| Message::EditRecurrence(Recurrence::ALL[index]))
        .into(),
    );

    // Recurrence radio choices and RecurrenceEndKind indices must keep the same order.
    if e.recurrence != Recurrence::None {
        let end_kind = e.recurrence_end_kind();
        fields.push(
            enz_combo_box(
                crate::l10n::tr("End repeat"),
                RECURRENCE_END_LABELS.iter().map(crate::l10n::tr).collect(),
                None,
                |_| Message::EditRecurrenceEndKind(RecurrenceEndKind::Never),
            )
            .selected_index(Some(end_kind.index()))
            .on_select_index(|index| {
                Message::EditRecurrenceEndKind(RecurrenceEndKind::from_index(index))
            })
            .into(),
        );
        if let Some(until) = e.recurrence_until {
            fields.push(date_picker_row(app, "Until", DateField::Until, until));
        }
        if let Some(count) = e.recurrence_count {
            fields.push(
                row![
                    container(
                        enz_text(crate::l10n::tr("Times"))
                            .size(TextSize::Caption)
                            .tone(TextTone::Muted)
                    )
                    .width(Length::Fixed(FORM_LABEL_COL_W_PX)),
                    enz_stepper_u32(count, Message::EditRecurrenceCount)
                        .aria_label(crate::l10n::tr("Times"))
                        .id(editor_recurrence_count_id())
                        .min(1)
                        .max(u32::MAX)
                        .step(1)
                        .value_text(crate::l10n::count(
                            "calendar-recurrence-times",
                            count as usize
                        )),
                ]
                .mirrored(crate::l10n::rtl())
                .spacing(SpaceToken::Space3.px())
                .align_y(Alignment::Center)
                .into(),
            );
        }
    }

    fields.push(
        enz_combo_box(
            crate::l10n::tr("Alert"),
            ALERT_CHOICES.iter().map(|a| alert_label(*a)).collect(),
            Some(alert_label(e.alert)),
            |label| {
                Message::EditAlert(
                    ALERT_CHOICES
                        .iter()
                        .copied()
                        .find(|a| alert_label(*a) == label)
                        .unwrap_or(None),
                )
            },
        )
        .into(),
    );

    // Color swatches include their pointer target, which must retain the 24 px minimum.
    let swatch_label = container(
        enz_text(crate::l10n::tr("Colors"))
            .size(TextSize::Footnote)
            .tone(TextTone::Muted),
    )
    .width(Length::Fixed(FORM_LABEL_COL_W_PX));
    let mut swatches = row![]
        .mirrored(crate::l10n::rtl())
        .spacing(SpaceToken::Xxs.px())
        .align_y(Alignment::Center);
    swatches = swatches.push(enz_tooltip(
        enz_color_swatch_none(Message::EditColor(None))
            .aria_label(crate::l10n::tr(SWATCH_NONE_LABEL))
            .size(swatch_hit_px())
            .radius(swatch_hit_px() / 2.0)
            .selected(e.color_category.is_none())
            .into_element(),
        crate::l10n::tr("Calendar Default"),
        iced::widget::tooltip::Position::Bottom,
    ));
    for key in COLOR_KEYS {
        let selected = e.color_category.as_deref() == Some(key);
        swatches = swatches.push(
            enz_color_swatch(palette_color(key), Message::EditColor(Some(key.to_owned())))
                .aria_label(color_key_label(key))
                .size(swatch_hit_px())
                .radius(swatch_hit_px() / 2.0)
                .selected(selected)
                .into_element(),
        );
    }
    fields.push(
        row![
            swatch_label,
            swatches.wrap().vertical_spacing(SpaceToken::Xxs.px()),
        ]
        .mirrored(crate::l10n::rtl())
        .spacing(SpaceToken::Xxs.px())
        .align_y(Alignment::Start)
        .into(),
    );

    fields.push(
        enz_text_field(
            crate::l10n::tr("URL"),
            crate::l10n::tr("Add URL"),
            &e.url,
            Message::EditUrl,
        )
        .input_purpose(iced::advanced::input_method::Purpose::Url)
        .into(),
    );
    fields.push(
        enz_text_field(
            crate::l10n::tr("Invitees"),
            crate::l10n::tr("Add Invitees (comma-separated)"),
            &e.invitees,
            Message::EditInvitees,
        )
        .input_purpose(iced::advanced::input_method::Purpose::Email)
        .into(),
    );
    fields.push(
        enz_text_field(
            crate::l10n::tr("Notes"),
            crate::l10n::tr("Add Notes…"),
            &e.notes,
            Message::EditNotes,
        )
        .into(),
    );

    let mut footer = row![Space::new().width(Length::Fill).height(Length::Shrink)]
        .mirrored(crate::l10n::rtl())
        .spacing(SpaceToken::Sm.px());
    if existing {
        footer = footer.push(
            enz_button(crate::l10n::tr("Delete"), Message::RequestDeleteEvent(e.id))
                .id(iced::advanced::widget::Id::new("event-edit-delete"))
                .kind(ButtonKind::Destructive)
                .size(ButtonSize::Sm),
        );
    }
    footer = footer
        .push(
            enz_button(crate::l10n::tr("Cancel"), Message::CloseOverlay)
                .id(iced::advanced::widget::Id::new("event-edit-cancel"))
                .kind(ButtonKind::Quiet)
                .size(ButtonSize::Sm),
        )
        .push(
            enz_button(
                crate::l10n::tr(if existing { "Save" } else { "Add Event" }),
                Message::SaveEvent,
            )
            .id(iced::advanced::widget::Id::new("event-edit-save"))
            .kind(ButtonKind::Accent)
            .size(ButtonSize::Sm),
        );

    dialog_surface(
        if existing { "Edit Event" } else { "New Event" },
        enz_scroll_view(
            enz_form_layout(fields)
                .form_padding(0.0)
                .row_gap(SpaceToken::Sm),
        )
        .insets(0.0, super::dialog_scroll_gutter(), 0.0, 0.0)
        .height(Length::Shrink)
        .into(),
        Some(footer.align_y(Alignment::Center).into()),
        editor_dialog_width(),
        Some(|| Message::CloseOverlay),
        Some(theme.palette.surface),
        true,
    )
}

#[must_use]
pub(super) fn editor_dialog_width() -> f32 {
    SizingTokens::default().dialog_width_md
}

#[must_use]
pub(super) fn swatch_hit_px() -> f32 {
    SizingTokens::default().min_hit_target
}

pub(super) const SWATCH_NONE_LABEL: &str = "Calendar default colour";

#[must_use]
pub(super) fn color_key_label(key: &str) -> String {
    let mut c = key.chars();
    match c.next() {
        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
