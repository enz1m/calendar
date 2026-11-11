//! Month grids, year cards and agenda lists.

use enzui_core::calendar_semantics::{day_cell_semantics, day_state, position_of_index};
use enzui_core::civil_date::YearMonth;
use enzui_theme::{SpaceToken, StrokeToken};
use enzui_widgets::calendar::{CalendarEventSpan, calendar_span_overlay};
use enzui_widgets::clickable::enz_clickable;
use enzui_widgets::context_target::enz_context_target;
use enzui_widgets::enz_text::{TextSize, TextTone, enz_text};
use enzui_widgets::focus_button::{FocusStatus, focus_button};
use enzui_widgets::list_row_surface::enz_list_row_surface;
use enzui_widgets::scroll_view::enz_scroll_view;
use enzui_widgets::status_page::enz_status_page;
use enzui_widgets::surface_interaction::composite;
use iced::widget::button::Style as ButtonStyle;
use iced::widget::{Space, column, container, row, stack};
use iced::{Alignment, Background, Border, Element, Length, Padding};

use crate::layout::Density;
use crate::message::Message;
use crate::model::{date_label, iso_week_number, weekday_labels};
use crate::state::{AppState, MenuTarget};

const YEAR_GRID_GAP: SpaceToken = SpaceToken::Space3;

fn cell_chip_gutter() -> f32 {
    SpaceToken::Xs.px()
}

const DAY_BOX_PX: f32 = 20.0;

const STRIPE_W_PX: f32 = 3.0;

const CHIP_STRIPE_H_PX: f32 = 9.0;

const WEEK_GUTTER_W_PX: f32 = 24.0;

const AGENDA_STRIPE_H_PX: f32 = 26.0;

const TIME_LABEL_COL_W_PX: f32 = 140.0;

