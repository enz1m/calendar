//! Closing waits for both workers and preserves unfinished task streams.

use crate::persistence::PersistedState;

pub(crate) fn document(events: usize) -> PersistedState {
    let mut state = crate::state::AppState::empty(today(), 540);
    for id in 1..=events as u64 {
        let mut event = crate::model::Event::new(id, "cal.home", today());
        event.title = format!("Event {id}");
        state.events.push(event);
    }
    state.next_id = events as u64 + 1;
    crate::persistence::snapshot(&state)
}

use crate::message::Message;
use enzui_core::civil_date::Date;

fn today() -> Date {
    Date::new(2026, 8, 19).expect("a real date")
}

#[test]
fn close_waits_for_the_persist_and_boot_workers_before_emitting() {
    use enzui_core::blocking_task::{BlockingTaskOwner, DEFAULT_QUEUE_CAPACITY};
    use std::num::NonZeroUsize;
    use std::sync::Arc;

    let dir = crate::support::TempDir::new("close-two-owner");
    let path = dir.state_file();

    let persist_owner = Arc::new(
        BlockingTaskOwner::new(
            "calendar-app-persist-test",
            NonZeroUsize::new(1).unwrap(),
            DEFAULT_QUEUE_CAPACITY,
        )
        .expect("start persist owner"),
    );
    let boot_owner = Arc::new(
        BlockingTaskOwner::new(
            "calendar-app-boot-test",
            NonZeroUsize::new(1).unwrap(),
            DEFAULT_QUEUE_CAPACITY,
        )
        .expect("start boot owner"),
    );
    crate::persistence::save_to(&path, &document(8)).unwrap();
    let mut app = crate::App::start(crate::persistence::boot_at(
        &path,
        today(),
        540,
        crate::persistence::FirstRun::Empty,
    ))
    .0;
    app.persist_worker = Arc::clone(&persist_owner);
    app.boot_worker = Arc::clone(&boot_owner);

    let persist_hold = hold_owner(&persist_owner);
    let dropped = app.update(Message::SetShowWeekNumbers(true));
    drop(dropped);
    assert!(app.pending_save.is_some());
    let requested = iced::window::Id::unique();
    let close = app.update(Message::CloseWindow(requested));
    let mut driver = CloseDriver::new(close);
    assert!(app.state.close_state.is_draining());
    assert_eq!(app.state.close_state.window(), Some(requested));

    let closed = driver.pump(&mut app, std::time::Duration::from_millis(50));
    assert_eq!(closed, 0, "held persist owner defers close");
    let boot_hold = hold_owner(&boot_owner);
    persist_hold.release();
    let closed = driver.pump(&mut app, std::time::Duration::from_millis(300));
    assert_eq!(closed, 0, "held boot owner still defers close");
    assert!(
        crate::persistence::load_from(&path)
            .expect("file after save")
            .settings
            .show_week_numbers,
        "the exact save debt settled while close is still deferred"
    );

    boot_hold.release();
    let closed = driver.pump(&mut app, std::time::Duration::from_secs(2));
    assert!(closed >= 1);
    assert!(
        driver.closed_ids.iter().all(|id| *id == requested),
        "the shipped close must target the exact requested window id, got {:?}",
        driver.closed_ids,
    );
    assert!(app.state.close_state.is_idle());
}

pub(crate) struct Hold {
    tx: std::sync::mpsc::Sender<()>,
    _handle: enzui_core::blocking_task::BlockingTask<()>,
}

impl Hold {
    pub(crate) fn release(self) {
        let _ = self.tx.send(());
    }
}

pub(crate) fn hold_owner(
    owner: &std::sync::Arc<enzui_core::blocking_task::BlockingTaskOwner>,
) -> Hold {
    use std::sync::mpsc;
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let handle = owner.submit(0, move |_context| {
        let _ = entered_tx.send(());
        let _ = release_rx.recv();
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("held closure reached the worker");
    Hold {
        tx: release_tx,
        _handle: handle,
    }
}

// Retain unfinished streams across pumps so debt-settling outputs are not dropped.
pub(crate) struct CloseDriver {
    streams: Vec<iced_test::futures::BoxStream<iced_test::runtime::Action<Message>>>,
    last_tick: std::time::Instant,
    closed_ids: Vec<iced::window::Id>,
}

impl CloseDriver {
    pub(crate) fn new(task: iced::Task<Message>) -> Self {
        Self {
            streams: iced_test::runtime::task::into_stream(task)
                .into_iter()
                .collect(),
            last_tick: std::time::Instant::now(),
            closed_ids: Vec::new(),
        }
    }

    pub(crate) fn pump(&mut self, app: &mut crate::App, budget: std::time::Duration) -> usize {
        use iced_test::runtime::{Action, window::Action as WindowAction};
        use std::task::{Context, Poll, Waker};

        let waker = Waker::noop();
        let mut ctx = Context::from_waker(waker);
        let deadline = std::time::Instant::now() + budget;
        let tick_gap = std::time::Duration::from_millis(10);
        let mut closed = 0usize;
        loop {
            if std::time::Instant::now() >= deadline {
                break;
            }
            if app.state.close_state.is_draining() && self.last_tick.elapsed() >= tick_gap {
                let tick_task = app.update(Message::CloseDrainTick);
                self.streams
                    .extend(iced_test::runtime::task::into_stream(tick_task));
                self.last_tick = std::time::Instant::now();
            }
            if self.streams.is_empty() && !app.state.close_state.is_draining() {
                break;
            }
            let mut index = 0;
            while index < self.streams.len() {
                let polled = iced_test::futures::futures::Stream::poll_next(
                    self.streams[index].as_mut(),
                    &mut ctx,
                );
                match polled {
                    Poll::Ready(None) => {
                        let _ = self.streams.swap_remove(index);
                        continue;
                    }
                    Poll::Ready(Some(Action::Window(WindowAction::GetLatest(sender)))) => {
                        let _ = sender.send(Some(iced::window::Id::unique()));
                    }
                    Poll::Ready(Some(Action::Window(WindowAction::Close(id)))) => {
                        closed += 1;
                        self.closed_ids.push(id);
                    }
                    Poll::Ready(Some(Action::Output(message))) => {
                        self.streams
                            .extend(iced_test::runtime::task::into_stream(app.update(message)));
                    }
                    Poll::Ready(Some(_)) | Poll::Pending => {}
                }
                index += 1;
            }
            std::thread::yield_now();
        }
        closed
    }
}
