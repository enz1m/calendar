//! Import/export tasks, schedule events, and proposals.

use super::*;

impl App {
    pub(crate) fn finish_import(
        &mut self,
        outcome: enzui_widgets::file_chooser::PortalOutcome<
            enzui_widgets::file_chooser::FileResult,
        >,
    ) -> Task<Message> {
        use enzui_widgets::file_chooser::PortalOutcome;
        match &outcome {
            PortalOutcome::Completed(result) => {
                let Some(path) = result.first_local_path() else {
                    self.state.publish_toast(
                        "That is not a local file — nothing to import.",
                        ToastTone::Warning,
                    );
                    return Task::none();
                };
                self.begin_bounded_import(path)
            }
            PortalOutcome::Cancelled(_) => Task::none(),
            other => {
                if let Some(text) = enzui_widgets::file_chooser::failure_text(other) {
                    self.state.publish_toast(text, ToastTone::Error);
                }
                Task::none()
            }
        }
    }

    pub(crate) fn begin_bounded_import(&mut self, path: std::path::PathBuf) -> Task<Message> {
        let Some(generation) = next_import_generation() else {
            self.state.publish_toast(
                "Can't import: this calendar has run out of import slots.",
                ToastTone::Error,
            );
            return Task::none();
        };
        self.pending_import_drop = None;
        self.pending_import_generation = generation;
        self.state.overlay = Overlay::Importing { generation };
        let blocking = self.boot_worker.submit(generation, move |context| {
            if context.is_cancelled() {
                None
            } else {
                Some(crate::ics_import::run_import(path))
            }
        });
        Task::future(async move {
            let settlement = blocking.await;
            let outcome = match settlement.into_result() {
                Ok(Some(outcome)) => outcome,
                Ok(None) => crate::ics_import::ImportOutcome::WorkerRefused(
                    crate::ics_import::ImportWorkerRefusal::Cancelled,
                ),
                Err(error) => {
                    crate::ics_import::ImportOutcome::WorkerRefused(import_refusal(error))
                }
            };
            Message::ImportFinished {
                generation,
                outcome,
            }
        })
    }

    // Both the overlay and generation must still own the boot result.
    pub(crate) fn apply_boot_finished(
        &mut self,
        generation: u64,
        outcome: Result<crate::boot::BootOutcome, enzui_core::blocking_task::BlockingTaskError>,
    ) -> Task<Message> {
        let matching = matches!(
            &self.state.overlay,
            Overlay::Booting { generation: g } if *g == generation
        );
        if !matching || self.pending_boot_generation != generation {
            return Task::none();
        }
        self.pending_boot_generation = self.pending_boot_generation.wrapping_add(1);
        let boot_outcome = match outcome {
            Ok(boot_outcome) => boot_outcome,
            Err(error) => {
                self.state.overlay = Overlay::None;
                self.state.persist_notice = Some(state::PersistNotice::BootUnavailable(
                    boot_worker_refusal_text(error),
                ));
                return Task::none();
            }
        };
        let live_today = self.state.today;
        let live_now_minute = self.state.now_minute;
        let live_window_width = self.state.window_width;
        let live_window_height = self.state.window_height;
        let crate::boot::BootOutcome {
            mut state,
            persist,
            origin,
            reminder_activation,
            system_hour_cycle,
        } = boot_outcome;
        state.today = live_today;
        state.now_minute = live_now_minute;
        state.window_width = live_window_width;
        state.window_height = live_window_height;
        self.state = state;
        self.persist = persist;
        let (claimed, invocation_seen, activation_failure) =
            self.notifier.install_durable(reminder_activation);
        if let Some(hour_cycle) = system_hour_cycle {
            self.state.system_hour_cycle = hour_cycle;
        }
        let uids_backfilled = self.state.backfill_event_uids();
        self.state.persist_notice = persist_notice_from_origin(&origin);
        apply_startup_overrides(&mut self.state);
        if let Some(failure) = activation_failure {
            self.disclose_reminder_activation_failure(&failure);
        }
        let claim = claimed.map(|key| reminders::apply_claimed(&mut self.state, key));
        let claimed_applied = matches!(claim, Some(reminders::Claim::Applied));
        if matches!(claim, Some(reminders::Claim::Stale)) {
            self.disclose_reminder_activation_failure(
                &crate::reminder_activation::stale_claim_failure(),
            );
        }
        let reminder_changed = if invocation_seen {
            claimed_applied
        } else {
            reminders::check_alerts(&mut self.state) || claimed_applied
        };
        if (uids_backfilled || reminder_changed)
            && let Some(task) = self.schedule_persistence()
        {
            return task;
        }
        self.dispatch_reminder_activation()
    }

