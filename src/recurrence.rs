//! Civil-date recurrence queries for daily, weekly, monthly and yearly events.

use enzui_core::civil_date::{Date, MAX_YEAR, MIN_YEAR, Weekday};

use crate::model::{Event, Recurrence};

const MONTH_WALK_GUARD: u32 = 8192;

fn interval(event: &Event) -> i64 {
    i64::from(event.recurrence_interval.max(1))
}

fn shift_days(base: Date, days: i64) -> Option<Date> {
    let serial = i64::from(base.to_days()).checked_add(days)?;
    let date = Date::from_days(i32::try_from(serial).ok()?);
    Date::new(date.year(), date.month(), date.day())
}

fn shift_months(base: Date, months: i64) -> Option<Date> {
    let total = i64::from(base.year()) * 12 + i64::from(base.month()) - 1;
    let target = total.checked_add(months)?;
    if !(i64::from(MIN_YEAR) * 12..=i64::from(MAX_YEAR) * 12 + 11).contains(&target) {
        return None;
    }
    Some(base.add_months(i32::try_from(months).ok()?))
}

fn grid_monday(base: Date, weeks: i64) -> Option<Date> {
    let serial = i64::from(base.to_days()).checked_add(weeks.checked_mul(7)?)?;
    if serial > i64::from(Date::new(MAX_YEAR, 12, 31)?.to_days()) {
        return None;
    }
    // The first ISO week can begin before MIN_YEAR while containing valid dates.
    Some(Date::from_days(i32::try_from(serial).ok()?))
}

fn is_byday_weekly(event: &Event) -> bool {
    event.recurrence == Recurrence::Weekly && !event.recurrence_weekdays.is_empty()
}

fn weekly_set(event: &Event) -> Vec<Weekday> {
    if event.recurrence_weekdays.is_empty() {
        vec![event.date.weekday()]
    } else {
        let mut set = event.recurrence_weekdays.clone();
        set.sort_by_key(|w| w.index());
        set.dedup();
        set
    }
}

fn week_monday(d: Date) -> Date {
    d.add_days(-(d.weekday().index() as i32))
}

fn week_span(base: Date, d: Date) -> i64 {
    i64::from(week_monday(d).to_days() - week_monday(base).to_days()) / 7
}

fn weekly_grid_take(event: &Event, n: u32) -> Option<Date> {
    let mut rank = u64::from(n.checked_sub(1)?);
    let set = weekly_set(event);
    let base_monday = week_monday(event.date);
    let mut first_week = vec![event.date];
    for weekday in &set {
        if let Some(date) = shift_days(base_monday, weekday.index() as i64)
            && date > event.date
        {
            first_week.push(date);
        }
    }
    if rank < first_week.len() as u64 {
        return first_week.get(rank as usize).copied();
    }
    rank -= first_week.len() as u64;
    let weeks = i64::try_from(1 + rank / set.len() as u64).ok()?;
    let monday = grid_monday(base_monday, weeks.checked_mul(interval(event))?)?;
    shift_days(
        monday,
        set[(rank % set.len() as u64) as usize].index() as i64,
    )
}

fn weekly_grid_in(event: &Event, lo: Date, hi: Date) -> Vec<Date> {
    let mut out = Vec::new();
    if hi < event.date || hi < lo {
        return out;
    }
    let set = weekly_set(event);
    let step = interval(event);
    let base_monday = week_monday(event.date);
    if event.date >= lo && event.date <= hi && !set.contains(&event.date.weekday()) {
        out.push(event.date);
    }
    let lo_eff = lo.max(event.date);
    let span = week_span(event.date, lo_eff).max(0);
    let mut w = span - (span % step);
    for _ in 0..MONTH_WALK_GUARD {
        let Some(monday) = grid_monday(base_monday, w) else {
            break;
        };
        if monday > hi {
            break;
        }
        for weekday in &set {
            let Some(d) = shift_days(monday, weekday.index() as i64) else {
                continue;
            };
            if d >= event.date && d >= lo && d <= hi {
                out.push(d);
            }
        }
        let Some(next) = w.checked_add(step) else {
            break;
        };
        w = next;
    }
    // DTSTART precedes generated weekdays even when its weekday is outside BYDAY.
    out.sort_unstable_by_key(|d| d.to_days());
    out.dedup();
    out
}

