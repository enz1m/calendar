//! Construction, view, subscription, and the update shell.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootOrder {
    Chained,
    #[cfg(feature = "center")]
    Concurrent,
}

impl App {
    pub(crate) fn new() -> (Self, Task<Message>) {
        Self::new_boot(BootOrder::Chained)
    }

    #[cfg(feature = "center")]
    pub(crate) fn new_windowless() -> (Self, Task<Message>) {
        Self::new_boot(BootOrder::Concurrent)
    }

    fn new_boot(order: BootOrder) -> (Self, Task<Message>) {
        // An unusable data root disables persistence; it must never fall back to shared temporary
        // storage.
        let resolver = enzui_core::app_data::AppDataResolver::from_env();
        let (mut app, task) = match persistence::resolve_state_location(&resolver) {
            Ok(path) => Self::new_at_ordered(&path, order),
            Err(refusal) => Self::new_root_unavailable(refusal),
        };
        app.notifier.enable();
        #[cfg(feature = "center")]
        app.request_center();
        (app, task)
    }

    pub(crate) fn new_root_unavailable(refusal: persistence::RootRefusal) -> (Self, Task<Message>) {
        let (today, now) = today_and_now();
        let state = AppState::empty(today, now);
        let baseline = persistence::snapshot(&state);
        let placeholder =
            std::path::PathBuf::from(persistence::APP_ID).join(persistence::STATE_FILE_NAME);
        Self::start((
            state,
            persistence::Persist::disabled(placeholder, baseline),
            persistence::Origin::PersistenceRootUnavailable(refusal),
        ))
    }

    pub(crate) fn new_at_ordered(
        state_file: &std::path::Path,
        order: BootOrder,
    ) -> (Self, Task<Message>) {
        let (today, now) = today_and_now();
        Self::start_async(
            state_file.to_path_buf(),
            today,
            now,
            first_run_policy(),
            order,
        )
    }

    // Only the matching boot generation may replace the loading state; cancellation retains worker
    // occupancy.
    pub(crate) fn start_async(
        state_file: std::path::PathBuf,
        today: enzui_core::civil_date::Date,
        now_minute: i32,
        first_run: persistence::FirstRun,
        order: BootOrder,
    ) -> (Self, Task<Message>) {
        let generation: u64 = 1;
        let pre_boot_state = AppState::pre_boot(today, now_minute, generation);
        let placeholder_persist = persistence::Persist::disabled(
            state_file.clone(),
            persistence::snapshot(&pre_boot_state),
        );
        let (mut app, boot_setup_task) = Self::start((
            pre_boot_state,
            placeholder_persist,
            persistence::Origin::FreshStart,
        ));
        app.pending_boot_generation = generation;
        let inputs = crate::boot::BootInputs {
            path: state_file,
            today,
            now_minute,
            first_run,
        };
        let blocking = app.boot_worker.submit(generation, move |context| {
            if context.is_cancelled() {
                None
            } else {
                #[cfg(not(test))]
                {
                    Some(crate::boot::run_production_boot(
                        inputs,
                        std::env::args().skip(1),
                    ))
                }
                #[cfg(test)]
                {
                    Some(crate::boot::run_boot(inputs))
                }
            }
        });
        let boot_task = Task::future(async move {
            let settlement = blocking.await;
            let outcome = match settlement.into_result() {
                Ok(Some(boot_outcome)) => Ok(boot_outcome),
                Ok(None) => Err(enzui_core::blocking_task::BlockingTaskError::Cancelled),
                Err(error) => Err(error),
            };
            Message::BootFinished {
                generation,
                outcome: crate::boot::BootDelivery::new(outcome),
            }
        });
        let boot = match order {
            BootOrder::Chained => boot_setup_task.chain(boot_task),
            #[cfg(feature = "center")]
            BootOrder::Concurrent => Task::batch([boot_setup_task, boot_task]),
        };
        (app, boot)
    }

    #[cfg(test)]
    pub(crate) fn test_new() -> (Self, Task<Message>) {
        let (today, now) = today_and_now();
        Self::start((
            AppState::new(today, now),
            persistence::Persist::inert(),
            persistence::Origin::FreshStart,
        ))
    }

    pub(crate) fn start(
        booted: (AppState, persistence::Persist, persistence::Origin),
    ) -> (Self, Task<Message>) {
        Self::start_with_queue(booted, persistence::PersistenceQueue::default())
    }

    pub(crate) fn start_with_queue(
        booted: (AppState, persistence::Persist, persistence::Origin),
        persistence_queue: persistence::PersistenceQueue,
    ) -> (Self, Task<Message>) {
        Self::start_with_inputs(booted, persistence_queue, system_hour_cycle)
    }

