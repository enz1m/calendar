//! Calendar message dispatch and asynchronous result admission.

use super::*;

impl App {
    pub(super) fn reduce(&mut self, message: Message) -> Task<Message> {
        self.retire_offscreen_date_popovers();
        if matches!(message, Message::RetireUnreadableState) {
            self.retire_unreadable_state();
            return match self.pending_save.clone() {
                Some(pending) => self.dispatch_persistence(pending),
                None => self.dispatch_reminder_activation(),
            };
        }
        let s = &mut self.state;
        match message {
            Message::Theme(event) => {
                let before = self.themes.theme_generation();
                let task = self.themes.update(*event);
                debug_assert!(
                    self.themes.theme_generation() >= before,
                    "theme generations must be monotonic"
                );
                return task.map(|event| Message::Theme(Box::new(event)));
            }
            Message::Shortcut(command) => {
                // Modal and context-menu ownership suppresses shortcuts that text fields do not
                // capture.
                if s.overlay != Overlay::None || s.menu.is_open() {
                    return Task::none();
                }
                match command {
                    shortcuts::ShortcutCommand::Today => {
                        s.focus = s.today;
                        s.focus_mini_day(s.today);
                    }
                    shortcuts::ShortcutCommand::PreviousPeriod => s.shift_period(-1),
                    shortcuts::ShortcutCommand::NextPeriod => s.shift_period(1),
                    shortcuts::ShortcutCommand::SelectView(view) => {
                        s.view = view;
                        s.menu.close();
                    }
                    shortcuts::ShortcutCommand::NewEvent => {
                        if let Some(draft) = s.try_draft_at(s.focus, s.settings.work_start_minute) {
                            s.set_overlay(Overlay::Editor(Box::new(draft)));
                        } else {
                            s.refuse_id_exhaustion();
                        }
                    }
                    shortcuts::ShortcutCommand::Search => {
                        s.overlay = Overlay::Search;
                    }
                    shortcuts::ShortcutCommand::DeleteSelected => {
                        if let Some(id) = s.selected {
                            return self.update(Message::RequestDeleteEvent(id));
                        }
                    }
                    shortcuts::ShortcutCommand::ShowShortcuts => {
                        s.overlay = Overlay::Shortcuts;
                    }
                }
            }
            Message::OpenDayView(date) => {
                s.focus = date;
                s.focus_mini_day(date);
                s.view = View::Day;
                s.menu.close();
                s.overlay = Overlay::None;
            }
            Message::MonthDayPressed { date, at, position } => {
                let report = self.click_detector.press(at, position);
                match report.count {
                    enzui_core::click::ClickCount::Single => {
                        s.focus = date;
                        s.focus_mini_day(date);
                        s.menu.close();
                        if s.overlay == Overlay::Search {
                            s.overlay = Overlay::None;
                        }
                    }
                    enzui_core::click::ClickCount::Double
                    | enzui_core::click::ClickCount::Triple => {
                        s.focus = date;
                        s.focus_mini_day(date);
                        s.view = View::Day;
                        s.menu.close();
                        s.overlay = Overlay::None;
                    }
                }
            }
            Message::Schedule(ev) => return self.on_schedule(ev),

            Message::SidebarState(next) => {
                self.nav = next;
                self.state.sidebar_hidden =
                    next.preference == enzui_widgets::nav_split::SidebarPreference::Hidden;
            }
            Message::DismissSidebar => self.nav.dismiss_overlay(),

            Message::OverlayEvent(event) => return self.reduce_overlay_host(event),
            Message::ImportEvents => {
                s.menu.close();
                return match self.picker.start() {
                    Ok(task) => task,
                    Err(outcome) => self.finish_import(outcome),
                };
            }
            Message::ChooseImportDestination(calendar_id) => {
                if let Overlay::ImportDestination(pending) = std::mem::take(&mut s.overlay) {
                    // Stale import completions cannot reopen a committed or cancelled destination
                    // picker.
                    self.pending_import_generation = 0;
                    let crate::state::PendingImport { parsed } = *pending;
                    let report = s.commit_import(parsed, &calendar_id);
                    if report.added > 0
                        && let Some(receipt) = self.pending_import_drop.take()
                    {
                        receipt.finish();
                    }
                    let tone = if report.warnings.is_empty() && report.omitted_warnings == 0 {
                        ToastTone::Success
                    } else {
                        ToastTone::Warning
                    };
                    s.publish_toast(report.summary(), tone);
                }
            }
            Message::ImportFinished {
                generation,
                outcome,
            } => {
                self.apply_import_finished(generation, outcome);
            }
            Message::BootFinished {
                generation,
                outcome,
            } => {
                let Some(outcome) = outcome.take() else {
                    return Task::none();
                };
                let task = self.apply_boot_finished(generation, outcome);
                #[cfg(feature = "center")]
                self.center_after_boot();
                return task;
            }
            #[cfg(feature = "center")]
            Message::Center(event) => return self.center_event(event),
            #[cfg(feature = "center")]
            Message::ApplicationWake => return self.application_wake(),
            #[cfg(feature = "center")]
            Message::BackgroundTaskRefused(error) => {
                eprintln!("calendar-app: background task refused: {error}");
                return Task::none();
            }
            Message::ExportEvents => {
                if self.picker.is_pending() {
                    return Task::none();
                }
                s.menu.close();
                // Close refuses new exports but drains any already authorized snapshot.
                if !s.close_state.is_idle() {
                    s.publish_toast(
                        "Can't export while the window is closing.",
                        ToastTone::Warning,
                    );
                    return Task::none();
                }
                // Export freezes the visible event snapshot at the user gesture.
                let events: Vec<model::Event> = s
                    .events
                    .iter()
                    .filter(|e| s.is_visible(e))
                    .cloned()
                    .collect();
                if events.is_empty() {
                    s.publish_toast(
                        "No visible events to export — turn a calendar on first.",
                        ToastTone::Warning,
                    );
                    return Task::none();
                }
                if events
                    .iter()
                    .all(crate::ics_export::is_unsupported_for_export)
                {
                    s.publish_toast(
                        crate::l10n::format("Couldn't export {} recurring event(s) anchored to UTC — this format can't round-trip them yet.", &[("arg0", (events.len() as i64).into())]),
                        ToastTone::Warning,
                    );
                    return Task::none();
                }
                let stamp = s.today;
                self.pending_export = Some(PendingExport {
                    events,
                    stamp,
                    destination: None,
                    generation: 0,
                    phase: ExportPhase::AwaitingDestination,
                    announced: false,
                });
                return match self
                    .picker
                    .start_save(crate::ics_export::SUGGESTED_FILENAME)
                {
                    Ok(task) => task,
                    Err(outcome) => self.finish_export(outcome),
                };
            }
            Message::PortalTick => return self.picker.pump(),
            Message::ChooserParent(generation, event) => {
                return self.picker.parent_event(generation, event);
            }
            Message::ChooserEvent(generation, event) => {
                if self.picker.accepts(generation) {
                    return self.picker.handle(event);
                }
            }
            Message::ChooserFinished(generation, purpose, outcome) => {
                if !self.picker.accepts(generation) {
                    return Task::none();
                }
                return match purpose {
                    picker::Purpose::Import => self.finish_import(outcome),
                    picker::Purpose::Export => self.finish_export(outcome),
                };
            }
            Message::ExportSettled {
                generation,
                result,
                disclosure,
            } => {
                let settled = self.apply_export_settled(generation, result, disclosure);
                // Busy submissions still owe a settlement even when they never occupy a worker.
                if self.state.close_state.is_draining() {
                    let close = self.try_finish_close().unwrap_or_else(Task::none);
                    return Task::batch([settled, close]);
                }
                return settled;
            }

            Message::EditorDatePicker(field, event) => {
                use enzui_widgets::date_picker::DatePickerEvent;

                let value = match (&s.overlay, field) {
                    (Overlay::Editor(e), state::DateField::Start) => Some(e.date),
                    (Overlay::Editor(e), state::DateField::End) => Some(e.end_date),
                    (Overlay::Editor(e), state::DateField::Until) => e.recurrence_until,
                    _ => None,
                };
                let picker = s.editor_pickers.get_mut(field);
                let trigger_id = picker.trigger_id();
                match event {
                    DatePickerEvent::TriggerPressed => {
                        if let Some(id) = picker.toggle(value, Some(trigger_id)) {
                            return restore_focus_task(id);
                        }
                    }
                    DatePickerEvent::Calendar(ev) => {
                        let (selected, restore) = picker.apply_calendar_event_and_close(ev);
                        if let Some(date) = selected {
                            let commit = match field {
                                state::DateField::Start => Message::EditDate(date),
                                state::DateField::End => Message::EditEndDate(date),
                                state::DateField::Until => Message::EditRecurrenceUntil(date),
                            };
                            let task = self.update(commit);
                            return match restore {
                                Some(id) => Task::batch([task, restore_focus_task(id)]),
                                None => task,
                            };
                        }
                        if let Some(id) = restore {
                            return restore_focus_task(id);
                        }
                    }
                    DatePickerEvent::Dismissed => {
                        // Picker dismissal consumes Escape before the surrounding modal host.
                        let outcome =
                            picker.apply_dismissal(enzui_core::dismissal::Dismissal::Dismiss);
                        if let Some(id) = outcome.restore_focus {
                            return restore_focus_task(id);
                        }
                    }
                }
            }
            Message::DeleteConfirmEvent(event) => {
                use enzui_widgets::confirm_dialog::ConfirmDialogEvent;
                let Overlay::DeleteConfirm(pending) = &s.overlay else {
                    return Task::none();
                };
                let id = pending.event_id;
                match event {
                    // Only DefaultAction confirms deletion; Escape and scrim dismissal cancel.
                    ConfirmDialogEvent::Resolved(cause) => {
                        if enzui_core::confirm::ConfirmDecision::from_cause(cause).is_confirmed() {
                            return self.update(Message::DeleteEvent(id));
                        }
                        s.overlay = Overlay::None;
                    }
                    ConfirmDialogEvent::FocusNext | ConfirmDialogEvent::FocusPrevious => {
                        if let Some(op) = event.focus_operation::<Message>(true) {
                            return iced::advanced::widget::operate(op);
                        }
                    }
                    ConfirmDialogEvent::CheckboxToggled(_) => {}
                }
            }
            Message::DeleteOccurrence { id, on } => {
                let recurring = s
                    .event(id)
                    .is_some_and(|e| e.recurrence != model::Recurrence::None);
                if !recurring {
                    return self.update(Message::RequestDeleteEvent(id));
                }
                let title = s.event(id).map_or_else(String::new, |e| e.title.clone());
                s.menu.close();
                s.overlay = Overlay::RecurrencePrompt(Box::new(PendingRecurrenceChoice {
                    event_id: id,
                    occurrence: on,
                    kind: RecurrenceActionKind::Delete,
                    title,
                }));
            }
            Message::CalendarDialog(event) => return self.calendar_dialog_event(event),
            Message::InstanceWake => return self.instance_wake(),
            Message::DroppedFiles(receipt) => {
                if !matches!(s.overlay, Overlay::None)
                    || self.picker.is_pending()
                    || !s.close_state.is_idle()
                    || receipt.paths().len() != 1
                {
                    return Task::none();
                }
                let task = self.begin_bounded_import(receipt.paths()[0].clone());
                if matches!(self.state.overlay, Overlay::Importing { .. }) {
                    // Success waits for admission and the user's destination choice; cancellation
                    // drops the unfinished COPY receipt without changing the source.
                    self.pending_import_drop = Some(receipt);
                }
                return task;
            }
            Message::ContextMenuEvent(event) => {
                let (chosen, focus) = s.menu.update(&event);
                return match chosen {
                    Some((_, message)) => Task::batch([focus, self.update(message)]),
                    None => focus,
                };
            }

            Message::SnoozeReminder => {
                if let Overlay::ReminderAlert { event_id, on, .. } = &s.overlay {
                    let key = reminders::ReminderKey {
                        event_id: *event_id,
                        on: *on,
                    };
                    reminders::snooze(s, key);
                    self.notifier.withdraw(key);
                    reminders::check_alerts(s);
                }
            }
            Message::DismissReminder => {
                if let Overlay::ReminderAlert { event_id, on, .. } = &s.overlay {
                    let key = reminders::ReminderKey {
                        event_id: *event_id,
                        on: *on,
                    };
                    reminders::dismiss(s, key);
                    self.notifier.withdraw(key);
                    reminders::check_alerts(s);
                }
            }
            // Notification callbacks are admitted against the durable reminder generation.
            Message::NotificationWake => {
                use enzui_core::portal::notification::NotificationEvent;
                for event in self.notifier.drain() {
                    if let NotificationEvent::Action(invocation) = &event {
                        let Some((key, action)) = notify::reminder_action(invocation) else {
                            continue;
                        };
                        match action {
                            notify::ReminderAction::Open => {
                                s.menu.close();
                                s.editing_occurrence = None;
                                s.pending_recurrence_edit = None;
                                if let Some(e) = s.event(key.event_id).cloned() {
                                    s.selected = Some(key.event_id);
                                    s.set_overlay(Overlay::Editor(Box::new(e)));
                                }
                            }
                            notify::ReminderAction::Snooze => {
                                reminders::snooze(s, key);
                                self.notifier.withdraw(key);
                                reminders::check_alerts(s);
                            }
                            notify::ReminderAction::Dismiss => {
                                reminders::dismiss(s, key);
                                self.notifier.withdraw(key);
                                reminders::check_alerts(s);
                            }
                        }
                    } else if let Some(disclosure) = notify::disclosure_for(&event) {
                        s.publish_toast(disclosure.message().to_owned(), ToastTone::Warning);
                    }
                }
                if let Some(disclosure) = self.notifier.queue_pressure() {
                    s.publish_toast(disclosure.message().to_owned(), ToastTone::Warning);
                }
            }
            other => return reducer::reduce_state(s, other),
        }
        Task::none()
    }
}