pub(crate) fn month(app: &AppState) -> Element<'_, Message> {
    let density = app.content_density();
    let chip_cap = density.month_chips();
    let grid = app.month_grid();
    let focus_month = YearMonth::of(app.focus);
    let first_visible = grid.cells()[0].date;
    let last_visible = grid.cells()[41].date;
    let spans: Vec<CalendarEventSpan> = app
        .events_intersecting(first_visible, last_visible)
        .into_iter()
        .filter(|e| e.all_day && e.end_date > e.date)
        .filter_map(|e| {
            let color = e.color(&app.calendars);
            let key = app.schedule_block_id(e.id, e.date)?;
            Some(
                CalendarEventSpan::new(enzui_core::schedule::EventId(key), e.date, e.end_date)
                    .title(e.title)
                    .color(color),
            )
        })
        .collect();
    app.retain_schedule_blocks(spans.iter().map(|span| span.id.get()));
    let span_announcements: Vec<String> = spans
        .iter()
        .map(|span| {
            crate::l10n::format(
                "{}, {}–{}, all day",
                &[
                    ("arg0", (span.title).as_str().into()),
                    (
                        "arg1",
                        (crate::model::full_date_label(span.start)).as_str().into(),
                    ),
                    (
                        "arg2",
                        (crate::model::full_date_label(span.end)).as_str().into(),
                    ),
                ],
            )
        })
        .collect();

    let mut head = row![]
        .mirrored(crate::l10n::rtl())
        .spacing(StrokeToken::Hairline.px());
    if app.settings.show_week_numbers {
        head = head.push(week_gutter_cell(String::new()));
    }
    for l in weekday_labels(app.settings.first_day_of_week) {
        head = head.push(
            container(enz_text(l).size(TextSize::Caption).tone(TextTone::Muted))
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .padding(Padding::from([SpaceToken::Xs.px(), 0.0])),
        );
    }

    let mut body = column![]
        .spacing(StrokeToken::Hairline.px())
        .height(Length::Fill);
    for (week_index, week) in grid.cells().chunks(7).enumerate() {
        let mut r = row![]
            .mirrored(crate::l10n::rtl())
            .spacing(StrokeToken::Hairline.px())
            .height(Length::Fill);
        if app.settings.show_week_numbers {
            r = r.push(week_gutter_cell(crate::l10n::number(iso_week_number(
                week[0].date,
            ))));
        }
        for (column_index, cell) in week.iter().enumerate() {
            let cell_index = week_index * 7 + column_index;
            let date = cell.date;
            let in_month = YearMonth::of(date) == focus_month;
            let is_today = date == app.today;
            let is_selected = in_month && date == app.focus;
            // Multi-day all-day spans are represented once by the span layer, not by repeated day
            // chips.
            let events: Vec<_> = app
                .events_on(date)
                .into_iter()
                .filter(|e| !(e.all_day && e.end_date > e.date))
                .collect();
            let shown = events.len().min(chip_cap);

            let mut inner = column![]
                .spacing(StrokeToken::Hairline.px())
                .padding(Padding::from([0.0, cell_chip_gutter()]))
                .width(Length::Fill);
            let mut day_number = enz_text(crate::l10n::number(date.day()))
                .size(TextSize::Footnote)
                // The day glyph needs explicit on-accent ink because enz_text does not inherit
                // container text color.
                .tone(if in_month || is_today {
                    TextTone::Default
                } else {
                    TextTone::Subtle
                });
            if is_today {
                day_number = day_number.color_resolved(|t| {
                    enzui_widgets::theme_lookup::palette(t)
                        .filled_color(enzui_widgets::theme_lookup::palette(t).accent)
                        .text
                });
            }
            let num = container(day_number)
                .width(Length::Fixed(DAY_BOX_PX))
                .height(Length::Fixed(DAY_BOX_PX))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(move |t| {
                    if is_today {
                        container::Style {
                            background: Some(
                                enzui_widgets::theme_lookup::palette(t)
                                    .filled_color(enzui_widgets::theme_lookup::palette(t).accent)
                                    .color
                                    .into(),
                            ),
                            text_color: Some(
                                enzui_widgets::theme_lookup::palette(t)
                                    .filled_color(enzui_widgets::theme_lookup::palette(t).accent)
                                    .text,
                            ),
                            border: Border {
                                radius: 10.0.into(),
                                ..Border::default()
                            },
                            ..container::Style::default()
                        }
                    } else {
                        container::Style::default()
                    }
                });
            inner = inner.push(container(num).align_x(Alignment::End).width(Length::Fill));

            for e in events.iter().take(shown) {
                let color = e.color(&app.calendars);
                let id = e.id;
                // Dense event chips cannot use a hit floor that overlaps neighboring calendar
                // cells.
                inner = inner.push(
                    enz_clickable(
                        container(
                            row![
                                container(
                                    Space::new()
                                        .width(Length::Fixed(STRIPE_W_PX))
                                        .height(Length::Fixed(CHIP_STRIPE_H_PX))
                                )
                                .style(move |_| container::Style {
                                    background: Some(color.into()),
                                    border: Border {
                                        radius: 1.5.into(),
                                        ..Border::default()
                                    },
                                    ..container::Style::default()
                                }),
                                enz_text(e.title.clone())
                                    .size(TextSize::Caption2)
                                    .single_line_elided(),
                            ]
                            .mirrored(crate::l10n::rtl())
                            .spacing(SpaceToken::Custom(3.0).px())
                            .align_y(Alignment::Center),
                        )
                        .padding(Padding::from([0.0, 3.0]))
                        .width(Length::Fill)
                        .clip(true),
                        Message::OpenDetails {
                            id,
                            on: Some(date),
                            at: None,
                        },
                    )
                    .on_press_with_point(move |at| Message::OpenDetails {
                        id,
                        on: Some(date),
                        at,
                    })
                    .aria_label(e.title.clone())
                    // Focus IDs include occurrence dates so different instances of a series remain
                    // distinct.
                    .id(crate::view::overlay::event_row_id(id, date))
                    .width(Length::Fill)
                    .min_hit(0.0)
                    .radius(0.0),
                );
            }
            if events.len() > shown {
                let more = events.len() - shown;
                inner = inner.push(
                    enz_clickable(
                        container(
                            enz_text(crate::l10n::count("calendar-more-count", more))
                                .size(TextSize::Caption2)
                                .tone(TextTone::Accent),
                        )
                        .padding(Padding::from([0.0, 3.0])),
                        Message::ShowDayEvents(date),
                    )
                    .aria_label(crate::l10n::count("calendar-show-more", more))
                    .width(Length::Fill)
                    .min_hit(0.0)
                    .radius(0.0),
                );
            }

            // Cell fills belong to the focusable surface so they cannot obscure hover and focus
            // states.
            let cell_el = container(inner)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(SpaceToken::Xxs.px())
                .clip(true);

            let semantics = day_cell_semantics(
                *cell,
                day_state(
                    date,
                    // Focused semantics come from the live widget operation, never a second
                    // builder-time claim.
                    None,
                    Some(app.focus),
                    None,
                    Some(app.today),
                    cell.in_month,
                ),
                position_of_index(cell_index),
                &enzui_core::localization::LocalizedCalendarLabels,
            );
            r = r.push(
                enz_context_target(
                    focus_button(cell_el, Message::JumpTo(date))
                        .on_press_with_point(move |at| Message::MonthDayPressed {
                            date,
                            at: std::time::Instant::now(),
                            position: at.unwrap_or(iced::Point::ORIGIN),
                        })
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .clip(true)
                        // Padding cells repeat day numbers; stable IDs require the full date.
                        .id(crate::view::overlay::month_day_cell_id(date))
                        .semantics(semantics)
                        .style_focus(move |t, status| {
                            let p = enzui_widgets::theme_lookup::palette(t);
                            let base = if is_selected {
                                p.surface_selected
                            } else if in_month {
                                p.surface
                            } else {
                                p.background
                            };
                            let background = match status {
                                FocusStatus::Hovered => composite(base, p.overlay_hover()),
                                FocusStatus::Pressed => composite(base, p.overlay_pressed()),
                                FocusStatus::Active
                                | FocusStatus::Focused
                                | FocusStatus::Disabled => base,
                            };
                            let border = if matches!(status, FocusStatus::Focused) {
                                Border {
                                    color: p.focus_ring,
                                    width: 2.0,
                                    radius: 0.0.into(),
                                }
                            } else {
                                Border::default()
                            };
                            ButtonStyle {
                                background: Some(Background::Color(background)),
                                text_color: p.text,
                                border,
                                ..ButtonStyle::default()
                            }
                        }),
                    move |at| Message::ContextMenu(MenuTarget::Day(date), at),
                )
                .width(Length::Fill)
                .height(Length::Fill),
            );
        }
        body = body.push(r);
    }

    let body: Element<'_, Message> = if spans.is_empty() {
        body.into()
    } else {
        let square_fill = iced::widget::canvas(SquareSpanFill {
            spans: spans.clone(),
            first_visible,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        let layer: Element<'_, Message> = stack![
            square_fill,
            calendar_span_overlay(spans, first_visible, span_announcements)
        ]
        .into();
        let overlay: Element<'_, Message> = if app.settings.show_week_numbers {
            row![
                Space::new()
                    .width(Length::Fixed(WEEK_GUTTER_W_PX + 1.0))
                    .height(Length::Fill),
                layer
            ]
            .mirrored(crate::l10n::rtl())
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            layer
        };
        stack![body, overlay].into()
    };

    column![head, body]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn week_gutter_cell<'a>(text: String) -> Element<'a, Message> {
    container(
        enz_text(text)
            .size(TextSize::Caption2)
            .tone(TextTone::Subtle),
    )
    .width(Length::Fixed(WEEK_GUTTER_W_PX))
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .into()
}

