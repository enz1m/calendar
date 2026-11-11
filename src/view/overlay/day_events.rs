//! Year-view day-events list.

use super::*;
use iced::widget::column;

pub(super) fn day_events(app: &AppState, date: Date) -> Element<'_, Message> {
    let evs = app.events_on(date);
    let title = date_label(date);
    let inner: Element<'_, Message> = if evs.is_empty() {
        enz_text(crate::l10n::tr("No events on this day."))
            .tone(TextTone::Muted)
            .into()
    } else {
        let mut col = column![].spacing(SpaceToken::Xs.px());
        for e in &evs {
            let color = e.color(&app.calendars);
            let id = e.id;
            let time_label = if e.all_day {
                "All day".to_owned()
            } else {
                e.start_minute.format(&app.time_format())
            };
            let title_line = enz_text(e.title.clone()).size(TextSize::Body);
            let sub_line = enz_text(time_label)
                .size(TextSize::Caption)
                .tone(TextTone::Muted);
            let row_body = row![
                container(
                    Space::new()
                        .width(Length::Fixed(3.0))
                        .height(Length::Fixed(28.0)),
                )
                .style(move |_| container::Style {
                    background: Some(color.into()),
                    border: Border {
                        radius: 1.5.into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                }),
                column![title_line, sub_line].spacing(SpaceToken::None.px()),
            ]
            .mirrored(crate::l10n::rtl())
            .spacing(SpaceToken::Sm.px())
            .align_y(Alignment::Center);
            col = col.push(
                enzui_widgets::clickable::enz_clickable(
                    container(row_body)
                        .padding(SpaceToken::Xs.px())
                        .width(Length::Fill),
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
                .width(Length::Fill)
                .radius(4.0),
            );
        }
        col.into()
    };
    let body = enz_scroll_view(inner).height(Length::Fill).into();
    let footer: Element<'_, Message> = row![
        Space::new().width(Length::Fill).height(Length::Shrink),
        enz_button(crate::l10n::tr("Open day"), Message::OpenDayView(date))
            .id(iced::advanced::widget::Id::new("calendar-open-day"))
            .kind(ButtonKind::Accent)
            .size(ButtonSize::Sm),
    ]
    .mirrored(crate::l10n::rtl())
    .align_y(Alignment::Center)
    .into();
    dialog(title, body, Some(footer), 380.0)
}
