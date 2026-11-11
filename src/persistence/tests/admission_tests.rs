use super::*;

#[test]
fn replacing_the_admitted_state_file_prevents_overwrite() {
    let dir = TempDir::new("custody");
    let path = dir.state_file();
    let state = AppState::empty(today(), NOW);
    save_to(&path, &snapshot(&state)).unwrap();
    let (_, mut persist, _) = boot_at(&path, today(), NOW, FirstRun::Empty);
    std::fs::rename(&path, path.with_extension("original")).unwrap();
    let foreign = b"foreign document must survive";
    std::fs::write(&path, foreign).unwrap();
    let mut edited = state;
    edited.calendars[0].name = "Changed".into();
    let queue = PersistenceQueue::default();
    let pending = persist.prepare(&edited, &queue).unwrap();
    assert!(!matches!(
        pending.complete(&queue),
        PersistenceCompletion::Saved
    ));
    assert_eq!(std::fs::read(&path).unwrap(), foreign);
}

#[test]
fn duplicate_event_ids_are_refused_without_modifying_the_file() {
    let dir = TempDir::new("duplicate-ids");
    let path = dir.state_file();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = format!(r#"{{"version":{FORMAT_VERSION},"events":[{{"id":1}},{{"id":1}}]}}"#);
    std::fs::write(&path, &bytes).unwrap();
    owner_only_file(&path);
    let (_, persist, origin) = boot_at(&path, today(), NOW, FirstRun::Demo);
    assert_eq!(origin, Origin::RefusedBoot(PersistenceRefusal::Overflow));
    assert!(!persist.is_enabled());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
}