fn weekly_next(event: &Event, from: Date) -> Option<Date> {
    let set = weekly_set(event);
    let step = interval(event);
    let base_monday = week_monday(event.date);
    let target = from.max(event.date);
    let last = if event.recurrence_count.is_some() {
        Some(count_ceiling(event)?)
    } else {
        None
    };
    let within_bounds = |d: Date| -> Option<bool> {
        if let Some(until) = event.recurrence_until
            && d > until
        {
            return None;
        }
        if let Some(last) = last
            && d > last
        {
            return None;
        }
        Some(!event.exceptions.contains(&d))
    };
    if target == event.date && !set.contains(&event.date.weekday()) {
        match within_bounds(event.date) {
            None => return None,
            Some(true) => return Some(event.date),
            Some(false) => {}
        }
    }
    let span = week_span(event.date, target).max(0);
    let mut w = span - (span % step);
    for _ in 0..MONTH_WALK_GUARD {
        let monday = grid_monday(base_monday, w)?;
        for weekday in &set {
            let Some(d) = shift_days(monday, weekday.index() as i64) else {
                continue;
            };
            if d < target || d < event.date {
                continue;
            }
            match within_bounds(d) {
                None => return None,
                Some(true) => return Some(d),
                Some(false) => {}
            }
        }
        w = w.checked_add(step)?;
    }
    None
}

fn day_step(event: &Event) -> Option<i64> {
    match event.recurrence {
        Recurrence::Daily => Some(interval(event)),
        Recurrence::Weekly => Some(7 * interval(event)),
        _ => None,
    }
}

fn month_step(event: &Event) -> Option<i64> {
    match event.recurrence {
        Recurrence::Monthly => Some(interval(event)),
        Recurrence::Yearly => Some(12 * interval(event)),
        _ => None,
    }
}

fn months_between(base: Date, date: Date) -> i64 {
    i64::from(date.year() - base.year()) * 12 + i64::from(date.month()) - i64::from(base.month())
}

fn on_grid(event: &Event, date: Date) -> bool {
    let step = interval(event);
    match event.recurrence {
        Recurrence::None => false,
        Recurrence::Daily => {
            let days = i64::from(date.to_days() - event.date.to_days());
            days >= 0 && days % step == 0
        }
        Recurrence::Weekly => {
            if !weekly_set(event).contains(&date.weekday()) {
                return false;
            }
            let span = week_span(event.date, date);
            span >= 0 && span % step == 0
        }
        Recurrence::Monthly => {
            if date.day() != event.date.day() {
                return false;
            }
            let months = months_between(event.date, date);
            months >= 0 && months % step == 0
        }
        Recurrence::Yearly => {
            if date.month() != event.date.month() || date.day() != event.date.day() {
                return false;
            }
            let years = i64::from(date.year() - event.date.year());
            years >= 0 && years % step == 0
        }
    }
}

#[must_use]
pub(crate) fn occurs_on(event: &Event, date: Date) -> bool {
    if event.exceptions.contains(&date) {
        return false;
    }
    if date == event.date {
        return true;
    }
    if date < event.date {
        return false;
    }
    if let Some(until) = event.recurrence_until
        && date > until
    {
        return false;
    }
    if event.recurrence_count.is_some() {
        let Some(last) = count_ceiling(event) else {
            return false;
        };
        if date > last {
            return false;
        }
    }
    on_grid(event, date)
}

fn nth_grid(event: &Event, n: u32) -> Option<Date> {
    let rank = i64::from(n.checked_sub(1)?);
    if rank == 0 {
        return Some(event.date);
    }
    if event.recurrence == Recurrence::None {
        return None;
    }
    if is_byday_weekly(event) {
        return weekly_grid_take(event, n);
    }
    if let Some(step_days) = day_step(event) {
        return shift_days(event.date, rank.checked_mul(step_days)?);
    }
    let step_months = month_step(event)?;
    let minimum_offset = rank.checked_mul(step_months)?;
    shift_months(event.date, minimum_offset)?;
    if event.date.day() <= 28 {
        return shift_months(event.date, minimum_offset);
    }

    // Gregorian anchor-day validity repeats every 4800 months.
    const CYCLE_MONTHS: i64 = 4800;
    let stride = step_months % CYCLE_MONTHS;
    let (mut a, mut b) = (CYCLE_MONTHS, stride);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let cycle_len = CYCLE_MONTHS / a;
    let phase =
        i64::from(event.date.year().rem_euclid(400)) * 12 + i64::from(event.date.month()) - 1;
    let valid: Vec<i64> = (0..cycle_len)
        .filter(|period| {
            let month = (phase + period * stride) % CYCLE_MONTHS;
            Date::new(
                400 + (month / 12) as i32,
                (month % 12 + 1) as u32,
                event.date.day(),
            )
            .is_some()
        })
        .collect();
    let per_cycle = i64::try_from(valid.len()).ok()?;
    let period = (rank / per_cycle)
        .checked_mul(cycle_len)?
        .checked_add(valid[(rank % per_cycle) as usize])?;
    shift_months(event.date, period.checked_mul(step_months)?)
}

fn count_ceiling(event: &Event) -> Option<Date> {
    let count = event.recurrence_count?;
    if count == 0 {
        return None;
    }
    nth_grid(event, count).or_else(|| Date::new(MAX_YEAR, 12, 31))
}

