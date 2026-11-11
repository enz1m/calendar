# Isolated reminder integration

Run from the repository root:

```sh
ICED_TEST_BACKEND=software cargo test --locked --bin calendar-app -- reminder_activation::route_tests --test-threads=1
```

The tests require Linux Bubblewrap user namespaces, Python 3, `dbus-daemon` and
the `America/New_York` timezone database. Missing prerequisites fail the tests.

The fixtures use generated HOME/XDG directories, a private bus without service
activation, read-only host files and private process/network/IPC namespaces.
`systemctl.py` stands in for the service manager inside that namespace; it never
contacts host services or fires timers. `session.py` bounds the private bus and
child lifetime. Generated files are removed on exit.

One scenario follows production boot, generated activation arguments, durable
claim, snooze, replay refusal, dismissal and relaunch through separate processes.
The other checks DST fold/gap and fixed-UTC deadlines in fresh timezone-specific
processes. Neither changes the host clock or waits for a reminder deadline.

The sandbox case verifies that in-app reminders still fire and close cleanly
without calling the host manager or publishing timer/state files. Python is
used only for these isolated fixtures, never for production activation.

These checks do not prove installed-binary argv dispatch, compositor presentation,
live notification delivery, real timer firing, or suspend/reboot recovery.
