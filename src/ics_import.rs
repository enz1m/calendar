//! Bounded, off-thread `.ics` admission.

use std::fs::{File, Metadata};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use enzui_core::atomic_state::open_state_file_nofollow;
use enzui_core::tempstage::verify_path_names_file;

use crate::ics::ParsedIcs;
use crate::ics_safety::{MAX_UID_LEN, MAX_URI_LEN};

pub(crate) const MAX_ICS_BYTES: usize = 4 * 1024 * 1024;

pub(crate) const MAX_PHYSICAL_LINES: usize = 200_000;

pub(crate) const MAX_UNFOLDED_LINE_BYTES: usize = 64 * 1024;

pub(crate) const MAX_EVENTS: usize = 5_000;

pub(crate) const MAX_TIMEZONES: usize = 128;

pub(crate) const MAX_PROPS_PER_COMPONENT: usize = 512;

pub(crate) const MAX_PARAMS_PER_PROPERTY: usize = 32;

pub(crate) const MAX_INVITEES: usize = 128;

pub(crate) const MAX_EXDATES: usize = 512;

pub(crate) const MAX_RRULE_PARTS: usize = 32;

pub(crate) const MAX_TEXT_FIELD_BYTES: usize = 4 * 1024;

pub(crate) const MAX_RETAINED_BYTES: usize = 1024 * 1024;

pub(crate) const MAX_TZID_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportRefusal {
    OpenFailed,
    NotRegularFile,
    TooLarge,
    InvalidUtf8,
    Grew,
    Overflow,
    Replaced,
}

impl ImportRefusal {
    #[must_use]
    pub(crate) const fn user_summary(self) -> &'static str {
        "The selected calendar file could not be imported."
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ImportCounters {
    pub physical_lines: usize,
    pub unfolded_lines: usize,
    pub source_bytes: usize,
    pub events: usize,
    pub timezones: usize,
    pub retained_bytes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AdmittedImport {
    pub parsed: ParsedIcs,
    pub counters: ImportCounters,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ImportOutcome {
    Admitted(AdmittedImport),
    Refused(ImportRefusal),
    ParseError(String),
    WorkerRefused(ImportWorkerRefusal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportWorkerRefusal {
    Busy,
    Cancelled,
    Shutdown,
    Panicked,
}

impl ImportWorkerRefusal {
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
            Self::Busy => "Calendar is busy reading a file. Try importing again in a moment.",
            Self::Cancelled | Self::Panicked => {
                "Reading that file was interrupted before it finished. Try importing it again."
            }
            Self::Shutdown => {
                "The background reader is no longer available. Restart Calendar to import again."
            }
        }
    }
}

#[must_use]
pub(crate) fn run_import(path: PathBuf) -> ImportOutcome {
    match admit_file(&path) {
        Err(refusal) => ImportOutcome::Refused(refusal),
        Ok(text) => {
            let mut counters = ImportCounters {
                source_bytes: text.len(),
                ..ImportCounters::default()
            };
            if let Err(refusal) = preflight(&text, &mut counters) {
                return ImportOutcome::Refused(refusal);
            }
            match crate::ics::parse_ics(&text, "") {
                Ok(parsed) => ImportOutcome::Admitted(AdmittedImport { parsed, counters }),
                Err(msg) => ImportOutcome::ParseError(msg),
            }
        }
    }
}

fn admit_file(path: &Path) -> Result<String, ImportRefusal> {
    let file = open_state_file_nofollow(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::InvalidInput {
            ImportRefusal::NotRegularFile
        } else {
            ImportRefusal::OpenFailed
        }
    })?;
    let meta = file.metadata().map_err(|_| ImportRefusal::OpenFailed)?;
    let text = admit_stream(&file, &meta)?;
    #[cfg(test)]
    test_seam::fire_after_read();
    // A pathname replacement cannot inherit the selected descriptor's admission.
    verify_path_still_names_file(&file, path)?;
    Ok(text)
}

fn verify_path_still_names_file(file: &File, path: &Path) -> Result<(), ImportRefusal> {
    verify_path_names_file(file, path).map_err(|_| ImportRefusal::Replaced)
}

fn admit_stream<R: Read>(reader: R, meta: &Metadata) -> Result<String, ImportRefusal> {
    if !meta.is_file() {
        return Err(ImportRefusal::NotRegularFile);
    }
    // An unlinked descriptor has no stable selected pathname to admit.
    if meta.nlink() == 0 {
        return Err(ImportRefusal::NotRegularFile);
    }
    let declared = meta.len();
    if declared > MAX_ICS_BYTES as u64 {
        return Err(ImportRefusal::TooLarge);
    }
    let mut buf = Vec::with_capacity(declared as usize);
    let mut limited = reader.take(MAX_ICS_BYTES as u64 + 1);
    limited
        .read_to_end(&mut buf)
        .map_err(|_| ImportRefusal::OpenFailed)?;
    if buf.len() > MAX_ICS_BYTES {
        return Err(ImportRefusal::Grew);
    }
    String::from_utf8(buf).map_err(|_| ImportRefusal::InvalidUtf8)
}

const MAX_COMPONENT_NAME_BYTES: usize = 32;

const MAX_STACK_DEPTH: usize = 8;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    VEvent,
    VTimezone,
    Other,
}

fn kind_of_ascii(uppercased: &[u8]) -> Kind {
    match uppercased {
        b"VEVENT" => Kind::VEvent,
        b"VTIMEZONE" => Kind::VTimezone,
        _ => Kind::Other,
    }
}

struct Frame {
    kind: Kind,
    name_len: u8,
    name: [u8; MAX_COMPONENT_NAME_BYTES],
    props: usize,
    invitees: usize,
    exdates: usize,
}

impl Frame {
    fn open(kind: Kind, raw: &str) -> Result<Self, ImportRefusal> {
        if raw.len() > MAX_COMPONENT_NAME_BYTES {
            return Err(ImportRefusal::Overflow);
        }
        let mut name = [0u8; MAX_COMPONENT_NAME_BYTES];
        for (i, b) in raw.bytes().enumerate() {
            name[i] = b.to_ascii_uppercase();
        }
        Ok(Self {
            kind,
            name_len: raw.len() as u8,
            name,
            props: 0,
            invitees: 0,
            exdates: 0,
        })
    }

