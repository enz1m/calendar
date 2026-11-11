//! The sidebar: grouped calendar rows with coloured checkboxes, plus the mini-month.

use enzui_core::calendar_semantics::{DayMarker, day_cell_semantics, day_state, position_of_index};
use enzui_core::civil_date::YearMonth;
use enzui_theme::{EnzTheme, SpaceToken, StrokeToken};

pub(crate) const MINI_MONTH_CELL_H_PX: f32 = 22.0;

pub(crate) const MINI_MONTH_CELL_GAP_PX: f32 = 1.0;

use enzui_widgets::calendar::enz_calendar;
use enzui_widgets::checkbox::{CheckboxTokens, enz_checkbox};
use enzui_widgets::context_target::enz_context_target;
use enzui_widgets::enz_text::{TextSize, TextTone, enz_text};
use enzui_widgets::focus_button::focus_button;
use enzui_widgets::scroll_view::enz_scroll_view;
use enzui_widgets::sidebar_content::enz_sidebar_content;
use iced::widget::{column, container, row};
use iced::{Alignment, Border, Element, Length, Padding};

use crate::layout::Density;
use crate::message::Message;
use crate::model::palette_color;
use crate::state::{AppState, MenuTarget, mini_month_cells};

#[must_use]
pub(crate) fn row_checkbox_tokens(
    palette: &enzui_theme::Palette,
    cal_color: iced::Color,
) -> CheckboxTokens {
    let hidden_bg = mix_towards(cal_color, palette.surface, 0.80);
    let hidden_border = darken(cal_color, 0.35);
    CheckboxTokens {
        border: hidden_border,
        background: hidden_bg,
        hover_border: cal_color,
        accent: cal_color,
        // Always white, as calendar apps draw it: a per-colour ink turned the
        // check black on the lighter calendar colours, so one row in a column
        // of white checks stood out for no reason.
        check_color: iced::Color::WHITE,
        text_color: palette.text,
        description_color: palette.text_muted,
        focus_ring: palette.focus_ring,
        ..CheckboxTokens::from_palette(palette)
    }
}

pub(crate) fn mix_towards(a: iced::Color, b: iced::Color, t: f32) -> iced::Color {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    iced::Color {
        r: a.r * inv + b.r * t,
        g: a.g * inv + b.g * t,
        b: a.b * inv + b.b * t,
        a: a.a * inv + b.a * t,
    }
}

pub(crate) fn darken(c: iced::Color, t: f32) -> iced::Color {
    mix_towards(c, iced::Color::BLACK, t)
}

pub(crate) fn sidebar<'a>(app: &'a AppState, theme: &EnzTheme) -> Element<'a, Message> {
    let palette = theme.palette.clone();
    let mut body = enz_sidebar_content()
        .spacing_scale(theme.spacing)
        .spacing(SpaceToken::Xxs)
        .section_gap(SpaceToken::Sm)
        .padding_top(SpaceToken::Sm)
        .padding_bottom(SpaceToken::Sm)
        .width(Length::Fill);

    for (section, cals) in app.calendar_sections() {
        body = body.push_section(crate::l10n::tr(section));
        for c in cals {
            let id_for_toggle = c.id.clone();
            let id_for_menu = c.id.clone();
            let cal_color = palette_color(&c.color);
            let checkbox_tokens = row_checkbox_tokens(&palette, cal_color);
            let cb = enz_checkbox(c.visible, &c.name, move |_| {
                Message::ToggleCalendar(id_for_toggle.clone())
            })
            .tokens(checkbox_tokens)
            // Calendar IDs retain focus across renaming and reordering.
            .id(crate::view::overlay::sidebar_calendar_row_id(&c.id));
            let line = row![cb]
                .mirrored(crate::l10n::rtl())
                .align_y(Alignment::Center);
            body = body.push(
                enz_context_target(
                    container(line)
                        // Checkboxes start on the sidebar rail, under the captions and the header title.
                        .padding(Padding::from([SpaceToken::Xxs.px(), SpaceToken::Md.px()])),
                    move |at| {
                        Message::ContextMenu(MenuTarget::CalendarRow(id_for_menu.clone()), at)
                    },
                )
                .height(Length::Shrink),
            );
        }
    }

    let list = enz_scroll_view(body.into_element()).height(Length::Fill);

    column![list, mini_month(app, theme)]
        .spacing(SpaceToken::Xs.px())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn mini_month<'a>(app: &'a AppState, theme: &EnzTheme) -> Element<'a, Message> {
    // Keep this compact control proportional to the user's typography, without
    // shrinking text elsewhere in the sidebar or changing shared theme state.
    let mut mini_theme = theme.clone();
    mini_theme.typography = theme
        .typography
        .with_base_body_size(theme.typography.base_ramp().body_size * (11.0 / 13.0));
    let _text_defaults = enzui_theme::text_defaults::TextDefaultsScope::enter(&mini_theme);
    let grid = enz_calendar(&app.mini_controller, Message::MiniCal)
        .today(Some(app.today))
        .selected(Some(app.focus))
        .flat(true)
        .fill_width(true)
        .round_days(true)
        .cell_size(MINI_MONTH_CELL_H_PX)
        .cell_spacing(MINI_MONTH_CELL_GAP_PX)
        .event_days(|d| app.has_events_on(d))
        .on_day_context(|d, at| Message::ContextMenu(MenuTarget::MiniDay(d), at));

    container(grid.into_element())
        .padding(Padding {
            top: SpaceToken::Xs.px(),
            right: SpaceToken::Sm.px(),
            bottom: SpaceToken::Md.px(),
            left: SpaceToken::Sm.px(),
        })
        .width(Length::Fill)
        .into()
}

