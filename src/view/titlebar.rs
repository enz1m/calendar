//! Titlebar: controls, actions, and overflow.

use super::*;

pub(super) fn titlebar(app: &AppState) -> Element<'_, Message> {
    let density = app.content_density();

    let switcher: Element<'_, Message> = if density.switcher_is_dropdown() {
        let options: Vec<String> = View::ALL
            .iter()
            .map(|view| crate::l10n::tr(view.label()))
            .collect();
        enz_combo_box("", options, None, |_| Message::SelectView(app.view))
            .selected_index(View::ALL.iter().position(|view| *view == app.view))
            .on_select_index(Message::SelectViewIndex)
            .placeholder(crate::l10n::tr(app.view.label()))
            .aria_label(crate::l10n::tr("Calendar view"))
            .id(iced::advanced::widget::Id::new("calendar-view-selector"))
            .size(TextFieldSize::Sm)
            .radius(enzui_theme::text_defaults::shape_radius(|_| {
                enzui_theme::RadiusStep::Full
            }))
            .width(Length::Fixed(if density == Density::Phone {
                100.0
            } else {
                110.0
            }))
            .into()
    } else {
        enz_view_switcher(
            View::ALL
                .iter()
                .map(|v| SwitcherPage::new(v.label().to_lowercase(), crate::l10n::tr(v.label())))
                .collect(),
            View::ALL.iter().position(|v| *v == app.view).unwrap_or(1),
            Message::SelectViewIndex,
        )
        .aria_label(crate::l10n::tr("Calendar view"))
        .size(TextFieldSize::Sm)
        .into()
    };

    enz_title_bar()
        .leading(switcher)
        .push(
            container(titlebar_search(app))
                .width(Length::Fill)
                .align_x(Alignment::End),
        )
        .fluid_content(
            enzui_widgets::toolbar_icon::TitlebarAffordanceTokens::DEFAULT.effective_control_size(),
        )
        .actions(titlebar_actions(), Message::ToggleTitlebarOverflow)
        .action_dismiss(Message::CloseTitlebarOverflow)
        .overflow_open(app.titlebar_overflow_open)
        .action_overflow_trigger_id(titlebar_overflow_button_id())
        .action_menu_id_prefix("calendar-titlebar-actions")
        .id("calendar-app-window")
        .into_element()
}

fn titlebar_actions<'a>() -> Vec<TitlebarActionItem<'a, Message>> {
    vec![
        TitlebarActionItem::icon(
            "calendar.today",
            "calendar-check",
            crate::l10n::tr("Today"),
            Message::JumpToday,
        ),
        TitlebarActionItem::icon(
            "calendar.previous-period",
            "chevron-left",
            crate::l10n::tr("Previous period"),
            Message::ShiftPeriod(-1),
        ),
        TitlebarActionItem::icon(
            "calendar.next-period",
            "chevron-right",
            crate::l10n::tr("Next period"),
            Message::ShiftPeriod(1),
        ),
        TitlebarActionItem::icon(
            "calendar.new-event",
            "plus",
            crate::l10n::tr("New event"),
            Message::NewEvent,
        ),
    ]
}

fn titlebar_search(app: &AppState) -> Element<'_, Message> {
    use enzui_widgets::{
        search_field::enz_titlebar_search,
        suggestion_list::{SuggestionOption, enz_suggestion_list},
    };
    let fmt = app.time_format();
    let options = app
        .search(&app.search_query)
        .iter()
        .take(TITLEBAR_SEARCH_MAX_RESULTS)
        .map(|e| {
            let subtitle = format!(
                "{}, {}",
                search_result_date_label(e, app.today),
                e.time_label(&fmt)
            );
            SuggestionOption::new(e.id.to_string(), e.title.clone(), e.id).subtitle(subtitle)
        })
        .collect();
    let results = enz_suggestion_list(options, |id: &u64| Message::JumpToEvent(*id))
        .embedded(true)
        .show_empty(true)
        .empty_text(crate::l10n::tr("No matching events"))
        .list_label(crate::l10n::tr("Search results"))
        .id_prefix("calendar-search-result");
    enz_titlebar_search(&app.search_query, Message::SearchChanged)
        .placeholder(crate::l10n::tr("Search events…"))
        .id(iced::advanced::widget::Id::new("calendar-titlebar-search"))
        .on_submit(Message::OpenSearch)
        .results(results)
        .into_element()
}

pub(crate) fn search_result_date_label(
    e: &crate::model::Event,
    today: enzui_core::civil_date::Date,
) -> String {
    if e.recurrence != crate::model::Recurrence::None
        && let Some(next) = e.next_occurrence(today)
    {
        crate::l10n::format("Next: {}", &[("arg0", (date_label(next)).as_str().into())])
    } else {
        date_label(e.date)
    }
}
