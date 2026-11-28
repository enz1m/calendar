//! Reducer-side persistence scheduling and the finite close state machine.

use std::time::Instant;

use enzui_core::blocking_task::BlockingTaskError;
use enzui_core::persist::PersistErrorKind;
use iced::Task;

use crate::message::Message;
use crate::state::{CloseState, CloseVetoReason};
use crate::{App, CLOSE_DRAIN_DEADLINE, persistence, state};

impl App {
    pub(super) fn dispatch_reminder_activation(&mut self) -> Task<Message> {
        let (task, _) = self.dispatch_reminder_activation_tracked();
        task
    }

    pub(super) fn dispatch_reminder_activation_tracked(&mut self) -> (Task<Message>, Option<u64>) {
        if !self.persist.is_enabled() || !self.notifier.can_schedule_durable() {
            return (Task::none(), None);
        }
        let desired = match crate::reminder_activation::desired(&self.state) {
            Ok(desired) => desired,
            Err(failure) => {
                if !self.notifier.note_durable_failure() {
                    self.disclose_reminder_activation_failure(&failure);
                }
                return (Task::none(), None);
            }
        };
        let dispatched = self
            .notifier
            .dispatch_durable(desired, &self.boot_worker)
            .map_or_else(Task::none, |task| {
                task.map(Message::ReminderActivationFinished)
            });
        let generation = self.notifier.pending_durable_generation();
        // Timezone adoption waits for the matching reminder completion, including scheduler
        // retries.
        if let Some(generation) = generation
            && let Some(pending) = self.state.pending_zone.as_mut()
            && pending.save_generation.is_none()
        {
            pending.reminder_generation = Some(generation);
        }
        (dispatched, generation)
    }

    pub(super) fn apply_reminder_activation_completion(
        &mut self,
        completion: crate::reminder_activation::SyncCompletion,
    ) -> (u64, bool) {
        let generation = completion.generation();
        let success = completion.is_success();
        if let Some(failure) = self.notifier.settle_durable(completion) {
            self.disclose_reminder_activation_failure(&failure);
        }
        (generation, success)
    }

    pub(super) fn disclose_reminder_activation_failure(
        &mut self,
        failure: &crate::reminder_activation::Failure,
    ) {
        let diagnostic: String = failure
            .diagnostic()
            .chars()
            .flat_map(char::escape_default)
            .collect();
        eprintln!("calendar reminder activation failed: diagnostic={diagnostic}");
        self.state.publish_toast(
            failure.user_summary(),
            enzui_widgets::toast::ToastTone::Warning,
        );
    }

    pub(super) fn observe_zone(&mut self, fingerprint: u64) -> Task<Message> {
        let would_change = self.state.fixed_utc_projection_would_change();
        let effective = self
            .state
            .pending_zone
            .map_or(self.state.observed_zone, |pending| pending.fingerprint);
        let fingerprint_moved = fingerprint != effective;
        if !would_change && !fingerprint_moved {
            return Task::none();
        }
        let base = self
            .state
            .pending_zone
            .map_or(self.state.zone_generation, |pending| pending.generation);
        let Some(next_generation) = base.checked_add(1) else {
            self.state.persist_notice = Some(state::PersistNotice::PersistenceFailed(
                persistence::ZONE_AUTHORITY_EXHAUSTED_SUMMARY.to_owned(),
            ));
            return Task::none();
        };

        if would_change {
            // Reprojection must be durable before scheduling its reminder job.
            let _ = self.state.reproject_fixed_utc_events();
            let persistence = self.schedule_persistence();
            let save_gen = self.pending_save.as_ref().map(|p| p.ticket().generation());
            self.state.pending_zone = Some(state::PendingZone {
                fingerprint,
                generation: next_generation,
                save_generation: save_gen,
                reminder_generation: None,
            });
            let close = self.fold_zone_save_into_drain(save_gen);
            return match persistence {
                Some(save) => Task::batch([save, close]),
                None => close,
            };
        }

        self.state.pending_zone = Some(state::PendingZone {
            fingerprint,
            generation: next_generation,
            save_generation: None,
            reminder_generation: None,
        });
        let (reminder, reminder_gen) = self.dispatch_reminder_activation_tracked();
        if reminder_gen.is_none() {
            // A projection without scheduler work has no further completion to await.
            self.adopt_pending_zone();
            return reminder;
        }
        let close = self.fold_zone_reminder_into_drain(reminder_gen);
        Task::batch([reminder, close])
    }

