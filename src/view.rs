//! Calendar surfaces, window composition, overlays, and storage notices.

pub(crate) mod month;
pub(crate) mod overlay;
pub(crate) mod schedule;
pub(crate) mod sidebar;
use enzui_core::semantics::HeadingLevel;
use enzui_theme::{EnzTheme, SpaceToken};
use enzui_widgets::action_bar::ActionItem as TitlebarActionItem;
use enzui_widgets::app_shell::{ShellLayout, enz_app_shell};
use enzui_widgets::combo_box::enz_combo_box;
use enzui_widgets::modal_host::{SurfacePlacement, enz_modal_host};

pub(crate) const OVERLAY_SCOPE_KEY: &str = "calendar-overlay";
use enzui_widgets::nav_split::NavSplitState;
use enzui_widgets::notice_stack::{EnzNotice, enz_notice_stack};
use enzui_widgets::positioned_overlay::enz_positioned_overlay_with_cause;
use enzui_widgets::text_field::TextFieldSize;
use enzui_widgets::title_bar::enz_title_bar;
use enzui_widgets::toast::ToastTone;
use enzui_widgets::toast_host::{ToastOverlayPlacement, enz_toast_layer};
use enzui_widgets::toolbar_icon::enz_toolbar_icon;
use enzui_widgets::view_switcher::{SwitcherPage, enz_view_switcher};
use iced::widget::{container, row};
use iced::{Alignment, Element, Length, Padding};

use crate::layout::{Density, shell_tokens, sidebar_presentation};
use crate::message::Message;
use crate::model::{View, date_label};
use crate::state::{AppState, PersistNotice, SettingsInvoker};

fn main_pane<'a>(app: &'a AppState, theme: &EnzTheme) -> Element<'a, Message> {
    match app.view {
        View::Day | View::Week => schedule::schedule(app, theme),
        View::Month => month::month(app),
        View::Year => month::year(app),
        View::Agenda => month::agenda(app),
    }
}

#[must_use]
pub(crate) fn titlebar_overflow_button_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("calendar.titlebar.actions-overflow")
}

pub(crate) const TITLEBAR_SEARCH_MAX_RESULTS: usize = 20;

#[path = "view/titlebar.rs"]
mod titlebar;

pub(crate) use titlebar::search_result_date_label;
use titlebar::titlebar;

fn sidebar_titlebar<'a>() -> Element<'a, Message> {
    enz_title_bar()
        .title(crate::l10n::tr(SIDEBAR_HEADER_TITLE))
        .title_heading(HeadingLevel::Section)
        .push(
            enz_toolbar_icon(
                "settings",
                Message::OpenSettings(SettingsInvoker::InlineGear),
            )
            .label(crate::l10n::tr("Settings"))
            .id(overlay::settings_button_id()),
        )
        .id("calendar-app-sidebar-header")
        .into_element()
}

pub(crate) const SIDEBAR_HEADER_TITLE: &str = "Calendars";

fn shell_composition<'a>(
    app: &'a AppState,
    nav: NavSplitState,
    theme: &EnzTheme,
    density: Density,
    window: &enzui_widgets::app_shell::ShellWindowState,
) -> Element<'a, Message> {
    let forcing_rule = move |state: NavSplitState| sidebar_presentation(density, state);
    let theme_for_sidebar_body = theme.clone();
    let theme_for_content = theme.clone();
    enz_app_shell(move || content_area(app, &theme_for_content))
        .layout(ShellLayout::TitlebarSidebarContent)
        .tokens(shell_tokens())
        .state(nav)
        .titlebar(move || titlebar(app))
        .sidebar_label(crate::l10n::tr(SIDEBAR_HEADER_TITLE))
        .sidebar_titlebar(sidebar_titlebar)
        .sidebar(move || sidebar::sidebar(app, &theme_for_sidebar_body))
        .on_sidebar_state(Message::SidebarState)
        .on_window_gesture(Message::Window)
        .maximized(window.maximized())
        .fullscreen(window.fullscreen())
        .resizable(window.resizable())
        .forced_by(forcing_rule)
        .on_dismiss_overlay(Message::DismissSidebar)
        .into_element()
}

