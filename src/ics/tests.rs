//! `.ics` reader gates.

use super::*;
use enzui_core::civil_date::Date;

fn d(y: i32, m: u32, day: u32) -> Date {
    Date::new(y, m, day).expect("valid fixture date")
}

#[test]
fn transition_zones_are_disclosed_instead_of_using_the_last_offset() {
    let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
        BEGIN:VTIMEZONE\r\nTZID:Fixture/Seasonal\r\n\
        BEGIN:STANDARD\r\nDTSTART:20251102T020000\r\nTZOFFSETFROM:-0400\r\nTZOFFSETTO:-0500\r\nEND:STANDARD\r\n\
        BEGIN:DAYLIGHT\r\nDTSTART:20260308T020000\r\nTZOFFSETFROM:-0500\r\nTZOFFSETTO:-0400\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n\
        BEGIN:VEVENT\r\nUID:seasonal@example.com\r\nDTSTART;TZID=Fixture/Seasonal:20260115T140000\r\nSUMMARY:Seasonal fixture\r\nEND:VEVENT\r\n\
        BEGIN:VEVENT\r\nUID:retained@example.com\r\nDTSTART:20260115T090000\r\nSUMMARY:Retained fixture\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    for text in [
        text.to_owned(),
        text.replace(
            "SUMMARY:Seasonal fixture",
            "RRULE:FREQ=WEEKLY\r\nSUMMARY:Seasonal fixture",
        ),
    ] {
        let parsed = parse_ics(&text, "cal.home").unwrap();
        assert_eq!(
            parsed.events.len(),
            1,
            "a seasonal offset must not be guessed"
        );
        assert_eq!(
            parsed.events[0].uid.as_deref(),
            Some("retained@example.com")
        );
        assert!(
            parsed
                .warnings
                .iter()
                .any(|warning| warning.contains("time zone"))
        );
    }

    let fixed = text
        .replace("TZOFFSETFROM:-0400", "TZOFFSETFROM:-0500")
        .replace("TZOFFSETTO:-0400", "TZOFFSETTO:-0500");
    let parsed = parse_ics(&fixed, "cal.home").unwrap();
    assert_eq!(
        parsed.events.len(),
        2,
        "a genuinely fixed offset remains supported"
    );
    let (exported, _, _) =
        crate::ics_export::serialize_calendar_checked(&parsed.events, d(2026, 1, 15));
    assert!(exported.contains("DTSTART:20260115T190000Z"));
}

#[test]
fn unknown_rrule_parts_are_preserved_re_emitted_and_warned() {
    let ics = "BEGIN:VCALENDAR\r\n\
        BEGIN:VEVENT\r\n\
        DTSTART:20260105T090000\r\n\
        SUMMARY:Last workday\r\n\
        RRULE:FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;X-VENDOR=acme\r\n\
        END:VEVENT\r\n\
        END:VCALENDAR\r\n";
    let p = parse_ics(ics, "cal.home").expect("valid calendar");
    let e = &p.events[0];
    assert!(
        e.recurrence_weekdays.is_empty(),
        "a BYSETPOS rule is not partly expanded"
    );
    let raw = e.recurrence_raw.clone().expect("the raw rule is preserved");
    assert_eq!(
        raw,
        "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;X-VENDOR=acme"
    );
    assert!(!p.warnings.is_empty(), "unsupported parts must warn");

    let text = crate::ics_export::serialize_calendar(std::slice::from_ref(e), d(2026, 1, 1));
    assert!(
        text.contains("RRULE:FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;X-VENDOR=acme"),
        "the unknown rule must round-trip byte-for-byte; got:\n{text}"
    );
}