    pub(crate) fn apply_import_finished(
        &mut self,
        generation: u64,
        outcome: crate::ics_import::ImportOutcome,
    ) {
        let matching = matches!(
            &self.state.overlay,
            Overlay::Importing { generation: g } if *g == generation
        );
        if !matching || self.pending_import_generation != generation {
            return;
        }
        self.pending_import_generation = 0;
        match outcome {
            crate::ics_import::ImportOutcome::Admitted(admitted) => {
                self.state.overlay =
                    Overlay::ImportDestination(Box::new(crate::state::PendingImport {
                        parsed: admitted.parsed,
                    }));
            }
            crate::ics_import::ImportOutcome::Refused(refusal) => {
                self.state.overlay = Overlay::None;
                self.state
                    .publish_toast(refusal.user_summary(), ToastTone::Error);
            }
            crate::ics_import::ImportOutcome::ParseError(msg) => {
                self.state.overlay = Overlay::None;
                self.state.publish_toast(
                    crate::ics::bounded_feedback("Import failed: ", &msg, ""),
                    ToastTone::Error,
                );
            }
            crate::ics_import::ImportOutcome::WorkerRefused(refusal) => {
                eprintln!(
                    "calendar import worker refusal: category={} retryable={}",
                    refusal.category(),
                    refusal.is_retryable(),
                );
                self.state.overlay = Overlay::None;
                self.state
                    .publish_toast(refusal.user_summary(), ToastTone::Error);
            }
        }
    }

    pub(crate) fn finish_export(
        &mut self,
        outcome: enzui_widgets::file_chooser::PortalOutcome<
            enzui_widgets::file_chooser::FileResult,
        >,
    ) -> Task<Message> {
        use enzui_widgets::file_chooser::PortalOutcome;
        match &outcome {
            PortalOutcome::Completed(result) => {
                let Some(path) = result.first_local_path() else {
                    self.pending_export = None;
                    self.state.publish_toast(
                        "That is not a local file — nothing was exported.",
                        ToastTone::Warning,
                    );
                    return Task::none();
                };
                let Some(mut pending) = self.pending_export.take() else {
                    return Task::none();
                };
                pending.destination = Some(path);
                self.submit_export(pending)
            }
            PortalOutcome::Cancelled(_) => {
                self.pending_export = None;
                Task::none()
            }
            other => {
                self.pending_export = None;
                if let Some(text) = enzui_widgets::file_chooser::failure_text(other) {
                    self.state.publish_toast(text, ToastTone::Error);
                }
                Task::none()
            }
        }
    }

    pub(crate) fn submit_export(&mut self, mut pending: PendingExport) -> Task<Message> {
        let Some(path) = pending.destination.clone() else {
            self.pending_export = None;
            return Task::none();
        };
        let Some(generation) = next_export_generation() else {
            pending.phase = ExportPhase::Exhausted;
            let announce = !pending.announced;
            pending.announced = true;
            self.pending_export = Some(pending);
            if announce {
                self.state.publish_toast(
                    "Can't export: this calendar has run out of export slots.",
                    ToastTone::Error,
                );
            }
            return Task::none();
        };
        pending.generation = generation;
        pending.phase = ExportPhase::InFlight;
        let events = pending.events.clone();
        let stamp = pending.stamp;
        self.pending_export = Some(pending);
        let blocking = self.persist_worker.submit(generation, move |_context| {
            run_export(&events, stamp, &path)
        });
        Task::future(async move {
            match blocking.await.into_result() {
                Ok((result, disclosure)) => Message::ExportSettled {
                    generation,
                    result,
                    disclosure,
                },
                Err(error) => Message::ExportSettled {
                    generation,
                    result: export_refusal_result(error),
                    disclosure: crate::ics_export::ExportDisclosure::none(),
                },
            }
        })
    }