pub(crate) fn year(app: &AppState) -> Element<'_, Message> {
    let density = app.content_density();
    let cols = match density {
        Density::Phone => 2,
        Density::Compact => 3,
        Density::Regular => 4,
    };
    let mut grid = column![]
        .spacing(YEAR_GRID_GAP.px())
        .padding(SpaceToken::Sm.px())
        .height(Length::Fill);
    let mut months: Vec<Element<'_, Message>> = (1..=12)
        .filter_map(|m| YearMonth::new(app.focus.year(), m))
        .map(|ym| super::sidebar::year_month_card(app, ym))
        .collect();
    while !months.is_empty() {
        let take = cols.min(months.len());
        let rest = months.split_off(take);
        let mut r = row![]
            .mirrored(crate::l10n::rtl())
            .spacing(YEAR_GRID_GAP.px())
            .height(Length::Fill);
        let n = months.len();
        for el in months {
            r = r.push(container(el).width(Length::Fill).height(Length::Fill));
        }
        for _ in n..cols {
            r = r.push(
                container(Space::new().width(Length::Fill).height(Length::Shrink))
                    .width(Length::Fill),
            );
        }
        grid = grid.push(r);
        months = rest;
    }
    // Year cards need finite pane height so Fill rows can divide the available space.
    container(grid)
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

