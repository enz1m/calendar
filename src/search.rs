//! Bounded desktop-search snapshots and occurrence navigation.

use crate::message::Message;
use crate::model::Event;
use crate::state::AppState;
use enzui_agent_app::{SearchProvider, SearchQuery, SearchRequest, SearchResult};
use enzui_core::civil_date::Date;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

pub(crate) const HORIZON_DAYS: i32 = 28;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    id: String,
    title: String,
    folded: String,
    when: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SearchIndex {
    entries: Arc<Mutex<Vec<Entry>>>,
    fingerprint: Arc<Mutex<Option<u64>>>,
}

impl SearchIndex {
    pub(crate) fn refresh(&self, state: &AppState) {
        let fingerprint = fingerprint(state);
        let mut current = lock(&self.fingerprint);
        if *current == Some(fingerprint) {
            return;
        }
        *current = Some(fingerprint);
        *lock(&self.entries) = entries(state);
    }

    pub(crate) fn provider(&self) -> SearchProvider {
        let entries = Arc::clone(&self.entries);
        SearchProvider::blocking(move |query| search(&lock(&entries), query))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn fingerprint(state: &AppState) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Only fields affecting desktop results participate; notes are not indexed here.
    state.today.hash(&mut hasher);
    state.events.len().hash(&mut hasher);
    for event in &state.events {
        event.id.hash(&mut hasher);
        event.calendar_id.hash(&mut hasher);
        event.title.hash(&mut hasher);
        event.date.hash(&mut hasher);
        event.end_date.hash(&mut hasher);
        event.start_minute.hash(&mut hasher);
        event.end_minute.hash(&mut hasher);
        event.all_day.hash(&mut hasher);
        event.recurrence.hash(&mut hasher);
        event.recurrence_interval.hash(&mut hasher);
        event.recurrence_weekdays.hash(&mut hasher);
        event.recurrence_until.hash(&mut hasher);
        event.recurrence_count.hash(&mut hasher);
        event.exceptions.hash(&mut hasher);
        event.color_category.hash(&mut hasher);
    }
    state.calendars.hash(&mut hasher);
    state.hidden_categories.hash(&mut hasher);
    state.time_format().cycle().hash(&mut hasher);
    crate::l10n::localizer().locale().hash(&mut hasher);
    crate::l10n::localizer()
        .locale_for(enzui_core::localization::LocaleCategory::Time)
        .hash(&mut hasher);
    hasher.finish()
}

fn entries(state: &AppState) -> Vec<Entry> {
    let mut out = Vec::new();
    let time_format = state.time_format();
    for offset in 0..HORIZON_DAYS {
        let date = state.today.add_days(offset);
        for (event, _) in state.occurrences_on(date) {
            if event.title.trim().is_empty() {
                continue;
            }
            out.push(Entry {
                id: result_id(event, date),
                folded: enzui_core::text_fold::fold(&event.title),
                title: event.title.clone(),
                when: when(event, date, &time_format),
            });
        }
    }
    out
}

fn when(event: &Event, date: Date, fmt: &enzui_core::time_of_day::TimeFormat) -> String {
    format!(
        "{} · {}",
        crate::l10n::date(date, false),
        if event.all_day {
            crate::l10n::tr("All day")
        } else {
            event.start_minute.format(fmt)
        }
    )
}

pub(crate) fn result_id(event: &Event, date: Date) -> String {
    format!("{}@{date}", event.id)
}

pub(crate) fn parse_result_id(id: &str) -> Option<(u64, Date)> {
    let (event, date) = id.split_once('@')?;
    let mut parts = date.split('-');
    let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let date = Date::new(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)?;
    Some((event.parse().ok()?, date))
}

fn search(entries: &[Entry], query: &SearchQuery) -> Vec<SearchResult> {
    let wanted = enzui_core::text_fold::fold(query.text.trim());
    if wanted.is_empty() {
        return Vec::new();
    }
    entries
        .iter()
        .filter(|entry| entry.folded.contains(&wanted))
        .take(query.limit)
        .filter_map(|entry| {
            SearchResult::new(entry.id.clone(), entry.title.clone())
                .ok()
                .map(|result| {
                    result
                        .subtitle(entry.when.clone())
                        .icon("x-office-calendar")
                })
        })
        .collect()
}

pub(crate) fn messages_for(request: &SearchRequest) -> Vec<Message> {
    match request {
        SearchRequest::Open { id, .. } => parse_result_id(id)
            .map(|(id, date)| {
                vec![
                    Message::SelectView(crate::model::View::Day),
                    Message::JumpTo(date),
                    Message::OpenDetails {
                        id,
                        on: Some(date),
                        at: None,
                    },
                ]
            })
            .unwrap_or_default(),
        SearchRequest::Search { query } => {
            vec![Message::OpenSearch, Message::SearchChanged(query.clone())]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_search_opens_an_occurrence_and_retires_hidden_results() {
        let date = Date::new(2026, 5, 13).unwrap();
        let mut state = AppState::empty(date, 600);
        let mut event = Event::new(1, "cal.home", date);
        event.title = "Café standup".into();
        event.recurrence = crate::model::Recurrence::Weekly;
        state.events.push(event);
        let index = SearchIndex::default();
        index.refresh(&state);
        let query = SearchQuery {
            text: "CAFE".into(),
            limit: 2,
            locale: None,
        };
        let found = search(&lock(&index.entries), &query);
        assert_eq!(found.len(), 2);
        for message in messages_for(&SearchRequest::Open {
            id: found[1].id().to_owned(),
            query: String::new(),
        }) {
            drop(crate::reducer::reduce_state(&mut state, message));
        }
        assert_eq!(state.focus, date.add_days(7));
        assert!(
            matches!(state.overlay, crate::state::Overlay::Details { id: 1, on: Some(on), .. } if on == state.focus)
        );
        state.events[0].title = "Café planning".into();
        state.events[0].start_minute = crate::model::MinuteOfDay::hm(14, 30);
        index.refresh(&state);
        assert_eq!(lock(&index.entries)[0].title, "Café planning");
        assert_eq!(
            lock(&index.entries)[0].when,
            when(&state.events[0], date, &state.time_format())
        );
        state.events[0].exceptions.push(date);
        index.refresh(&state);
        let updated = search(&lock(&index.entries), &query);
        assert_eq!(updated[0].id(), found[1].id());
        state.toggle_calendar("cal.home");
        index.refresh(&state);
        assert!(search(&lock(&index.entries), &query).is_empty());
        assert_eq!(state.events[0].title, "Café planning");
    }
}
