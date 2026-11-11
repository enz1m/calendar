//! Calendar events, settings, display labels, and clock-format preferences.

use enzui_core::civil_date::{Date, Weekday, YearMonth};
use enzui_core::time_of_day::{HourCycle, TimeFormat, TimeOfDay};
use iced::Color;

pub(crate) const MINUTES_PER_DAY: i32 = 24 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct MinuteOfDay(i32);

impl MinuteOfDay {
    #[must_use]
    pub(crate) fn new(minute: i32) -> Self {
        Self(minute.clamp(0, MINUTES_PER_DAY))
    }

    #[must_use]
    pub(crate) fn hm(hour: i32, minute: i32) -> Self {
        Self::new(hour * 60 + minute)
    }

    #[must_use]
    pub(crate) const fn get(self) -> i32 {
        self.0
    }

    #[must_use]
    pub(crate) const fn hour(self) -> i32 {
        self.0 / 60
    }

    #[must_use]
    pub(crate) const fn minute(self) -> i32 {
        self.0 % 60
    }

    #[must_use]
    pub(crate) fn to_time_of_day(self) -> TimeOfDay {
        let clamped = Self(self.0.min(MINUTES_PER_DAY - 1));
        TimeOfDay::hm(clamped.hour() as u32, clamped.minute() as u32)
            .expect("a minute in 0..1440 is a valid clock time")
    }

    #[must_use]
    pub(crate) fn format(self, fmt: &TimeFormat) -> String {
        fmt.format(self.to_time_of_day())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TimeFormatPref {
    #[default]
    System,
    TwentyFourHour,
    TwelveHour,
}

impl TimeFormatPref {
    pub(crate) const ALL: [Self; 3] = [Self::System, Self::TwentyFourHour, Self::TwelveHour];

    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::TwentyFourHour => "24-hour",
            Self::TwelveHour => "12-hour",
        }
    }

    #[must_use]
    pub(crate) const fn hour_cycle(self, system: HourCycle) -> HourCycle {
        match self {
            Self::System => system,
            Self::TwentyFourHour => HourCycle::H23,
            Self::TwelveHour => HourCycle::H12,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub(crate) enum Recurrence {
    #[default]
    None,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Recurrence {
    pub(crate) const ALL: [Self; 5] = [
        Self::None,
        Self::Daily,
        Self::Weekly,
        Self::Monthly,
        Self::Yearly,
    ];

    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::None => "Does not repeat",
            Self::Daily => "Every day",
            Self::Weekly => "Every week",
            Self::Monthly => "Every month",
            Self::Yearly => "Every year",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RecurrenceEndKind {
    #[default]
    Never,
    OnDate,
    AfterCount,
}

impl RecurrenceEndKind {
    pub(crate) const ALL: [Self; 3] = [Self::Never, Self::OnDate, Self::AfterCount];

    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Never => "Never",
            Self::OnDate => "Ends on a date",
            Self::AfterCount => "After a number of times",
        }
    }

    #[must_use]
    pub(crate) fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|k| *k == self)
            .expect("every variant is in ALL")
    }

    #[must_use]
    pub(crate) fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Never)
    }
}

pub(crate) const ALERT_CHOICES: [Option<i32>; 7] = [
    None,
    Some(0),
    Some(5),
    Some(15),
    Some(30),
    Some(60),
    Some(1440),
];