    fn name_matches(&self, raw: &str) -> bool {
        let len = self.name_len as usize;
        if raw.len() != len {
            return false;
        }
        for (i, b) in raw.bytes().enumerate() {
            if b.to_ascii_uppercase() != self.name[i] {
                return false;
            }
        }
        true
    }
}

// Reject malformed component nesting before parser scopes can merge fields or reset limits.
fn preflight(text: &str, counters: &mut ImportCounters) -> Result<(), ImportRefusal> {
    let mut current = String::new();
    let mut unfolded_count: usize = 0;
    let mut retained: usize = 0;
    let mut stack: Vec<Frame> = Vec::new();

    for raw in text.split('\n') {
        counters.physical_lines = counters.physical_lines.saturating_add(1);
        if counters.physical_lines > MAX_PHYSICAL_LINES {
            return Err(ImportRefusal::Overflow);
        }
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        match line.chars().next() {
            Some(' ' | '\t') if !current.is_empty() => {
                let addition = line
                    .strip_prefix(' ')
                    .or_else(|| line.strip_prefix('\t'))
                    .unwrap_or(line);
                if current.len().saturating_add(addition.len()) > MAX_UNFOLDED_LINE_BYTES {
                    return Err(ImportRefusal::Overflow);
                }
                current.push_str(addition);
            }
            Some(_) => {
                if !current.is_empty() {
                    process_line(&current, &mut stack, &mut retained, counters)?;
                    unfolded_count = unfolded_count.saturating_add(1);
                }
                if line.len() > MAX_UNFOLDED_LINE_BYTES {
                    return Err(ImportRefusal::Overflow);
                }
                current.clear();
                current.push_str(line);
            }
            None => {}
        }
    }
    if !current.is_empty() {
        process_line(&current, &mut stack, &mut retained, counters)?;
        unfolded_count = unfolded_count.saturating_add(1);
    }
    if !stack.is_empty() {
        return Err(ImportRefusal::Overflow);
    }
    counters.unfolded_lines = unfolded_count;
    counters.retained_bytes = retained;
    Ok(())
}

fn process_line(
    line: &str,
    stack: &mut Vec<Frame>,
    retained: &mut usize,
    counters: &mut ImportCounters,
) -> Result<(), ImportRefusal> {
    let Some(colon) = first_unquoted_colon(line) else {
        return charge_content_prop(stack);
    };
    let (head, rest) = line.split_at(colon);
    let value = rest.strip_prefix(':').ok_or(ImportRefusal::Overflow)?;
    let (name, params) = head.split_once(';').unwrap_or((head, ""));
    let name = name.trim();
    let param_count = if params.is_empty() {
        0
    } else {
        params.matches(';').count().saturating_add(1)
    };
    if param_count > MAX_PARAMS_PER_PROPERTY {
        return Err(ImportRefusal::Overflow);
    }

    if name.eq_ignore_ascii_case("BEGIN") {
        return open_component(value.trim(), stack, counters);
    }
    if name.eq_ignore_ascii_case("END") {
        return close_component(value.trim(), stack);
    }

    charge_content_prop(stack)?;

    let field = value.trim();

    let in_vtimezone = stack.iter().any(|f| f.kind == Kind::VTimezone);
    if in_vtimezone && eq_ascii(name, "TZID") {
        return retain_field(retained, field.len(), MAX_TZID_BYTES);
    }

    let Some(vevent) = stack.iter_mut().rev().find(|f| f.kind == Kind::VEvent) else {
        return Ok(());
    };

    if eq_ascii(name, "SUMMARY") || eq_ascii(name, "LOCATION") || eq_ascii(name, "DESCRIPTION") {
        retain_field(retained, field.len(), MAX_TEXT_FIELD_BYTES)
    } else if eq_ascii(name, "URL") {
        retain_field(retained, field.len(), MAX_URI_LEN)
    } else if eq_ascii(name, "ATTENDEE") {
        vevent.invitees = vevent.invitees.saturating_add(1);
        if vevent.invitees > MAX_INVITEES {
            return Err(ImportRefusal::Overflow);
        }
        retain_field(retained, field.len(), MAX_URI_LEN)
    } else if eq_ascii(name, "UID") {
        retain_field(retained, field.len(), MAX_UID_LEN)
    } else if eq_ascii(name, "EXDATE") {
        let parts = field.split(',').count();
        vevent.exdates = vevent.exdates.saturating_add(parts);
        if vevent.exdates > MAX_EXDATES {
            return Err(ImportRefusal::Overflow);
        }
        Ok(())
    } else if eq_ascii(name, "RRULE") {
        let parts = if field.is_empty() {
            0
        } else {
            field.split(';').count()
        };
        if parts > MAX_RRULE_PARTS {
            return Err(ImportRefusal::Overflow);
        }
        retain_field(retained, field.len(), MAX_TEXT_FIELD_BYTES)
    } else {
        Ok(())
    }
}

fn open_component(
    raw: &str,
    stack: &mut Vec<Frame>,
    counters: &mut ImportCounters,
) -> Result<(), ImportRefusal> {
    if stack.len() >= MAX_STACK_DEPTH {
        return Err(ImportRefusal::Overflow);
    }
    let frame = Frame::open(kind_from_raw(raw), raw)?;
    if frame.kind != Kind::Other && stack.iter().any(|f| f.kind == frame.kind) {
        return Err(ImportRefusal::Overflow);
    }
    match frame.kind {
        Kind::VEvent => {
            counters.events = counters.events.saturating_add(1);
            if counters.events > MAX_EVENTS {
                return Err(ImportRefusal::Overflow);
            }
        }
        Kind::VTimezone => {
            counters.timezones = counters.timezones.saturating_add(1);
            if counters.timezones > MAX_TIMEZONES {
                return Err(ImportRefusal::Overflow);
            }
        }
        Kind::Other => {}
    }
    stack.push(frame);
    Ok(())
}

fn close_component(raw: &str, stack: &mut Vec<Frame>) -> Result<(), ImportRefusal> {
    let Some(top) = stack.last() else {
        return Err(ImportRefusal::Overflow);
    };
    if !top.name_matches(raw) {
        return Err(ImportRefusal::Overflow);
    }
    stack.pop();
    Ok(())
}

fn charge_content_prop(stack: &mut [Frame]) -> Result<(), ImportRefusal> {
    if let Some(top) = stack.last_mut() {
        top.props = top.props.saturating_add(1);
        if top.props > MAX_PROPS_PER_COMPONENT {
            return Err(ImportRefusal::Overflow);
        }
    }
    Ok(())
}

fn retain_field(retained: &mut usize, len: usize, field_cap: usize) -> Result<(), ImportRefusal> {
    if len > field_cap {
        return Err(ImportRefusal::Overflow);
    }
    let next = retained.saturating_add(len);
    if next > MAX_RETAINED_BYTES {
        return Err(ImportRefusal::Overflow);
    }
    *retained = next;
    Ok(())
}

fn eq_ascii(name: &str, upper: &str) -> bool {
    name.eq_ignore_ascii_case(upper)
}

fn kind_from_raw(raw: &str) -> Kind {
    let mut buf = [0u8; MAX_COMPONENT_NAME_BYTES];
    let n = raw.len().min(MAX_COMPONENT_NAME_BYTES);
    for (i, b) in raw.bytes().take(n).enumerate() {
        buf[i] = b.to_ascii_uppercase();
    }
    kind_of_ascii(&buf[..n])
}

// Preflight and parsing must agree on quoted colons when locating property values.
fn first_unquoted_colon(line: &str) -> Option<usize> {
    let mut in_quote = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quote = !in_quote,
            ':' if !in_quote => return Some(i),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
pub(crate) use test_seam::arm_after_read;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_seam {
    use std::cell::RefCell;

    thread_local! {
        static AFTER_READ: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }

    pub(crate) fn arm_after_read(hook: Box<dyn FnOnce()>) {
        AFTER_READ.with(|slot| *slot.borrow_mut() = Some(hook));
    }

    pub(crate) fn fire_after_read() {
        let hook = AFTER_READ.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
    }
}
