use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use enzui_core::civil_date::Date;
use serde_json::{Value, json};

use crate::message::Message;
use crate::model::{Event, MinuteOfDay, TzAnchor};
use crate::reminders::{ReminderKey, abs_minute};
use crate::state::{AppState, Overlay};
use crate::{App, persistence};

const JOB: &str = "next-reminder";

struct Fixture {
    root: PathBuf,
    test_name: String,
}

struct ChildGuard(std::process::Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_some() {
            return;
        }
        let _ = self.0.kill();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        eprintln!("isolated child did not exit after its kill deadline");
    }
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/.activation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .expect("new private fixture");
        for directory in ["home", "runtime", "config", "data", "state", "cache"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(directory))
                .unwrap();
        }
        fs::write(root.join("fixture-marker"), "calendar-activation-test").unwrap();
        fs::write(
            root.join("systemctl"),
            include_str!("../tests/fixtures/reminder_activation/systemctl.py"),
        )
        .unwrap();
        fs::set_permissions(root.join("systemctl"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            root.join("session.py"),
            include_str!("../tests/fixtures/reminder_activation/session.py"),
        )
        .unwrap();
        Self {
            root,
            test_name: std::thread::current().name().unwrap().to_owned(),
        }
    }

    fn seed(&self, date: Date, now: i32, due: i32, anchor: TzAnchor) {
        let mut state = AppState::empty(date, now);
        state.events.push(Event {
            title: "Stable occurrence".into(),
            start_minute: MinuteOfDay::new(due),
            end_minute: MinuteOfDay::new(due + 30),
            alert: Some(0),
            tz_anchor: anchor,
            ..Event::new(73, "cal.home", date)
        });
        fs::DirBuilder::new()
            .mode(0o700)
            .create(self.root.join("data/calendar-app"))
            .unwrap();
        state.next_id = 74;
        state.reminders.last_checked = Some(abs_minute(date, now));
        persistence::save_to(&self.state_path(), &persistence::snapshot(&state)).unwrap();
    }

    fn state_path(&self) -> PathBuf {
        self.root.join("data/calendar-app/state.json")
    }

    fn step(
        &self,
        date: Date,
        minute: i32,
        action: &str,
        arguments: Vec<String>,
        zone: &str,
    ) -> Value {
        fs::write(
            self.root.join("input.json"),
            serde_json::to_vec(&json!({
                "day": date.to_days(), "minute": minute, "action": action, "arguments": arguments,
            }))
            .unwrap(),
        )
        .unwrap();
        let log_path = self.root.join("child.log");
        let log = fs::File::create(&log_path).unwrap();
        let mut command = Command::new("/usr/bin/bwrap");
        command
            .env_clear()
            .args([
                "--unshare-all",
                "--die-with-parent",
                "--ro-bind",
                "/",
                "/",
                "--proc",
                "/proc",
                "--dev",
                "/dev",
                "--tmpfs",
                "/run",
                "--tmpfs",
                "/tmp",
                "--bind",
            ])
            .arg(&self.root)
            .arg("/tmp/fixture")
            .arg("--ro-bind")
            .arg(self.root.join("systemctl"))
            .arg("/usr/bin/systemctl")
            .args(["--chdir", "/tmp/fixture"]);
        for (key, value) in [
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/tmp/fixture/home"),
            ("XDG_RUNTIME_DIR", "/tmp/fixture/runtime"),
            ("XDG_DATA_HOME", "/tmp/fixture/data"),
            ("XDG_CONFIG_HOME", "/tmp/fixture/config"),
            ("XDG_STATE_HOME", "/tmp/fixture/state"),
            ("XDG_CACHE_HOME", "/tmp/fixture/cache"),
            (
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/tmp/fixture/runtime/bus",
            ),
            ("LANG", "C.UTF-8"),
            ("TZ", zone),
            ("ICED_TEST_BACKEND", "tiny-skia"),
            ("CALENDAR_ACTIVATION_CHILD", "1"),
        ] {
            command.args(["--setenv", key, value]);
        }
        if action == "sandbox-boot" {
            command.args(["--setenv", "FLATPAK_ID", crate::package::APP_ID]);
        }
        let child = command
            .args(["/usr/bin/python3", "/tmp/fixture/session.py"])
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", &self.test_name, "--nocapture"])
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .expect("launch isolated route child");
        let mut child = ChildGuard(child);
        let deadline = Instant::now() + Duration::from_secs(40);
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                panic!(
                    "route child deadline: {}",
                    fs::read_to_string(&log_path).unwrap()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let log = fs::read_to_string(log_path).unwrap();
        print!("{log}");
        assert!(status.success(), "isolated route child failed: {status}");
        assert!(
            log.contains("test result: ok. 1 passed"),
            "child must execute one test"
        );
        serde_json::from_slice(&fs::read(self.root.join("output.json")).unwrap()).unwrap()
    }

    fn units(&self, extension: &str) -> Vec<PathBuf> {
        let directory = self.root.join("config/systemd/user");
        let Ok(entries) = fs::read_dir(directory) else {
            return Vec::new();
        };
        let mut paths: Vec<_> = entries
            .map(Result::unwrap)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == extension))
            .collect();
        paths.sort();
        paths
    }

    fn invocation(&self) -> Vec<String> {
        let units = self.units("service");
        assert_eq!(units.len(), 1, "one current service");
        let service = fs::read_to_string(&units[0]).unwrap();
        let command = service
            .lines()
            .find_map(|line| line.strip_prefix("ExecStart=:"))
            .unwrap();
        let words: Vec<_> = command
            .split('"')
            .enumerate()
            .filter(|(i, _)| i % 2 == 1)
            .map(|(_, word)| word.to_owned())
            .collect();
        assert_eq!(
            words.len(),
            5,
            "executable and framework activation argv: {command}"
        );
        assert_eq!(Path::new(&words[0]), std::env::current_exe().unwrap());
        assert_eq!(
            &words[1..4],
            [
                "--enzui-durable-activation",
                "io.github.enzui.Calendar",
                JOB
            ]
        );
        words[1..].to_vec()
    }

    fn instant(&self) -> u64 {
        let units = self.units("timer");
        assert_eq!(units.len(), 1, "one next-reminder timer");
        let timer = fs::read_to_string(&units[0]).unwrap();
        assert!(timer.contains("Persistent=true"));
        timer
            .lines()
            .find_map(|l| l.strip_prefix("OnCalendar=@"))
            .unwrap()
            .parse()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove generated session");
    }
}

