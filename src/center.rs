//! Calendar's enzui-center functions, behind the `center` feature.

use enzui_agent::center::{
    AppFunction, AppFunctionRegistry, AppMetadata, CenterFailure, CenterHost, CenterOutcome,
    CenterProvision, CenterTurn, ConnectorEvent, ConsentScope, FunctionRisk, FunctionSpec,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::*;

pub(crate) const CENTER_APP_ID: &str = crate::package::APP_ID;
pub(crate) const LIST_RANGE_DAYS: i32 = 62;
pub(crate) const LIST_LIMIT: usize = 200;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CenterCall {
    ListEvents {
        from: String,
        to: String,
    },
    CreateEvent {
        title: String,
        date: String,
        start_minute: i32,
        duration_minutes: i32,
        location: Option<String>,
    },
    GoToDate {
        date: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeArgs {
    from: String,
    to: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateArgs {
    title: String,
    date: String,
    start_minute: i32,
    duration_minutes: i32,
    location: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DateArgs {
    date: String,
}

use enzui_agent::center::object_schema as object;

fn date_schema() -> Value {
    json!({"type": "string", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$"})
}

pub(crate) fn registry() -> AppFunctionRegistry<CenterCall> {
    let once_or_session = vec![ConsentScope::Once, ConsentScope::Session];
    let list = FunctionSpec::new(
        "list_events",
        "List events",
        "Lists event occurrences on visible calendars between two dates (inclusive, \
         at most 62 days), in start order.",
        object(
            json!({"from": date_schema(), "to": date_schema()}),
            &["from", "to"],
        ),
        object(
            json!({
                "events": {"type": "array", "items": object(json!({
                    "event_id": {"type": "integer"},
                    "title": {"type": "string"},
                    "calendar": {"type": "string"},
                    "location": {"type": "string"},
                    "date": {"type": "string"},
                    "all_day": {"type": "boolean"},
                    "start_minute": {"type": "integer"},
                    "end_minute": {"type": "integer"}
                }), &["event_id", "title", "calendar", "date", "all_day", "start_minute", "end_minute"])},
                "truncated": {"type": "boolean"}
            }),
            &["events", "truncated"],
        ),
        FunctionRisk::SensitiveRead,
        vec![
            ConsentScope::Once,
            ConsentScope::Session,
            ConsentScope::Always,
        ],
    );
    let create = FunctionSpec::new(
        "create_event",
        "Create an event",
        "Creates a timed event on the default calendar, as quick add does.",
        object(
            json!({
                "title": {"type": "string", "minLength": 1, "maxLength": 200},
                "date": date_schema(),
                "start_minute": {"type": "integer", "minimum": 0, "maximum": 1439},
                "duration_minutes": {"type": "integer", "minimum": 5, "maximum": 1440},
                "location": {"type": "string", "maxLength": 500}
            }),
            &["title", "date", "start_minute", "duration_minutes"],
        ),
        object(json!({"event_id": {"type": "integer"}}), &["event_id"]),
        FunctionRisk::Mutating,
        once_or_session.clone(),
    );
    let go_to = FunctionSpec::new(
        "go_to_date",
        "Go to a date",
        "Moves the calendar view to this date.",
        object(json!({"date": date_schema()}), &["date"]),
        object(json!({"date": {"type": "string"}}), &["date"]),
        FunctionRisk::Mutating,
        once_or_session,
    );
    AppFunctionRegistry::from_functions(vec![
        AppFunction::typed(list, |args: RangeArgs| CenterCall::ListEvents {
            from: args.from,
            to: args.to,
        })
        .expect("list_events spec"),
        AppFunction::typed(create, |args: CreateArgs| CenterCall::CreateEvent {
            title: args.title,
            date: args.date,
            start_minute: args.start_minute,
            duration_minutes: args.duration_minutes,
            location: args.location,
        })
        .expect("create_event spec"),
        AppFunction::typed(go_to, |args: DateArgs| CenterCall::GoToDate {
            date: args.date,
        })
        .expect("go_to_date spec"),
    ])
    .expect("the calendar registry is valid")
}

pub(crate) fn metadata() -> AppMetadata {
    AppMetadata {
        id: CENTER_APP_ID.into(),
        name: "Calendar".into(),
        description: "Calendars and events".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

pub(crate) fn parse_date(text: &str) -> Result<Date, CenterFailure> {
    let invalid = || CenterFailure::new("invalid_arguments", "dates are YYYY-MM-DD");
    let mut parts = text.split('-');
    let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid());
    };
    let year: i32 = year.parse().map_err(|_| invalid())?;
    let month: u32 = month.parse().map_err(|_| invalid())?;
    let day: u32 = day.parse().map_err(|_| invalid())?;
    Date::new(year, month, day).ok_or_else(invalid)
}

impl App {
    pub(crate) fn request_center(&mut self) {
        self.center_requested = true;
        self.center_after_boot();
    }

    pub(crate) fn center_after_boot(&mut self) {
        if self.center_requested
            && self.center.is_none()
            && !matches!(self.state.overlay, Overlay::Booting { .. })
        {
            self.start_center();
        }
    }

    fn start_center(&mut self) {
        let Some(provision) = CenterProvision::discover(CENTER_APP_ID) else {
            return;
        };
        match CenterHost::spawn(provision, metadata(), registry()) {
            Ok(host) => self.center = Some(host),
            Err(error) => eprintln!("calendar-app: center connector disabled: {error}"),
        }
    }

    pub(crate) fn center_subscription(&self) -> Subscription<Message> {
        self.center
            .as_ref()
            .map_or_else(Subscription::none, |host| {
                host.subscription(Message::Center)
            })
    }

    pub(crate) fn shutdown_center(&mut self) {
        if let Some(host) = self.center.take() {
            host.shutdown();
        }
    }

    pub(crate) fn center_event(&mut self, event: ConnectorEvent) -> Task<Message> {
        let Some(host) = self.center.as_mut() else {
            return Task::none();
        };
        // Calls are applied and answered in this turn; no call-owned work survives to cancel.
        let CenterTurn::Call(call, request) = host.receive_with_ends(event).turn else {
            return Task::none();
        };
        if matches!(self.state.overlay, Overlay::Booting { .. }) {
            host.fail(call, "unavailable", "the calendar is still starting");
            return Task::none();
        }
        let (task, outcome) = self.apply_center_call(request);
        if let Some(host) = self.center.as_mut() {
            host.answer(call, outcome);
        }
        task
    }

    fn apply_center_call(&mut self, request: CenterCall) -> (Task<Message>, CenterOutcome) {
        match request {
            CenterCall::ListEvents { from, to } => {
                (Task::none(), self.list_center_events(&from, &to))
            }
            CenterCall::CreateEvent {
                title,
                date,
                start_minute,
                duration_minutes,
                location,
            } => {
                let date = match parse_date(&date) {
                    Ok(date) => date,
                    Err(failure) => return (Task::none(), Err(failure)),
                };
                if title.trim().is_empty() {
                    return (
                        Task::none(),
                        Err(CenterFailure::new(
                            "invalid_arguments",
                            "an event needs a title",
                        )),
                    );
                }
                let before = self.state.next_id;
                let mut tasks = vec![self.update(Message::OpenQuickAdd {
                    date,
                    minute: start_minute,
                    at: iced::Point::ORIGIN,
                })];
                let Overlay::QuickAdd(draft) = &mut self.state.overlay else {
                    return (
                        Task::batch(tasks),
                        Err(CenterFailure::new(
                            "unavailable",
                            "a new event cannot be added now",
                        )),
                    );
                };
                draft.draft.location = location.unwrap_or_default();
                tasks.push(self.update(Message::QuickAddTitle(title)));
                tasks.push(self.update(Message::QuickAddDuration(duration_minutes)));
                tasks.push(self.update(Message::QuickAddSave));
                let created = self
                    .state
                    .events
                    .iter()
                    .find(|event| event.id >= before && self.state.selected == Some(event.id));
                let outcome = created
                    .map(|event| json!({ "event_id": event.id }))
                    .ok_or_else(|| CenterFailure::new("unavailable", "the event was not saved"));
                (Task::batch(tasks), outcome)
            }
            CenterCall::GoToDate { date } => {
                let parsed = match parse_date(&date) {
                    Ok(date) => date,
                    Err(failure) => return (Task::none(), Err(failure)),
                };
                let task = self.update(Message::JumpTo(parsed));
                let outcome = if self.state.focus == parsed {
                    Ok(json!({ "date": parsed.to_string() }))
                } else {
                    Err(CenterFailure::new("unavailable", "the view did not move"))
                };
                (task, outcome)
            }
        }
    }

    fn list_center_events(&self, from: &str, to: &str) -> CenterOutcome {
        let (from, to) = (parse_date(from)?, parse_date(to)?);
        if to < from {
            return Err(CenterFailure::new(
                "invalid_arguments",
                "`to` is before `from`",
            ));
        }
        if from.days_until(to) >= LIST_RANGE_DAYS {
            return Err(CenterFailure::new(
                "invalid_arguments",
                "the range may cover at most 62 days",
            ));
        }
        let mut occurrences: Vec<(Date, i32, &Event, String)> = Vec::new();
        for event in &self.state.events {
            let Some(calendar) = self
                .state
                .calendars
                .iter()
                .find(|calendar| calendar.id == event.calendar_id && calendar.visible)
            else {
                continue;
            };
            for occurrence in event.occurrences_intersecting(from, to) {
                occurrences.push((
                    occurrence.date,
                    event.start_minute.get(),
                    event,
                    calendar.name.clone(),
                ));
            }
        }
        occurrences.sort_by_key(|(date, minute, event, _)| (*date, *minute, event.id));
        let truncated = occurrences.len() > LIST_LIMIT;
        let events: Vec<Value> = occurrences
            .into_iter()
            .take(LIST_LIMIT)
            .map(|(date, _, event, calendar)| {
                json!({
                    "event_id": event.id,
                    "title": event.title,
                    "calendar": calendar,
                    "location": event.location,
                    "date": date.to_string(),
                    "all_day": event.all_day,
                    "start_minute": event.start_minute.get(),
                    "end_minute": event.end_minute.get(),
                })
            })
            .collect();
        Ok(json!({ "events": events, "truncated": truncated }))
    }
}