    pub(crate) fn apply_export_settled(
        &mut self,
        generation: u64,
        result: crate::ics_export::ExportResult,
        disclosure: crate::ics_export::ExportDisclosure,
    ) -> Task<Message> {
        use crate::ics_export::ExportResult;
        if self.pending_export.as_ref().map(|p| p.generation) != Some(generation) {
            return Task::none();
        }
        match result {
            ExportResult::Busy => {
                let announce = match self.pending_export.as_mut() {
                    Some(pending) => {
                        pending.phase = ExportPhase::Waiting;
                        let first = !pending.announced;
                        pending.announced = true;
                        first
                    }
                    None => false,
                };
                if announce {
                    self.state.publish_toast(
                        "The calendar is busy — waiting to export…",
                        ToastTone::Warning,
                    );
                }
                Task::none()
            }
            ExportResult::Wrote => {
                self.pending_export = None;
                let mut notes: Vec<String> = Vec::new();
                if !disclosure.skipped.is_empty() {
                    notes.push(crate::l10n::format("skipped {} recurring event(s) anchored to UTC this format can't round-trip (e.g. \"{}\")", &[("arg0", (disclosure.skipped.len() as i64).into()),("arg1", (disclosure.skipped[0].title).as_str().into())]));
                }
                if let Some(text) = export_omission_summary(&disclosure.omissions) {
                    notes.push(text);
                }
                if notes.is_empty() {
                    self.state.publish_toast(
                        crate::l10n::format(
                            "Exported to {}",
                            &[("arg0", (disclosure.path_display).as_str().into())],
                        ),
                        ToastTone::Success,
                    );
                } else {
                    self.state.publish_toast(
                        crate::l10n::format(
                            "Exported to {} — {}.",
                            &[
                                ("arg0", (disclosure.path_display).as_str().into()),
                                ("arg1", (notes.join("; ")).as_str().into()),
                            ],
                        ),
                        ToastTone::Warning,
                    );
                }
                Task::none()
            }
            ExportResult::NothingWritten => {
                self.pending_export = None;
                self.state.publish_toast(
                    crate::l10n::format("Nothing was exported — {} recurring event(s) anchored to UTC can't round-trip in this format yet.", &[("arg0", (disclosure.skipped.len() as i64).into())]),
                    ToastTone::Warning,
                );
                Task::none()
            }
            ExportResult::WriteFailed(kind) => {
                self.pending_export = None;
                self.state
                    .publish_toast(export_write_failure_text(kind), ToastTone::Error);
                Task::none()
            }
            ExportResult::OwnerUnavailable => {
                self.pending_export = None;
                self.state.publish_toast(
                    "The calendar could not be exported right now.",
                    ToastTone::Error,
                );
                Task::none()
            }
        }
    }

    // Immediate retries on Busy would spin; retry only on a later completion or wake.
    pub(crate) fn retry_waiting_export(&mut self) -> Task<Message> {
        let ready = self
            .pending_export
            .as_ref()
            .is_some_and(|p| matches!(p.phase, ExportPhase::Waiting) && p.destination.is_some());
        if !ready {
            return Task::none();
        }
        let pending = self.pending_export.take().expect("waiting export present");
        self.submit_export(pending)
    }

    pub(crate) fn has_pending_export(&self) -> bool {
        self.pending_export
            .as_ref()
            .is_some_and(|p| matches!(p.phase, ExportPhase::InFlight | ExportPhase::Waiting))
    }

    // Retire the chooser before close can stop admission, preventing late export authorization.
    pub(crate) fn cancel_unauthorized_export(&mut self) {
        if matches!(
            self.pending_export.as_ref().map(|p| p.phase),
            Some(ExportPhase::AwaitingDestination)
        ) {
            self.picker.cancel_pending();
            self.pending_export = None;
        }
    }

