//! Settings dialog and the calendars page.

use super::*;
use iced::widget::column;

pub(crate) fn settings(app: &AppState) -> Element<'_, Message> {
    let s = &app.settings;

    // Settings menu option order must match the stored indices.
    let week = {
        let idx = week_start_index(s.first_day_of_week);
        let options: Vec<(String, Message)> = WEEK_START_LABELS
            .iter()
            .enumerate()
            .map(|(i, _label)| {
                (
                    crate::model::weekday_full(enzui_core::civil_date::Weekday::ALL[i]),
                    Message::SetFirstDayOfWeek(i),
                )
            })
            .collect();
        enz_preferences_row(crate::l10n::tr("Week Start"))
            .value_menu(
                crate::model::weekday_full(enzui_core::civil_date::Weekday::ALL[idx]),
                app.settings_menu_open == Some(crate::state::SettingsMenu::WeekStart),
                Message::OpenSettingsMenu(crate::state::SettingsMenu::WeekStart),
                Message::CloseSettingsMenus,
                options,
            )
            .into_element()
    };

    let time_format = {
        let idx = time_format_index(s.time_format);
        let options: Vec<(String, Message)> = TIME_FORMAT_LABELS
            .iter()
            .enumerate()
            .map(|(i, label)| (crate::l10n::tr(label), Message::SetTimeFormat(i)))
            .collect();
        enz_preferences_row(crate::l10n::tr("Time Format"))
            .value_menu(
                crate::l10n::tr(TIME_FORMAT_LABELS[idx]),
                app.settings_menu_open == Some(crate::state::SettingsMenu::TimeFormat),
                Message::OpenSettingsMenu(crate::state::SettingsMenu::TimeFormat),
                Message::CloseSettingsMenus,
                options,
            )
            .into_element()
    };

    let hours = enz_preferences_group({
        let end = s.work_end_minute;
        let start = s.work_start_minute;
        let fmt = app.time_format();
        let (start_fmt, end_fmt) = (fmt.clone(), fmt.clone());
        row![
            enz_stepper(start, move |v| Message::SetWorkHours(v, end))
                .aria_label(crate::l10n::tr("Working hours start"))
                .id(iced::advanced::widget::Id::new("calendar-work-start"))
                .min(0)
                .max(23 * 60)
                .step(60)
                .value_text(crate::time_entry::work_hour_text(start, &fmt))
                .parse_value_text(move |text| {
                    crate::time_entry::parse_work_hour(text, &start_fmt, false)
                }),
            enz_text(crate::l10n::tr("to"))
                .size(TextSize::Caption)
                .tone(TextTone::Muted),
            enz_stepper(end, move |v| Message::SetWorkHours(start, v))
                .aria_label(crate::l10n::tr("Working hours end"))
                .id(iced::advanced::widget::Id::new("calendar-work-end"))
                .min(60)
                .max(24 * 60)
                .step(60)
                .value_text(crate::time_entry::work_hour_text(end, &fmt))
                .parse_value_text(move |text| {
                    crate::time_entry::parse_work_hour(text, &end_fmt, true)
                }),
        ]
        .mirrored(crate::l10n::rtl())
        .spacing(SpaceToken::Xs.px())
        .align_y(Alignment::Center)
    })
    .title(&crate::l10n::tr("Working Hours"));

    let tz_toggle = enz_preferences_row(crate::l10n::tr("Secondary timezone"))
        .subtitle(crate::l10n::tr(
            "The main ruler always reads in your local (system) timezone; this adds a second one.",
        ))
        .switch(s.show_secondary_tz, Message::SetShowSecondaryTz)
        .into_element();
    let tz: Element<'_, Message> = if s.show_secondary_tz {
        column![
            tz_toggle,
            row![
                container(
                    enz_text(crate::l10n::tr("Timezone"))
                        .size(TextSize::Caption)
                        .tone(TextTone::Muted),
                )
                .width(Length::Fixed(FORM_LABEL_COL_W_PX)),
                enz_stepper(s.secondary_tz_offset_minutes, Message::SetSecondaryTzOffset)
                    .aria_label(crate::l10n::tr("Secondary timezone offset"))
                    .id(iced::advanced::widget::Id::new("calendar-secondary-offset"))
                    .min(-12 * 60)
                    .max(14 * 60)
                    .step(60)
                    .value_text(crate::model::secondary_tz_label(
                        s.secondary_tz_offset_minutes,
                    )),
            ]
            .mirrored(crate::l10n::rtl())
            .spacing(SpaceToken::Xs.px())
            .align_y(Alignment::Center),
        ]
        .spacing(SpaceToken::Sm.px())
        .into()
    } else {
        tz_toggle
    };

    let week_numbers = enz_preferences_row(crate::l10n::tr("Week numbers"))
        .subtitle(crate::l10n::tr(
            "Show ISO week numbers in the Month grid and beside the Week and Day headings.",
        ))
        .switch(s.show_week_numbers, Message::SetShowWeekNumbers)
        .into_element();

    let reminders = enz_preferences_group(
        enz_text(crate::l10n::tr(if enzui_core::portal::sandbox::is_sandboxed(
            &enzui_core::portal::sandbox::SandboxEvidence::from_host(),
        ) {
            "Keep Calendar open to receive reminders in this sandboxed build. Reminders cannot wake the app after it closes."
        } else {
            "Calendar can schedule the next reminder while closed when your desktop supports it. \
             If background reminders are unavailable, keep Calendar open. \
             You can snooze or dismiss reminders in the app or desktop notifications."
        }))
        .size(TextSize::Caption)
        .tone(TextTone::Muted),
    )
    .title(&crate::l10n::tr("Reminders"));

    let body: Element<'_, Message> = column![hours, week, time_format, week_numbers, tz, reminders]
        .spacing(SpaceToken::Md.px())
        .into();

    let builder = enz_settings_dialog::<Message>(true, app.window_size())
        .title(crate::l10n::tr(SETTINGS_TITLE))
        .size(DialogSize::Compact)
        .presentation(EnzSettingsPresentation::Adaptive)
        .page(
            SettingsPage::new(
                SettingsTab::General.id(),
                crate::l10n::tr(SettingsTab::General.title()),
            ),
            body,
        )
        .page(
            SettingsPage::new(
                SettingsTab::Calendars.id(),
                crate::l10n::tr(SettingsTab::Calendars.title()),
            ),
            calendars_page(app),
        )
        .selected(app.settings_tab.id())
        .on_select(Message::SelectSettingsPage)
        .stacked_navigation(app.settings_page_open, Message::SettingsStackedBack)
        .page_id_prefix(SETTINGS_ID_PREFIX);

    builder.into_presented_surface(Some(Box::new(|| Message::CloseOverlay)))
}

