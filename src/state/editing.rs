//! Event queries, drafts, and import commit.

use super::overlays::{ImportDedupKey, import_dedup_key};
use super::*;

impl AppState {
    #[must_use]
    pub(crate) fn is_visible(&self, event: &Event) -> bool {
        let cal_visible = self
            .calendars
            .iter()
            .find(|c| c.id == event.calendar_id)
            .is_none_or(|c| c.visible);
        let category_visible = event
            .color_category
            .as_ref()
            .is_none_or(|k| !self.hidden_categories.contains(k));
        cal_visible && category_visible
    }

    #[must_use]
    pub(crate) fn events_on(&self, date: Date) -> Vec<Event> {
        self.occurrences_on(date)
            .into_iter()
            .map(|(event, start)| event.occurrence_on(start))
            .collect()
    }

    #[must_use]
    pub(crate) fn occurrences_on(&self, date: Date) -> Vec<(&Event, Date)> {
        let mut out: Vec<_> = self
            .events
            .iter()
            .filter(|e| self.is_visible(e))
            .flat_map(|e| {
                e.occurrence_starts_intersecting(date, date)
                    .into_iter()
                    .map(move |start| (e, start))
            })
            .collect();
        out.sort_by_key(|(e, start)| {
            let visible_start = if *start < date {
                0
            } else {
                e.start_minute.get()
            };
            (!e.all_day, visible_start, e.id, *start)
        });
        out
    }

    #[must_use]
    pub(crate) fn events_intersecting(&self, start: Date, end: Date) -> Vec<Event> {
        let mut out: Vec<Event> = self
            .events
            .iter()
            .filter(|e| self.is_visible(e))
            .flat_map(|e| e.occurrences_intersecting(start, end))
            .collect();
        out.sort_by_key(|e| (e.date, !e.all_day, e.start_minute.get(), e.id));
        out
    }

    #[must_use]
    pub(crate) fn has_events_on(&self, date: Date) -> bool {
        self.events.iter().any(|e| {
            self.is_visible(e)
                && crate::recurrence::has_occurrence_in_range(
                    e,
                    date.add_days(-e.span_days()),
                    date,
                )
        })
    }

    #[must_use]
    pub(crate) fn focused_week(&self) -> Vec<Date> {
        week_days(self.focus, self.settings.first_day_of_week)
    }

    #[must_use]
    pub(crate) fn schedule_dates(&self) -> Vec<Date> {
        match self.view {
            View::Day => vec![self.focus],
            _ => self.focused_week(),
        }
    }

    #[must_use]
    pub(crate) fn month_grid(&self) -> MonthGrid {
        MonthGrid::build(
            YearMonth::of(self.focus),
            self.settings.first_day_of_week,
            Default::default(),
        )
    }

    #[must_use]
    pub(crate) fn agenda(&self, days: i32) -> Vec<(Date, Vec<Event>)> {
        (0..days.max(0))
            .map(|i| self.focus.add_days(i))
            .filter_map(|date| {
                let evs = self.events_on(date);
                (!evs.is_empty()).then_some((date, evs))
            })
            .collect()
    }

    #[must_use]
    pub(crate) fn search(&self, query: &str) -> Vec<&Event> {
        let q = enzui_core::text_fold::fold(query.trim());
        if q.is_empty() {
            return Vec::new();
        }
        self.events
            .iter()
            .filter(|e| self.is_visible(e))
            .filter(|e| {
                enzui_core::text_fold::fold(&e.title).contains(&q)
                    || enzui_core::text_fold::fold(&e.location).contains(&q)
                    || enzui_core::text_fold::fold(&e.notes).contains(&q)
            })
            .collect()
    }

    #[must_use]
    pub(crate) fn event(&self, id: u64) -> Option<&Event> {
        self.events.iter().find(|e| e.id == id)
    }

    pub(crate) fn event_mut(&mut self, id: u64) -> Option<&mut Event> {
        self.events.iter_mut().find(|e| e.id == id)
    }

    pub(crate) fn refuse_id_exhaustion(&mut self) {
        self.publish_toast(
            "This calendar has no room for another event.",
            ToastTone::Error,
        );
    }

    #[must_use]
    pub(crate) fn try_mint_id(&mut self) -> Option<u64> {
        let id = self.next_id;
        if id == 0 {
            return None;
        }
        self.next_id = id.checked_add(1).unwrap_or(0);
        Some(id)
    }

