//! Desktop notification payloads, action routing, and delivery disclosures for reminders.

use enzui_core::app_data::AppId;
use enzui_core::civil_date::Date;
use enzui_core::portal::notification::{
    DesktopNotification, DesktopNotificationClient, NotificationAction, NotificationActionId,
    NotificationActionInvocation, NotificationBackendStats, NotificationButton, NotificationError,
    NotificationEvent, NotificationId, NotificationOperationOutcome, NotificationPriority,
    NotificationWake, UnsupportedNotificationFeature,
};
use enzui_widgets::file_chooser::DbusBackend;
use iced::{Subscription, Task};

use crate::reminders::ReminderKey;

const NOTIFY_APP_ID: &str = crate::package::APP_ID;

pub(crate) const ACTION_OPEN: &str = "open";
pub(crate) const ACTION_SNOOZE: &str = "snooze";
pub(crate) const ACTION_DISMISS: &str = "dismiss";

const ID_PREFIX: &str = "reminder";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ReminderAction {
    Open,
    Snooze,
    Dismiss,
}

impl ReminderAction {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            ACTION_OPEN => Some(Self::Open),
            ACTION_SNOOZE => Some(Self::Snooze),
            ACTION_DISMISS => Some(Self::Dismiss),
            _ => None,
        }
    }
}

#[must_use]
pub(crate) fn reminder_notification_id(key: ReminderKey) -> NotificationId {
    NotificationId::new(format!("{ID_PREFIX}.{}.{}", key.event_id, key.on.to_days()))
        .expect("the fixed reminder prefix and numeric key must form a valid bounded id")
}

#[must_use]
pub(crate) fn parse_reminder_id(id: &NotificationId) -> Option<ReminderKey> {
    let mut parts = id.as_str().split('.');
    if parts.next()? != ID_PREFIX {
        return None;
    }
    let event_id: u64 = parts.next()?.parse().ok()?;
    let days: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(ReminderKey {
        event_id,
        on: Date::from_days(days),
    })
}

#[must_use]
pub(crate) fn reminder_notification(
    key: ReminderKey,
    title: &str,
    when: &str,
) -> DesktopNotification {
    // Notification previews must obey the desktop UTF-8 byte limit without changing stored titles.
    let mut chars = title.chars().filter(|c| !c.is_control());
    let mut shown_title: String = chars.by_ref().take(60).collect();
    if chars.next().is_some() {
        shown_title.push('…');
    }
    if shown_title.trim().is_empty() {
        shown_title = crate::l10n::tr("Reminder");
    }
    DesktopNotification::new(reminder_notification_id(key), shown_title)
        .body(when.to_owned())
        .priority(NotificationPriority::High)
        .default_action(action(ACTION_OPEN))
        .button(NotificationButton::new(
            crate::l10n::tr("Snooze"),
            action(ACTION_SNOOZE),
        ))
        .button(NotificationButton::new(
            crate::l10n::tr("Dismiss"),
            action(ACTION_DISMISS),
        ))
}

fn action(name: &str) -> NotificationAction {
    NotificationAction::new(
        NotificationActionId::new(name).expect("a reminder action name is a fixed literal"),
    )
}

#[must_use]
pub(crate) fn reminder_action(
    invocation: &NotificationActionInvocation,
) -> Option<(ReminderKey, ReminderAction)> {
    let key = parse_reminder_id(&invocation.notification_id)?;
    let which = ReminderAction::from_name(invocation.action_id.as_str())?;
    Some((key, which))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NotifyDisclosure {
    Unavailable,
    Denied,
    Unsupported,
    ServiceLost,
    ActionsUnavailable,
    QueuePressure,
    Failed,
}

impl NotifyDisclosure {
    #[must_use]
    pub(crate) fn message(&self) -> &'static str {
        match self {
            Self::Unavailable => {
                "Desktop notifications are unavailable — reminders still appear in the app."
            }
            Self::Denied => {
                "The desktop declined a reminder notification — reminders still appear in the app."
            }
            Self::Unsupported => {
                "This desktop can't show part of a reminder notification — reminders still appear in the app."
            }
            Self::ServiceLost => {
                "The notification service restarted — reminders still appear in the app."
            }
            Self::ActionsUnavailable => {
                "Reminder notification buttons are unavailable — use the reminder in the app."
            }
            Self::QueuePressure => {
                "Some reminder notification actions were dropped — use the reminder in the app."
            }
            Self::Failed => {
                "A reminder notification couldn't be delivered — reminders still appear in the app."
            }
        }
    }
}

#[must_use]
pub(crate) fn disclosure_for(event: &NotificationEvent) -> Option<NotifyDisclosure> {
    match event {
        NotificationEvent::Operation { outcome, .. } => match outcome {
            NotificationOperationOutcome::Sent
            | NotificationOperationOutcome::Replaced
            | NotificationOperationOutcome::Withdrawn
            | NotificationOperationOutcome::Superseded => None,
            NotificationOperationOutcome::Unavailable => Some(NotifyDisclosure::ServiceLost),
            NotificationOperationOutcome::Denied => Some(NotifyDisclosure::Denied),
            NotificationOperationOutcome::Unsupported(feature) => {
                Some(unsupported_disclosure(feature))
            }
            NotificationOperationOutcome::Failed => Some(NotifyDisclosure::Failed),
        },
        NotificationEvent::ActionDeliveryUnavailable => Some(NotifyDisclosure::ActionsUnavailable),
        NotificationEvent::ServiceChanged { available, .. } => {
            (!available).then_some(NotifyDisclosure::ServiceLost)
        }
        // Closing a desktop notification does not dismiss or snooze the calendar reminder.
        NotificationEvent::Closed { .. } => None,
        // Rejected stale, duplicate or unowned notification callbacks are not user-actionable
        // failures.
        NotificationEvent::IgnoredAction { .. } | NotificationEvent::Action(_) => None,
    }
}