fn calendars_page(app: &AppState) -> Element<'_, Message> {
    let mut list = column![].spacing(SpaceToken::Xxs.px());
    for c in &app.calendars {
        let id = c.id.clone();
        let edit_id = id.clone();
        list = list.push(
            enz_preferences_row(c.name.as_str())
                .subtitle(c.section.as_str())
                .suffix(
                    enz_button(
                        crate::l10n::tr("Edit"),
                        Message::Calendar(crate::calendars::Action::Edit(edit_id)),
                    )
                    .kind(ButtonKind::Quiet)
                    .id(iced::advanced::widget::Id::from(format!(
                        "calendar-edit-{}",
                        c.id
                    )))
                    .size(ButtonSize::Sm),
                )
                .switch(c.visible, move |_| Message::ToggleCalendar(id.clone()))
                .into_element(),
        );
    }
    let calendars = enz_preferences_group(list)
        .title(&crate::l10n::tr("Your calendars"))
        .description(&crate::l10n::tr(
            "Turn a calendar off to hide its events everywhere.",
        ))
        .into_element();

    let actions = enz_preferences_group(
        column![
            enz_preferences_row(crate::l10n::tr("Add calendar"))
                .subtitle(crate::l10n::tr(
                    "Create an empty calendar to file events under."
                ))
                .action(crate::l10n::tr("Add"), Message::AddCalendar)
                .into_element(),
            enz_preferences_row(crate::l10n::tr("Import events"))
                .subtitle(crate::l10n::tr(
                    "Read events from an .ics file into your calendars."
                ))
                .action(crate::l10n::tr("Import\u{2026}"), Message::ImportEvents)
                .into_element(),
            enz_preferences_row(crate::l10n::tr("Export events"))
                .subtitle(crate::l10n::tr(
                    "Save your visible calendars' events to an .ics file."
                ))
                .action(crate::l10n::tr("Export\u{2026}"), Message::ExportEvents)
                .into_element(),
        ]
        .spacing(SpaceToken::Xxs.px()),
    )
    .title(&crate::l10n::tr("Add, import and export"))
    .into_element();

    column![actions, calendars]
        .spacing(SpaceToken::Md.px())
        .into()
}