    pub(crate) fn backfill_event_uids(&mut self) -> bool {
        let mut changed = false;
        for event in &mut self.events {
            if event
                .uid
                .as_deref()
                .is_some_and(crate::ics_safety::safe_uid)
            {
                continue;
            }
            let replacement = crate::ics_safety::mint_uid();
            if event.uid != replacement {
                event.uid = replacement;
                changed = true;
            }
        }
        changed
    }

    #[must_use]
    pub(crate) fn try_draft_at(&mut self, date: Date, minute: i32) -> Option<Event> {
        let id = self.try_mint_id()?;
        let cal = self
            .calendars
            .iter()
            .find(|c| c.visible)
            .map_or("cal.home", |c| c.id.as_str())
            .to_owned();
        let start = MinuteOfDay::new(minute);
        Some(Event {
            start_minute: start,
            end_minute: MinuteOfDay::new(start.get() + 60),
            // Locally created UIDs use OS entropy and remain stable across export and re-import.
            uid: crate::ics_safety::mint_uid(),
            ..Event::new(id, cal, date)
        })
    }

    pub(crate) fn commit_import(
        &mut self,
        parsed: crate::ics::ParsedIcs,
        calendar_id: &str,
    ) -> ImportReport {
        let existing: std::collections::HashSet<ImportDedupKey> = self
            .events
            .iter()
            .filter(|e| e.calendar_id.as_str() == calendar_id)
            .flat_map(|event| {
                // Keep content keys alongside minted UIDs so repeated UID-less imports still
                // deduplicate.
                let content = ImportDedupKey::Content(event.import_signature());
                match import_dedup_key(event) {
                    uid @ ImportDedupKey::Uid(..) => vec![uid, content],
                    ImportDedupKey::Content(..) => vec![content],
                }
            })
            .collect();
        // UID-less duplicates in one file stay distinct; repeated UID keys identify the same
        // occurrence.
        let mut admitted_uids: std::collections::HashSet<ImportDedupKey> =
            std::collections::HashSet::new();

        let mut report = ImportReport {
            warnings: parsed.warnings,
            omitted_warnings: parsed.omitted_warnings,
            ..ImportReport::default()
        };

        let events_len = parsed.events.len() as u64;
        if self.next_id == 0 || self.next_id.checked_add(events_len).is_none() {
            report.refused_id_exhaustion = true;
            return report;
        }

        // Overrides follow every parent so each finds its series regardless of file order.
        let (overrides, parents): (Vec<Event>, Vec<Event>) = parsed
            .events
            .into_iter()
            .partition(|ev| ev.recurrence_id.is_some());

        // Resolve anchored endpoints independently into local time to preserve a DST-straddling
        // span.
        for mut ev in parents.into_iter().chain(overrides) {
            calendar_id.clone_into(&mut ev.calendar_id);
            if ev.tz_anchor.is_anchored() {
                let start_offset =
                    crate::sysclock::local_offset_at_utc(ev.date, ev.start_minute.get());
                let end_offset =
                    crate::sysclock::local_offset_at_utc(ev.end_date, ev.end_minute.get());
                ev.anchor_utc_to_local(start_offset, end_offset);
            }
            let key = import_dedup_key(&ev);
            let duplicate = existing.contains(&key)
                || (matches!(key, ImportDedupKey::Uid(..)) && !admitted_uids.insert(key.clone()));
            if duplicate {
                report.skipped_duplicates += 1;
                continue;
            }
            if ev.recurrence_raw.is_some() {
                report.unsupported += 1;
            }
            let Some(id) = self.try_mint_id() else {
                report.refused_id_exhaustion = true;
                report.added = 0;
                return report;
            };
            if let (Some(uid), Some(date)) = (&ev.uid, ev.recurrence_id) {
                self.except_series_occurrence(calendar_id, uid, date);
            }
            ev.id = id;
            if ev.uid.is_none() {
                ev.uid = crate::ics_safety::mint_uid();
            }
            self.events.push(ev);
            report.added += 1;
        }
        for (uid, date) in &parsed.cancelled_occurrences {
            self.except_series_occurrence(calendar_id, uid, *date);
        }

        report
    }

    fn except_series_occurrence(&mut self, calendar_id: &str, uid: &str, date: Date) {
        let series = self.events.iter_mut().find(|event| {
            event.calendar_id == calendar_id
                && event.recurrence_id.is_none()
                && event.recurrence != crate::model::Recurrence::None
                && event.uid.as_deref() == Some(uid)
        });
        if let Some(series) = series
            && !series.exceptions.contains(&date)
        {
            series.exceptions.push(date);
        }
    }
}