    fn adopt_pending_zone(&mut self) {
        if let Some(pending) = self.state.pending_zone.take() {
            self.state.observed_zone = pending.fingerprint;
            self.state.zone_generation = pending.generation;
            if matches!(
                self.state.persist_notice.as_ref(),
                Some(state::PersistNotice::PersistenceFailed(_))
            ) {
                self.state.persist_notice = None;
            }
        }
    }

    pub(super) fn settle_pending_zone_after_reminder(&mut self, generation: u64, success: bool) {
        let Some(pending) = self.state.pending_zone else {
            return;
        };
        if pending.reminder_generation != Some(generation) {
            return;
        }
        if success {
            self.adopt_pending_zone();
        } else {
            self.state.persist_notice = Some(state::PersistNotice::PersistenceFailed(
                persistence::ZONE_SCHEDULER_RETRY_SUMMARY.to_owned(),
            ));
        }
    }

    pub(super) fn advance_pending_zone_after_close_save(&mut self) -> Task<Message> {
        if !self.state.pending_zone.is_some_and(|pending| {
            pending.save_generation.is_none() && pending.reminder_generation.is_none()
        }) {
            return Task::none();
        }
        let (reminder, reminder_gen) = self.dispatch_reminder_activation_tracked();
        let close = self.fold_zone_reminder_into_drain(reminder_gen);
        Task::batch([reminder, close])
    }

    fn fold_zone_save_into_drain(&mut self, save_gen: Option<u64>) -> Task<Message> {
        let CloseState::Draining {
            window,
            deadline,
            reminder_debt,
            ..
        } = self.state.close_state.clone()
        else {
            return Task::none();
        };
        self.state.close_state = CloseState::Draining {
            window,
            deadline,
            save_debt: save_gen,
            reminder_debt,
        };
        self.try_finish_close().unwrap_or_else(Task::none)
    }

    fn fold_zone_reminder_into_drain(&mut self, reminder_gen: Option<u64>) -> Task<Message> {
        let CloseState::Draining {
            window,
            deadline,
            save_debt,
            ..
        } = self.state.close_state.clone()
        else {
            return Task::none();
        };
        self.state.close_state = CloseState::Draining {
            window,
            deadline,
            save_debt,
            reminder_debt: reminder_gen,
        };
        self.try_finish_close().unwrap_or_else(Task::none)
    }

    pub(super) fn schedule_persistence(&mut self) -> Option<Task<Message>> {
        let pending = self.persist.prepare(&self.state, &self.persistence_queue)?;
        Some(self.dispatch_persistence(pending))
    }

    pub(super) fn dispatch_persistence(
        &mut self,
        pending: persistence::PendingSave,
    ) -> Task<Message> {
        let generation = pending.ticket().generation();
        self.pending_save = Some(pending.clone());
        let queue = self.persistence_queue.clone();
        let blocking = self
            .persist_worker
            .submit(generation, move |_context| pending.complete(&queue));
        Task::future(async move {
            let settlement = blocking.await;
            let outcome = match settlement.into_result() {
                Ok(outcome) => outcome,
                Err(error) => {
                    persistence::PersistenceCompletion::WorkerRefused(map_worker_refusal(error))
                }
            };
            Message::PersistenceFinished {
                generation,
                outcome,
            }
        })
    }