    pub(crate) fn start_with_inputs(
        booted: (AppState, persistence::Persist, persistence::Origin),
        persistence_queue: persistence::PersistenceQueue,
        resolve_system_hour_cycle: impl FnOnce() -> HourCycle,
    ) -> (Self, Task<Message>) {
        let (mut state, mut persist, origin) = booted;
        let pre_boot = matches!(state.overlay, Overlay::Booting { .. });
        let (themes, theme_boot) = ThemeIntegration::boot_for(
            enzui_core::app_data::AppId::new(crate::package::APP_ID)
                .expect("Calendar application ID"),
        );
        state.sizing = themes.theme().sizing;
        let uids_backfilled = if pre_boot {
            false
        } else {
            state.system_hour_cycle = resolve_system_hour_cycle();
            let backfilled = state.backfill_event_uids();
            state.persist_notice = persist_notice_from_origin(&origin);
            apply_startup_overrides(&mut state);
            backfilled
        };
        // A fired reminder mark must be persisted even if the alert remains open until exit.
        let reminders_fired = !pre_boot && reminders::check_alerts(&mut state);
        let pending_save = (reminders_fired || uids_backfilled)
            .then(|| persist.prepare(&state, &persistence_queue))
            .flatten();
        let mut app = Self {
            state,
            themes,
            shell_window: ShellWindowState::new(),
            nav: NavSplitState::new(),
            picker: picker::Picker::new(),
            instance: None,
            pending_open: None,
            persist,
            persistence_queue,
            pending_save,
            pending_export: None,
            click_detector: enzui_core::click::ClickDetector::new(
                enzui_core::click::SingleClickPolicy::Immediate,
            ),
            notifier: notify::Notifier::new(),
            pending_import_generation: 0,
            pending_import_drop: None,
            import_drop_epoch: 0,
            pending_boot_generation: 0,
            #[cfg(feature = "center")]
            center: None,
            #[cfg(feature = "center")]
            center_requested: false,
            #[cfg(feature = "center")]
            agent: enzui_agent_app::AgentProcess::default(),
            #[cfg(feature = "center")]
            search_index: crate::search::SearchIndex::default(),
            boot_worker: default_boot_worker(),
            persist_worker: default_persist_worker(),
        };
        if let Some(pending) = app.pending_save.clone() {
            let generation = pending.ticket().generation();
            let outcome = pending.complete(&app.persistence_queue);
            app.apply_persistence_completion(generation, outcome);
        }
        let focus_boot = app.sync_overlay_focus();
        (
            app,
            Task::batch([
                theme_boot.map(|event| Message::Theme(Box::new(event))),
                focus_boot,
            ]),
        )
    }

    pub(crate) fn theme(&self) -> iced::Theme {
        self.themes.iced_theme()
    }

    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let events =
            iced::event::listen_with(|event, status, _window| classify_event(&event, status));

        let minute = minute_tick();

        let zone = zone_tick();