fn hard_end(event: &Event, ceiling: Date) -> Option<Date> {
    let mut end = ceiling;
    if let Some(until) = event.recurrence_until
        && until < end
    {
        end = until;
    }
    if event.recurrence_count.is_some() {
        let last = count_ceiling(event)?;
        if last < end {
            end = last;
        }
    }
    Some(end)
}

fn first_day_grid_on_or_after(base: Date, target: Date, step_days: i64) -> Option<Date> {
    let diff = i64::from(target.to_days() - base.to_days());
    if diff <= 0 {
        return Some(base);
    }
    let k = (diff + step_days - 1) / step_days;
    shift_days(base, k.checked_mul(step_days)?)
}

#[must_use]
pub(crate) fn occurrences_in_range(event: &Event, from: Date, to: Date) -> Vec<Date> {
    let mut out = Vec::new();
    if to < from || to < event.date {
        return out;
    }
    let Some(end) = hard_end(event, to) else {
        return out;
    };
    if end < event.date {
        return out;
    }
    let excepted = |d: &Date| event.exceptions.contains(d);

    if is_byday_weekly(event) {
        out = weekly_grid_in(event, from, end);
        out.retain(|d| !excepted(d));
        return out;
    }

    match event.recurrence {
        Recurrence::None => {
            if event.date >= from && event.date <= end && !excepted(&event.date) {
                out.push(event.date);
            }
        }
        Recurrence::Daily | Recurrence::Weekly => {
            let step = day_step(event).expect("daily/weekly has a day step");
            let Some(mut d) = first_day_grid_on_or_after(event.date, from.max(event.date), step)
            else {
                return out;
            };
            while d <= end {
                if !excepted(&d) {
                    out.push(d);
                }
                let Some(next) = shift_days(d, step) else {
                    break;
                };
                d = next;
            }
        }
        Recurrence::Monthly | Recurrence::Yearly => {
            let step = month_step(event).expect("monthly/yearly has a month step");
            // Jump arithmetically to the period near the range start instead of walking from the
            // series anchor.
            let target = from.max(event.date);
            let approx = months_between(event.date, target).max(0);
            let mut k = (approx / step) * step;
            for _ in 0..MONTH_WALK_GUARD {
                let Some(candidate) = shift_months(event.date, k) else {
                    break;
                };
                if candidate > end {
                    break;
                }
                if candidate >= from && candidate.day() == event.date.day() && !excepted(&candidate)
                {
                    out.push(candidate);
                }
                let Some(next_k) = k.checked_add(step) else {
                    break;
                };
                k = next_k;
            }
        }
    }
    out
}

#[must_use]
pub(crate) fn has_occurrence_in_range(event: &Event, from: Date, to: Date) -> bool {
    if to < from || to < event.date {
        return false;
    }
    hard_end(event, to)
        .is_some_and(|end| next_on_or_after(event, from).is_some_and(|date| date <= end))
}

#[must_use]
pub(crate) fn next_on_or_after(event: &Event, from: Date) -> Option<Date> {
    if event.recurrence == Recurrence::None {
        return (event.date >= from && !event.exceptions.contains(&event.date))
            .then_some(event.date);
    }
    if is_byday_weekly(event) {
        return weekly_next(event, from);
    }
    let target = from.max(event.date);
    let excepted = |d: &Date| event.exceptions.contains(d);

    if let Some(step) = day_step(event) {
        let mut d = first_day_grid_on_or_after(event.date, target, step)?;
        // Only finitely many EXDATEs can interpose between successive recurrence-grid dates.
        for _ in 0..=event.exceptions.len() {
            if let Some(until) = event.recurrence_until
                && d > until
            {
                return None;
            }
            if event.recurrence_count.is_some()
                && !count_ceiling(event).is_some_and(|last| d <= last)
            {
                return None;
            }
            if !excepted(&d) {
                return Some(d);
            }
            d = shift_days(d, step)?;
        }
        return None;
    }

    let step = month_step(event)?;
    let approx = months_between(event.date, target).max(0);
    let mut k = (approx / step) * step;
    let last = if event.recurrence_count.is_some() {
        Some(count_ceiling(event)?)
    } else {
        None
    };
    for _ in 0..MONTH_WALK_GUARD {
        let candidate = shift_months(event.date, k)?;
        if let Some(until) = event.recurrence_until
            && candidate > until
        {
            return None;
        }
        if let Some(last) = last
            && candidate > last
        {
            return None;
        }
        if candidate >= target && candidate.day() == event.date.day() && !excepted(&candidate) {
            return Some(candidate);
        }
        k = k.checked_add(step)?;
    }
    None
}

#[cfg(test)]
mod tests;