    pub(super) fn apply_persistence_completion(
        &mut self,
        generation: u64,
        outcome: persistence::PersistenceCompletion,
    ) -> bool {
        let Some(pending) = self.pending_save.as_ref() else {
            return false;
        };
        if pending.ticket().generation() != generation {
            return false;
        }
        match outcome {
            persistence::PersistenceCompletion::Saved => {
                let pending = self.pending_save.take().expect("matching pending save");
                self.persist.acknowledge_saved(&pending);
                if let Some(proposal) = self.state.pending_zone.as_mut()
                    && proposal.save_generation == Some(generation)
                {
                    proposal.save_generation = None;
                }
                if matches!(
                    self.state.persist_notice.as_ref(),
                    Some(state::PersistNotice::PersistenceFailed(_))
                ) {
                    self.state.persist_notice = None;
                }
                // Publish desktop notifications only after their fired identity is durable.
                self.announce_reminder_notification();
                true
            }
            persistence::PersistenceCompletion::Superseded => false,
            persistence::PersistenceCompletion::PublishedButNotDurable(failure) => {
                let diagnostic: String = failure
                    .diagnostic()
                    .chars()
                    .flat_map(char::escape_default)
                    .collect();
                eprintln!("calendar persistence: publish-but-not-durable diagnostic={diagnostic}");
                self.state.clear_toasts();
                self.state.persist_notice = Some(state::PersistNotice::PersistenceFailed(
                    persistence::PUBLISH_NOT_DURABLE_SUMMARY.to_owned(),
                ));
                false
            }
            persistence::PersistenceCompletion::CustodyLost(failure) => {
                let diagnostic: String = failure
                    .diagnostic()
                    .chars()
                    .flat_map(char::escape_default)
                    .collect();
                eprintln!("calendar persistence: custody-lost diagnostic={diagnostic}");
                self.state.clear_toasts();
                self.state.persist_notice = Some(state::PersistNotice::CustodyLost);
                let _pending = self.pending_save.take();
                false
            }
            persistence::PersistenceCompletion::Error(failure) => {
                let kind = match failure.kind() {
                    PersistErrorKind::Permission => "permission",
                    PersistErrorKind::MissingDestination => "missing_destination",
                    PersistErrorKind::StorageExhausted => "storage_exhausted",
                    PersistErrorKind::Serialization => "serialization",
                    PersistErrorKind::Protected => "protected",
                    PersistErrorKind::Unknown => "unknown",
                };
                let diagnostic: String = failure
                    .diagnostic()
                    .chars()
                    .flat_map(char::escape_default)
                    .collect();
                eprintln!("calendar persistence failed: kind={kind} diagnostic={diagnostic}");
                self.state.clear_toasts();
                self.state.persist_notice = Some(state::PersistNotice::PersistenceFailed(
                    failure.user_summary(),
                ));
                false
            }
            persistence::PersistenceCompletion::WorkerRefused(refusal) => {
                eprintln!(
                    "calendar persistence worker refusal: category={} retryable={}",
                    refusal.category(),
                    refusal.is_retryable(),
                );
                self.state.clear_toasts();
                self.state.persist_notice = Some(state::PersistNotice::PersistenceFailed(
                    refusal.user_summary().to_owned(),
                ));
                false
            }
        }
    }

    pub(super) fn announce_reminder_notification(&mut self) {
        let raised = match &self.state.overlay {
            state::Overlay::ReminderAlert {
                event_id,
                on,
                title,
                when,
            } => Some((
                crate::reminders::ReminderKey {
                    event_id: *event_id,
                    on: *on,
                },
                title.clone(),
                when.clone(),
            )),
            _ => None,
        };
        if let Some((key, title, when)) = raised
            && let Some(disclosure) = self.notifier.post_reminder(key, &title, &when)
        {
            self.state.publish_toast(
                disclosure.message().to_owned(),
                enzui_widgets::toast::ToastTone::Warning,
            );
        }
    }