fn child_mode() -> bool {
    if std::env::var_os("CALENDAR_ACTIVATION_CHILD").is_none() {
        return false;
    }
    assert_eq!(std::env::var("HOME").unwrap(), "/tmp/fixture/home");
    assert_eq!(
        fs::read_to_string("/tmp/fixture/fixture-marker").unwrap(),
        "calendar-activation-test"
    );
    let input: Value =
        serde_json::from_slice(&fs::read("/tmp/fixture/input.json").unwrap()).unwrap();
    let date = Date::from_days(i32::try_from(input["day"].as_i64().unwrap()).unwrap());
    let now = i32::try_from(input["minute"].as_i64().unwrap()).unwrap();
    let resolver = enzui_core::app_data::AppDataResolver::from_env();
    let path = persistence::resolve_state_location(&resolver).unwrap();
    assert_eq!(path, Path::new("/tmp/fixture/data/calendar-app/state.json"));
    if input["action"] == "derive" {
        let (state, _, _) = persistence::boot_at(&path, date, now, persistence::FirstRun::Empty);
        let result = match super::desired(&state) {
            Ok(Some(desired)) => {
                json!({"instant": desired.instant.duration_since(UNIX_EPOCH).unwrap().as_secs()})
            }
            other => {
                assert!(other.is_err(), "expected a deadline or explicit failure");
                json!({"unrepresentable": true})
            }
        };
        fs::write(
            "/tmp/fixture/output.json",
            serde_json::to_vec(&result).unwrap(),
        )
        .unwrap();
        return true;
    }
    let pre_boot = AppState::pre_boot(date, now, 1);
    let persist = persistence::Persist::disabled(path.clone(), persistence::snapshot(&pre_boot));
    let (mut app, _) = App::start((pre_boot, persist, persistence::Origin::FreshStart));
    app.boot_worker = std::sync::Arc::new(
        enzui_core::blocking_task::BlockingTaskOwner::with_defaults("reminder-route-test").unwrap(),
    );
    app.pending_boot_generation = 1;
    let arguments: Vec<String> = serde_json::from_value(input["arguments"].clone()).unwrap();
    let outcome = crate::boot::run_production_boot(
        crate::boot::BootInputs {
            path,
            today: date,
            now_minute: now,
            first_run: persistence::FirstRun::Empty,
        },
        arguments,
    );
    route(
        &mut app,
        Message::BootFinished {
            generation: 1,
            outcome: crate::boot::BootDelivery::new(Ok(outcome)),
        },
    );
    let raised = match app.state.overlay {
        Overlay::ReminderAlert { event_id, on, .. } => json!({
            "event": event_id, "day": on.to_days(),
            "notification": crate::notify::reminder_notification_id(ReminderKey { event_id, on }).as_str(),
        }),
        _ => Value::Null,
    };
    match input["action"].as_str().unwrap() {
        "snooze" => {
            route(&mut app, Message::SnoozeReminder);
        }
        "dismiss" => {
            route(&mut app, Message::DismissReminder);
        }
        "boot" | "sandbox-boot" => {}
        action => panic!("unknown fixture action {action}"),
    }
    let overlay_after = matches!(app.state.overlay, Overlay::ReminderAlert { .. });
    let close = route(&mut app, Message::CloseWindow(iced::window::Id::unique()));
    fs::write(
        "/tmp/fixture/output.json",
        serde_json::to_vec(&json!({
            "raised": raised, "close": close, "overlay_after": overlay_after,
            "snoozed": app.state.reminders.snoozed.len(), "fired": app.state.reminders.fired.len(),
        }))
        .unwrap(),
    )
    .unwrap();
    true
}