fn unsupported_disclosure(feature: &UnsupportedNotificationFeature) -> NotifyDisclosure {
    match feature {
        UnsupportedNotificationFeature::Notifications => NotifyDisclosure::Unavailable,
        UnsupportedNotificationFeature::ActionDelivery => NotifyDisclosure::ActionsUnavailable,
        _ => NotifyDisclosure::Unsupported,
    }
}

#[must_use]
pub(crate) fn queue_pressure_disclosure(
    stats: NotificationBackendStats,
) -> Option<NotifyDisclosure> {
    (stats.dropped_actions > 0).then_some(NotifyDisclosure::QueuePressure)
}

pub(crate) struct Notifier {
    client: Option<DesktopNotificationClient<DbusBackend>>,
    enabled: bool,
    attempted: bool,
    announced_unavailable: bool,
    active: Option<ReminderKey>,
    durable: crate::reminder_activation::Owner,
}

impl Notifier {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            client: None,
            enabled: false,
            attempted: false,
            announced_unavailable: false,
            active: None,
            durable: crate::reminder_activation::Owner::new(),
        }
    }

    pub(crate) fn enable(&mut self) {
        self.enabled = true;
    }

    pub(crate) fn install_durable(
        &mut self,
        boot: crate::reminder_activation::BootState,
    ) -> (
        Option<ReminderKey>,
        bool,
        Option<crate::reminder_activation::Failure>,
    ) {
        self.durable.install(boot)
    }

    pub(crate) fn dispatch_durable(
        &mut self,
        desired: Option<crate::reminder_activation::DesiredReminder>,
        worker: &std::sync::Arc<enzui_core::blocking_task::BlockingTaskOwner>,
    ) -> Option<Task<crate::reminder_activation::SyncCompletion>> {
        self.durable.dispatch(desired, worker)
    }

    pub(crate) fn settle_durable(
        &mut self,
        completion: crate::reminder_activation::SyncCompletion,
    ) -> Option<crate::reminder_activation::Failure> {
        self.durable.settle(completion)
    }

    pub(crate) fn note_durable_failure(&mut self) -> bool {
        self.durable.note_failure()
    }

    pub(crate) fn pending_durable_generation(&self) -> Option<u64> {
        self.durable.pending_generation()
    }

    pub(crate) fn wake_subscription(&self) -> Subscription<NotificationWake> {
        match &self.client {
            Some(client) => client.wake_subscription(),
            None => Subscription::none(),
        }
    }

    fn client_mut(&mut self) -> Option<&mut DesktopNotificationClient<DbusBackend>> {
        if !self.enabled {
            return None;
        }
        if self.client.is_none() && !self.attempted {
            self.attempted = true;
            let app_id = AppId::new(NOTIFY_APP_ID).expect("the calendar app id is a fixed literal");
            let sandbox = enzui_core::portal::sandbox::SandboxEvidence::from_host();
            let identity = if sandbox.flatpak_info || sandbox.flatpak_id || sandbox.snap {
                enzui_core::portal::dbus_backend::PortalIdentity::Sandbox
            } else {
                enzui_core::portal::dbus_backend::PortalIdentity::Host
            };
            if let Ok(backend) = DbusBackend::for_application(app_id.clone(), identity) {
                self.client = Some(DesktopNotificationClient::new(app_id, backend));
            }
        }
        self.client.as_mut()
    }

    pub(crate) fn post_reminder(
        &mut self,
        key: ReminderKey,
        title: &str,
        when: &str,
    ) -> Option<NotifyDisclosure> {
        if !self.enabled || self.active == Some(key) {
            return None;
        }
        let notification = reminder_notification(key, title, when);
        if self.client_mut().is_none() {
            return (!std::mem::replace(&mut self.announced_unavailable, true))
                .then_some(NotifyDisclosure::Unavailable);
        }
        let client = self.client_mut()?;
        let disclosure = client
            .send(notification)
            .err()
            .and_then(|error| send_disclosure(&error));
        if disclosure.is_none() {
            self.active = Some(key);
        }
        disclosure
    }

    pub(crate) fn withdraw(&mut self, key: ReminderKey) {
        if self.active == Some(key) {
            self.active = None;
        }
        if let Some(client) = self.client.as_mut() {
            let _ = client.withdraw(&reminder_notification_id(key));
        }
    }

    pub(crate) fn drain(&mut self) -> Vec<NotificationEvent> {
        match self.client.as_mut() {
            Some(client) => std::iter::from_fn(|| client.poll_event()).collect(),
            None => Vec::new(),
        }
    }

    #[must_use]
    pub(crate) fn queue_pressure(&self) -> Option<NotifyDisclosure> {
        queue_pressure_disclosure(self.client.as_ref()?.backend_stats())
    }
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}

fn send_disclosure(error: &NotificationError) -> Option<NotifyDisclosure> {
    match error {
        NotificationError::Deferred => None,
        NotificationError::Capacity { .. } | NotificationError::PendingCapacity { .. } => {
            Some(NotifyDisclosure::QueuePressure)
        }
        _ => Some(NotifyDisclosure::Failed),
    }
}