        let portal = self
            .picker
            .completion_subscription()
            .map(|_| Message::PortalTick);
        let toast = match self.state.toast_expiry.armed() {
            Some((deadline, wait)) => iced::time::every(wait)
                .with(deadline)
                .map(|(_deadline, now)| Message::Toast(ToastEvent::Tick(now))),
            None => Subscription::none(),
        };
        let notifications = self
            .notifier
            .wake_subscription()
            .map(|_| Message::NotificationWake);
        let close_drain = if self.state.close_state.is_draining() {
            iced::time::every(CLOSE_DRAIN_TICK_INTERVAL).map(|_| Message::CloseDrainTick)
        } else {
            Subscription::none()
        };
        // Compositor close requests need the exact window identity so pending saves can veto
        // teardown.
        let close_requests = iced::window::close_requests().map(Message::CloseWindow);
        let close_events = iced::window::close_events().map(Message::WindowClosed);
        let subscriptions = Subscription::batch([
            events,
            self.shell_window.subscription().map(Message::ShellWindow),
            minute,
            zone,
            portal,
            toast,
            self.themes
                .subscription()
                .map(|event| Message::Theme(Box::new(event))),
            notifications,
            close_drain,
            close_requests,
            close_events,
            self.instance
                .as_ref()
                .map_or_else(Subscription::none, |instance| {
                    instance.subscription(|_| Message::InstanceWake)
                }),
        ]);
        #[cfg(feature = "center")]
        let subscriptions = Subscription::batch([
            subscriptions,
            self.center_subscription(),
            self.agent_subscription(),
        ]);
        subscriptions
    }

    pub(crate) fn view(&self) -> Element<'_, Message> {
        static READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if std::env::var("ENZUI_CAPTURE_READY").as_deref() == Ok("1")
            && !matches!(self.state.overlay, Overlay::Booting { .. })
            && !READY.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            eprintln!("[CAPTURE_READY] calendar-app");
        }

        self.picker.view(view::view(
            &self.state,
            self.nav,
            self.themes.theme(),
            self.import_drop_epoch,
            &self.shell_window,
        ))
    }

    pub(crate) fn retire_offscreen_date_popovers(&mut self) {
        match &self.state.overlay {
            Overlay::Editor(e) => {
                if e.recurrence_until.is_none() {
                    let _ = self.state.editor_pickers.until.close();
                }
            }
            _ => self.state.editor_pickers.close_all(),
        }
    }

    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        // Crash breadcrumbs contain message kinds only, before any early return.
        enzui_core::crash_report::observe(&message);
        let occurrence_ids = std::sync::Arc::clone(&self.state.occurrence_ids);
        let previous_import_context = (
            std::mem::discriminant(&self.state.overlay),
            self.state.close_state.is_idle(),
            self.picker.generation(),
        );
        let previous_notice = (
            self.state.persist_notice.clone(),
            self.state.close_state.is_vetoed(),
        );
        let task = self.update_inner(message);
        // A hidden storage notice returns when its state changes or a close is refused.
        if previous_notice.0 != self.state.persist_notice
            || (!previous_notice.1 && self.state.close_state.is_vetoed())
        {
            self.state.persist_notice_hidden = false;
        }
        let task = Task::batch([
            task,
            self.open_deferred_launch(),
            self.sync_calendar_dialogs(),
        ]);
        if previous_import_context
            != (
                std::mem::discriminant(&self.state.overlay),
                self.state.close_state.is_idle(),
                self.picker.generation(),
            )
        {
            self.import_drop_epoch = self.import_drop_epoch.wrapping_add(1);
        }
        if !matches!(
            self.state.overlay,
            Overlay::Importing { .. } | Overlay::ImportDestination(_)
        ) {
            self.pending_import_drop = None;
        }
        self.state.occurrence_ids = occurrence_ids;
        self.state.sizing = self.themes.theme().sizing;
        let _ = self
            .state
            .toasts
            .sync_expiry(&mut self.state.toast_expiry, std::time::Instant::now());
        #[cfg(feature = "center")]
        self.search_index.refresh(&self.state);
        task
    }

    pub(crate) fn update_inner(&mut self, message: Message) -> Task<Message> {
        let message = match message {
            Message::ShellWindow(event) => {
                if let enzui_widgets::app_shell::ShellWindowEvent::Closed(window) = &event {
                    self.picker.parent_closed(*window);
                }
                return self.shell_window.update(event).map(Message::ShellWindow);
            }
            Message::Window(WindowCommand::Close) => {
                return self
                    .shell_window
                    .window_id()
                    .map_or_else(Task::none, |id| Task::done(Message::CloseWindow(id)));
            }
            Message::Window(command) => return self.shell_window.run_command(command),
            other => other,
        };
        if matches!(self.state.overlay, Overlay::Booting { .. }) && !boot_accepts(&message) {
            return Task::none();
        }
        // Vetoing close retains save and reminder debt on their owners for an explicit retry.
        if !matches!(
            message,
            Message::CloseWindow(_)
                | Message::WindowClosed(_)
                | Message::RetryPersistence
                | Message::PersistenceFinished { .. }
                | Message::ReminderActivationFinished(..)
                | Message::CloseDrainTick
                | Message::ExportSettled { .. }
                | Message::ImportFinished { .. }
                | Message::ChooserParent(..)
                | Message::ChooserFinished(..)
                | Message::ChooserEvent(..)
                | Message::PortalTick
                // Clock and worker completions cannot cancel close; newly created save debt joins
                // its drain.
                | Message::MinuteTick(..)
                | Message::ZoneObserved(..)
        ) && !self.state.close_state.is_idle()
        {
            self.state.close_state = CloseState::Idle;
        }

        if let Message::CloseWindow(window) = message {
            if matches!(self.state.overlay, Overlay::Booting { .. }) {
                self.pending_boot_generation = self.pending_boot_generation.wrapping_add(1);
            }
            return self.begin_close_drain(window);
        }
        if let Message::WindowClosed(window) = message {
            return self.observe_window_closed(window);
        }
        let message = match message {
            Message::PersistenceFinished {
                generation,
                outcome,
            } => {
                self.observe_persistence_for_close(generation, &outcome);
                let saved = self.apply_persistence_completion(generation, outcome);
                // Retry Busy exports only after a later completion or wake, never in the same turn.
                let retry = self.retry_waiting_export();
                if self.state.close_state.is_draining() {
                    // Reminder scheduling follows confirmed persistence of the timezone projection.
                    let zone = if saved {
                        self.advance_pending_zone_after_close_save()
                    } else {
                        Task::none()
                    };
                    let close = self.try_finish_close().unwrap_or_else(Task::none);
                    return Task::batch([retry, zone, close]);
                }
                let follow = if saved {
                    self.dispatch_reminder_activation()
                } else {
                    Task::none()
                };
                return Task::batch([retry, follow]);
            }
            Message::ReminderActivationFinished(completion) => {
                let (generation, success) = self.apply_reminder_activation_completion(completion);
                self.observe_reminder_for_close(generation, success);
                // A timezone proposal is adopted only after its matching reminder scheduling
                // succeeds.
                self.settle_pending_zone_after_reminder(generation, success);
                return if self.state.close_state.is_draining() {
                    match self.try_finish_close() {
                        Some(close) => close,
                        None => Task::none(),
                    }
                } else {
                    Task::none()
                };
            }
            Message::RetryPersistence => return self.retry_persistence(),
            Message::ZoneObserved(fingerprint) => return self.observe_zone(fingerprint),
            Message::CloseDrainTick => {
                self.check_close_deadline();
                let retry = self.retry_waiting_export();
                let close = self.try_finish_close().unwrap_or_else(Task::none);
                return Task::batch([retry, close]);
            }
            other => other,
        };
        let task = self.reduce(message);
        // Logical modal ownership ends before retained Settings exit paint.
        let focus_task = self.sync_overlay_focus();
        self.state.sync_settings_presentation();
        let task = Task::batch([task, focus_task]);
        match self.schedule_persistence() {
            Some(persistence) if self.state.close_state.is_draining() => {
                let close = self.fold_background_save_into_drain();
                Task::batch([persistence, task, close])
            }
            Some(persistence) => Task::batch([persistence, task]),
            None => task,
        }
    }

    pub(crate) fn fold_background_save_into_drain(&mut self) -> Task<Message> {
        let CloseState::Draining {
            window,
            deadline,
            reminder_debt,
            ..
        } = self.state.close_state.clone()
        else {
            return Task::none();
        };
        let Some(save_gen) = self.pending_save.as_ref().map(|p| p.ticket().generation()) else {
            return Task::none();
        };
        self.state.close_state = CloseState::Draining {
            window,
            deadline,
            save_debt: Some(save_gen),
            reminder_debt,
        };
        self.try_finish_close().unwrap_or_else(Task::none)
    }

    pub(crate) fn observe_window_closed(&mut self, window: iced::window::Id) -> Task<Message> {
        self.picker.parent_closed(window);
        if self.state.close_state.window() != Some(window) {
            return Task::none();
        }
        self.state.close_state = CloseState::Idle;
        if let Some(state::PersistNotice::PersistenceFailed(text)) = &self.state.persist_notice
            && text.as_str() == persistence::CLOSE_DEADLINE_SUMMARY
        {
            self.state.persist_notice = None;
        }
        Task::none()
    }

    pub(crate) fn retire_unreadable_state(&mut self) {
        let was_future = match &self.state.persist_notice {
            Some(state::PersistNotice::ReadOnlyFuture(_)) => true,
            Some(state::PersistNotice::Quarantined(None)) => false,
            _ => return,
        };
        let path = self.persist.path().to_path_buf();
        // Retirement must use the admitted identity so a replacement file cannot be moved aside.
        let Some(identity) = self.persist.retire_identity() else {
            self.state.publish_toast(
                crate::l10n::format(
                    "Could not retire {} — admission did not retain its identity.",
                    &[("arg0", path.to_string_lossy().into_owned().into())],
                ),
                ToastTone::Error,
            );
            return;
        };
        match persistence::retire_expected(&path, identity) {
            Some(kept) => {
                if let Some(pending) = self
                    .persist
                    .enable_for(&self.state, &self.persistence_queue)
                {
                    self.pending_save = Some(pending);
                }
                if !self.persist.is_enabled() {
                    self.state.persist_notice = Some(state::PersistNotice::RetiredReadOnly(kept));
                    return;
                }
                self.state.persist_notice = Some(if was_future {
                    state::PersistNotice::StartedFresh(kept)
                } else {
                    state::PersistNotice::Quarantined(Some(kept))
                });
            }
            None => {
                self.state.publish_toast(
                    crate::l10n::format(
                        "Could not move {} aside — this calendar is still read-only.",
                        &[("arg0", path.to_string_lossy().into_owned().into())],
                    ),
                    ToastTone::Error,
                );
            }
        }
    }
}