fn route(app: &mut App, message: Message) -> usize {
    crate::close_tests::CloseDriver::new(app.update(message)).pump(app, Duration::from_secs(10))
}

fn recent_clock() -> (Date, i32) {
    let minutes = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 60;
    let day = i32::try_from((minutes - 60) / 1440).unwrap();
    let now = i32::try_from((minutes - 60) % 1440).unwrap().min(1380);
    (Date::from_days(day), now)
}

fn assert_raised(output: &Value, date: Date) {
    assert_eq!(
        output["raised"]["event"], 73,
        "claimed occurrence must reach the app overlay: {output}"
    );
    assert_eq!(output["raised"]["day"], date.to_days());
    assert_eq!(
        output["raised"]["notification"],
        format!("reminder.73.{}", date.to_days())
    );
    assert_eq!(output["close"], 1);
}

#[test]
fn claimed_snooze_and_dismiss_survive_separate_app_sessions() {
    if child_mode() {
        return;
    }
    let fixture = Fixture::new();
    let (date, now) = recent_clock();
    fixture.seed(date, now, now + 1, TzAnchor::Floating);
    let scheduled = fixture.step(date, now, "boot", vec![], "UTC");
    assert_eq!(scheduled["raised"], Value::Null);
    assert_eq!(scheduled["close"], 1);
    assert_eq!(
        fixture.instant(),
        u64::try_from(abs_minute(date, now + 1)).unwrap() * 60
    );
    let original = fixture.invocation();
    let snoozed = fixture.step(date, now + 1, "snooze", original.clone(), "UTC");
    assert_raised(&snoozed, date);
    assert_eq!(snoozed["snoozed"], 1);
    assert_eq!(snoozed["overlay_after"], false);
    assert_eq!(
        fixture.instant(),
        u64::try_from(abs_minute(date, now + 6)).unwrap() * 60
    );
    let postponed = fixture.invocation();
    assert_ne!(postponed, original);
    let replay = fixture.step(date, now + 6, "boot", original, "UTC");
    assert_eq!(replay["raised"], Value::Null);
    let dismissed = fixture.step(date, now + 6, "dismiss", fixture.invocation(), "UTC");
    assert_raised(&dismissed, date);
    assert_eq!(dismissed["snoozed"], 0);
    assert_eq!(dismissed["overlay_after"], false);
    assert_eq!(dismissed["fired"], 1);
    assert!(fixture.units("timer").is_empty());
    let restarted = fixture.step(date, now + 7, "boot", vec![], "UTC");
    assert_eq!(restarted["raised"], Value::Null);
    assert_eq!(restarted["fired"], 1);
}

