//! Calendar keyboard shortcuts: one table for dispatch and discoverability.

use enzui_core::shortcut::{Modifier, Shortcut};
use enzui_core::{ShortcutBinding, dispatch_shortcut_command};
use enzui_widgets::shortcut_list::{ShortcutEntry, shortcut_catalog_entries};
use iced::keyboard::key::Named;

use crate::model::View;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShortcutCommand {
    Today,
    PreviousPeriod,
    NextPeriod,
    SelectView(View),
    NewEvent,
    Search,
    DeleteSelected,
    ShowShortcuts,
}

fn bindings() -> Vec<ShortcutBinding<ShortcutCommand>> {
    vec![
        action(
            "calendar.today",
            "Today",
            ShortcutCommand::Today,
            Shortcut::new(Modifier::NONE, 't'),
        ),
        action(
            "calendar.previous",
            "Previous period",
            ShortcutCommand::PreviousPeriod,
            Shortcut::new(Modifier::NONE, 'p'),
        ),
        action(
            "calendar.next",
            "Next period",
            ShortcutCommand::NextPeriod,
            Shortcut::new(Modifier::NONE, 'n'),
        ),
        action(
            "view.day",
            "Day view",
            ShortcutCommand::SelectView(View::Day),
            Shortcut::new(Modifier::NONE, 'd'),
        ),
        action(
            "view.week",
            "Week view",
            ShortcutCommand::SelectView(View::Week),
            Shortcut::new(Modifier::NONE, 'w'),
        ),
        action(
            "view.month",
            "Month view",
            ShortcutCommand::SelectView(View::Month),
            Shortcut::new(Modifier::NONE, 'm'),
        ),
        action(
            "view.year",
            "Year view",
            ShortcutCommand::SelectView(View::Year),
            Shortcut::new(Modifier::NONE, 'y'),
        ),
        action(
            "view.agenda",
            "Agenda view",
            ShortcutCommand::SelectView(View::Agenda),
            Shortcut::new(Modifier::NONE, 'a'),
        ),
        action(
            "event.new",
            "New event",
            ShortcutCommand::NewEvent,
            Shortcut::new(Modifier::CTRL, 'n'),
        ),
        action(
            "event.search",
            "Search events",
            ShortcutCommand::Search,
            Shortcut::new(Modifier::CTRL, 'f'),
        ),
        action(
            "event.delete",
            "Delete selected event",
            ShortcutCommand::DeleteSelected,
            Shortcut::new(Modifier::NONE, Named::Delete),
        ),
        action(
            "help.shortcuts",
            "Keyboard shortcuts",
            ShortcutCommand::ShowShortcuts,
            Shortcut::new(Modifier::NONE, Named::F1),
        ),
    ]
}

fn action(
    id: &'static str,
    label: &'static str,
    command: ShortcutCommand,
    shortcut: Shortcut,
) -> ShortcutBinding<ShortcutCommand> {
    ShortcutBinding::new(id, command, shortcut)
        .with_description(crate::l10n::tr(label))
        .with_section(crate::l10n::tr("Calendar"))
}

#[must_use]
pub(crate) fn dispatch(event: &iced::keyboard::Event) -> Option<ShortcutCommand> {
    dispatch_shortcut_command(&bindings(), event, &())
}

#[must_use]
pub(crate) fn shortcut_entries() -> Vec<ShortcutEntry> {
    shortcut_catalog_entries(&bindings())
}
