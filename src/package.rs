//! One desktop identity and framework-generated installation metadata.
use enzui_core::packaging::AppPackage;
use std::path::Path;
pub(crate) const APP_ID: &str = "io.github.enzui.Calendar";
pub(crate) fn identity() -> AppPackage {
    AppPackage::new(
        APP_ID,
        "Calendar",
        "Manage events, schedules, and reminders",
        "calendar-app",
    )
    .expect("Calendar identity")
    .description([
        "Calendar creates, finds and organizes local events across multiple calendars.",
        "Import and export iCalendar files, edit repeating events and receive reminders.",
    ])
    .categories(["Office", "Calendar"])
    .keywords(["calendar", "event", "schedule", "reminder"])
    .project_license("MIT")
    .single_main_window(true)
    .dbus_activatable(cfg!(feature = "center"))
    .search_provider(cfg!(feature = "center"))
    .translation(
        "de",
        "Kalender",
        "Termine, Zeitpläne und Erinnerungen verwalten",
    )
    .icon_svg(include_str!("../assets/io.github.enzui.Calendar.svg"))
}
pub(crate) fn write(root: &Path, prefix: &Path) -> std::io::Result<()> {
    identity().write(root, prefix)?;
    #[cfg(feature = "center")]
    {
        use enzui_agent::center::installed;
        // The promoted desktop process must not inherit a worker-only read-only-home sandbox.
        let executable = prefix.join("bin/calendar-app");
        let executable = executable.to_string_lossy();
        let sandbox = installed::UnitSandbox {
            network: false,
            writable_home: false,
            own_directories: Some("calendar-app".into()),
            writable: vec![],
            desktop: true,
        };
        let unit = installed::unit(APP_ID, &executable, &sandbox).map_err(std::io::Error::other)?;
        let service =
            installed::dbus_service(APP_ID, &executable).map_err(std::io::Error::other)?;
        let manifest = installed::manifest(&crate::center::registry(), crate::center::metadata())
            .map_err(std::io::Error::other)?;
        let background = installed::background_declaration(
            APP_ID,
            &executable,
            &installed::BackgroundDeclaration {
                title: "Calendar".into(),
                purpose:
                    "Answers approved agent requests about your calendar without opening a window."
                        .into(),
                activation_sources: vec![
                    installed::ActivationSource::User,
                    installed::ActivationSource::Agent,
                ],
                autostart_supported: false,
                graceful_stop: false,
                work: false,
            },
        )
        .map_err(std::io::Error::other)?;
        for (path, contents) in [
            (
                format!("lib/systemd/user/enzui-agent-{APP_ID}.service"),
                unit.into_bytes(),
            ),
            (
                format!("share/dbus-1/services/{APP_ID}.service"),
                service.into_bytes(),
            ),
            (format!("share/enzui/agent-apps/{APP_ID}.json"), manifest),
            (
                format!("share/enzui/background-apps/{APP_ID}.json"),
                background,
            ),
        ] {
            let path = root
                .join(prefix.strip_prefix("/").unwrap_or(prefix))
                .join(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, contents)?;
        }
    }
    Ok(())
}
pub(crate) fn command() -> Option<std::process::ExitCode> {
    let args: Vec<_> = std::env::args_os().collect();
    let checking = args
        .get(1)
        .is_some_and(|arg| arg == "--check-package-files");
    if !checking && args.get(1).is_none_or(|arg| arg != "--write-package-files") {
        return None;
    }
    if args.len() != 4 {
        eprintln!("usage: calendar-app --write-package-files ROOT PREFIX");
        return Some(std::process::ExitCode::from(2));
    }
    if checking {
        let mismatches = identity().check(Path::new(&args[2]), Path::new(&args[3]));
        for mismatch in &mismatches {
            eprintln!("{}: {}", mismatch.path.display(), mismatch.problem);
        }
        return Some(if mismatches.is_empty() {
            std::process::ExitCode::SUCCESS
        } else {
            std::process::ExitCode::FAILURE
        });
    }
    Some(match write(Path::new(&args[2]), Path::new(&args[3])) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Calendar packaging failed: {error}");
            std::process::ExitCode::FAILURE
        }
    })
}
