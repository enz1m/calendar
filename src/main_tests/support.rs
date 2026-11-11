//! Isolated state files and bounded reducer task driving.

use super::*;

pub(crate) fn drain_task(task: Task<Message>) -> Vec<Message> {
    use std::task::{Context, Poll};

    let Some(stream) = iced_test::runtime::task::into_stream(task) else {
        return Vec::new();
    };
    let mut stream = Box::pin(stream);
    let waker = std::task::Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut messages = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match iced_test::futures::futures::Stream::poll_next(stream.as_mut(), &mut context) {
            Poll::Ready(Some(iced_test::runtime::Action::Output(message))) => {
                messages.push(message);
            }
            Poll::Ready(Some(_)) => {}
            Poll::Ready(None) => break,
            Poll::Pending => {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }
    messages
}

impl App {
    pub(crate) fn step(&mut self, message: Message) {
        let mut pending = std::collections::VecDeque::from([message]);
        let mut budget = 64usize;
        while let Some(message) = pending.pop_front() {
            budget = budget
                .checked_sub(1)
                .expect("App::step exceeded its message budget: an arm is cycling");
            let task = self.update(message);
            pending.extend(drain_task(task));
        }
    }
}

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn new(tag: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "calendar-test-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }

    pub(crate) fn state_file(&self) -> PathBuf {
        self.0.join("calendar-app").join("state.json")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
