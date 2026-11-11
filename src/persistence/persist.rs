//! Persist handle: bind, decode, and boot.

#[cfg(test)]
use super::records::{RemindersRec, SettingsRec};
use super::*;

impl Persist {
    #[must_use]
    pub(crate) fn with_authority(
        path: PathBuf,
        baseline: PersistedState,
        authority: SharedAuthority,
        retire_identity: Option<ReadIdentity>,
    ) -> Self {
        Self {
            backend: state_file_backend(path),
            last_saved: baseline,
            last_prepared: None,
            enabled: true,
            authority: Some(authority),
            retire_identity,
            writes: 0,
        }
    }

    #[must_use]
    pub(crate) fn disabled(path: PathBuf, baseline: PersistedState) -> Self {
        Self {
            backend: state_file_backend(path),
            last_saved: baseline,
            last_prepared: None,
            enabled: false,
            authority: None,
            retire_identity: None,
            writes: 0,
        }
    }

    #[must_use]
    pub(crate) fn disabled_with_identity(
        path: PathBuf,
        baseline: PersistedState,
        retire_identity: ReadIdentity,
    ) -> Self {
        Self {
            backend: state_file_backend(path),
            last_saved: baseline,
            last_prepared: None,
            enabled: false,
            authority: None,
            retire_identity: Some(retire_identity),
            writes: 0,
        }
    }

    #[must_use]
    pub(crate) fn retire_identity(&self) -> Option<ReadIdentity> {
        self.retire_identity
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn inert() -> Self {
        Self::disabled(
            PathBuf::from("/nonexistent/calendar-app/state.json"),
            PersistedState {
                version: FORMAT_VERSION,
                calendars: Vec::new(),
                events: Vec::new(),
                next_id: 0,
                settings: SettingsRec::default(),
                hidden_categories: Vec::new(),
                reminders: RemindersRec::default(),
            },
        )
    }

    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        self.backend.path()
    }