/// The storage notice for the current persistence state, floated bottom-right with the toasts.
///
/// Every notice can be closed. Closing an informational notice clears it; closing a notice that
/// reports a lasting storage state only hides it until that state changes (see
/// `AppState::persist_notice_hidden`).
fn persist_notice(app: &AppState) -> Option<EnzNotice<Message>> {
    let notice = app.persist_notice.as_ref()?;
    if app.persist_notice_hidden {
        return None;
    }
    let card = |title: String, body: String, tone: ToastTone| {
        EnzNotice::new(PERSIST_NOTICE_ID, title, tone).description(body)
    };
    let card = match notice {
        PersistNotice::Quarantined(Some(kept)) => card(
            crate::l10n::tr("Your calendar file could not be read"),
            crate::l10n::format("The unreadable file was kept as {}. Changes in this calendar are saved normally from now on.", &[("arg0", (kept.to_string_lossy()).to_string().into())]),
            ToastTone::Warning,
        ),
        PersistNotice::Quarantined(None) => card(
            crate::l10n::tr("Your calendar file could not be read"),
            crate::l10n::tr("It was left untouched and saving is off. Retire that exact file to keep it under \
             a recovery name and start a new calendar."),
            ToastTone::Error,
        )
        .action(crate::l10n::tr(RETIRE_STATE_ACTION), Message::RetireUnreadableState),
        PersistNotice::ReadOnlyFuture(version) => card(
            crate::l10n::tr("This calendar is read-only"),
            crate::l10n::format("Your calendar file was written by a newer version of this app (format v{version}), so it cannot be opened safely here. Nothing you change now will be saved.", &[("version", (version).to_string().into())]),
            ToastTone::Error,
        )
        .action(crate::l10n::tr(START_FRESH_ACTION), Message::RetireUnreadableState),
        PersistNotice::PersistenceFailed(summary) => card(
            crate::l10n::tr("Changes are not saved"),
            crate::l10n::tr(summary),
            ToastTone::Error,
        )
        .action(crate::l10n::tr(PERSISTENCE_RETRY_ACTION), Message::RetryPersistence),
        PersistNotice::StartedFresh(kept) => card(
            crate::l10n::tr("Saving is on again"),
            crate::l10n::format("Your newer calendar file was kept as {}. Changes in this calendar are saved normally from now on.", &[("arg0", (kept.to_string_lossy()).to_string().into())]),
            ToastTone::Info,
        ),
        PersistNotice::RetiredReadOnly(kept) => card(
            crate::l10n::tr("Your calendar file was retained, but saving is still off"),
            crate::l10n::format("The unreadable file was kept as {}. Quit and reopen Calendar before making changes.", &[("arg0", (kept.to_string_lossy()).to_string().into())]),
            ToastTone::Error,
        ),
        // Refused on-disk state remains untouched; its notice exposes no admission details.
        PersistNotice::BootRefused(refusal) => card(
            crate::l10n::tr("Your calendar file was not opened"),
            crate::l10n::tr(refusal.user_summary()),
            ToastTone::Error,
        ),
        PersistNotice::BootUnavailable(summary) => card(
            crate::l10n::tr("Calendar could not be opened"),
            crate::l10n::format("{summary} Saving is off for this session.", &[("summary", crate::l10n::tr(summary).into())]),
            ToastTone::Error,
        ),
        PersistNotice::CustodyLost => card(
            crate::l10n::tr("Custody of your calendar file was lost"),
            crate::l10n::tr("Another writer replaced the file, so automatic saves are off. Please quit and \
             reopen the app to bind a fresh authority to the file that is there now."),
            ToastTone::Error,
        ),
        // Data-root refusal disables saving and discloses no path or errno.
        PersistNotice::PersistenceRootUnavailable(refusal) => card(
            crate::l10n::tr("Changes are not saved"),
            crate::l10n::format("{} Nothing you change in this session will be saved.", &[("arg0", crate::l10n::tr(refusal.user_summary()).into())]),
            ToastTone::Error,
        ),
    };
    Some(card.on_dismiss(Message::DismissPersistNotice))
}

const PERSIST_NOTICE_ID: &str = "calendar-persist-notice";

pub(crate) const START_FRESH_ACTION: &str = "Start fresh (your file is kept)";
pub(crate) const RETIRE_STATE_ACTION: &str = "Retire file and start fresh";
pub(crate) const PERSISTENCE_RETRY_ACTION: &str = "Retry";

