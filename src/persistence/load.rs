//! Load and save: refusal, admission, and serialization.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]

pub(crate) enum PersistenceRefusal {
    OpenFailed,
    NotRegularFile,
    TooLarge,
    Grew,
    InvalidUtf8,
    Replaced,
    Overflow,
}

impl PersistenceRefusal {
    #[must_use]
    pub(crate) const fn user_summary(self) -> &'static str {
        "Your saved calendar was not readable and has been left untouched."
    }
}

// Nonzero next_id exceeds every retained identity; zero is the exhausted allocator sentinel.
pub(super) fn validate_ids(doc: &PersistedState) -> Result<(), PersistenceRefusal> {
    let mut seen: std::collections::HashSet<u64> =
        std::collections::HashSet::with_capacity(doc.events.len());
    let mut max_id: Option<u64> = None;
    for e in &doc.events {
        if !seen.insert(e.id) {
            return Err(PersistenceRefusal::Overflow);
        }
        max_id = Some(max_id.map_or(e.id, |m| m.max(e.id)));
    }
    // A retained u64::MAX identity is valid only with the exhausted zero allocator sentinel.
    if let Some(m) = max_id
        && m == u64::MAX
        && doc.next_id != 0
    {
        return Err(PersistenceRefusal::Overflow);
    }
    if doc.next_id != 0
        && let Some(m) = max_id
        && doc.next_id <= m
    {
        return Err(PersistenceRefusal::Overflow);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReadIdentity {
    device: u64,
    inode: u64,
}

impl ReadIdentity {
    pub(crate) const fn device(self) -> u64 {
        self.device
    }

    pub(crate) const fn inode(self) -> u64 {
        self.inode
    }
}

pub(super) fn admit_state_file(
    path: &Path,
) -> Result<Option<(String, ReadIdentity)>, PersistenceRefusal> {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;

    let backend = state_file_backend(path);
    let path = backend.path();
    let max_bytes = usize::try_from(backend.raw_byte_limit().bytes())
        .unwrap_or(usize::MAX)
        .min(MAX_STATE_FILE_BYTES);
    let file = match open_state_file_nofollow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            return Err(PersistenceRefusal::NotRegularFile);
        }
        Err(_) => return Err(PersistenceRefusal::OpenFailed),
    };
    let meta = file
        .metadata()
        .map_err(|_| PersistenceRefusal::OpenFailed)?;
    if !meta.is_file() || meta.nlink() == 0 {
        return Err(PersistenceRefusal::NotRegularFile);
    }
    let max_bytes_u64 = u64::try_from(max_bytes).unwrap_or(u64::MAX);
    if meta.len() > max_bytes_u64 {
        return Err(PersistenceRefusal::TooLarge);
    }
    let identity = ReadIdentity {
        device: meta.dev(),
        inode: meta.ino(),
    };

    let mut buf = Vec::with_capacity(meta.len() as usize);
    let mut limited = (&file).take(max_bytes_u64.saturating_add(1));
    limited
        .read_to_end(&mut buf)
        .map_err(|_| PersistenceRefusal::OpenFailed)?;
    if buf.len() > max_bytes {
        return Err(PersistenceRefusal::Grew);
    }

    verify_path_names_file(&file, path).map_err(|_| PersistenceRefusal::Replaced)?;
    let text = String::from_utf8(buf).map_err(|_| PersistenceRefusal::InvalidUtf8)?;
    drop(file);
    Ok(Some((text, identity)))
}

pub(super) fn validate_bounded(doc: &PersistedState) -> Result<(), PersistenceRefusal> {
    if doc.calendars.len() > MAX_CALENDARS
        || doc.events.len() > MAX_EVENTS
        || doc.hidden_categories.len() > MAX_HIDDEN_CATEGORIES
        || doc.reminders.fired.len() > MAX_REMINDERS_FIRED
        || doc.reminders.snoozed.len() > MAX_REMINDERS_SNOOZED
    {
        return Err(PersistenceRefusal::Overflow);
    }
    for cal in &doc.calendars {
        if cal.id.len() > MAX_CALENDAR_ID_BYTES
            || cal.name.len() > MAX_CALENDAR_TEXT_BYTES
            || cal.section.len() > MAX_CALENDAR_TEXT_BYTES
            || cal.color.len() > MAX_CALENDAR_TEXT_BYTES
        {
            return Err(PersistenceRefusal::Overflow);
        }
    }
    for ev in &doc.events {
        if ev.calendar_id.len() > MAX_CALENDAR_ID_BYTES
            || ev.title.len() > MAX_EVENT_TEXT_FIELD_BYTES
            || ev.location.len() > MAX_EVENT_TEXT_FIELD_BYTES
            || ev.notes.len() > MAX_EVENT_TEXT_FIELD_BYTES
            || ev.url.len() > MAX_EVENT_TEXT_FIELD_BYTES
            || ev.invitees.len() > MAX_EVENT_INVITEES_BYTES
            || ev.exceptions.len() > MAX_EVENT_EXCEPTIONS
        {
            return Err(PersistenceRefusal::Overflow);
        }
    }
    Ok(())
}

pub(super) fn serialize_pretty_raw(doc: &PersistedState) -> serde_json::Result<String> {
    serde_json::to_string_pretty(doc)
}

pub(super) fn serialize_pretty_classified(doc: &PersistedState) -> Result<String, PersistError> {
    serialize_pretty_raw(doc).map_err(|e| PersistError::serialization(e.to_string()))
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoadError {
    Missing,
    Corrupt(String),
    Unsupported(u32),
    Refused(PersistenceRefusal),
}

#[cfg(test)]
pub(crate) fn load_from(path: &Path) -> Result<PersistedState, LoadError> {
    let text = match admit_state_file(path) {
        Ok(Some((text, _identity))) => text,
        Ok(None) => return Err(LoadError::Missing),
        Err(refusal) => return Err(LoadError::Refused(refusal)),
    };
    bounded::reset();
    let doc: PersistedState = match serde_json::from_str(&text) {
        Ok(doc) => doc,
        Err(e) => {
            if let Some(refusal) = bounded::tripped() {
                return Err(LoadError::Refused(refusal));
            }
            return Err(LoadError::Corrupt(e.to_string()));
        }
    };
    if doc.version > FORMAT_VERSION {
        return Err(LoadError::Unsupported(doc.version));
    }
    if let Err(refusal) = validate_bounded(&doc) {
        return Err(LoadError::Refused(refusal));
    }
    if let Err(refusal) = validate_ids(&doc) {
        return Err(LoadError::Refused(refusal));
    }
    Ok(doc)
}

#[cfg(test)]
pub(crate) fn save_to(path: &Path, doc: &PersistedState) -> std::io::Result<()> {
    let text = serialize_pretty_raw(doc)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_serialized(path, &text)
}
