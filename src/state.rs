//! Application state and the pure queries over it.

use enzui_core::calendar_nav::MonthController;
use enzui_core::civil_date::{Date, DateBounds, MONTH_CELLS, MonthGrid, Weekday, YearMonth};
use enzui_core::time_of_day::{HourCycle, TimeFormat};
use enzui_widgets::date_picker::DatePickerState;
use enzui_widgets::modal_host::{ModalPresentation, ModalPresentationKey};
use enzui_widgets::toast::ToastTone;
use enzui_widgets::toast_host::{ToastExpirySchedule, ToastHost, ToastSpec, WaitingBudget};

use crate::layout::Density;
use crate::model::{Calendar, Event, MinuteOfDay, Settings, View, week_days};

pub(crate) const AGENDA_RANGE_DAYS: i32 = 30;

pub(crate) const DEFAULT_SYSTEM_HOUR_CYCLE: HourCycle = HourCycle::H12;

pub(crate) const TOAST_ID_PREFIX: &str = "calendar-action-feedback";
const TOAST_COALESCE_KEY: &str = "calendar-action-feedback";

#[path = "state/occurrence_ids.rs"]
mod occurrence_ids;

#[path = "state/editing.rs"]
mod editing;
#[path = "state/overlays.rs"]
mod overlays;

use overlays::EditorPickers;
pub(crate) use overlays::{
    CloseState, CloseVetoReason, DateField, ImportReport, MenuTarget, Overlay, OverlayFocusKind,
    PendingDelete, PendingImport, PendingRecurrenceChoice, PendingZone, PersistNotice, QuickAdd,
    RecurrenceActionKind, SettingsInvoker,
};

#[derive(Debug)]
pub(crate) struct AppState {
    pub calendars: Vec<Calendar>,
    pub events: Vec<Event>,
    pub next_id: u64,
    pub view: View,
    pub focus: Date,
    pub today: Date,
    pub now_minute: i32,
    pub observed_zone: u64,
    pub zone_generation: u64,
    pub pending_zone: Option<PendingZone>,
    pub mini_controller: MonthController,
    pub settings: Settings,
    pub system_hour_cycle: HourCycle,
    pub overlay: Overlay,
    pub overlay_focus: Option<(
        OverlayFocusKind,
        enzui_widgets::modal_host::ModalFocusLifecycle,
    )>,
    pub menu: enzui_widgets::context_menu_host::ContextMenu<MenuTarget, crate::Message>,
    pub selected: Option<u64>,
    pub editing_occurrence: Option<Date>,
    pub pending_recurrence_edit: Option<Event>,
    pub hidden_categories: Vec<String>,
    pub search_query: String,
    pub toasts: ToastHost,
    pub toast_expiry: ToastExpirySchedule,
    pub persist_notice: Option<PersistNotice>,
    /// The user closed a storage notice that reports a lasting state; it stays hidden until that
    /// state changes or a close is vetoed.
    pub persist_notice_hidden: bool,
    pub close_state: CloseState,
    pub delete_confirm_state: enzui_core::confirm::ConfirmState,
    pub time_drafts: crate::time_entry::TimeDrafts,
    pub window_width: f32,
    pub sizing: enzui_theme::SizingTokens,
    // Occurrence keys survive root state replacement so queued widget events retain identity.
    pub(crate) occurrence_ids: std::sync::Arc<std::sync::Mutex<occurrence_ids::OccurrenceIds>>,
    pub window_height: f32,
    pub settings_presentation: ModalPresentation<()>,
    pub settings_invoker: Option<SettingsInvoker>,
    pub sidebar_hidden: bool,
    pub reminders: crate::reminders::ReminderLog,
    pub editor_pickers: EditorPickers,
    pub titlebar_overflow_open: bool,
    pub settings_tab: SettingsTab,
    /// A page is open over the page list in the narrow-window (stacked)
    /// settings navigation; Back returns to the list.
    pub settings_page_open: bool,
    pub settings_menu_open: Option<SettingsMenu>,
    pub calendar_editor: Option<crate::calendars::Editor>,
    pub calendar_dialogs:
        enzui_widgets::modal_host::DialogStack<crate::calendar_dialogs::CalendarDialog>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsMenu {
    WeekStart,
    TimeFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SettingsTab {
    #[default]
    General,
    Calendars,
}

impl SettingsTab {
    pub(crate) const ALL: [SettingsTab; 2] = [SettingsTab::General, SettingsTab::Calendars];

    #[must_use]
    pub(crate) fn id(self) -> &'static str {
        match self {
            SettingsTab::General => "general",
            SettingsTab::Calendars => "calendars",
        }
    }

    #[must_use]
    pub(crate) fn title(self) -> &'static str {
        match self {
            SettingsTab::General => "General",
            SettingsTab::Calendars => "Calendars",
        }
    }