// Variable-width period labels stay outside the toolbar to avoid moving its controls.
fn content_area<'a>(app: &'a AppState, theme: &EnzTheme) -> Element<'a, Message> {
    let period_text = enzui_widgets::enz_text::enz_text(app.period_label())
        .size(enzui_widgets::enz_text::TextSize::Headline)
        .tone(enzui_widgets::enz_text::TextTone::Default)
        .heading(HeadingLevel::Page);
    let heading: Element<'_, Message> = match app.week_number_label() {
        Some(week) => row![
            enzui_widgets::enz_text::enz_text(week)
                .size(enzui_widgets::enz_text::TextSize::Callout)
                .tone(enzui_widgets::enz_text::TextTone::Muted),
            iced::widget::Space::new()
                .width(Length::Fill)
                .height(Length::Shrink),
            period_text,
        ]
        .mirrored(crate::l10n::rtl())
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into(),
        None => period_text.into(),
    };
    let header = container(heading)
        .width(Length::Fill)
        .padding(Padding::from([
            0.0,
            enzui_widgets::title_bar::DEFAULT_TITLEBAR_PADDING_X,
        ]));
    iced::widget::column![]
        .push(header)
        .push(
            container(main_pane(app, theme))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .spacing(SpaceToken::Sm.px())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub(crate) fn view<'a>(
    app: &'a AppState,
    nav: NavSplitState,
    theme: &EnzTheme,
    import_drop_epoch: u64,
    window: &enzui_widgets::app_shell::ShellWindowState,
) -> Element<'a, Message> {
    let density = Density::of_sizing(app.window_width, &theme.sizing);

    let base = enzui_widgets::file_drop::enz_file_drop_target_with_receipt(
        shell_composition(app, nav, theme, density, window),
        Message::DroppedFiles,
    )
    .copy_only(true)
    .key(import_drop_epoch);

    let with_menu: Element<'_, Message> = app
        .menu
        .host(base, Message::ContextMenuEvent)
        .label(crate::l10n::tr(overlay::CONTEXT_MENU_LABEL))
        .id_prefix("calendar-context-menu")
        .into_element();

    let popover = if nav.effective_overlay_open(sidebar_presentation(density, nav)) {
        None
    } else {
        overlay::popover(app)
    };
    let with_popover: Element<'_, Message> =
        enz_positioned_overlay_with_cause(with_menu, popover, Message::PositionedOverlayDismissed)
            .into();

    // Modal ownership and accessibility end immediately, while a retained Settings surface may
    // paint its exit.
    let live = overlay::overlay(app, theme);
    let showing = live.is_some();
    let (surface, placement) = match live {
        Some(el) => (Some(el), overlay::overlay_placement(app)),
        None if app.settings_is_closing() => (
            Some(overlay::settings(app)),
            overlay::settings_placement(app),
        ),
        None => (None, SurfacePlacement::Centred),
    };
    let mut host = enz_modal_host(with_popover, surface, Message::OverlayEvent)
        .presentation_open(showing)
        .placement(placement);
    if let Some((_, lifecycle)) = app.overlay_focus.as_ref() {
        host = host.scoped_to(lifecycle);
    }
    if let Some(label) = overlay::modal_host_label(&app.overlay) {
        host = host.label(crate::l10n::tr(label));
    }
    if let Some(key) = app.settings_presentation_key() {
        // Settings exit settlements are keyed to the generation that published them.
        host = host.on_exit_settled_with(move || Message::SettingsExitSettled(key));
    }
    let shell: Element<'a, Message> = host.into();
    let shell = app.calendar_dialogs.view_with(
        shell,
        |key| overlay::calendar_dialog(app, *key),
        Message::CalendarDialog,
        |key, host| {
            let host = if *key == crate::calendar_dialogs::CalendarDialog::CalendarEditor {
                host.placement(overlay::settings_placement(app))
            } else {
                host
            };
            host.label(crate::l10n::tr(match key {
                crate::calendar_dialogs::CalendarDialog::CalendarEditor => "Edit calendar",
                crate::calendar_dialogs::CalendarDialog::CalendarRemoval => "Remove calendar",
            }))
        },
    );

    // Storage notices and toasts share the bottom-right corner, above any modal, so a failed-save
    // Retry stays actionable even while a reminder modal owns the background.
    let notice = persist_notice(app);
    let corner = (notice.is_some() || !app.toasts.is_empty()).then(|| {
        let mut corner = iced::widget::column![]
            .spacing(SpaceToken::Xs.px())
            .align_x(Alignment::End);
        if let Some(notice) = notice {
            corner = corner.push(enz_notice_stack(vec![notice]).into_element());
        }
        if !app.toasts.is_empty() {
            corner = corner.push(app.toasts.view().map(Message::Toast));
        }
        corner.into()
    });
    enz_toast_layer(shell, corner, ToastOverlayPlacement::default())
}
