//! Serializable accessibility declarations for native GPUI interactivity.
use crate::{Action, AriaCurrent, HasPopup, Invalid, Live};
use gpui::SharedString;
use serde::{Deserialize, Serialize};

/// Sparse on the wire, as [`Interactivity`](crate::Interactivity) is.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, remote = "Self")]
pub struct Aria {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_id: Option<SharedString>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<SharedString>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<SharedString>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyshortcuts: Option<SharedString>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub active_descendant: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<SharedString>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<SharedString>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric_value_step: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_numeric_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_numeric_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_in_set: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_of_set: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toggled: Option<gpui::Toggled>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<gpui::Orientation>,
    /// How a change inside the node is announced; `Off` is `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<Live>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub busy: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid: Option<Invalid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_popup: Option<HasPopup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<AriaCurrent>,
    /// Actions the node advertises beyond Click/Focus, each with the
    /// handler route [`crate::Event::A11yAction`] answers with.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<(Action, u32)>,
    /// `(id, description)`; requested as [`Action::CustomAction`] with
    /// `ActionData::CustomAction(id)`, routed through that action's handler.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub custom_actions: Vec<(i32, String)>,
}

crate::codec::sparse!(Aria);

/// Actions the host answers itself, whatever a view advertises.
pub(crate) const HOST_ACTIONS: [Action; 6] = [
    Action::Click,
    Action::Focus,
    Action::Blur,
    Action::SetValue,
    Action::ReplaceSelectedText,
    Action::SetTextSelection,
];

/// The role a view may give a node: `GenericContainer` and `Unknown` say
/// nothing (and trip gpui's debug assert), and the window-level roles are
/// the host's.
pub(crate) fn view_role(role: Option<gpui::Role>) -> Option<gpui::Role> {
    use gpui::Role::*;
    role.filter(|role| {
        !matches!(
            role,
            GenericContainer
                | Unknown
                | Window
                | Application
                | RootWebArea
                | Pane
                | Iframe
                | IframePresentational
                | WebView
                | TitleBar
        )
    })
}

impl Aria {
    /// Bounds everything the node's role does not decide; the rest is
    /// `frame_sanitize::sanitize_interactivity`'s.
    pub(crate) fn sanitize(&mut self) {
        for value in [
            &mut self.numeric_value,
            &mut self.numeric_value_step,
            &mut self.min_numeric_value,
            &mut self.max_numeric_value,
        ]
        .into_iter()
        .flatten()
        {
            if !value.is_finite() {
                *value = 0.;
            }
        }
        if self.live == Some(Live::Off) {
            self.live = None;
        }
        let mut seen = std::collections::HashSet::new();
        self.actions
            .retain(|(action, _)| !HOST_ACTIONS.contains(action) && seen.insert(*action));
        let mut seen = std::collections::HashSet::new();
        self.custom_actions.retain(|(id, _)| seen.insert(*id));
    }
}
