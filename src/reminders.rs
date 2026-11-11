//! Reminder planning, catch-up limits, snooze deadlines, and persisted fired marks.

use enzui_core::civil_date::Date;

use crate::model::Event;
use crate::state::{AppState, Overlay};

pub(crate) const MINUTES_PER_DAY: i64 = 1440;

pub(crate) const SNOOZE_MINUTES: i64 = 5;

pub(crate) const MISSED_WINDOW_MINUTES: i64 = 24 * 60;

pub(crate) const MAX_MISSED_AT_ONCE: usize = 3;

pub(crate) const LIVE_TOLERANCE_MINUTES: i64 = 2;

pub(crate) const LOG_RETENTION_DAYS: i32 = 30;

#[must_use]
pub(crate) fn abs_minute(date: Date, minute: i32) -> i64 {
    i64::from(date.to_days()) * MINUTES_PER_DAY + i64::from(minute)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ReminderKey {
    pub event_id: u64,
    pub on: Date,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Snoozed {
    pub key: ReminderKey,
    pub until_utc: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ReminderLog {
    pub fired: Vec<ReminderKey>,
    pub snoozed: Vec<Snoozed>,
    pub last_checked: Option<i64>,
    pub reconciliation_debt: Vec<Snoozed>,
}

impl ReminderLog {
    #[must_use]
    pub(crate) fn has_fired(&self, key: ReminderKey) -> bool {
        self.fired.contains(&key)
    }

    pub(crate) fn mark_fired(&mut self, key: ReminderKey) {
        if !self.has_fired(key) {
            self.fired.push(key);
        }
    }

    pub(crate) fn snooze(&mut self, key: ReminderKey, until_utc: i64) {
        self.clear_snooze(key);
        self.snoozed.push(Snoozed { key, until_utc });
    }

    pub(crate) fn clear_snooze(&mut self, key: ReminderKey) {
        self.snoozed.retain(|s| s.key != key);
    }

    pub(crate) fn remap_occurrence(&mut self, event_id: u64, from: Date, to: Date) {
        if from == to {
            return;
        }
        for key in &mut self.fired {
            if key.event_id == event_id && key.on == from {
                key.on = to;
            }
        }
        for snoozed in &mut self.snoozed {
            if snoozed.key.event_id == event_id && snoozed.key.on == from {
                // Rekey only the occurrence date; a snooze's UTC deadline is invariant.
                snoozed.key.on = to;
            }
        }
    }

    #[must_use]
    pub(crate) fn snoozed_until(&self, key: ReminderKey) -> Option<i64> {
        self.snoozed
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.until_utc)
    }

    pub(crate) fn prune(&mut self, today: Date, live_event_ids: &[u64]) {
        let horizon = today.add_days(-LOG_RETENTION_DAYS);
        self.fired.retain(|k| k.on >= horizon);
        self.snoozed
            .retain(|s| s.key.on >= horizon && live_event_ids.contains(&s.key.event_id));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Due {
    pub key: ReminderKey,
    pub title: String,
    pub start_minute: i32,
    pub all_day: bool,
    pub at: i64,
    pub from_snooze: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    pub fire: Vec<Due>,
    pub suppress: Vec<ReminderKey>,
}

#[must_use]
pub(crate) fn floor_of(now: i64, log: &ReminderLog) -> i64 {
    log.last_checked
        .unwrap_or(now)
        .clamp(now - MISSED_WINDOW_MINUTES, now)
}

#[must_use]
fn now_utc_of(now: i64) -> i64 {
    match crate::sysclock::local_minute_to_utc(now) {
        // Live scheduling chooses the earlier instant during a fold so a due snooze is not refused.
        crate::sysclock::LocalInstant::Utc(utc) | crate::sysclock::LocalInstant::Ambiguous(utc) => {
            utc
        }
        crate::sysclock::LocalInstant::Unresolvable => now,
    }
}

#[must_use]
pub(crate) fn plan<'a>(
    events: impl IntoIterator<Item = &'a Event>,
    now: i64,
    now_utc: i64,
    log: &ReminderLog,
) -> Plan {
    let floor = floor_of(now, log);
    let offset_now = now - now_utc;
    let floor_utc = floor - offset_now;
    let mut eligible: Vec<Due> = Vec::new();

    for event in events {
        let Some(alert) = event.alert else {
            continue;
        };
        let lead = i64::from(alert);
        let start_minute = if event.all_day {
            0
        } else {
            event.start_minute.get()
        };
        // An alert in (floor, now] comes from a start in (floor + lead, now + lead].
        let first_day = (floor + lead).div_euclid(MINUTES_PER_DAY);
        let last_day = (now + lead).div_euclid(MINUTES_PER_DAY);
        for day in first_day..=last_day {
            let Ok(day) = i32::try_from(day) else {
                continue;
            };
            let date = Date::from_days(day);
            if !event.occurs_on(date) {
                continue;
            }
            let at = abs_minute(date, start_minute) - lead;
            if at <= floor || at > now {
                continue;
            }
            let key = ReminderKey {
                event_id: event.id,
                on: date,
            };
            if log.has_fired(key) {
                continue;
            }
            eligible.push(Due {
                key,
                title: event.title.clone(),
                start_minute,
                all_day: event.all_day,
                at,
                from_snooze: false,
            });
        }

        // Snooze can fire despite its fired mark, but only while its occurrence still exists.
        for snoozed in &log.snoozed {
            if snoozed.key.event_id != event.id {
                continue;
            }
            if !event.occurs_on(snoozed.key.on) {
                continue;
            }
            // Local projections order reminders; UTC deadlines determine when snoozes are due.
            if snoozed.until_utc <= floor_utc || snoozed.until_utc > now_utc {
                continue;
            }
            eligible.push(Due {
                key: snoozed.key,
                title: event.title.clone(),
                start_minute,
                all_day: event.all_day,
                at: snoozed.until_utc + offset_now,
                from_snooze: true,
            });
        }
    }

    eligible.sort_by(|a, b| a.at.cmp(&b.at).then(a.key.cmp(&b.key)));
    eligible.dedup_by(|a, b| a.key == b.key && a.at == b.at);

    // Catch-up limits do not cap alerts first observed on an ordinary minute tick.
    let live_from = now - LIVE_TOLERANCE_MINUTES;
    let (live, missed): (Vec<Due>, Vec<Due>) = eligible.into_iter().partition(|d| d.at > live_from);

    let mut suppress = Vec::new();
    let mut kept = missed;
    if kept.len() > MAX_MISSED_AT_ONCE {
        let drop_count = kept.len() - MAX_MISSED_AT_ONCE;
        for due in kept.drain(..drop_count) {
            suppress.push(due.key);
        }
    }
    kept.extend(live);
    kept.sort_by(|a, b| a.at.cmp(&b.at).then(a.key.cmp(&b.key)));
    Plan {
        fire: kept,
        suppress,
    }
}

pub(crate) fn apply_claimed(state: &mut AppState, key: ReminderKey) -> bool {
    let snoozed_until = state.reminders.snoozed_until(key);
    if (state.reminders.has_fired(key) && snoozed_until.is_none())
        || !matches!(state.overlay, Overlay::None)
    {
        return false;
    }
    let Some(event) = state
        .event(key.event_id)
        .filter(|event| event.alert.is_some() && event.occurs_on(key.on) && state.is_visible(event))
        .cloned()
    else {
        return false;
    };
    let due_at = snoozed_until.unwrap_or_else(|| {
        abs_minute(key.on, event.start_minute.get()) - i64::from(event.alert.unwrap_or(0))
    });
    let now = abs_minute(state.today, state.now_minute);
    // Occurrence lead times and snooze deadlines are checked in their own local and UTC frames.
    let now_ref = if snoozed_until.is_some() {
        now_utc_of(now)
    } else {
        now
    };
    if due_at > now_ref || due_at < now_ref.saturating_sub(MISSED_WINDOW_MINUTES) {
        return false;
    }
    let due = Due {
        key,
        title: event.title.clone(),
        start_minute: event.start_minute.get(),
        all_day: event.all_day,
        at: due_at,
        from_snooze: snoozed_until.is_some(),
    };
    state.reminders.mark_fired(key);
    state.reminders.clear_snooze(key);
    state.overlay = Overlay::ReminderAlert {
        event_id: key.event_id,
        on: key.on,
        title: event.title,
        when: reminder_when(&due, &state.time_format()),
    };
    true
}

pub(crate) fn check_alerts(state: &mut AppState) -> bool {
    let now = abs_minute(state.today, state.now_minute);
    let live_ids: Vec<u64> = state.events.iter().map(|e| e.id).collect();
    state.reminders.prune(state.today, &live_ids);

    if !matches!(state.overlay, Overlay::None) {
        return false;
    }

    // Hidden calendars do not interrupt with reminders.
    let visible: Vec<&Event> = state
        .events
        .iter()
        .filter(|e| state.is_visible(e))
        .collect();
    let now_utc = now_utc_of(now);
    let plan = plan(visible, now, now_utc, &state.reminders);

    for key in &plan.suppress {
        state.reminders.mark_fired(*key);
        state.reminders.clear_snooze(*key);
    }

    let Some(due) = plan.fire.first().cloned() else {
        state.reminders.last_checked = Some(now);
        return false;
    };
    state.reminders.mark_fired(due.key);
    state.reminders.clear_snooze(due.key);
    state.overlay = Overlay::ReminderAlert {
        event_id: due.key.event_id,
        on: due.key.on,
        title: due.title.clone(),
        when: reminder_when(&due, &state.time_format()),
    };
    // A raised alert holds the catch-up floor until the queue drains; its durable fired mark
    // prevents repetition.
    true
}

#[must_use]
pub(crate) fn reminder_when(due: &Due, fmt: &enzui_core::time_of_day::TimeFormat) -> String {
    let date = crate::model::date_label(due.key.on);
    if due.all_day {
        format!("{date} · {}", crate::l10n::tr("All day"))
    } else {
        format!(
            "{date} · {}",
            crate::model::MinuteOfDay::new(due.start_minute).format(fmt)
        )
    }
}

pub(crate) fn snooze(state: &mut AppState, key: ReminderKey) {
    // Snooze records a UTC instant so changing the system zone cannot reinterpret its deadline.
    let now = abs_minute(state.today, state.now_minute);
    let until_utc = now_utc_of(now) + SNOOZE_MINUTES;
    state.reminders.snooze(key, until_utc);
    state.overlay = Overlay::None;
}

pub(crate) fn dismiss(state: &mut AppState, key: ReminderKey) {
    state.reminders.clear_snooze(key);
    state.reminders.mark_fired(key);
    state.overlay = Overlay::None;
}

#[cfg(test)]
mod tests;
