use std::path::PathBuf;

use super::*;

const REALISTIC: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:20260610T090000\r\nSUMMARY:Import\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

fn stamp() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

fn tmp_dir(tag: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("ics-import-{tag}-{}", stamp()));
    std::fs::create_dir_all(&path).expect("create scratch dir");
    path
}

fn tmp_file(tag: &str, bytes: &[u8]) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("ics-import-{tag}-{}.ics", stamp()));
    std::fs::write(&path, bytes).expect("write scratch file");
    path
}

fn tmp_from_text(tag: &str, text: &str) -> PathBuf {
    tmp_file(tag, text.as_bytes())
}

fn refused(outcome: ImportOutcome) -> ImportRefusal {
    match outcome {
        ImportOutcome::Refused(r) => r,
        other => panic!("expected Refused, got {other:?}"),
    }
}

#[test]
fn a_file_larger_than_the_ceiling_is_refused_before_reading() {
    let mut bytes = Vec::with_capacity(MAX_ICS_BYTES + 32);
    bytes.resize(MAX_ICS_BYTES + 16, b'X');
    let path = tmp_file("too-large", &bytes);
    assert_eq!(refused(run_import(path.clone())), ImportRefusal::TooLarge);
    let _ = std::fs::remove_file(path);
}

// The hook replaces the path while its original descriptor is still held.
#[test]
fn a_path_replaced_between_read_and_verify_is_refused_and_leaves_foreign_bytes_intact() {
    let dir = tmp_dir("replaced");
    let path = dir.join("selected.ics");
    std::fs::write(&path, REALISTIC).expect("write original");
    let foreign: &[u8] = b"FOREIGN-BYTES-THAT-MUST-STAY-INTACT\n";
    let swap_path = path.clone();
    let swap_bytes = foreign.to_vec();
    super::arm_after_read(Box::new(move || {
        std::fs::remove_file(&swap_path).expect("unlink original");
        std::fs::write(&swap_path, &swap_bytes).expect("write foreign");
    }));
    assert_eq!(refused(run_import(path.clone())), ImportRefusal::Replaced);
    assert_eq!(std::fs::read(&path).expect("read foreign"), foreign);
    let _ = std::fs::remove_dir_all(dir);
}

// Nested VEVENT blocks can bypass per-scope counters unless structural admission rejects them.
fn nested_vevent_bypass_fixture() -> String {
    use std::fmt::Write as _;
    let mut ics = String::from(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n\
         DTSTART:20260610T090000\r\nSUMMARY:S\r\n",
    );
    let each = MAX_INVITEES;
    for i in 0..each {
        write!(ics, "ATTENDEE:mailto:a{i}@example.com\r\n").unwrap();
    }
    ics.push_str("BEGIN:VEVENT\r\n");
    for i in 0..each {
        write!(ics, "ATTENDEE:mailto:b{i}@example.com\r\n").unwrap();
    }
    ics.push_str("END:VEVENT\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
    ics
}

#[test]
fn nested_events_cannot_bypass_admission_limits() {
    let ics = nested_vevent_bypass_fixture();
    let path = tmp_from_text("nested-vevent-bypass", &ics);
    assert_eq!(refused(run_import(path.clone())), ImportRefusal::Overflow);

    let _ = std::fs::remove_file(path);
}