    #[must_use]
    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
            && self.authority.as_ref().is_none_or(|shared| {
                !shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .has_lost_custody()
            })
    }

    pub(crate) fn enable_for(
        &mut self,
        state: &AppState,
        queue: &PersistenceQueue,
    ) -> Option<PendingSave> {
        match StateAuthority::create_new(self.backend.path()) {
            Ok(authority) => {
                self.authority = Some(Arc::new(Mutex::new(authority)));
                self.retire_identity = None;
                self.enabled = true;
                self.prepare(state, queue)
            }
            Err(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn sync(&mut self, state: &AppState) -> bool {
        let queue = PersistenceQueue::default();
        match self.prepare(state, &queue) {
            Some(pending) => {
                match pending.complete(&queue) {
                    PersistenceCompletion::Saved => self.acknowledge_saved(&pending),
                    PersistenceCompletion::Error(_)
                    | PersistenceCompletion::PublishedButNotDurable(_)
                    | PersistenceCompletion::CustodyLost(_)
                    | PersistenceCompletion::WorkerRefused(_) => {
                        // Only confirmed durability advances last_saved; failed publication remains
                        // retryable.
                        self.last_prepared = None;
                    }
                    PersistenceCompletion::Superseded => {}
                }
                true
            }
            None => false,
        }
    }

    pub(crate) fn prepare(
        &mut self,
        state: &AppState,
        queue: &PersistenceQueue,
    ) -> Option<PendingSave> {
        if !self.is_enabled() {
            return None;
        }
        let doc = snapshot(state);
        let comparison = self.last_prepared.as_ref().unwrap_or(&self.last_saved);
        if &doc == comparison {
            return None;
        }
        self.writes += 1;
        self.last_prepared = Some(doc.clone());
        Some(PendingSave {
            ticket: queue.register(STATE_DESTINATION),
            doc: Arc::new(doc),
            backend: self.backend.clone(),
            authority: self.authority.as_ref().map(Arc::clone),
        })
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn last_saved(&self) -> &PersistedState {
        &self.last_saved
    }

    pub(crate) fn acknowledge_saved(&mut self, pending: &PendingSave) {
        self.last_saved.clone_from(pending.document());
        self.last_prepared = None;
    }
}

enum Admitted {
    Restored {
        doc: Box<PersistedState>,
        authority: StateAuthority,
        identity: ReadIdentity,
    },
    Fresh {
        authority: StateAuthority,
    },
    Corrupt {
        identity: ReadIdentity,
    },
    Unsupported {
        version: u32,
        identity: ReadIdentity,
    },
    Refused(PersistenceRefusal),
}

// Authority binding and bounded reading must agree on the descriptor identity.
fn admit_and_bind(path: &Path) -> Admitted {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Admitted::Refused(PersistenceRefusal::OpenFailed);
    };
    if ensure_private_dir(parent).is_err() {
        return Admitted::Refused(PersistenceRefusal::OpenFailed);
    }
    let Ok(authority) = StateAuthority::open_or_create_tightening(path) else {
        return Admitted::Refused(PersistenceRefusal::OpenFailed);
    };

    let read = match admit_state_file(path) {
        Ok(read) => read,
        Err(refusal) => return Admitted::Refused(refusal),
    };
    match (authority.identity(), read) {
        (None, None) => Admitted::Fresh { authority },
        (Some(auth_id), Some((text, read_id)))
            if auth_id.device() == read_id.device() && auth_id.inode() == read_id.inode() =>
        {
            decode(text, authority, read_id)
        }
        // Any identity mismatch leaves the pathname occupant untouched and disables persistence.
        _ => Admitted::Refused(PersistenceRefusal::Replaced),
    }
}

fn decode(text: String, authority: StateAuthority, identity: ReadIdentity) -> Admitted {
    bounded::reset();
    let doc: PersistedState = match serde_json::from_str::<PersistedState>(&text) {
        Ok(doc) => doc,
        Err(_) => {
            if let Some(refusal) = bounded::tripped() {
                return Admitted::Refused(refusal);
            }
            return Admitted::Corrupt { identity };
        }
    };
    if doc.version > FORMAT_VERSION {
        return Admitted::Unsupported {
            version: doc.version,
            identity,
        };
    }
    if let Err(refusal) = validate_bounded(&doc) {
        return Admitted::Refused(refusal);
    }
    if let Err(refusal) = validate_ids(&doc) {
        return Admitted::Refused(refusal);
    }
    Admitted::Restored {
        doc: Box::new(doc),
        authority,
        identity,
    }
}

#[must_use]
pub(crate) fn boot_at(
    path: &Path,
    today: Date,
    now_minute: i32,
    first_run: FirstRun,
) -> (AppState, Persist, Origin) {
    match admit_and_bind(path) {
        Admitted::Restored {
            doc,
            authority,
            identity,
        } => {
            let state = restore(today, now_minute, &doc);
            // The baseline is the restored state, not the raw document, so load normalization alone
            // cannot trigger a save.
            let baseline = snapshot(&state);
            (
                state,
                Persist::with_authority(
                    path.to_path_buf(),
                    baseline,
                    Arc::new(Mutex::new(authority)),
                    Some(identity),
                ),
                Origin::Restored,
            )
        }
        Admitted::Fresh { authority } => {
            let state = match first_run {
                FirstRun::Empty => AppState::empty(today, now_minute),
                FirstRun::Demo => {
                    let mut state = AppState::new(today, now_minute);
                    let _ = state.backfill_event_uids();
                    state
                }
            };
            let baseline = snapshot(&state);
            (
                state,
                Persist::with_authority(
                    path.to_path_buf(),
                    baseline,
                    Arc::new(Mutex::new(authority)),
                    None,
                ),
                Origin::FreshStart,
            )
        }
        Admitted::Corrupt { identity } => {
            let state = AppState::empty(today, now_minute);
            let baseline = snapshot(&state);
            (
                state,
                Persist::disabled_with_identity(path.to_path_buf(), baseline, identity),
                Origin::RecoveredFromCorrupt(None),
            )
        }
        Admitted::Unsupported { version, identity } => {
            // Newer formats remain untouched with saving disabled until explicit identity-bound
            // retirement.
            let state = AppState::empty(today, now_minute);
            let baseline = snapshot(&state);
            (
                state,
                Persist::disabled_with_identity(path.to_path_buf(), baseline, identity),
                Origin::ReadOnlyFutureVersion(version),
            )
        }
        Admitted::Refused(refusal) => {
            // A refused file stays untouched with saving disabled; no read identity was proven for
            // recovery.
            let state = AppState::empty(today, now_minute);
            let baseline = snapshot(&state);
            (
                state,
                Persist::disabled(path.to_path_buf(), baseline),
                Origin::RefusedBoot(refusal),
            )
        }
    }
}