    #[must_use]
    pub(crate) fn from_id(id: &str) -> Option<SettingsTab> {
        SettingsTab::ALL.into_iter().find(|t| t.id() == id)
    }
}

impl AppState {
    pub(crate) fn publish_toast(&mut self, text: impl Into<String>, tone: ToastTone) {
        self.toasts.push(
            ToastSpec::new(crate::l10n::tr(text.into()))
                .tone(tone)
                .coalesce_key(TOAST_COALESCE_KEY),
        );
        let _ = self.toasts.take_replaced();
        let _ = self.toasts.take_dropped();
    }

    pub(crate) fn clear_toasts(&mut self) {
        for id in self.toasts.ids() {
            self.toasts.dismiss(id);
        }
        let _ = self.toasts.take_replaced();
        let _ = self.toasts.take_dropped();
    }

    #[must_use]
    pub(crate) fn new(today: Date, now_minute: i32) -> Self {
        let (calendars, events, next_id) = crate::seed::seed(today);
        Self::with_data(
            today,
            now_minute,
            calendars,
            events,
            next_id,
            Settings::default(),
            Vec::new(),
            crate::reminders::ReminderLog::default(),
        )
    }

    #[must_use]
    pub(crate) fn empty(today: Date, now_minute: i32) -> Self {
        let default_calendar = Calendar {
            id: "cal.home".to_owned(),
            name: crate::l10n::tr("Home"),
            section: "On This Device".into(),
            color: "blue".to_owned(),
            visible: true,
        };
        Self::with_data(
            today,
            now_minute,
            vec![default_calendar],
            Vec::new(),
            1,
            Settings::default(),
            Vec::new(),
            crate::reminders::ReminderLog::default(),
        )
    }