    pub(crate) fn on_schedule(&mut self, ev: SchedEvent) -> Task<Message> {
        // Column indices refer to the rendered rolling page, which can differ from the full week.
        let dates = crate::view::schedule::visible_dates(&self.state);

        let fallback_anchor = || iced::Point::new(self.state.window_width * 0.5, 300.0);
        match &ev {
            SchedEvent::BlockActivated { id, at } | SchedEvent::AllDayActivated { id, at } => {
                let Some((model_id, on)) = self.state.schedule_occurrence(id.get()) else {
                    return Task::none();
                };
                return self.update(Message::OpenDetails {
                    id: model_id,
                    on: Some(on),
                    at: *at,
                });
            }
            SchedEvent::SlotActivated { column, minute, at } => {
                if let Some(date) = dates.get(*column).copied() {
                    let anchor = at.unwrap_or_else(fallback_anchor);
                    return self.update(Message::OpenQuickAdd {
                        date,
                        minute: *minute,
                        at: anchor,
                    });
                }
                return Task::none();
            }
            _ => {}
        }

        let s = &mut self.state;
        match ev {
            SchedEvent::BlockActivated { .. }
            | SchedEvent::AllDayActivated { .. }
            | SchedEvent::SlotActivated { .. } => {}
            SchedEvent::HeaderActivated(column) => {
                if let Some(date) = dates.get(column).copied() {
                    s.focus = date;
                    s.focus_mini_day(date);
                    s.menu.close();
                }
            }
            SchedEvent::Proposal(p) => self.apply_proposal(p, &dates),
            // Drag previews do not mutate stored events so Escape can cancel without rollback.
            SchedEvent::Previewing(_) | SchedEvent::Scrolled(_) => {}
            SchedEvent::ContextRequested { hit, at } => {
                use enzui_widgets::schedule::ScheduleHit;
                let target = match hit {
                    ScheduleHit::Block { id }
                    | ScheduleHit::BlockResizeEdge { id }
                    | ScheduleHit::AllDayBlock { id } => {
                        s.schedule_occurrence(id.get())
                            .map(|(model_id, on)| MenuTarget::Event {
                                id: model_id,
                                on: Some(on),
                            })
                    }
                    ScheduleHit::Slot { column, minute } => dates
                        .get(column)
                        .map(|d| MenuTarget::Slot { date: *d, minute }),
                    ScheduleHit::AllDaySlot { column } | ScheduleHit::Header { column } => {
                        dates.get(column).map(|d| MenuTarget::Day(*d))
                    }
                    ScheduleHit::None => None,
                };
                if let Some(target) = target {
                    open_menu(s, target, at);
                } else {
                    s.menu.close();
                }
            }
        }
        Task::none()
    }

    pub(crate) fn apply_proposal(
        &mut self,
        p: enzui_core::schedule::ScheduleProposal,
        dates: &[Date],
    ) {
        use enzui_core::schedule::ScheduleProposal as P;
        let s = &mut self.state;
        match p {
            P::Move {
                id,
                new_start_minute,
                new_end_minute,
                column_offset,
            } => {
                let Some((model_id, occurrence)) = s.schedule_occurrence(id.get()) else {
                    return;
                };
                if s.event(model_id).is_some_and(|e| {
                    e.recurrence != model::Recurrence::None && e.occurs_on(occurrence)
                }) {
                    stage_recurring_drag_edit(
                        s,
                        model_id,
                        occurrence,
                        column_offset,
                        new_start_minute,
                        new_end_minute,
                        true,
                    );
                } else if let Some(e) = s.event_mut(model_id) {
                    let before = wallclock_endpoints(e);
                    translate_move(e, column_offset, new_start_minute);
                    retire_anchor_on_wallclock_change(e, before);
                    s.selected = Some(model_id);
                }
            }
            P::Resize {
                id,
                start_minute,
                new_end_minute,
            } => {
                let Some((model_id, occurrence)) = s.schedule_occurrence(id.get()) else {
                    return;
                };
                if s.event(model_id).is_some_and(|e| {
                    e.recurrence != model::Recurrence::None && e.occurs_on(occurrence)
                }) {
                    stage_recurring_drag_edit(
                        s,
                        model_id,
                        occurrence,
                        0,
                        start_minute,
                        new_end_minute,
                        false,
                    );
                } else if let Some(e) = s.event_mut(model_id) {
                    let before = wallclock_endpoints(e);
                    e.start_minute = MinuteOfDay::new(start_minute);
                    e.end_minute = MinuteOfDay::new(new_end_minute);
                    retire_anchor_on_wallclock_change(e, before);
                    s.selected = Some(model_id);
                }
            }
            P::Create {
                start_minute,
                end_minute,
                column,
            } => {
                if let Some(date) = dates.get(column).copied() {
                    if let Some(mut draft) = s.try_draft_at(date, start_minute) {
                        draft.end_minute = MinuteOfDay::new(end_minute);
                        s.set_overlay(Overlay::Editor(Box::new(draft)));
                    } else {
                        s.refuse_id_exhaustion();
                    }
                }
            }
        }
    }
}
