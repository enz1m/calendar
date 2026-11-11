//! Overlay opening and event-edit helpers.

use super::*;

pub(super) fn open_details_overlay(
    s: &mut AppState,
    id: u64,
    on: Option<Date>,
    at: Option<iced::Point>,
) {
    s.menu.close();
    if s.event(id).is_some() {
        s.selected = Some(id);
        let at = at.unwrap_or_else(|| iced::Point::new(s.window_width * 0.5, 300.0));
        s.overlay = Overlay::Details { id, on, at };
    }
}

pub(super) fn open_menu(state: &mut AppState, target: MenuTarget, at: iced::Point) {
    use enzui_core::context_menu::{InvocationKind, Point as CorePoint};
    use enzui_widgets::context_menu_host::FocusRestoreTarget;

    let items = crate::view::overlay::menu_items(state, &target);
    let invocation = InvocationKind::Pointer(CorePoint::new(at.x, at.y));
    if let Some(invoker) = crate::view::overlay::menu_invoker_id(&target) {
        state.menu.open_restoring(
            target,
            invocation,
            items,
            FocusRestoreTarget::widget(invoker),
        );
    } else {
        state.menu.open(target, invocation, items);
    }
}

pub(super) fn restore_focus_task(id: iced::advanced::widget::Id) -> Task<Message> {
    use iced::advanced::widget::operation::focusable::focus;
    iced::advanced::widget::operate(focus(id))
}

// Focus restoration is capped to rendered rows because Month hides excess events behind overflow.
pub(super) fn event_focus_after_removal(
    state: &AppState,
    day: enzui_core::civil_date::Date,
    removed_pos: usize,
) -> Option<iced::advanced::widget::Id> {
    let remaining = state.events_on(day);
    let rendered = rendered_event_rows(state, remaining.len());
    enzui_core::focus_after_removal::focus_pos_after_removal(removed_pos, rendered)
        .and_then(|pos| remaining.get(pos))
        .map(|e| crate::view::overlay::event_row_id(e.id, day))
}

pub(super) fn rendered_event_rows(state: &AppState, total: usize) -> usize {
    match state.view {
        model::View::Month => {
            let density = state.content_density();
            total.min(density.month_chips())
        }
        _ => total,
    }
}

// RestoreFocus preserves existing focus if the target has left the tree.
pub(super) fn restore_event_focus_task(id: iced::advanced::widget::Id) -> Task<Message> {
    iced::advanced::widget::operate(enzui_core::focus::RestoreFocus::new(id))
}

pub(super) fn recurrence_edited(orig: &Event, draft: &Event) -> bool {
    orig.recurrence != draft.recurrence
        || orig.recurrence_interval != draft.recurrence_interval
        || orig.recurrence_until != draft.recurrence_until
        || orig.recurrence_count != draft.recurrence_count
        || orig.recurrence_weekdays != draft.recurrence_weekdays
}

pub(super) fn edit(state: &mut AppState, f: impl FnOnce(&mut Event)) {
    let fmt = state.time_format();
    let mut drafts = std::mem::take(&mut state.time_drafts);
    if let Overlay::Editor(e) = &mut state.overlay {
        let times = (e.start_minute, e.end_minute);
        f(e);
        // Unrelated edits must preserve uncommitted time-field text.
        if (e.start_minute, e.end_minute) != times {
            drafts.reseed(e, &fmt);
        }
    }
    state.time_drafts = drafts;
}

pub(super) fn wallclock_endpoints(event: &Event) -> (Date, Date, MinuteOfDay, MinuteOfDay) {
    (
        event.date,
        event.end_date,
        event.start_minute,
        event.end_minute,
    )
}

// Moving either endpoint retires its fixed-UTC anchor: an ambiguous local fold has no unique
// inverse.
pub(super) fn retire_anchor_on_wallclock_change(
    event: &mut Event,
    before: (Date, Date, MinuteOfDay, MinuteOfDay),
) {
    if event.tz_anchor.is_anchored() && wallclock_endpoints(event) != before {
        event.tz_anchor = model::TzAnchor::Floating;
    }
}