    pub(super) fn retry_persistence(&mut self) -> Task<Message> {
        // Retry preserves the vetoed window identity and renews its retained save and reminder
        // debt.
        let persist_task = match self.pending_save.clone() {
            Some(pending) => {
                let renewed = pending.renewed(&self.persistence_queue);
                self.dispatch_persistence(renewed)
            }
            None => Task::none(),
        };
        let Some(window) = self.state.close_state.window() else {
            let reminder = if self
                .state
                .pending_zone
                .is_some_and(|pending| pending.save_generation.is_none())
            {
                self.dispatch_reminder_activation()
            } else {
                Task::none()
            };
            return Task::batch([persist_task, reminder]);
        };
        if !self.state.close_state.is_vetoed() {
            return persist_task;
        }
        let (reminder_task, reminder_gen) = self.dispatch_reminder_activation_tracked();
        let save_gen = self.pending_save.as_ref().map(|p| p.ticket().generation());
        self.state.close_state = CloseState::Draining {
            window,
            deadline: Instant::now() + CLOSE_DRAIN_DEADLINE,
            save_debt: save_gen,
            reminder_debt: reminder_gen,
        };
        let export_task = self.retry_waiting_export();
        let mut tasks = vec![persist_task, reminder_task, export_task];
        if let Some(close) = self.try_finish_close() {
            tasks.push(close);
        }
        Task::batch(tasks)
    }

    pub(super) fn begin_close_drain(&mut self, window: iced::window::Id) -> Task<Message> {
        if !self.state.close_state.is_idle() {
            return Task::none();
        }
        // Closing cancels exports without a selected destination because they have no write
        // authorization.
        self.cancel_unauthorized_export();
        self.picker.cancel_pending();
        self.pending_import_generation = 0;
        self.pending_import_drop = None;
        let save_task = self.dispatch_close_persistence();
        let save_gen = self.pending_save.as_ref().map(|p| p.ticket().generation());
        let (reminder_task, reminder_gen) = self.dispatch_reminder_activation_tracked();
        self.state.close_state = CloseState::Draining {
            window,
            deadline: Instant::now() + CLOSE_DRAIN_DEADLINE,
            save_debt: save_gen,
            reminder_debt: reminder_gen,
        };
        let export_task = self.retry_waiting_export();
        let mut tasks = vec![save_task, reminder_task, export_task];
        if let Some(close) = self.try_finish_close() {
            tasks.push(close);
        }
        Task::batch(tasks)
    }

    fn dispatch_close_persistence(&mut self) -> Task<Message> {
        if let Some(pending) = self.persist.prepare(&self.state, &self.persistence_queue) {
            self.pending_save = Some(pending);
        }
        let Some(pending) = self.pending_save.clone() else {
            return Task::none();
        };
        let renewed = pending.renewed(&self.persistence_queue);
        self.dispatch_persistence(renewed)
    }

    pub(super) fn observe_persistence_for_close(
        &mut self,
        generation: u64,
        outcome: &persistence::PersistenceCompletion,
    ) {
        let (window, deadline_or_now, save_debt, reminder_debt, was_vetoed) =
            match self.state.close_state.clone() {
                CloseState::Draining {
                    window,
                    deadline,
                    save_debt,
                    reminder_debt,
                } => (window, deadline, save_debt, reminder_debt, false),
                CloseState::Vetoed {
                    window,
                    save_debt,
                    reminder_debt,
                    ..
                } => (
                    window,
                    Instant::now() + CLOSE_DRAIN_DEADLINE,
                    save_debt,
                    reminder_debt,
                    true,
                ),
                CloseState::Idle => return,
            };
        if save_debt != Some(generation) {
            return;
        }
        match outcome {
            persistence::PersistenceCompletion::Saved => {
                self.state.close_state = CloseState::Draining {
                    window,
                    deadline: deadline_or_now,
                    save_debt: None,
                    reminder_debt,
                };
            }
            // A superseded save is inert only when a newer exact ticket owns the close debt.
            persistence::PersistenceCompletion::Superseded => {
                if let Some(new_gen) = self.pending_save.as_ref().map(|p| p.ticket().generation())
                    && new_gen != generation
                {
                    self.state.close_state = CloseState::Draining {
                        window,
                        deadline: deadline_or_now,
                        save_debt: Some(new_gen),
                        reminder_debt,
                    };
                } else if !was_vetoed {
                    self.state.close_state = CloseState::Vetoed {
                        window,
                        reason: CloseVetoReason::PersistenceFailed,
                        save_debt: Some(generation),
                        reminder_debt,
                    };
                }
            }
            persistence::PersistenceCompletion::Error(_)
            | persistence::PersistenceCompletion::PublishedButNotDurable(_)
            | persistence::PersistenceCompletion::CustodyLost(_)
            | persistence::PersistenceCompletion::WorkerRefused(_) => {
                if !was_vetoed {
                    self.state.close_state = CloseState::Vetoed {
                        window,
                        reason: CloseVetoReason::PersistenceFailed,
                        save_debt: Some(generation),
                        reminder_debt,
                    };
                }
            }
        }
    }

