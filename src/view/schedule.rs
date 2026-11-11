//! Day and Week schedule columns, blocks, and responsive date paging.

use enzui_core::schedule::{EventId, ScheduleNow};
use enzui_widgets::schedule::{
    ScheduleBlock, ScheduleColumn, ScheduleConfig, ScheduleTokens, enz_schedule,
};
use iced::{Element, Length};

use crate::message::Message;
use crate::model::{View, weekday_abbr};
use crate::state::AppState;

pub(crate) fn schedule<'a>(
    app: &'a AppState,
    theme: &enzui_theme::EnzTheme,
) -> Element<'a, Message> {
    let density = app.content_density();
    let dates = visible_dates(app);

    let columns = schedule_columns(app, &dates);
    let blocks = blocks_for_dates(app, &dates);

    let now = dates
        .iter()
        .position(|d| *d == app.today)
        .map(|col| ScheduleNow::new(app.now_minute, col));

    enz_schedule(columns, blocks, Message::Schedule)
        .id_prefix("calendar-schedule")
        .config(schedule_config(app))
        .tokens(ScheduleTokens {
            block_radius: 0.0,
            ..ScheduleTokens::from_palette(&theme.palette)
        })
        .now(now)
        .compact(density.schedule_compact())
        .initial_scroll_minute(Some(app.settings.work_start_minute - 60))
        .label(crate::l10n::tr(match app.view {
            View::Day => "Day schedule",
            _ => "Week schedule",
        }))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub(crate) fn schedule_columns(
    app: &AppState,
    dates: &[enzui_core::civil_date::Date],
) -> Vec<ScheduleColumn> {
    let mark_selected = dates.len() > 1;
    dates
        .iter()
        .map(|d| {
            ScheduleColumn::new(weekday_abbr(d.weekday()), crate::l10n::number(d.day()))
                .today(*d == app.today)
                .selected(mark_selected && *d == app.focus)
        })
        .collect()
}

pub(crate) fn schedule_config(app: &AppState) -> ScheduleConfig {
    let density = app.content_density();
    let secondary_offset_minutes = (app.settings.show_secondary_tz
        && density.allows_secondary_tz())
    .then_some(app.settings.secondary_tz_offset_minutes);
    ScheduleConfig {
        work_start_minute: app.settings.work_start_minute,
        work_end_minute: app.settings.work_end_minute,
        secondary_offset_minutes,
        secondary_label: secondary_offset_minutes.map(crate::model::secondary_tz_label),
        ..ScheduleConfig::default()
    }
}

pub(crate) fn visible_dates(app: &AppState) -> Vec<enzui_core::civil_date::Date> {
    let density = app.content_density();
    let mut dates = app.schedule_dates();
    // Compact Week pages advance by their visible three-day span, including across week boundaries.
    if app.view == View::Week && dates.len() > density.week_columns() {
        let want = density.week_columns();
        dates = (0..want)
            .map(|offset| app.focus.add_days(offset as i32))
            .collect();
    }
    dates
}

pub(crate) fn blocks_for_dates(
    app: &AppState,
    dates: &[enzui_core::civil_date::Date],
) -> Vec<ScheduleBlock> {
    let mut blocks = Vec::new();
    if let (Some(first), Some(last)) = (dates.first().copied(), dates.last().copied()) {
        for e in app.events_intersecting(first, last) {
            let visible_start = e.date.max(first);
            let visible_end = e.end_date.min(last);
            let start_col =
                usize::try_from(visible_start.to_days() - first.to_days()).unwrap_or_default();
            let end_col =
                usize::try_from(visible_end.to_days() - first.to_days()).unwrap_or(start_col);
            // Occurrence keys use the start date rather than continuation dates to avoid duplicate
            // announcements.
            let Some(key) = app.schedule_block_id(e.id, e.date) else {
                continue;
            };
            let id = EventId(key);
            let color = e.color(&app.calendars);
            let selected = app.selected == Some(e.id);
            let spans_days = start_col != end_col || e.date != e.end_date;
            let block = if e.all_day {
                (if spans_days {
                    ScheduleBlock::all_day_span(id, start_col, end_col)
                } else {
                    ScheduleBlock::all_day(id, start_col)
                })
                .title(e.title.clone())
                .color(color)
                .selected(selected)
            } else {
                let start_minute = if e.date < first {
                    0
                } else {
                    e.start_minute.get()
                };
                let end_minute = if e.end_date > last {
                    crate::model::MINUTES_PER_DAY
                } else {
                    e.end_minute.get()
                };
                (if spans_days {
                    ScheduleBlock::timed_span(id, start_col, start_minute, end_col, end_minute)
                } else {
                    ScheduleBlock::timed(id, start_col, start_minute, end_minute)
                })
                .title(e.title.clone())
                .subtitle(if e.location.is_empty() {
                    e.time_label(&app.time_format())
                } else {
                    format!("{} · {}", e.time_label(&app.time_format()), e.location)
                })
                .color(color)
                .selected(selected)
            };
            blocks.push(block);
        }
    }
    app.retain_schedule_blocks(blocks.iter().map(|block| block.id.get()));
    blocks
}
