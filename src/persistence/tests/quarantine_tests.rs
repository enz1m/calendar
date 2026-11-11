use super::*;

#[test]
fn corrupt_state_requires_explicit_retirement_and_retains_original_bytes() {
    let dir = TempDir::new("corrupt");
    let path = dir.state_file();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = b"{\"version\":1,\"events\":[";
    std::fs::write(&path, bytes).unwrap();
    owner_only_file(&path);
    let (mut state, mut persist, origin) = boot_at(&path, today(), NOW, FirstRun::Demo);
    assert_eq!(origin, Origin::RecoveredFromCorrupt(None));
    assert!(state.events.is_empty());
    assert!(!persist.sync(&state));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let moved = retire_expected(&path, persist.retire_identity().unwrap()).unwrap();
    let queue = PersistenceQueue::default();
    drop(persist.enable_for(&state, &queue));
    let mut event = state.try_draft_at(today(), 600).unwrap();
    event.title = "After recovery".into();
    state.events.push(event);
    assert!(persist.sync(&state));
    assert_eq!(load_from(&path).unwrap().events[0].title, "After recovery");
    assert_eq!(std::fs::read(moved).unwrap(), bytes);
}

#[test]
fn future_format_cannot_be_overwritten() {
    let dir = TempDir::new("future");
    let path = dir.state_file();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = format!(r#"{{"version":{}}}"#, FORMAT_VERSION + 1);
    std::fs::write(&path, &bytes).unwrap();
    owner_only_file(&path);
    let (mut state, mut persist, origin) = boot_at(&path, today(), NOW, FirstRun::Demo);
    assert_eq!(origin, Origin::ReadOnlyFutureVersion(FORMAT_VERSION + 1));
    state.calendars[0].name = "Attempted edit".into();
    assert!(!persist.sync(&state));
    assert_eq!(std::fs::read_to_string(path).unwrap(), bytes);
}
