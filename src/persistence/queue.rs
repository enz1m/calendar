//! Save queue: authority, tickets, and the worker.

use super::*;

pub(crate) type SharedAuthority = Arc<Mutex<StateAuthority>>;

fn map_publish_error(err: PublishError) -> PersistError {
    PersistError::from(std::io::Error::new(err.io_error().kind(), format!("{err}")))
}

pub(crate) fn write_serialized(path: &Path, text: &str) -> std::io::Result<()> {
    enzui_core::atomic_state::save_atomic_raw(path, text.as_bytes())
}

pub(crate) fn retire_expected(path: &Path, expected: ReadIdentity) -> Option<PathBuf> {
    let expected = ExpectedSourceIdentity::from_dev_ino(expected.device(), expected.inode());
    match quarantine_sibling_expected(path, expected) {
        QuarantineOutcome::Moved { custody } => {
            custody.source_paths().next().map(Path::to_path_buf)
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin {
    FreshStart,
    Restored,
    RecoveredFromCorrupt(Option<PathBuf>),
    ReadOnlyFutureVersion(u32),
    RefusedBoot(PersistenceRefusal),
    PersistenceRootUnavailable(RootRefusal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FirstRun {
    Empty,
    Demo,
}

pub(super) const STATE_DESTINATION: &str = "calendar-state";

#[derive(Clone, Default, Debug)]
pub(crate) struct PersistenceQueue {
    inner: Arc<PersistenceQueueInner>,
}

#[derive(Default, Debug)]
struct PersistenceQueueInner {
    next_generation: AtomicU64,
    latest_by_destination: Mutex<HashMap<String, u64>>,
    writer: Mutex<()>,
    #[cfg(test)]
    injected_error: Mutex<Option<(std::io::ErrorKind, String)>>,
}

#[derive(Clone, Debug)]
pub(crate) struct PersistenceTicket {
    destination: String,
    generation: u64,
}

impl PersistenceTicket {
    #[must_use]
    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }
}

impl PersistenceQueue {
    #[cfg(test)]
    pub(crate) fn inject_write_error(
        &self,
        kind: std::io::ErrorKind,
        diagnostic: impl Into<String>,
    ) {
        *self
            .inner
            .injected_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((kind, diagnostic.into()));
    }

    #[cfg(test)]
    pub(crate) fn clear_injected_write_error(&self) {
        *self
            .inner
            .injected_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    pub(crate) fn register(&self, destination: impl Into<String>) -> PersistenceTicket {
        let destination = destination.into();
        let generation = self
            .inner
            .next_generation
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        self.inner
            .latest_by_destination
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(destination.clone(), generation);
        PersistenceTicket {
            destination,
            generation,
        }
    }

    fn execute_result<E>(
        &self,
        ticket: &PersistenceTicket,
        publish: impl FnOnce() -> Result<(), E>,
    ) -> Result<bool, E>
    where
        E: From<std::io::Error>,
    {
        let _writer = self
            .inner
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let is_latest = self
            .inner
            .latest_by_destination
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&ticket.destination)
            .copied()
            == Some(ticket.generation);
        if !is_latest {
            return Ok(false);
        }
        #[cfg(test)]
        {
            let injected = self
                .inner
                .injected_error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if let Some((kind, diagnostic)) = injected {
                return Err(std::io::Error::new(kind, diagnostic).into());
            }
        }
        publish()?;
        Ok(true)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PersistenceCompletion {
    Saved,
    Superseded,
    Error(PersistError),
    PublishedButNotDurable(PersistError),
    CustodyLost(PersistError),
    WorkerRefused(WorkerRefusal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkerRefusal {
    Busy,
    Cancelled,
    Shutdown,
    Panicked,
}

impl WorkerRefusal {
    #[must_use]
    pub(crate) const fn category(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::Cancelled => "cancelled",
            Self::Shutdown => "shutdown",
            Self::Panicked => "panicked",
        }
    }

    #[must_use]
    pub(crate) const fn is_retryable(self) -> bool {
        !matches!(self, Self::Shutdown)
    }

    #[must_use]
    pub(crate) const fn user_summary(self) -> &'static str {
        match self {
            Self::Busy => "Calendar is still catching up on saves. Retry in a moment.",
            Self::Cancelled | Self::Panicked => {
                "The last save was interrupted before it finished. Retry to save again."
            }
            Self::Shutdown => {
                "The background save worker is no longer available. Restart Calendar to save again."
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PendingSave {
    pub(super) ticket: PersistenceTicket,
    pub(super) doc: Arc<PersistedState>,
    pub(super) backend: FileBackend,
    // Lost publication custody disables writes across every clone.
    pub(super) authority: Option<SharedAuthority>,
}

impl PendingSave {
    #[must_use]
    pub(crate) fn ticket(&self) -> &PersistenceTicket {
        &self.ticket
    }

    #[must_use]
    pub(crate) fn complete(&self, queue: &PersistenceQueue) -> PersistenceCompletion {
        let Some(authority) = self.authority.as_ref() else {
            return PersistenceCompletion::Error(PersistError::from(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "no state authority bound for this save",
            )));
        };
        // Post-publication rotation and custody loss need distinct outcomes despite the queue
        // closure's Result<bool, E> contract.
        publish_slot::clear();
        let authority = Arc::clone(authority);
        let byte_limit = self.backend.raw_byte_limit();
        let result = queue.execute_result(&self.ticket, || {
            let text = serialize_pretty_classified(&self.doc)?;
            let body_bytes = u64::try_from(text.len())
                .map_err(|_| PersistError::serialization("serialized calendar length overflow"))?;
            if body_bytes
                > byte_limit
                    .bytes()
                    .min(u64::try_from(MAX_STATE_FILE_BYTES).unwrap_or(u64::MAX))
            {
                return Err(PersistError::serialization(
                    "serialized calendar exceeds its configured byte limit",
                ));
            }
            let mut guard = authority
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match guard.publish(text.as_bytes()) {
                Ok(_identity) => Ok(()),
                Err(err) => match err.custody() {
                    PublishCustody::Unchanged => Err(map_publish_error(err)),
                    PublishCustody::Rotated => {
                        publish_slot::set(PublishOutcome::Rotated(map_publish_error(err)));
                        Ok(())
                    }
                    PublishCustody::Lost => {
                        publish_slot::set(PublishOutcome::Lost(map_publish_error(err)));
                        Ok(())
                    }
                    _ => {
                        publish_slot::set(PublishOutcome::Lost(map_publish_error(err)));
                        Ok(())
                    }
                },
            }
        });
        match result {
            Ok(true) => match publish_slot::take() {
                Some(PublishOutcome::Rotated(err)) => {
                    PersistenceCompletion::PublishedButNotDurable(err)
                }
                Some(PublishOutcome::Lost(err)) => PersistenceCompletion::CustodyLost(err),
                None => PersistenceCompletion::Saved,
            },
            Ok(false) => PersistenceCompletion::Superseded,
            Err(error) => PersistenceCompletion::Error(error),
        }
    }

    #[must_use]
    pub(crate) fn document(&self) -> &PersistedState {
        &self.doc
    }

    pub(crate) fn renewed(&self, queue: &PersistenceQueue) -> Self {
        Self {
            ticket: queue.register(self.ticket.destination.clone()),
            doc: Arc::clone(&self.doc),
            backend: self.backend.clone(),
            authority: self.authority.as_ref().map(Arc::clone),
        }
    }
}

enum PublishOutcome {
    Rotated(PersistError),
    Lost(PersistError),
}

mod publish_slot {
    use std::cell::RefCell;

    use super::PublishOutcome;

    thread_local! {
        static SLOT: RefCell<Option<PublishOutcome>> = const { RefCell::new(None) };
    }

    pub(super) fn clear() {
        SLOT.with(|s| s.borrow_mut().take());
    }

    pub(super) fn set(outcome: PublishOutcome) {
        SLOT.with(|s| *s.borrow_mut() = Some(outcome));
    }

    pub(super) fn take() -> Option<PublishOutcome> {
        SLOT.with(|s| s.borrow_mut().take())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Persist {
    pub(super) backend: FileBackend,
    pub(super) last_saved: PersistedState,
    // Registration suppresses duplicate saves without acknowledging on-disk durability.
    pub(super) last_prepared: Option<PersistedState>,
    pub(super) enabled: bool,
    // Lost publication custody disables writes across every clone.
    pub(super) authority: Option<SharedAuthority>,
    // Recovery can retire only the occupant verified during admission.
    pub(super) retire_identity: Option<ReadIdentity>,
    pub(super) writes: u64,
}
