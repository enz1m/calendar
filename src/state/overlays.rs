//! Overlays and dialogs: editor, import, close, and menus.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DateField {
    Start,
    End,
    Until,
}

#[derive(Debug, Clone)]
pub(crate) struct EditorPickers {
    pub start: DatePickerState,
    pub end: DatePickerState,
    pub until: DatePickerState,
}

impl EditorPickers {
    #[must_use]
    pub(crate) fn new(today: Date, first_weekday: Weekday) -> Self {
        Self {
            start: DatePickerState::new(today, first_weekday, DateBounds::unbounded()),
            end: DatePickerState::new(today, first_weekday, DateBounds::unbounded()),
            until: DatePickerState::new(today, first_weekday, DateBounds::unbounded()),
        }
    }

    #[must_use]
    pub(crate) const fn get(&self, field: DateField) -> &DatePickerState {
        match field {
            DateField::Start => &self.start,
            DateField::End => &self.end,
            DateField::Until => &self.until,
        }
    }

    pub(crate) const fn get_mut(&mut self, field: DateField) -> &mut DatePickerState {
        match field {
            DateField::Start => &mut self.start,
            DateField::End => &mut self.end,
            DateField::Until => &mut self.until,
        }
    }

    pub(crate) fn close_all(&mut self) {
        let _ = self.start.close();
        let _ = self.end.close();
        let _ = self.until.close();
    }

    #[must_use]
    pub(crate) const fn any_open(&self) -> bool {
        self.start.is_open() || self.end.is_open() || self.until.is_open()
    }

