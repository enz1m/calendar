//! Limits field lengths and collection counts while decoding persisted state.

use std::cell::Cell;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, DeserializeSeed, Deserializer, SeqAccess, Visitor};

use super::{FiredRec, PersistenceRefusal, SnoozeRec};

pub(super) const MAX_RETAINED_STATE_BYTES: usize = 8 * 1024 * 1024;

const SENTINEL: &str = "calendar_app_bounded_input_trip";

thread_local! {
    static TRIP: Cell<Option<PersistenceRefusal>> = const { Cell::new(None) };
    static RETAINED: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn reset() {
    TRIP.with(|c| c.set(None));
    RETAINED.with(|c| c.set(0));
}

pub(super) fn tripped() -> Option<PersistenceRefusal> {
    TRIP.with(Cell::get)
}

fn trip(reason: PersistenceRefusal) {
    TRIP.with(|c| c.set(Some(reason)));
}

fn charge(n: usize) -> bool {
    RETAINED.with(|c| {
        let next = c.get().saturating_add(n);
        if next > MAX_RETAINED_STATE_BYTES {
            trip(PersistenceRefusal::Overflow);
            false
        } else {
            c.set(next);
            true
        }
    })
}

fn bound_error<E: de::Error>() -> E {
    E::custom(SENTINEL)
}

struct CappedStringVisitor {
    cap: usize,
}

impl<'de> Visitor<'de> for CappedStringVisitor {
    type Value = String;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "a string of at most {} bytes", self.cap)
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
        if v.len() > self.cap {
            trip(PersistenceRefusal::Overflow);
            return Err(bound_error());
        }
        if !charge(v.len()) {
            return Err(bound_error());
        }
        Ok(v.to_owned())
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<String, E> {
        if v.len() > self.cap {
            trip(PersistenceRefusal::Overflow);
            return Err(bound_error());
        }
        if !charge(v.len()) {
            return Err(bound_error());
        }
        Ok(v)
    }
}

fn cap_string<'de, D: Deserializer<'de>>(d: D, cap: usize) -> Result<String, D::Error> {
    d.deserialize_string(CappedStringVisitor { cap })
}

pub(super) fn short_string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    cap_string(d, 256)
}

pub(super) fn medium_string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    cap_string(d, 1024)
}

pub(super) fn text_field<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    cap_string(d, 16 * 1024)
}

pub(super) fn invitees_string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    cap_string(d, 8 * 1024)
}

struct OptionCappedStringVisitor {
    cap: usize,
}

impl<'de> Visitor<'de> for OptionCappedStringVisitor {
    type Value = Option<String>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "an optional string of at most {} bytes", self.cap)
    }

    fn visit_none<E: de::Error>(self) -> Result<Option<String>, E> {
        Ok(None)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Option<String>, E> {
        Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Option<String>, D::Error> {
        cap_string(d, self.cap).map(Some)
    }
}

fn cap_option_string<'de, D: Deserializer<'de>>(
    d: D,
    cap: usize,
) -> Result<Option<String>, D::Error> {
    d.deserialize_option(OptionCappedStringVisitor { cap })
}

pub(super) fn short_option_string<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<String>, D::Error> {
    cap_option_string(d, 256)
}

pub(super) fn text_option_string<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<String>, D::Error> {
    cap_option_string(d, 16 * 1024)
}

struct CappedStringVec {
    count_cap: usize,
    per_string_cap: usize,
}

impl<'de> Visitor<'de> for CappedStringVec {
    type Value = Vec<String>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "up to {} strings, each at most {} bytes",
            self.count_cap, self.per_string_cap
        )
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<String>, A::Error> {
        let mut out: Vec<String> = Vec::new();
        loop {
            if out.len() >= self.count_cap {
                // One extra bounded element distinguishes a full collection from overflow.
                let more = seq.next_element_seed(CappedStringSeed {
                    cap: self.per_string_cap,
                })?;
                if more.is_some() {
                    trip(PersistenceRefusal::Overflow);
                    return Err(bound_error());
                }
                if !charge(std::mem::size_of::<String>() * out.len()) {
                    return Err(bound_error());
                }
                return Ok(out);
            }
            match seq.next_element_seed(CappedStringSeed {
                cap: self.per_string_cap,
            })? {
                Some(s) => out.push(s),
                None => {
                    if !charge(std::mem::size_of::<String>() * out.len()) {
                        return Err(bound_error());
                    }
                    return Ok(out);
                }
            }
        }
    }
}

