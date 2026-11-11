//! Window and content density from shared sizing and sidebar placement.

pub(crate) const PHONE_MAX_WIDTH: f32 = 620.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Density {
    Phone,
    Compact,
    Regular,
}

impl Density {
    #[must_use]
    pub(crate) fn of_sizing(width: f32, sizing: &enzui_theme::SizingTokens) -> Self {
        if !width.is_finite() {
            return Self::Regular;
        }
        if width < PHONE_MAX_WIDTH {
            Self::Phone
        } else if width < sizing.layout_switch_compact {
            Self::Compact
        } else {
            Self::Regular
        }
    }

    #[must_use]
    pub(crate) fn content_pane_width_sizing(
        window_width: f32,
        sidebar_hidden: bool,
        sizing: &enzui_theme::SizingTokens,
    ) -> f32 {
        if !window_width.is_finite() {
            return 0.0;
        }
        use enzui_widgets::app_shell::enz_app_shell;
        use enzui_widgets::nav_split::{NavSplitState, SidebarPreference};
        let nav = NavSplitState {
            preference: if sidebar_hidden {
                SidebarPreference::Hidden
            } else {
                SidebarPreference::Automatic
            },
            ..NavSplitState::new()
        };
        enz_app_shell(|| iced::widget::Space::new().into())
            .layout(enzui_widgets::app_shell::ShellLayout::TitlebarSidebarContent)
            .tokens(shell_tokens())
            .state(nav)
            .on_sidebar_state(|_| ())
            .forced(sidebar_presentation(
                Self::of_sizing(window_width, sizing),
                nav,
            ))
            .geometry_for(window_width)
            .content_width
    }

    #[must_use]
    pub(crate) fn of_content_pane_sizing(
        window_width: f32,
        sidebar_hidden: bool,
        sizing: &enzui_theme::SizingTokens,
    ) -> Self {
        Self::of_sizing(
            Self::content_pane_width_sizing(window_width, sidebar_hidden, sizing),
            sizing,
        )
    }

    #[must_use]
    pub(crate) fn schedule_compact(self) -> bool {
        matches!(self, Self::Phone)
    }

    #[must_use]
    pub(crate) fn switcher_is_dropdown(self) -> bool {
        !matches!(self, Self::Regular)
    }

    #[must_use]
    pub(crate) fn sidebar_is_drawer(self) -> bool {
        !matches!(self, Self::Regular)
    }

    #[must_use]
    pub(crate) fn allows_secondary_tz(self) -> bool {
        !matches!(self, Self::Phone)
    }

    #[must_use]
    pub(crate) fn week_columns(self) -> usize {
        match self {
            Self::Phone => 3,
            _ => 7,
        }
    }

    #[must_use]
    pub(crate) fn month_chips(self) -> usize {
        match self {
            Self::Phone => 1,
            Self::Compact => 2,
            Self::Regular => 3,
        }
    }
}

pub(crate) fn shell_tokens() -> enzui_widgets::app_shell::ShellTokens {
    enzui_widgets::app_shell::ShellTokens::default()
        .sidebar_width(220.0)
        .sidebar_header(enzui_widgets::app_shell::SidebarHeader::Inside)
}

pub(crate) fn sidebar_presentation(
    density: Density,
    nav: enzui_widgets::nav_split::NavSplitState,
) -> enzui_widgets::nav_split::SidebarPresentation {
    use enzui_widgets::nav_split::{SidebarPreference, SidebarPresentation};
    if density.sidebar_is_drawer() {
        SidebarPresentation::Overlay
    } else if nav.preference == SidebarPreference::Hidden {
        SidebarPresentation::Hidden
    } else {
        SidebarPresentation::Inline
    }
}