#[must_use]
pub(crate) fn mini_cell_is_today(
    cell: &enzui_core::civil_date::DayCell,
    today: enzui_core::civil_date::Date,
) -> bool {
    cell.in_month && cell.date == today
}

pub(crate) fn year_month_card(app: &AppState, month: YearMonth) -> Element<'_, Message> {
    let declares_dense = app.content_density() != Density::Regular;
    let mut grid = column![]
        .spacing(StrokeToken::Hairline.px())
        .height(Length::Fill);
    grid = grid.push(
        row((0..7)
            .map(|offset| {
                use enzui_core::calendar_semantics::CalendarLabels;
                let day = enzui_core::civil_date::Weekday::ALL
                    [(app.settings.first_day_of_week.index() + offset) % 7];
                let label = crate::l10n::localizer().scope(|| {
                    enzui_core::localization::LocalizedCalendarLabels.weekday_narrow(day)
                });
                container(
                    enz_text(label)
                        .size(TextSize::Caption2)
                        .tone(TextTone::Muted),
                )
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .into()
            })
            .collect::<Vec<_>>())
        .mirrored(crate::l10n::rtl())
        .spacing(StrokeToken::Hairline.px()),
    );
    for (week_index, week) in mini_month_cells(month, app.settings.first_day_of_week)
        .chunks(7)
        .enumerate()
    {
        let mut r = row![]
            .mirrored(crate::l10n::rtl())
            .spacing(StrokeToken::Hairline.px())
            .height(Length::Fill);
        for (column_index, cell) in week.iter().enumerate() {
            let cell_index = week_index * 7 + column_index;
            let date = cell.date;
            let is_today = mini_cell_is_today(cell, app.today);
            let has = cell.in_month && app.has_events_on(date);
            let marker = DayMarker::present(has);
            let mut label = enz_text(crate::l10n::number(date.day())).size(TextSize::Caption2);
            if !cell.in_month {
                label = label.tone(TextTone::Subtle);
            }
            if is_today {
                label = label.color_resolved(|t| {
                    enzui_widgets::theme_lookup::palette(t)
                        .filled_color(enzui_widgets::theme_lookup::palette(t).accent)
                        .text
                });
            }
            let body = container(label)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center);
            let semantics = day_cell_semantics(
                *cell,
                day_state(
                    date,
                    None,
                    Some(app.focus),
                    None,
                    is_today.then_some(app.today),
                    cell.in_month,
                )
                .with_marker(marker),
                position_of_index(cell_index),
                &enzui_core::localization::LocalizedCalendarLabels,
            );
            let day_cell = enz_context_target(
                focus_button(body.width(Length::Fill), Message::ShowDayEvents(date))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .id(crate::view::overlay::year_day_cell_id(month, date))
                    .semantics(semantics)
                    .outset_focus_ring(true)
                    .style_focus(move |t, status| {
                        let p = enzui_widgets::theme_lookup::palette(t);
                        let fill = if is_today {
                            Some(p.filled_color(p.accent).color)
                        } else if has {
                            Some(p.surface_selected)
                        } else {
                            None
                        };
                        let fill = match status {
                            enzui_widgets::focus_button::FocusStatus::Hovered => {
                                Some(enzui_widgets::surface_interaction::composite(
                                    fill.unwrap_or(p.surface),
                                    p.overlay_hover(),
                                ))
                            }
                            enzui_widgets::focus_button::FocusStatus::Pressed => {
                                fill.or(Some(p.surface_selected))
                            }
                            _ => fill,
                        };
                        iced::widget::button::Style {
                            background: fill.map(Into::into),
                            text_color: if is_today {
                                p.filled_color(p.accent).text
                            } else {
                                p.text
                            },
                            border: Border {
                                radius: enzui_widgets::theme_lookup::radius(t).full.into(),
                                ..Border::default()
                            },
                            ..Default::default()
                        }
                    }),
                move |at| Message::ContextMenu(MenuTarget::MiniDay(date), at),
            )
            .width(Length::Fill)
            .height(Length::Fill);
            r = r.push(
                enzui_widgets::accessibility_scan_widgets::HitTargetScanProbe::new(Element::from(
                    day_cell,
                ))
                .allow_small(declares_dense),
            );
        }
        grid = grid.push(r);
    }
    container(
        column![
            enz_text(crate::model::month_name(month.month())).size(TextSize::Footnote),
            grid
        ]
        .spacing(SpaceToken::Custom(3.0).px())
        .height(Length::Fill),
    )
    .padding(SpaceToken::Xs.px())
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