    pub(super) fn observe_reminder_for_close(&mut self, generation: u64, success: bool) {
        let (window, deadline_or_now, save_debt, reminder_debt, was_vetoed) =
            match self.state.close_state.clone() {
                CloseState::Draining {
                    window,
                    deadline,
                    save_debt,
                    reminder_debt,
                } => (window, deadline, save_debt, reminder_debt, false),
                CloseState::Vetoed {
                    window,
                    save_debt,
                    reminder_debt,
                    ..
                } => (
                    window,
                    Instant::now() + CLOSE_DRAIN_DEADLINE,
                    save_debt,
                    reminder_debt,
                    true,
                ),
                CloseState::Idle => return,
            };
        if reminder_debt != Some(generation) {
            return;
        }
        if success {
            self.state.close_state = CloseState::Draining {
                window,
                deadline: deadline_or_now,
                save_debt,
                reminder_debt: None,
            };
        } else if !was_vetoed {
            self.state.close_state = CloseState::Vetoed {
                window,
                reason: CloseVetoReason::ReminderFailed,
                save_debt,
                reminder_debt: Some(generation),
            };
        }
    }

    pub(super) fn try_finish_close(&mut self) -> Option<Task<Message>> {
        let CloseState::Draining {
            window,
            save_debt,
            reminder_debt,
            ..
        } = &self.state.close_state
        else {
            return None;
        };
        if save_debt.is_some() || reminder_debt.is_some() {
            return None;
        }
        // Waiting exports block teardown even when no worker is occupied.
        if self.has_pending_export() {
            return None;
        }
        if self.persist_worker.dispatcher().snapshot().live() != 0 {
            return None;
        }
        // Cancelling the overlay does not release accepted worker occupancy.
        if self.boot_worker.dispatcher().snapshot().live() != 0 {
            return None;
        }
        let window = *window;
        self.state.close_state = CloseState::Idle;
        #[cfg(feature = "center")]
        self.shutdown_center();
        Some(iced::window::close(window))
    }

    pub(super) fn check_close_deadline(&mut self) {
        let CloseState::Draining {
            window,
            deadline,
            save_debt,
            reminder_debt,
        } = self.state.close_state.clone()
        else {
            return;
        };
        if Instant::now() < deadline {
            return;
        }
        self.state.close_state = CloseState::Vetoed {
            window,
            reason: CloseVetoReason::Deadline,
            save_debt,
            reminder_debt,
        };
        if self.state.persist_notice.is_none() {
            self.state.persist_notice = Some(state::PersistNotice::PersistenceFailed(
                persistence::CLOSE_DEADLINE_SUMMARY.to_owned(),
            ));
        }
    }
}

fn map_worker_refusal(error: BlockingTaskError) -> persistence::WorkerRefusal {
    match error {
        BlockingTaskError::Busy => persistence::WorkerRefusal::Busy,
        BlockingTaskError::Cancelled => persistence::WorkerRefusal::Cancelled,
        BlockingTaskError::Shutdown => persistence::WorkerRefusal::Shutdown,
        BlockingTaskError::Panicked => persistence::WorkerRefusal::Panicked,
        _ => persistence::WorkerRefusal::Shutdown,
    }
}