    pub(crate) fn set_first_weekday(&mut self, today: Date, first: Weekday) {
        debug_assert!(
            !self.any_open(),
            "rebuilding the picker states remints their trigger ids, so it \
             must not happen while a popover is open and holding a restore target"
        );
        *self = Self::new(today, first);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QuickAdd {
    pub at: iced::Point,
    pub draft: Box<Event>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsInvoker {
    InlineGear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayFocusKind {
    Editor,
    Search,
    Shortcuts,
    Settings,
    DeleteConfirm,
    RecurrencePrompt,
    DayEvents,
    ReminderAlert,
    ImportDestination,
    RetireRecurrenceConfirm,
    Importing,
    Booting,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum Overlay {
    #[default]
    None,
    Editor(Box<Event>),
    Details {
        id: u64,
        on: Option<Date>,
        at: iced::Point,
    },
    QuickAdd(Box<QuickAdd>),
    Search,
    Shortcuts,
    Settings,
    DeleteConfirm(Box<PendingDelete>),
    RecurrencePrompt(Box<PendingRecurrenceChoice>),
    DayEvents(Date),
    ReminderAlert {
        event_id: u64,
        on: Date,
        title: String,
        when: String,
    },
    ImportDestination(Box<PendingImport>),
    Importing {
        generation: u64,
    },
    Booting {
        generation: u64,
    },
    RetireRecurrenceConfirm(Box<Event>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PendingImport {
    pub parsed: crate::ics::ParsedIcs,
}

impl Overlay {
    #[must_use]
    pub(crate) const fn focus_kind(&self) -> Option<OverlayFocusKind> {
        match self {
            Self::Editor(_) => Some(OverlayFocusKind::Editor),
            Self::Search => Some(OverlayFocusKind::Search),
            Self::Shortcuts => Some(OverlayFocusKind::Shortcuts),
            Self::Settings => Some(OverlayFocusKind::Settings),
            Self::DeleteConfirm(_) => Some(OverlayFocusKind::DeleteConfirm),
            Self::RecurrencePrompt(_) => Some(OverlayFocusKind::RecurrencePrompt),
            Self::DayEvents(_) => Some(OverlayFocusKind::DayEvents),
            Self::ReminderAlert { .. } => Some(OverlayFocusKind::ReminderAlert),
            Self::ImportDestination(_) => Some(OverlayFocusKind::ImportDestination),
            Self::RetireRecurrenceConfirm(_) => Some(OverlayFocusKind::RetireRecurrenceConfirm),
            Self::Importing { .. } => Some(OverlayFocusKind::Importing),
            Self::Booting { .. } => Some(OverlayFocusKind::Booting),
            Self::None | Self::Details { .. } | Self::QuickAdd(_) => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ImportReport {
    pub added: usize,
    pub skipped_duplicates: usize,
    pub unsupported: usize,
    pub warnings: Vec<String>,
    pub omitted_warnings: usize,
    pub refused_id_exhaustion: bool,
}

impl ImportReport {
    #[must_use]
    pub(crate) fn summary(&self) -> String {
        if self.refused_id_exhaustion {
            return crate::l10n::tr("This calendar has no room for another event.");
        }
        let mut prefix = crate::l10n::count("calendar-imported-count", self.added);
        if self.skipped_duplicates > 0 {
            prefix.push_str(" · ");
            prefix.push_str(&crate::l10n::count(
                "calendar-duplicates-count",
                self.skipped_duplicates,
            ));
        }
        if self.unsupported > 0 {
            prefix.push_str(" · ");
            prefix.push_str(&crate::l10n::count(
                "calendar-unsupported-count",
                self.unsupported,
            ));
        }

        let warning_count = self.warnings.len().saturating_add(self.omitted_warnings);
        if warning_count == 0 {
            return prefix;
        }
        prefix.push_str(" · ");
        prefix.push_str(&crate::l10n::count(
            "calendar-warnings-count",
            warning_count,
        ));
        prefix.push_str(": ");
        crate::ics::bounded_warning_feedback(&prefix, &self.warnings, self.omitted_warnings)
    }
}

// Import deduplication is scoped to one destination calendar.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum ImportDedupKey {
    Uid(String, Option<i32>),
    Content(u64),
}

pub(super) fn import_dedup_key(e: &Event) -> ImportDedupKey {
    match &e.uid {
        Some(uid) => ImportDedupKey::Uid(uid.clone(), e.recurrence_id.map(Date::to_days)),
        None => ImportDedupKey::Content(e.import_signature()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PersistNotice {
    Quarantined(Option<std::path::PathBuf>),
    ReadOnlyFuture(u32),
    PersistenceFailed(String),
    StartedFresh(std::path::PathBuf),
    RetiredReadOnly(std::path::PathBuf),
    BootRefused(crate::persistence::PersistenceRefusal),
    BootUnavailable(String),
    CustodyLost,
    PersistenceRootUnavailable(crate::persistence::RootRefusal),
}

impl PersistNotice {
    #[must_use]
    pub(crate) const fn is_dismissible(&self) -> bool {
        !matches!(
            self,
            Self::Quarantined(None)
                | Self::ReadOnlyFuture(_)
                | Self::RetiredReadOnly(_)
                | Self::PersistenceFailed(_)
                | Self::BootRefused(_)
                | Self::BootUnavailable(_)
                | Self::CustodyLost
                | Self::PersistenceRootUnavailable(_)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingZone {
    pub fingerprint: u64,
    pub generation: u64,
    pub save_generation: Option<u64>,
    pub reminder_generation: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CloseState {
    Idle,
    Draining {
        window: iced::window::Id,
        deadline: std::time::Instant,
        save_debt: Option<u64>,
        reminder_debt: Option<u64>,
    },
    Vetoed {
        window: iced::window::Id,
        reason: CloseVetoReason,
        save_debt: Option<u64>,
        reminder_debt: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseVetoReason {
    PersistenceFailed,
    ReminderFailed,
    Deadline,
}

impl CloseState {
    #[must_use]
    pub(crate) const fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    #[must_use]
    pub(crate) const fn is_draining(&self) -> bool {
        matches!(self, Self::Draining { .. })
    }

    #[must_use]
    pub(crate) const fn is_vetoed(&self) -> bool {
        matches!(self, Self::Vetoed { .. })
    }

    #[must_use]
    pub(crate) const fn window(&self) -> Option<iced::window::Id> {
        match self {
            Self::Idle => None,
            Self::Draining { window, .. } | Self::Vetoed { window, .. } => Some(*window),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MenuTarget {
    Event { id: u64, on: Option<Date> },
    Slot { date: Date, minute: i32 },
    Day(Date),
    CalendarRow(String),
    MiniDay(Date),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecurrenceActionKind {
    Delete,
    Edit,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PendingDelete {
    pub event_id: u64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PendingRecurrenceChoice {
    pub event_id: u64,
    pub occurrence: Date,
    pub kind: RecurrenceActionKind,
    pub title: String,
}
