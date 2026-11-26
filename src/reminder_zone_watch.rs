//! Systemd wakeups to replan durable reminders without opening a window.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use super::{Failure, FailureKind, Scheduler};
use crate::persistence;
use enzui_core::cancellation::CancelToken;
use enzui_core::{atomic_state::StateLock, civil_date::Date};
use enzui_services::process_transport::{self, ProcessRequest, StdinPolicy};

const SERVICE: &str = "calendar-reminder-replan.service";
pub(super) const TIMER: &str = "calendar-reminder-replan.timer";
const ARGUMENT: &str = "--replan-reminders";
const MARKER: &str = "# Managed by Calendar.\n";

pub(super) fn enable(directory: &Path) -> io::Result<()> {
    use std::fmt::Write as _;

    let executable = std::env::current_exe()?;
    let mut quoted = String::new();
    for byte in executable.as_os_str().as_encoded_bytes() {
        match byte {
            b'%' => quoted.push_str("%%"),
            b'/' | b'.' | b'-' | b'_' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' => {
                quoted.push(char::from(*byte));
            }
            _ => {
                let _ = write!(quoted, "\\x{byte:02x}");
            }
        }
    }
    fs::create_dir_all(directory)?;
    let service_changed = write_unit(
        &directory.join(SERVICE),
        &format!("{MARKER}[Service]\nType=oneshot\nExecStart=:\"{quoted}\" {ARGUMENT}\n"),
    )?;
    let timer_changed = write_unit(
        &directory.join(TIMER),
        &format!(
            "{MARKER}[Timer]\nOnTimezoneChange=yes\nOnStartupSec=5s\nAccuracySec=1s\n\n[Install]\nWantedBy=timers.target\n"
        ),
    )?;
    if !service_changed
        && !timer_changed
        && directory.join("timers.target.wants").join(TIMER).exists()
    {
        return Ok(());
    }
    let request = ProcessRequest::new(
        "/usr/bin/systemctl",
        StdinPolicy::Null,
        4096,
        4096,
        Duration::from_secs(5),
    )
    .args(["--user", "enable", "--now", "--no-block", TIMER])
    .envs(
        ["XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"]
            .into_iter()
            .filter_map(|key| {
                std::env::var_os(key).map(|value| (std::ffi::OsString::from(key), value))
            }),
    );
    let output = process_transport::run(request, &CancelToken::new()).map_err(io::Error::other)?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "could not enable {TIMER}: {}",
            String::from_utf8_lossy(&output.stderr).trim(),
        )));
    }
    Ok(())
}

fn write_unit(path: &Path, contents: &str) -> io::Result<bool> {
    match fs::read_to_string(path) {
        Ok(existing) if existing == contents => return Ok(false),
        Ok(existing) if existing.starts_with(MARKER) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Ok(_) => {
            return Err(io::Error::other(
                "Calendar reminder unit name is already in use",
            ));
        }
        Err(error) => return Err(error),
    }
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().unwrap_or_else(|| Path::new(".")))?;
    temporary.write_all(contents.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(true)
}

pub(super) fn lock(scheduler: &Scheduler) -> io::Result<StateLock> {
    StateLock::acquire(&scheduler.state_path().with_file_name("calendar-replan"))
}

pub(crate) fn run_if_requested() -> Option<ExitCode> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(ARGUMENT)) || args.next().is_some() {
        return None;
    }
    let (today, now) = crate::sysclock::local_now();
    Some(match replan(today, now) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!(
                "calendar-app: time-zone reminder replan: {}",
                error.diagnostic()
            );
            ExitCode::FAILURE
        }
    })
}

pub(super) fn replan(today: Date, now: i32) -> Result<(), Failure> {
    let mut scheduler = super::native_scheduler()?;
    // Serialize loading the persisted snapshot with the GUI's post-save scheduling.
    let _lock =
        lock(&scheduler).map_err(|error| Failure::new(FailureKind::Schedule, error.to_string()))?;
    let resolver = enzui_core::app_data::AppDataResolver::from_env();
    let path = persistence::resolve_state_location(&resolver)
        .map_err(|error| Failure::new(FailureKind::Schedule, format!("{error:?}")))?;
    let (state, _, origin) = persistence::boot_at(&path, today, now, persistence::FirstRun::Empty);
    if matches!(origin, persistence::Origin::Restored) {
        // A new timer generation may have no persistent stamp for an already missed deadline.
        let earliest_wakeup = SystemTime::now() + Duration::from_secs(2);
        super::sync_scheduler_locked(
            &mut scheduler,
            super::desired(&state)?,
            Some(earliest_wakeup),
        )?;
    }
    Ok(())
}