pub(crate) fn agenda(app: &AppState) -> Element<'_, Message> {
    let days = app.agenda(crate::state::AGENDA_RANGE_DAYS);
    if days.is_empty() {
        return container(
            enz_status_page(crate::l10n::tr("No upcoming events"))
                .description(crate::l10n::tr(
                    "Events you create or import will appear here.",
                ))
                .icon("calendar")
                .primary_action(
                    enzui_core::action::Action::new("event.new", Message::NewEvent)
                        .with_label(crate::l10n::tr("New Event")),
                ),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into();
    }

    let mut list = column![]
        .spacing(SpaceToken::Xs.px())
        .padding(SpaceToken::Md.px());
    for (date, events) in days {
        let is_today = date == app.today;
        list = list.push(
            container(
                row![
                    enz_text(date_label(date))
                        .size(TextSize::Footnote)
                        .tone(if is_today {
                            TextTone::Accent
                        } else {
                            TextTone::Default
                        }),
                    enz_text(crate::model::month_year_label(
                        enzui_core::civil_date::YearMonth::of(date)
                    ))
                    .size(TextSize::Caption)
                    .tone(TextTone::Muted),
                ]
                .mirrored(crate::l10n::rtl())
                .spacing(SpaceToken::Sm.px())
                .align_y(Alignment::Center),
            )
            .padding(Padding {
                top: SpaceToken::Xs.px(),
                right: 0.0,
                bottom: SpaceToken::Xxs.px(),
                left: 0.0,
            }),
        );
        for e in events {
            let color = e.color(&app.calendars);
            let id = e.id;
            let title = e.title.clone();
            list = list.push(
                enz_context_target(
                    enz_list_row_surface(
                        container(
                            row![
                                container(
                                    Space::new()
                                        .width(Length::Fixed(STRIPE_W_PX))
                                        .height(Length::Fixed(AGENDA_STRIPE_H_PX))
                                )
                                .style(move |_| container::Style {
                                    background: Some(color.into()),
                                    border: Border {
                                        radius: 1.5.into(),
                                        ..Border::default()
                                    },
                                    ..container::Style::default()
                                }),
                                container(
                                    enz_text(e.time_label(&app.time_format()))
                                        .size(TextSize::Caption2)
                                        .tone(TextTone::Muted)
                                )
                                .width(Length::Fixed(TIME_LABEL_COL_W_PX)),
                                column![
                                    enz_text(e.title.clone()).size(TextSize::Footnote),
                                    enz_text(e.location.clone())
                                        .size(TextSize::Caption2)
                                        .tone(TextTone::Subtle),
                                ]
                                .spacing(StrokeToken::Hairline.px()),
                            ]
                            .mirrored(crate::l10n::rtl())
                            .spacing(SpaceToken::Sm.px())
                            .align_y(Alignment::Center),
                        )
                        .padding(Padding::from([SpaceToken::Sm.px(), SpaceToken::Md.px()]))
                        .width(Length::Fill)
                        .clip(true),
                        Message::OpenDetails {
                            id,
                            on: Some(date),
                            at: None,
                        },
                    )
                    .content_height(true)
                    .content_padding(SpaceToken::Xxs.px())
                    .on_activate_with_point(move |at| Message::OpenDetails {
                        id,
                        on: Some(date),
                        at,
                    })
                    .label(title)
                    // Month and Agenda never co-render, so their occurrence focus IDs can be
                    // shared.
                    .id(crate::view::overlay::event_row_id(id, date))
                    .width(Length::Fill),
                    move |at| {
                        // Occurrence dates must reach details and deletion so recurring actions
                        // cannot silently target the whole series.
                        Message::ContextMenu(MenuTarget::Event { id, on: Some(date) }, at)
                    },
                )
                .height(Length::Shrink),
            );
        }
    }
    enz_scroll_view(list)
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

// Alpha's span overlay pins 2.5px corners. Paint square underlays using its public
// geometry, retaining the framework's labels, clipping and accessibility nodes.
struct SquareSpanFill {
    spans: Vec<CalendarEventSpan>,
    first_visible: enzui_core::civil_date::Date,
}

impl<Message> iced::widget::canvas::Program<Message> for SquareSpanFill {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use enzui_widgets::calendar::{CalendarCell, layout_calendar_spans_in};
        let palette = enzui_widgets::theme_lookup::palette(theme);
        let mut frame = iced::widget::canvas::Frame::new(renderer, bounds.size());
        let cell = CalendarCell {
            width: bounds.width / 7.0,
            height: bounds.height / 6.0,
        };
        for segment in layout_calendar_spans_in(&self.spans, self.first_visible, cell) {
            let rect = segment.rect;
            let color = self.spans[segment.source].color.unwrap_or(palette.accent);
            frame.fill_rectangle(
                rect.position(),
                rect.size(),
                palette.filled_color(color).color,
            );
        }
        vec![frame.into_geometry()]
    }
}