#[must_use]
pub(crate) fn alert_label(alert: Option<i32>) -> String {
    match alert {
        None => crate::l10n::tr("None"),
        Some(0) => crate::l10n::tr("At time of event"),
        Some(1440) => crate::l10n::tr("1 day before"),
        Some(60) => crate::l10n::tr("1 hour before"),
        Some(m) => crate::l10n::count("calendar-minutes-before", m.max(0) as usize),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Calendar {
    pub id: String,
    pub name: String,
    pub section: String,
    pub color: String,
    pub visible: bool,
}

pub(crate) const CALENDAR_COLOR_KEYS: [&str; 10] = [
    "blue", "green", "orange", "purple", "red", "teal", "magenta", "yellow", "brown", "gray",
];

pub(crate) fn palette_color(key: &str) -> Color {
    match key {
        "red" => Color::from_rgb8(0xE1, 0x45, 0x3A),
        "orange" => Color::from_rgb8(0xF0, 0x8C, 0x1E),
        "yellow" => Color::from_rgb8(0xE8, 0xB4, 0x22),
        "green" => Color::from_rgb8(0x33, 0xA8, 0x52),
        "teal" => Color::from_rgb8(0x1E, 0xA8, 0xA8),
        "blue" => Color::from_rgb8(0x2E, 0x7C, 0xE4),
        "purple" => Color::from_rgb8(0x8A, 0x5C, 0xD6),
        "magenta" => Color::from_rgb8(0xC0, 0x46, 0xA0),
        "brown" => Color::from_rgb8(0x8B, 0x6B, 0x4A),
        "gray" | "grey" => Color::from_rgb8(0x7A, 0x7F, 0x88),
        _ => Color::from_rgb8(0x2E, 0x7C, 0xE4),
    }
}

pub(crate) const COLOR_KEYS: [&str; 10] = [
    "red", "orange", "yellow", "green", "teal", "blue", "purple", "magenta", "brown", "gray",
];

#[path = "model/events.rs"]
mod events;

pub(crate) use events::{Event, TzAnchor};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Settings {
    pub first_day_of_week: Weekday,
    pub time_format: TimeFormatPref,
    pub work_start_minute: i32,
    pub work_end_minute: i32,
    pub show_week_numbers: bool,
    pub show_secondary_tz: bool,
    pub secondary_tz_offset_minutes: i32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            first_day_of_week: crate::l10n::localizer()
                .locale_for(enzui_core::localization::LocaleCategory::Time)
                .first_weekday()
                .unwrap_or(Weekday::Monday),
            time_format: TimeFormatPref::default(),
            work_start_minute: 9 * 60,
            work_end_minute: 17 * 60,
            show_week_numbers: false,
            show_secondary_tz: false,
            secondary_tz_offset_minutes: 0,
        }
    }
}

#[must_use]
pub(crate) fn secondary_tz_label(offset_minutes: i32) -> String {
    // Sub-hour western offsets need the sign of the entire offset because integer hours truncate to
    // zero.
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let abs = offset_minutes.abs();
    let (h, m) = (abs / 60, abs % 60);
    if m == 0 {
        format!("UTC{sign}{h}")
    } else {
        format!("UTC{sign}{h}:{m:02}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum View {
    Day,
    #[default]
    Week,
    Month,
    Year,
    Agenda,
}

impl View {
    pub(crate) const ALL: [Self; 5] =
        [Self::Day, Self::Week, Self::Month, Self::Year, Self::Agenda];

    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Day => "Day",
            Self::Week => "Week",
            Self::Month => "Month",
            Self::Year => "Year",
            Self::Agenda => "Agenda",
        }
    }
}

#[must_use]
pub(crate) fn week_start(date: Date, first: Weekday) -> Date {
    let offset = date.weekday().offset_from(first) as i32;
    date.add_days(-offset)
}

#[must_use]
pub(crate) fn week_days(date: Date, first: Weekday) -> Vec<Date> {
    let start = week_start(date, first);
    (0..7).map(|i| start.add_days(i)).collect()
}

#[must_use]
pub(crate) fn iso_week_number(date: Date) -> u32 {
    let dow = date.weekday().offset_from(Weekday::Monday) as i32;
    let thursday = date.add_days(3 - dow);
    let jan1 = Date::new(thursday.year(), 1, 1).unwrap_or(thursday);
    ((thursday.to_days() - jan1.to_days()) / 7 + 1) as u32
}

#[must_use]
pub(crate) fn weekday_labels(first: Weekday) -> Vec<String> {
    (0..7)
        .map(|i| weekday_abbr(Weekday::ALL[(first.index() + i) % 7]))
        .collect()
}

pub(crate) fn month_year_label(month: YearMonth) -> String {
    use enzui_core::calendar_semantics::CalendarLabels;
    crate::l10n::localizer()
        .scope(|| enzui_core::localization::LocalizedCalendarLabels.month_year(month))
}
pub(crate) fn month_name(month: u32) -> String {
    use enzui_core::calendar_semantics::CalendarLabels;
    crate::l10n::localizer()
        .scope(|| enzui_core::localization::LocalizedCalendarLabels.month_name(month))
}
pub(crate) fn weekday_abbr(day: Weekday) -> String {
    use enzui_core::calendar_semantics::CalendarLabels;
    crate::l10n::localizer()
        .scope(|| enzui_core::localization::LocalizedCalendarLabels.weekday_short(day))
}
pub(crate) fn weekday_full(day: Weekday) -> String {
    use enzui_core::calendar_semantics::CalendarLabels;
    crate::l10n::localizer()
        .scope(|| enzui_core::localization::LocalizedCalendarLabels.weekday_name(day))
}
pub(crate) fn date_label(date: Date) -> String {
    crate::l10n::date(date, false)
}
pub(crate) fn full_date_label(date: Date) -> String {
    crate::l10n::date(date, true)
}