struct CappedStringSeed {
    cap: usize,
}

impl<'de> DeserializeSeed<'de> for CappedStringSeed {
    type Value = String;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<String, D::Error> {
        cap_string(d, self.cap)
    }
}

pub(super) fn hidden_categories<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    d.deserialize_seq(CappedStringVec {
        count_cap: 128,
        per_string_cap: 256,
    })
}

pub(super) fn exceptions<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    d.deserialize_seq(CappedStringVec {
        count_cap: 512,
        per_string_cap: 32,
    })
}

pub(super) fn recurrence_weekdays<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<String>, D::Error> {
    d.deserialize_seq(CappedStringVec {
        count_cap: 7,
        per_string_cap: 3,
    })
}

struct CappedRecordVec<T> {
    count_cap: usize,
    estimate: fn(&T) -> usize,
}

impl<'de, T> Visitor<'de> for CappedRecordVec<T>
where
    T: Deserialize<'de>,
{
    type Value = Vec<T>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "up to {} records", self.count_cap)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
        let mut out: Vec<T> = Vec::new();
        loop {
            if out.len() >= self.count_cap {
                let more: Option<T> = seq.next_element()?;
                if more.is_some() {
                    trip(PersistenceRefusal::Overflow);
                    return Err(bound_error());
                }
                return Ok(out);
            }
            match seq.next_element::<T>()? {
                Some(t) => {
                    if !charge((self.estimate)(&t)) {
                        return Err(bound_error());
                    }
                    out.push(t);
                }
                None => return Ok(out),
            }
        }
    }
}

fn calendar_bytes(c: &super::CalendarRec) -> usize {
    c.id.len() + c.name.len() + c.section.len() + c.color.len() + 64
}

fn event_bytes(e: &super::EventRec) -> usize {
    let opt = |s: &Option<String>| s.as_deref().map_or(0, str::len);
    e.calendar_id.len()
        + e.title.len()
        + e.location.len()
        + e.notes.len()
        + e.url.len()
        + e.invitees.len()
        + e.date.len()
        + opt(&e.end_date)
        + e.recurrence.len()
        + opt(&e.recurrence_until)
        + opt(&e.recurrence_raw)
        + opt(&e.uid)
        + opt(&e.recurrence_id)
        + opt(&e.color_category)
        + e.exceptions.iter().map(String::len).sum::<usize>()
        + e.recurrence_weekdays.iter().map(String::len).sum::<usize>()
        + 256
}

fn fired_bytes(f: &FiredRec) -> usize {
    f.on.len() + 32
}

fn snoozed_bytes(s: &SnoozeRec) -> usize {
    s.on.len() + s.until_date.len() + 48
}

pub(super) fn calendars<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<super::CalendarRec>, D::Error> {
    d.deserialize_seq(CappedRecordVec {
        count_cap: 512,
        estimate: calendar_bytes,
    })
}

pub(super) fn events<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<super::EventRec>, D::Error> {
    d.deserialize_seq(CappedRecordVec {
        count_cap: 50_000,
        estimate: event_bytes,
    })
}

pub(super) fn fired_vec<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<FiredRec>, D::Error> {
    d.deserialize_seq(CappedRecordVec {
        count_cap: 100_000,
        estimate: fired_bytes,
    })
}

pub(super) fn snoozed_vec<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<SnoozeRec>, D::Error> {
    d.deserialize_seq(CappedRecordVec {
        count_cap: 1_000,
        estimate: snoozed_bytes,
    })
}