pub(super) fn clamp_day_serial(days: i64) -> i32 {
    days.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

// An end exactly at midnight belongs to the previous day at 24:00, avoiding a spurious continuation
// date.
pub(super) fn translate_move(event: &mut Event, column_offset: i32, new_start_minute: i32) {
    let per_day = i64::from(model::MINUTES_PER_DAY);
    let start = i64::from(event.date.to_days()) * per_day + i64::from(event.start_minute.get());
    let end = i64::from(event.end_date.to_days()) * per_day + i64::from(event.end_minute.get());
    let delta =
        i64::from(column_offset) * per_day + i64::from(new_start_minute - event.start_minute.get());
    // Translate both endpoints by one delta to preserve multi-day duration.
    let new_start = start + delta;
    let new_end = end + delta;
    event.date = Date::from_days(clamp_day_serial(new_start.div_euclid(per_day)));
    event.start_minute = MinuteOfDay::new(new_start.rem_euclid(per_day) as i32);
    let mut end_day = new_end.div_euclid(per_day);
    let mut end_min = new_end.rem_euclid(per_day);
    if end_min == 0 && new_end > new_start {
        end_day -= 1;
        end_min = per_day;
    }
    event.end_date = Date::from_days(clamp_day_serial(end_day));
    event.end_minute = MinuteOfDay::new(i32::try_from(end_min).unwrap_or(model::MINUTES_PER_DAY));
}

pub(super) fn stage_recurring_drag_edit(
    state: &mut AppState,
    model_id: u64,
    occurrence: Date,
    column_offset: i32,
    new_start_minute: i32,
    new_end_minute: i32,
    is_move: bool,
) {
    let Some(mut draft) = state
        .event(model_id)
        .map(|series| series.occurrence_on(occurrence))
    else {
        return;
    };
    if is_move {
        translate_move(&mut draft, column_offset, new_start_minute);
    } else {
        draft.date = draft.date.add_days(column_offset);
        draft.end_date = draft.end_date.add_days(column_offset);
        draft.start_minute = MinuteOfDay::new(new_start_minute);
        draft.end_minute = MinuteOfDay::new(new_end_minute);
    }
    draft.tz_anchor = model::TzAnchor::Floating;
    let title = draft.title.clone();
    state.selected = Some(model_id);
    state.pending_recurrence_edit = Some(draft);
    state.overlay = Overlay::RecurrencePrompt(Box::new(PendingRecurrenceChoice {
        event_id: model_id,
        occurrence,
        kind: RecurrenceActionKind::Edit,
        title,
    }));
}

// A detached occurrence is represented by a parent EXDATE and a standalone event with a fresh UID.
pub(super) fn apply_edit_to_this_occurrence(
    state: &mut AppState,
    pending: &PendingRecurrenceChoice,
    mut draft: Event,
) {
    let Some(_) = state.event(pending.event_id) else {
        return;
    };
    let Some(new_id) = state.try_mint_id() else {
        state.refuse_id_exhaustion();
        return;
    };
    if let Some(series) = state.event_mut(pending.event_id)
        && !series.exceptions.contains(&pending.occurrence)
    {
        series.exceptions.push(pending.occurrence);
    }
    draft.id = new_id;
    draft.uid = crate::ics_safety::mint_uid();
    draft.recurrence = model::Recurrence::None;
    draft.recurrence_until = None;
    draft.recurrence_count = None;
    draft.exceptions.clear();
    state.focus = draft.date;
    state.selected = Some(new_id);
    state.publish_toast(
        crate::l10n::format(
            "Saved this occurrence of “{}”",
            &[("arg0", (draft.title).as_str().into())],
        ),
        ToastTone::Success,
    );
    state.events.push(draft);
}

pub(super) fn apply_edit_to_following_occurrences(
    state: &mut AppState,
    pending: &PendingRecurrenceChoice,
    mut draft: Event,
) {
    let Some(original) = state.event(pending.event_id).cloned() else {
        return;
    };
    let delta = draft.date.to_days() - pending.occurrence.to_days();
    let ordinal = occurrence_ordinal(&original, pending.occurrence).unwrap_or(1);
    // Reserve the successor identity before truncating the original series.
    let Some(new_id) = state.try_mint_id() else {
        state.refuse_id_exhaustion();
        return;
    };

    if pending.occurrence <= original.date {
        state.events.retain(|event| event.id != pending.event_id);
    } else if let Some(earlier) = state.event_mut(pending.event_id) {
        earlier.recurrence_until = Some(pending.occurrence.add_days(-1));
        earlier.recurrence_count = None;
    }

    draft.id = new_id;
    draft.uid = crate::ics_safety::mint_uid();
    if draft.recurrence == model::Recurrence::None {
        draft.recurrence_until = None;
        draft.recurrence_count = None;
        draft.exceptions.clear();
    } else {
        // A split series carries only future exception dates, translated with the edited
        // occurrence.
        draft.exceptions = original
            .exceptions
            .iter()
            .copied()
            .filter(|date| *date >= pending.occurrence)
            .map(|date| date.add_days(delta))
            .collect();
        if draft.recurrence_until == original.recurrence_until {
            draft.recurrence_until = original.recurrence_until.map(|date| date.add_days(delta));
        }
        if draft.recurrence_count == original.recurrence_count {
            draft.recurrence_count = original
                .recurrence_count
                .map(|total| total.saturating_sub(ordinal.saturating_sub(1)).max(1));
        }
    }

    state.focus = draft.date;
    state.selected = Some(new_id);
    state.publish_toast(
        crate::l10n::format(
            "Saved this and following occurrences of “{}”",
            &[("arg0", (draft.title).as_str().into())],
        ),
        ToastTone::Success,
    );
    state.events.push(draft);
}

pub(super) fn apply_edit_to_all_occurrences(
    state: &mut AppState,
    pending: &PendingRecurrenceChoice,
    mut draft: Event,
) {
    let Some(original) = state.event(pending.event_id).cloned() else {
        return;
    };
    let edited_occurrence_date = draft.date;
    let delta = draft.date.to_days() - pending.occurrence.to_days();
    let span = draft.span_days();
    draft.id = original.id;
    draft.date = original.date.add_days(delta);
    draft.end_date = draft.date.add_days(span);
    if draft.recurrence == model::Recurrence::None {
        draft.recurrence_until = None;
        draft.recurrence_count = None;
        draft.exceptions.clear();
    } else {
        if draft.recurrence_until == original.recurrence_until {
            draft.recurrence_until = original.recurrence_until.map(|date| date.add_days(delta));
        }
        draft.exceptions = original
            .exceptions
            .iter()
            .copied()
            .map(|date| date.add_days(delta))
            .collect();
    }
    if let Some(slot) = state
        .events
        .iter_mut()
        .find(|event| event.id == original.id)
    {
        *slot = draft.clone();
    }
    state.focus = edited_occurrence_date;
    state.selected = Some(original.id);
    state.publish_toast(
        crate::l10n::format(
            "Saved all occurrences of “{}”",
            &[("arg0", (draft.title).as_str().into())],
        ),
        ToastTone::Success,
    );
}

pub(super) fn occurrence_ordinal(series: &Event, occurrence: Date) -> Option<u32> {
    if occurrence < series.date || !series.occurs_on(occurrence) {
        return None;
    }
    let mut date = series.date;
    let mut ordinal = 0_u32;
    while date <= occurrence {
        if series.occurs_on(date) {
            ordinal = ordinal.saturating_add(1);
        }
        date = date.add_days(1);
    }
    Some(ordinal)
}