#[test]
fn timezone_fold_gap_and_fixed_utc_produce_absolute_deadlines() {
    if child_mode() {
        return;
    }
    let date = Date::new(2026, 11, 1).unwrap();
    let fixture = Fixture::new();
    fixture.seed(date, 0, 90, TzAnchor::Floating);
    let folded = fixture.step(date, 0, "derive", vec![], "America/New_York");
    let utc_midnight = u64::try_from(abs_minute(date, 0)).unwrap() * 60;
    assert_eq!(folded["instant"], utc_midnight + 5 * 3600 + 30 * 60);
    let utc = fixture.step(date, 0, "derive", vec![], "UTC");
    assert_eq!(utc["instant"], utc_midnight + 90 * 60);

    let anchored = Fixture::new();
    anchored.seed(
        date,
        0,
        90,
        TzAnchor::FixedUtc {
            start_minutes: -240,
            end_minutes: -240,
        },
    );
    let new_york = anchored.step(date, 0, "derive", vec![], "America/New_York");
    let utc = anchored.step(date, 0, "derive", vec![], "UTC");
    assert_eq!(new_york, utc);
    assert_eq!(utc["instant"], utc_midnight + 5 * 3600 + 30 * 60);

    let gap = Fixture::new();
    let date = Date::new(2026, 3, 8).unwrap();
    gap.seed(date, 0, 150, TzAnchor::Floating);
    let refused = gap.step(date, 0, "derive", vec![], "America/New_York");
    assert_eq!(refused["unrepresentable"], true);
    let allowed = gap.step(date, 0, "derive", vec![], "UTC");
    assert_eq!(
        allowed["instant"],
        u64::try_from(abs_minute(date, 150)).unwrap() * 60
    );
}

#[test]
fn sandboxed_reminders_stay_in_app_without_publishing_host_timers() {
    if child_mode() {
        return;
    }
    let fixture = Fixture::new();
    let (date, now) = recent_clock();
    fixture.seed(date, now, now + 1, TzAnchor::Floating);
    let waiting = fixture.step(date, now, "sandbox-boot", vec![], "UTC");
    assert_eq!(waiting["raised"], Value::Null);
    assert_eq!(
        waiting["close"], 1,
        "sandbox fallback must not leave close debt"
    );
    let due = fixture.step(date, now + 1, "sandbox-boot", vec![], "UTC");
    assert_raised(&due, date);
    assert_eq!(due["fired"], 1);
    assert!(fixture.units("timer").is_empty());
    assert!(!fixture.root.join("manager-calls.jsonl").exists());
    assert!(
        !fixture
            .root
            .join("state/io.github.enzui.Calendar/durable-activation")
            .exists()
    );
}
