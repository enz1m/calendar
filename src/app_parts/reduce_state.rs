//! State-field reducer arms: everything `App::reduce` handles without `self`.

use super::*;

pub(super) fn reduce_state(s: &mut AppState, message: Message) -> Task<Message> {
    match message {
        Message::SelectView(v) => {
            s.view = v;
            s.menu.close();
        }

        Message::SelectViewIndex(i) => {
            s.view = View::ALL.get(i).copied().unwrap_or(View::Week);
        }

        Message::ShiftPeriod(delta) => s.shift_period(delta),

        Message::JumpToday => {
            s.focus = s.today;
            s.focus_mini_day(s.today);
        }

        Message::JumpTo(date) => {
            s.focus = date;
            s.focus_mini_day(date);
            s.menu.close();
            if s.overlay == Overlay::Search {
                s.overlay = Overlay::None;
            }
        }

        Message::MiniCal(event) => {
            if let Some(date) = enzui_widgets::calendar::apply_event(&mut s.mini_controller, event)
            {
                s.focus = date;
                s.menu.close();
                if s.overlay == Overlay::Search {
                    s.overlay = Overlay::None;
                }
            }
        }

        Message::ShowDayEvents(date) => {
            s.menu.close();
            s.focus = date;
            s.focus_mini_day(date);
            s.overlay = Overlay::DayEvents(date);
        }

        Message::ToggleCalendar(id) => {
            s.toggle_calendar(&id);
            s.menu.close();
        }

        Message::OpenEditor(id) => {
            s.menu.close();
            s.editing_occurrence = None;
            s.pending_recurrence_edit = None;
            if let Some(e) = s.event(id).cloned() {
                s.selected = Some(id);
                s.set_overlay(Overlay::Editor(Box::new(e)));
            }
        }

        Message::OpenOccurrenceEditor { id, on } => {
            s.menu.close();
            s.pending_recurrence_edit = None;
            if let Some(series) = s.event(id).cloned() {
                s.selected = Some(id);
                if series.recurrence != model::Recurrence::None && series.occurs_on(on) {
                    s.editing_occurrence = Some(on);
                    s.set_overlay(Overlay::Editor(Box::new(series.occurrence_on(on))));
                } else {
                    s.editing_occurrence = None;
                    s.set_overlay(Overlay::Editor(Box::new(series)));
                }
            }
        }

        Message::OpenDetails { id, on, at } => {
            open_details_overlay(s, id, on, at);
        }

        Message::OpenQuickAdd { date, minute, at } => {
            s.menu.close();
            s.selected = None;
            let Some(draft) = s.try_draft_at(date, minute) else {
                s.refuse_id_exhaustion();
                return Task::none();
            };
            s.set_overlay(Overlay::QuickAdd(Box::new(state::QuickAdd {
                at,
                draft: Box::new(draft),
            })));
            return iced::widget::operation::focus(view::overlay::quick_add_title_id());
        }

        Message::QuickAddTitle(v) => {
            if let Overlay::QuickAdd(q) = &mut s.overlay {
                q.draft.title = v;
            }
        }

        #[cfg(feature = "center")]
        Message::QuickAddDuration(v) => {
            let fmt = s.time_format();
            let mut drafts = std::mem::take(&mut s.time_drafts);
            if let Overlay::QuickAdd(q) = &mut s.overlay {
                let dur = v.max(15);
                q.draft.set_duration_minutes(dur);
                drafts.reseed(&q.draft, &fmt);
            }
            s.time_drafts = drafts;
        }

        Message::QuickAddStartTime(outcome) => {
            let fmt = s.time_format();
            let mut drafts = std::mem::take(&mut s.time_drafts);
            if let Overlay::QuickAdd(q) = &mut s.overlay {
                crate::time_entry::apply_start(&mut q.draft, &mut drafts, &fmt, outcome);
            }
            s.time_drafts = drafts;
        }

        Message::QuickAddEndTime(outcome) => {
            let fmt = s.time_format();
            let mut drafts = std::mem::take(&mut s.time_drafts);
            if let Overlay::QuickAdd(q) = &mut s.overlay {
                crate::time_entry::apply_end(&mut q.draft, &mut drafts, &fmt, outcome);
            }
            s.time_drafts = drafts;
        }

        Message::QuickAddSave => {
            if let Overlay::QuickAdd(q) = std::mem::take(&mut s.overlay) {
                let mut e = *q.draft;
                if e.title.trim().is_empty() {
                    "New Event".clone_into(&mut e.title);
                }
                s.focus = e.date;
                s.selected = Some(e.id);
                s.publish_toast(
                    crate::l10n::format("Saved “{}”", &[("arg0", (e.title).as_str().into())]),
                    ToastTone::Success,
                );
                s.events.push(e);
            }
        }

        Message::QuickAddMoreDetails => {
            if let Overlay::QuickAdd(q) = std::mem::take(&mut s.overlay) {
                s.editing_occurrence = None;
                s.pending_recurrence_edit = None;
                s.set_overlay(Overlay::Editor(q.draft));
            }
        }

        Message::NewEventAt(date, minute) => {
            s.menu.close();
            s.editing_occurrence = None;
            s.pending_recurrence_edit = None;
            if let Some(draft) = s.try_draft_at(date, minute) {
                s.set_overlay(Overlay::Editor(Box::new(draft)));
            } else {
                s.refuse_id_exhaustion();
            }
        }

        Message::NewEvent => {
            s.editing_occurrence = None;
            s.pending_recurrence_edit = None;
            let minute = s.settings.work_start_minute;
            let date = s.focus;
            if let Some(draft) = s.try_draft_at(date, minute) {
                s.set_overlay(Overlay::Editor(Box::new(draft)));
            } else {
                s.refuse_id_exhaustion();
            }
        }

        Message::CloseOverlay => return dismiss_overlay(s),

        Message::PositionedOverlayDismissed(cause) => {
            // Popover dismissal causes remain distinct from explicit surface actions.
            debug_assert!(
                matches!(
                    cause,
                    enzui_core::menu_model::CloseCause::Escape
                        | enzui_core::menu_model::CloseCause::OutsideClick
                ),
                "positioned surfaces report only light-dismiss causes"
            );
            s.overlay = Overlay::None;
            s.selected = None;
        }

        Message::OpenSearch => {
            s.overlay = Overlay::Search;
        }

        Message::OpenSettings(invoker) => {
            s.settings_tab = crate::state::SettingsTab::General;
            s.settings_page_open = false;
            s.settings_menu_open = None;
            s.calendar_editor = None;
            s.settings_invoker = Some(invoker);
            s.titlebar_overflow_open = false;
            s.overlay = Overlay::Settings;
        }

        Message::SettingsExitSettled(key) => s.finish_settings_exit(key),

        Message::SelectSettingsPage(id) => {
            if let Some(tab) = crate::state::SettingsTab::from_id(&id) {
                s.settings_tab = tab;
                s.settings_page_open = true;
            }
        }

        Message::SettingsStackedBack => s.settings_page_open = false,

        Message::Calendar(action) => return crate::calendars::reduce(s, action),
        Message::AddCalendar => {
            if s.calendars.len() >= crate::persistence::MAX_CALENDARS {
                s.publish_toast(
                    "Calendar limit reached. Remove an unused calendar first.",
                    ToastTone::Warning,
                );
                return Task::none();
            }
            let cal = s.add_calendar();
            s.publish_toast(
                crate::l10n::format("Added “{cal}”", &[("cal", (cal).as_str().into())]),
                ToastTone::Success,
            );
        }

        Message::ToggleTitlebarOverflow => {
            s.titlebar_overflow_open = !s.titlebar_overflow_open;
        }

        Message::CloseTitlebarOverflow => {
            // Dismissal must be idempotent: a folded command can already have closed the menu.
            s.titlebar_overflow_open = false;
        }

        Message::ConfirmRetireRecurrence => {
            if let Overlay::RetireRecurrenceConfirm(draft) = std::mem::take(&mut s.overlay) {
                let mut draft = *draft;
                draft.recurrence_raw = None;
                s.overlay = Overlay::Editor(Box::new(draft));
                return Task::done(Message::SaveEvent);
            }
        }

        Message::EditTitle(v) => edit(s, |e| e.title = v),

        Message::EditLocation(v) => edit(s, |e| e.location = v),

        Message::EditNotes(v) => edit(s, |e| e.notes = v),

        Message::EditUrl(v) => edit(s, |e| e.url = v),

        Message::EditInvitees(v) => edit(s, |e| e.invitees = v),

        Message::EditAllDay(v) => edit(s, |e| e.all_day = v),

        Message::EditDate(v) => edit(s, |e| {
            let span = e.span_days();
            e.date = v;
            e.end_date = v.add_days(span);
        }),

        Message::EditEndDate(v) => edit(s, |e| {
            if v >= e.date {
                e.end_date = v;
            }
        }),

        Message::EditStartTime(outcome) => {
            let fmt = s.time_format();
            let mut drafts = std::mem::take(&mut s.time_drafts);
            if let Overlay::Editor(e) = &mut s.overlay {
                crate::time_entry::apply_start(e, &mut drafts, &fmt, outcome);
            }
            s.time_drafts = drafts;
        }

        Message::EditEndTime(outcome) => {
            let fmt = s.time_format();
            let mut drafts = std::mem::take(&mut s.time_drafts);
            if let Overlay::Editor(e) = &mut s.overlay {
                crate::time_entry::apply_end(e, &mut drafts, &fmt, outcome);
            }
            s.time_drafts = drafts;
        }

        Message::EditCalendar(v) => edit(s, |e| e.calendar_id = v),

        Message::EditRecurrence(v) => edit(s, |e| {
            e.recurrence = v;
            // Disabling recurrence clears its bounds so re-enabling cannot revive stale limits.
            if v == model::Recurrence::None {
                e.recurrence_until = None;
                e.recurrence_count = None;
            }
        }),

        Message::EditRecurrenceUntil(v) => edit(s, move |e| {
            if v >= e.date {
                e.recurrence_until = Some(v);
            }
        }),

        Message::EditRecurrenceEndKind(kind) => edit(s, |e| {
            use model::RecurrenceEndKind::{AfterCount, Never, OnDate};
            match kind {
                Never => {
                    e.recurrence_until = None;
                    e.recurrence_count = None;
                }
                OnDate => {
                    e.recurrence_count = None;
                    e.recurrence_until = e.recurrence_until.or_else(|| Some(e.date.add_days(30)));
                }
                AfterCount => {
                    e.recurrence_until = None;
                    e.recurrence_count = e.recurrence_count.or(Some(10));
                }
            }
        }),

        Message::EditRecurrenceCount(v) => edit(s, |e| {
            // A queued control event must not restore a recurrence count cleared by another
            // edit.
            if e.recurrence_count.is_some() {
                e.recurrence_count = Some(v.max(1));
            }
        }),

        Message::EditAlert(v) => edit(s, |e| e.alert = v),

        Message::EditColor(v) => edit(s, |e| e.color_category = v),

        Message::SaveEvent => {
            if let Overlay::Editor(e) = std::mem::take(&mut s.overlay) {
                let e = *e;
                let title = if e.title.trim().is_empty() {
                    "New Event".to_owned()
                } else {
                    e.title.clone()
                };
                let mut e = Event { title, ..e };
                // Retiring an unsupported raw recurrence rule requires confirmation before
                // changing the schedule.
                if e.recurrence_raw.is_some()
                    && s.event(e.id)
                        .is_some_and(|orig| recurrence_edited(orig, &e))
                {
                    s.overlay = Overlay::RetireRecurrenceConfirm(Box::new(e));
                    return Task::none();
                }
                let occurrence = s.editing_occurrence.take();
                let recurring_occurrence = occurrence.filter(|on| {
                    s.event(e.id).is_some_and(|series| {
                        series.recurrence != model::Recurrence::None && series.occurs_on(*on)
                    })
                });
                if let Some(on) = recurring_occurrence {
                    // The original series remains untouched until an occurrence scope is
                    // confirmed.
                    s.pending_recurrence_edit = Some(e.clone());
                    s.overlay = Overlay::RecurrencePrompt(Box::new(PendingRecurrenceChoice {
                        event_id: e.id,
                        occurrence: on,
                        kind: RecurrenceActionKind::Edit,
                        title: e.title.clone(),
                    }));
                } else {
                    if let Some(before) = s.event(e.id).map(wallclock_endpoints) {
                        retire_anchor_on_wallclock_change(&mut e, before);
                    }
                    match s.events.iter_mut().find(|x| x.id == e.id) {
                        Some(slot) => *slot = e.clone(),
                        None => s.events.push(e.clone()),
                    }
                    s.focus = e.date;
                    s.selected = Some(e.id);
                    s.publish_toast(
                        crate::l10n::format("Saved “{}”", &[("arg0", (e.title).as_str().into())]),
                        ToastTone::Success,
                    );
                }
            }
        }

        Message::RequestDeleteEvent(id) => {
            let Some(title) = s.event(id).map(|e| e.title.clone()) else {
                return Task::none();
            };
            s.menu.close();
            s.overlay = Overlay::DeleteConfirm(Box::new(crate::state::PendingDelete {
                event_id: id,
                title,
            }));
        }

        Message::DeleteEvent(id) => {
            let day = s.focus;
            let removed_pos = s.events_on(day).iter().position(|e| e.id == id);
            s.events.retain(|e| e.id != id);
            s.overlay = Overlay::None;
            s.menu.close();
            if s.selected == Some(id) {
                s.selected = None;
            }
            s.publish_toast("Event deleted", ToastTone::Success);
            if let Some(target) = removed_pos.and_then(|pos| event_focus_after_removal(s, day, pos))
            {
                return restore_event_focus_task(target);
            }
        }

        Message::RecurrenceChoiceThis => {
            if let Overlay::RecurrencePrompt(p) = std::mem::take(&mut s.overlay) {
                if p.kind == RecurrenceActionKind::Edit {
                    if let Some(draft) = s.pending_recurrence_edit.take() {
                        apply_edit_to_this_occurrence(s, &p, draft);
                    }
                } else {
                    let occurrence = p.occurrence;
                    if let Some(e) = s.event_mut(p.event_id)
                        && !e.exceptions.contains(&occurrence)
                    {
                        e.exceptions.push(occurrence);
                    }
                    s.publish_toast("Occurrence deleted", ToastTone::Success);
                }
            }
        }

        Message::RecurrenceChoiceFollowing => {
            if let Overlay::RecurrencePrompt(p) = std::mem::take(&mut s.overlay) {
                if p.kind == RecurrenceActionKind::Edit {
                    if let Some(draft) = s.pending_recurrence_edit.take() {
                        apply_edit_to_following_occurrences(s, &p, draft);
                    }
                } else {
                    let base_ge = s.event(p.event_id).is_some_and(|e| e.date >= p.occurrence);
                    if base_ge {
                        s.events.retain(|e| e.id != p.event_id);
                        if s.selected == Some(p.event_id) {
                            s.selected = None;
                        }
                    } else if let Some(e) = s.event_mut(p.event_id) {
                        e.recurrence_until = Some(p.occurrence.add_days(-1));
                    }
                    s.publish_toast("Following occurrences deleted", ToastTone::Success);
                }
            }
        }

        Message::RecurrenceChoiceAll => {
            if let Overlay::RecurrencePrompt(p) = std::mem::take(&mut s.overlay) {
                if p.kind == RecurrenceActionKind::Edit {
                    if let Some(draft) = s.pending_recurrence_edit.take() {
                        apply_edit_to_all_occurrences(s, &p, draft);
                    }
                } else {
                    let id = p.event_id;
                    s.events.retain(|e| e.id != id);
                    if s.selected == Some(id) {
                        s.selected = None;
                    }
                    s.publish_toast("All occurrences deleted", ToastTone::Success);
                }
            }
        }

        Message::DuplicateEvent(id) => {
            s.menu.close();
            if let Some(src) = s.event(id).cloned() {
                let Some(new_id) = s.try_mint_id() else {
                    s.refuse_id_exhaustion();
                    return Task::none();
                };
                s.events.push(Event {
                    id: new_id,
                    title: crate::l10n::format(
                        "{} (copy)",
                        &[("arg0", (src.title).as_str().into())],
                    ),
                    // A duplicate receives its own UID; the source identity must remain unique.
                    uid: crate::ics_safety::mint_uid(),
                    ..src
                });
                s.selected = Some(new_id);
                s.publish_toast("Event duplicated", ToastTone::Success);
            }
        }

        Message::SearchChanged(v) => s.search_query = v,

        Message::JumpToEvent(id) => {
            s.search_query.clear();
            if let Some(e) = s.event(id).cloned() {
                s.view = View::Day;
                let on = e.next_occurrence(s.today).unwrap_or(e.date);
                s.focus = on;
                s.focus_mini_day(on);
                open_details_overlay(s, id, Some(on), None);
            }
        }

        Message::OpenSettingsMenu(menu) => {
            s.settings_menu_open = if s.settings_menu_open == Some(menu) {
                None
            } else {
                Some(menu)
            };
        }

        Message::CloseSettingsMenus => s.settings_menu_open = None,

        Message::SetFirstDayOfWeek(i) => {
            s.settings_menu_open = None;
            // Weekday menu indices and calendar controllers share Monday-first order.
            s.set_first_day_of_week(Weekday::from_index(i));
        }

        Message::SetTimeFormat(i) => {
            s.settings_menu_open = None;
            if let Some(pref) = model::TimeFormatPref::ALL.get(i).copied() {
                s.settings.time_format = pref;
            }
        }

        Message::SetWorkHours(start, end) => {
            let start = start.clamp(0, 23 * 60);
            let end = end.clamp(start + 60, 24 * 60);
            s.settings.work_start_minute = start;
            s.settings.work_end_minute = end;
        }

        Message::SetShowWeekNumbers(v) => s.settings.show_week_numbers = v,

        Message::SetShowSecondaryTz(v) => s.settings.show_secondary_tz = v,

        Message::SetSecondaryTzOffset(v) => {
            s.settings.secondary_tz_offset_minutes = v.clamp(-12 * 60, 14 * 60);
        }

        Message::ContextMenu(target, at) => {
            open_menu(s, target, at);
        }

        Message::WindowResized(w, h) => s.set_window_size(w, h),

        Message::Toast(event) => {
            let _ = s.toasts.update(event);
            let _ = s.toasts.take_replaced();
            let _ = s.toasts.take_dropped();
        }

        Message::DismissPersistNotice => {
            if s.persist_notice
                .as_ref()
                .is_some_and(state::PersistNotice::is_dismissible)
            {
                s.persist_notice = None;
            } else if s.persist_notice.is_some() {
                // A lasting storage state cannot be cleared; closing its notice hides it until
                // the state changes.
                s.persist_notice_hidden = true;
            }
        }

        Message::RetireUnreadableState => {}

        Message::PersistenceFinished { .. }
        | Message::ReminderActivationFinished(..)
        | Message::RetryPersistence
        | Message::CloseDrainTick
        | Message::CloseWindow(_)
        | Message::WindowClosed(_)
        | Message::ZoneObserved(_) => {}

        Message::MinuteTick(today, now) => {
            // Reducer time is supplied by messages rather than sampled from the wall clock.
            s.today = today;
            s.now_minute = now;
            reminders::check_alerts(s);
        }

        _ => unreachable!("reduce_state reached a variant the parent reducer keeps"),
    }
    Task::none()
}