    #[must_use]
    pub(crate) fn pre_boot(today: Date, now_minute: i32, generation: u64) -> Self {
        let mut s = Self::with_data(
            today,
            now_minute,
            Vec::new(),
            Vec::new(),
            1,
            Settings::default(),
            Vec::new(),
            crate::reminders::ReminderLog::default(),
        );
        s.overlay = Overlay::Booting { generation };
        s
    }

    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "Hydrates persisted state without changing field ownership"
    )]
    pub(crate) fn with_data(
        today: Date,
        now_minute: i32,
        calendars: Vec<Calendar>,
        events: Vec<Event>,
        next_id: u64,
        settings: Settings,
        hidden_categories: Vec<String>,
        reminders: crate::reminders::ReminderLog,
    ) -> Self {
        let first_weekday = settings.first_day_of_week;
        Self {
            system_hour_cycle: DEFAULT_SYSTEM_HOUR_CYCLE,
            time_drafts: crate::time_entry::TimeDrafts::default(),
            calendars,
            events,
            next_id,
            view: View::Week,
            focus: today,
            today,
            now_minute,
            observed_zone: crate::sysclock::zone_fingerprint(),
            zone_generation: 0,
            pending_zone: None,
            mini_controller: MonthController::new(today, first_weekday, DateBounds::unbounded()),
            settings,
            overlay: Overlay::None,
            overlay_focus: None,
            menu: enzui_widgets::context_menu_host::ContextMenu::new(),
            selected: None,
            editing_occurrence: None,
            pending_recurrence_edit: None,
            hidden_categories,
            search_query: String::new(),
            toasts: ToastHost::new()
                .id_prefix(TOAST_ID_PREFIX)
                .max_visible(1)
                .waiting_budget(WaitingBudget::unbounded().max_waiting(Some(0))),
            toast_expiry: ToastExpirySchedule::default(),
            persist_notice: None,
            persist_notice_hidden: false,
            close_state: CloseState::Idle,
            delete_confirm_state: enzui_core::confirm::ConfirmState::new(None),
            window_width: 1400.0,
            sizing: enzui_theme::SizingTokens::default(),
            occurrence_ids: std::sync::Arc::default(),
            window_height: 900.0,
            settings_presentation: ModalPresentation::default(),
            settings_invoker: None,
            sidebar_hidden: false,
            reminders,
            editor_pickers: EditorPickers::new(today, first_weekday),
            titlebar_overflow_open: false,
            settings_tab: SettingsTab::default(),
            settings_page_open: false,
            settings_menu_open: None,
            calendar_editor: None,
            calendar_dialogs: enzui_widgets::modal_host::DialogStack::new(),
        }
    }

    #[must_use]
    pub(crate) fn fixed_utc_projection_would_change(&self) -> bool {
        self.events
            .iter()
            .filter(|e| e.tz_anchor.is_anchored())
            .any(|event| {
                let mut trial = event.clone();
                trial.reproject_fixed_utc(crate::sysclock::local_offset_at_utc);
                (
                    trial.date,
                    trial.end_date,
                    trial.start_minute,
                    trial.end_minute,
                    trial.tz_anchor,
                ) != (
                    event.date,
                    event.end_date,
                    event.start_minute,
                    event.end_minute,
                    event.tz_anchor,
                )
            })
    }

    pub(crate) fn reproject_fixed_utc_events(&mut self) -> bool {
        let mut changed = false;
        // Zone reprojection rekeys fired dates but cannot change UTC snooze deadlines.
        let mut date_moves: Vec<(u64, Date, Date)> = Vec::new();
        for event in &mut self.events {
            if !event.tz_anchor.is_anchored() {
                continue;
            }
            let before = (
                event.date,
                event.end_date,
                event.start_minute,
                event.end_minute,
                event.tz_anchor,
            );
            event.reproject_fixed_utc(crate::sysclock::local_offset_at_utc);
            let after = (
                event.date,
                event.end_date,
                event.start_minute,
                event.end_minute,
                event.tz_anchor,
            );
            changed |= before != after;
            if before.0 != after.0 {
                date_moves.push((event.id, before.0, after.0));
            }
        }
        for (event_id, from, to) in date_moves {
            self.reminders.remap_occurrence(event_id, from, to);
        }
        changed
    }

    #[must_use]
    pub(crate) fn window_size(&self) -> iced::Size {
        let width = if self.window_width > 0.5 {
            self.window_width
        } else {
            1400.0
        };
        let height = if self.window_height > 0.5 {
            self.window_height
        } else {
            900.0
        };
        iced::Size::new(width, height)
    }

    pub(crate) fn sync_settings_presentation(&mut self) {
        let showing = matches!(self.overlay, Overlay::Settings);
        if showing {
            if !self.settings_presentation.is_open() {
                let _ = self.settings_presentation.present(());
            }
            return;
        }
        if self.settings_presentation.is_open() {
            let _ = self.settings_presentation.dismiss();
        }
        if self.overlay != Overlay::None
            && let Some(key) = self.settings_presentation.key()
        {
            // A new modal retires outgoing Settings paint so a dismissed dialog cannot reappear.
            let _ = self.settings_presentation.finish_exit(key);
        }
    }

    #[must_use]
    pub(crate) fn settings_presentation_key(&self) -> Option<ModalPresentationKey> {
        self.settings_presentation.key()
    }

    #[must_use]
    pub(crate) fn settings_is_closing(&self) -> bool {
        self.settings_presentation.is_closing()
    }

    pub(crate) fn finish_settings_exit(&mut self, key: ModalPresentationKey) {
        let _ = self.settings_presentation.finish_exit(key);
    }

    pub(crate) fn set_window_size(&mut self, width: f32, height: f32) {
        self.window_width = width;
        self.window_height = height;
    }

    #[must_use]
    pub(crate) fn time_format(&self) -> TimeFormat {
        let localizer = crate::l10n::localizer();
        let system = if std::env::var_os("ENZUI_CALENDAR_CLOCK").is_some() {
            self.system_hour_cycle
        } else {
            localizer
                .locale_for(enzui_core::localization::LocaleCategory::Time)
                .hour_cycle()
                .unwrap_or(self.system_hour_cycle)
        };
        TimeFormat::new(self.settings.time_format.hour_cycle(system)).with_day_period_labels(
            enzui_core::time_of_day::DayPeriodLabels::new(
                localizer.text("enzui-time-am"),
                localizer.text("enzui-time-pm"),
            ),
        )
    }

    pub(crate) fn set_overlay(&mut self, overlay: Overlay) {
        let fmt = self.time_format();
        match &overlay {
            Overlay::Editor(event) => {
                self.time_drafts = crate::time_entry::TimeDrafts::seeded(event, &fmt);
            }
            Overlay::QuickAdd(quick) => {
                self.time_drafts = crate::time_entry::TimeDrafts::seeded(&quick.draft, &fmt);
            }
            _ => {}
        }
        self.overlay = overlay;
    }

    #[must_use]
    pub(crate) fn content_density(&self) -> Density {
        Density::of_content_pane_sizing(self.window_width, self.sidebar_hidden, &self.sizing)
    }

    pub(crate) fn shift_period(&mut self, delta: i32) {
        self.focus = match self.view {
            View::Day => self.focus.add_days(delta),
            // Compact Week navigation advances by the rendered three-day page, without skipping
            // dates.
            View::Week => self
                .focus
                .add_days(delta * self.content_density().week_columns() as i32),
            View::Agenda => self.focus.add_days(delta * AGENDA_RANGE_DAYS),
            View::Month => self.focus.add_months(delta),
            View::Year => self.focus.add_years(delta),
        };
        self.focus_mini_day(self.focus);
    }

    #[must_use]
    pub(crate) fn mini_month(&self) -> YearMonth {
        self.mini_controller.visible_month()
    }

    pub(crate) fn set_mini_month(&mut self, ym: YearMonth) {
        let _ = self.mini_controller.jump_to_month(ym);
    }

    pub(crate) fn focus_mini_day(&mut self, date: Date) {
        let _ = self.mini_controller.focus_date(date);
    }

    pub(crate) fn set_first_day_of_week(&mut self, first: Weekday) {
        self.settings.first_day_of_week = first;
        self.editor_pickers.set_first_weekday(self.today, first);
        self.set_mini_first_weekday(first);
    }

    pub(crate) fn set_mini_first_weekday(&mut self, first: Weekday) {
        let visible = self.mini_controller.visible_month();
        let focused = self.mini_controller.focused();
        self.mini_controller = MonthController::new(focused, first, DateBounds::unbounded());
        self.set_mini_month(visible);
    }

    #[must_use]
    pub(crate) fn week_number_label(&self) -> Option<String> {
        use crate::model::iso_week_number;
        if !self.settings.show_week_numbers {
            return None;
        }
        let anchor = match self.view {
            View::Day => self.focus,
            View::Week => *self.focused_week().first()?,
            View::Month | View::Year | View::Agenda => return None,
        };
        Some(crate::l10n::format(
            "Week {}",
            &[("arg0", i64::from(iso_week_number(anchor)).into())],
        ))
    }

    #[must_use]
    pub(crate) fn period_label(&self) -> String {
        use crate::model::{full_date_label, month_year_label};
        match self.view {
            View::Day => full_date_label(self.focus),
            View::Week => {
                let days = self.focused_week();
                let (a, b) = (days[0], days[6]);
                if a.month() == b.month() {
                    month_year_label(YearMonth::of(a))
                } else {
                    format!(
                        "{} – {}",
                        month_year_label(YearMonth::of(a)),
                        month_year_label(YearMonth::of(b))
                    )
                }
            }
            View::Month => month_year_label(YearMonth::of(self.focus)),
            View::Year => {
                use enzui_core::calendar_semantics::CalendarLabels;
                crate::l10n::localizer().scope(|| {
                    enzui_core::localization::LocalizedCalendarLabels.year_number(self.focus.year())
                })
            }
            View::Agenda => format!(
                "{} – {}",
                crate::l10n::date(self.focus, false),
                crate::l10n::date(self.focus.add_days(AGENDA_RANGE_DAYS - 1), false)
            ),
        }
    }

    pub(crate) fn add_calendar(&mut self) -> String {
        let n = (1..)
            .find(|n| {
                let id = format!("cal.user{n}");
                let name = crate::l10n::format("Calendar {n}", &[("n", i64::from(*n).into())]);
                !self.calendars.iter().any(|c| c.id == id || c.name == name)
            })
            .unwrap_or(1);
        let keys = crate::model::CALENDAR_COLOR_KEYS;
        let color = keys[self.calendars.len() % keys.len()];
        let name = crate::l10n::format("Calendar {n}", &[("n", i64::from(n).into())]);
        self.calendars.push(Calendar {
            id: format!("cal.user{n}"),
            name: name.clone(),
            section: "Other".into(),
            color: color.to_owned(),
            visible: true,
        });
        name
    }

    pub(crate) fn toggle_calendar(&mut self, id: &str) {
        if let Some(c) = self.calendars.iter_mut().find(|c| c.id == id) {
            c.visible = !c.visible;
        }
    }

    #[must_use]
    pub(crate) fn calendar_sections(&self) -> Vec<(String, Vec<&Calendar>)> {
        let mut out: Vec<(String, Vec<&Calendar>)> = Vec::new();
        for c in &self.calendars {
            match out.iter_mut().find(|(s, _)| *s == c.section) {
                Some((_, v)) => v.push(c),
                None => out.push((c.section.clone(), vec![c])),
            }
        }
        out
    }
}

#[must_use]
pub(crate) fn mini_month_cells(
    month: YearMonth,
    first: Weekday,
) -> Vec<enzui_core::civil_date::DayCell> {
    let grid = MonthGrid::build(month, first, Default::default());
    let cells = grid.cells();
    debug_assert_eq!(
        cells.len(),
        MONTH_CELLS,
        "month grids contain exactly six complete weeks"
    );
    cells.to_vec()
}

impl AppState {
    pub(crate) fn schedule_block_id(&self, event_id: u64, date: Date) -> Option<u64> {
        self.occurrence_ids.lock().ok()?.key_for(event_id, date)
    }

    pub(crate) fn schedule_occurrence(&self, key: u64) -> Option<(u64, Date)> {
        self.occurrence_ids.lock().ok()?.pair_for(key)
    }

    pub(crate) fn retain_schedule_blocks(&self, keys: impl IntoIterator<Item = u64>) {
        if let Ok(mut ids) = self.occurrence_ids.lock() {
            ids.retain_keys(keys);
        }
    }
}
