//! Lossless payloads for GPUI element listeners.
//!
//! These values describe the platform event data that a view callback can
//! observe. They intentionally do not contain native hitboxes, focus handles,
//! windows, or action objects: those stay on the host side of the wire.

use crate::{click, keyboard, mouse};
use gpui::{Pixels, Point};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DispatchPhase {
    Capture,
    Bubble,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MouseDown {
    pub button: click::MouseButton,
    pub position: Point<Pixels>,
    pub modifiers: gpui::Modifiers,
    pub click_count: u32,
    pub first_mouse: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MouseUp {
    pub button: click::MouseButton,
    pub position: Point<Pixels>,
    pub modifiers: gpui::Modifiers,
    pub click_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MouseMove {
    pub position: Point<Pixels>,
    pub pressed_button: Option<click::MouseButton>,
    pub modifiers: gpui::Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MouseExit {
    pub position: Point<Pixels>,
    pub pressed_button: Option<click::MouseButton>,
    pub modifiers: gpui::Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MousePressure {
    pub pressure: f32,
    pub stage: PressureStage,
    pub position: Point<Pixels>,
    pub modifiers: gpui::Modifiers,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum PressureStage {
    #[default]
    Zero,
    Normal,
    Force,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollWheel {
    pub position: Point<Pixels>,
    pub delta: mouse::ScrollDelta,
    pub modifiers: gpui::Modifiers,
    pub touch_phase: TouchPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pinch {
    pub position: Point<Pixels>,
    pub delta: f32,
    pub modifiers: gpui::Modifiers,
    pub phase: TouchPhase,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyDown {
    pub state: keyboard::KeyState,
    pub repeat: bool,
    pub prefer_character_input: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyUp {
    pub state: keyboard::KeyState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModifiersChanged {
    pub modifiers: gpui::Modifiers,
    pub capslock: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoverListenerMode {
    #[default]
    InputModalityAware,
    InputModalityIndependent,
}

/// A node's tooltip: the route the host asks ([`Event::TooltipRequest`])
/// when the pointer rests on the node, and how to show what comes back.
/// The content never travels in the tree: it comes back as a
/// [`TooltipResponse`] in the frame, and the host keeps it beside the
/// tree, by route, for as long as the route lives.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tooltip {
    pub request: u32,
    pub hoverable: bool,
    pub delay_ms: u64,
}

/// What the view built for a tooltip request: the content for the route,
/// or `None` where the builder had nothing to show.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TooltipResponse {
    pub request: u32,
    /// Echoes the rich-text character index; ordinary tooltips use `None`.
    pub character_index: Option<u32>,
    /// `None` is an explicit result from an index-sensitive builder.
    pub content: Option<Box<crate::Node>>,
}

fn point(x: f32, y: f32) -> Point<Pixels> {
    gpui::point(gpui::px(x), gpui::px(y))
}

fn key_name(value: &keyboard::Key) -> String {
    match value {
        keyboard::Key::Character(value) => value.clone(),
        keyboard::Key::Unidentified => "unidentified".into(),
        keyboard::Key::Named(value) => match value {
            keyboard::Named::Enter => "enter",
            keyboard::Named::Tab => "tab",
            keyboard::Named::Space => "space",
            keyboard::Named::Escape => "escape",
            keyboard::Named::Backspace => "backspace",
            keyboard::Named::Delete => "delete",
            keyboard::Named::Insert => "insert",
            keyboard::Named::ArrowUp => "up",
            keyboard::Named::ArrowDown => "down",
            keyboard::Named::ArrowLeft => "left",
            keyboard::Named::ArrowRight => "right",
            keyboard::Named::PageUp => "pageup",
            keyboard::Named::PageDown => "pagedown",
            keyboard::Named::Control => "control",
            keyboard::Named::Alt => "alt",
            keyboard::Named::Shift => "shift",
            keyboard::Named::Super => "platform",
            keyboard::Named::Fn => "function",
            keyboard::Named::BrowserBack => "back",
            keyboard::Named::BrowserForward => "forward",
            value => return format!("{value:?}").to_ascii_lowercase(),
        }
        .into(),
    }
}

impl From<&keyboard::KeyState> for gpui::Keystroke {
    fn from(value: &keyboard::KeyState) -> Self {
        Self {
            modifiers: value.modifiers,
            key: key_name(&value.key),
            key_char: match &value.modified_key {
                keyboard::Key::Character(value) => Some(value.clone()),
                _ => None,
            },
        }
    }
}

impl From<gpui::TouchPhase> for TouchPhase {
    fn from(value: gpui::TouchPhase) -> Self {
        match value {
            gpui::TouchPhase::Started => Self::Started,
            gpui::TouchPhase::Moved => Self::Moved,
            gpui::TouchPhase::Ended => Self::Ended,
            gpui::TouchPhase::Cancelled => Self::Cancelled,
        }
    }
}

impl From<TouchPhase> for gpui::TouchPhase {
    fn from(value: TouchPhase) -> Self {
        match value {
            TouchPhase::Started => Self::Started,
            TouchPhase::Moved => Self::Moved,
            TouchPhase::Ended => Self::Ended,
            TouchPhase::Cancelled => Self::Cancelled,
        }
    }
}

fn wire_key(value: &str) -> keyboard::Key {
    use keyboard::Named;
    let named = match value {
        "enter" => Named::Enter,
        "tab" => Named::Tab,
        "space" => Named::Space,
        "escape" => Named::Escape,
        "backspace" => Named::Backspace,
        "delete" => Named::Delete,
        "insert" => Named::Insert,
        "up" => Named::ArrowUp,
        "down" => Named::ArrowDown,
        "left" => Named::ArrowLeft,
        "right" => Named::ArrowRight,
        "home" => Named::Home,
        "end" => Named::End,
        "pageup" => Named::PageUp,
        "pagedown" => Named::PageDown,
        "shift" => Named::Shift,
        "control" => Named::Control,
        "alt" => Named::Alt,
        "platform" => Named::Super,
        "function" => Named::Fn,
        "back" => Named::BrowserBack,
        "forward" => Named::BrowserForward,
        "f1" => Named::F1,
        "f2" => Named::F2,
        "f3" => Named::F3,
        "f4" => Named::F4,
        "f5" => Named::F5,
        "f6" => Named::F6,
        "f7" => Named::F7,
        "f8" => Named::F8,
        "f9" => Named::F9,
        "f10" => Named::F10,
        "f11" => Named::F11,
        "f12" => Named::F12,
        "f13" => Named::F13,
        "f14" => Named::F14,
        "f15" => Named::F15,
        "f16" => Named::F16,
        "f17" => Named::F17,
        "f18" => Named::F18,
        "f19" => Named::F19,
        "f20" => Named::F20,
        "f21" => Named::F21,
        "f22" => Named::F22,
        "f23" => Named::F23,
        "f24" => Named::F24,
        "f25" => Named::F25,
        "f26" => Named::F26,
        "f27" => Named::F27,
        "f28" => Named::F28,
        "f29" => Named::F29,
        "f30" => Named::F30,
        "f31" => Named::F31,
        "f32" => Named::F32,
        "f33" => Named::F33,
        "f34" => Named::F34,
        "f35" => Named::F35,
        _ => return keyboard::Key::Character(value.to_owned()),
    };
    keyboard::Key::Named(named)
}

impl From<&gpui::Keystroke> for keyboard::KeyState {
    /// gpui publishes logical keys only: no physical scan code is invented.
    fn from(value: &gpui::Keystroke) -> Self {
        let key = wire_key(&value.key);
        let modified_key = value
            .key_char
            .as_deref()
            .map(|key| keyboard::Key::Character(key.to_owned()))
            .unwrap_or_else(|| key.clone());
        Self {
            key,
            modified_key,
            physical_key: keyboard::Physical::Unidentified(keyboard::NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers: value.modifiers,
        }
    }
}

impl From<&gpui::MouseDownEvent> for MouseDown {
    fn from(value: &gpui::MouseDownEvent) -> Self {
        Self {
            button: value.button.into(),
            position: value.position,
            modifiers: value.modifiers,
            click_count: value.click_count.min(u32::MAX as usize) as u32,
            first_mouse: value.first_mouse,
        }
    }
}

impl From<&gpui::MouseUpEvent> for MouseUp {
    fn from(value: &gpui::MouseUpEvent) -> Self {
        Self {
            button: value.button.into(),
            position: value.position,
            modifiers: value.modifiers,
            click_count: value.click_count.min(u32::MAX as usize) as u32,
        }
    }
}

impl From<&gpui::MouseMoveEvent> for MouseMove {
    fn from(value: &gpui::MouseMoveEvent) -> Self {
        Self {
            position: value.position,
            pressed_button: value.pressed_button.map(Into::into),
            modifiers: value.modifiers,
        }
    }
}

impl From<&gpui::MouseExitEvent> for MouseExit {
    fn from(value: &gpui::MouseExitEvent) -> Self {
        Self {
            position: value.position,
            pressed_button: value.pressed_button.map(Into::into),
            modifiers: value.modifiers,
        }
    }
}

impl From<&gpui::MousePressureEvent> for MousePressure {
    fn from(value: &gpui::MousePressureEvent) -> Self {
        Self {
            pressure: value.pressure,
            stage: match value.stage {
                gpui::PressureStage::Zero => PressureStage::Zero,
                gpui::PressureStage::Normal => PressureStage::Normal,
                gpui::PressureStage::Force => PressureStage::Force,
            },
            position: value.position,
            modifiers: value.modifiers,
        }
    }
}

impl From<&gpui::ScrollWheelEvent> for ScrollWheel {
    fn from(value: &gpui::ScrollWheelEvent) -> Self {
        let delta = match value.delta {
            gpui::ScrollDelta::Pixels(point) => mouse::ScrollDelta::Pixels {
                x: point.x.as_f32(),
                y: point.y.as_f32(),
            },
            gpui::ScrollDelta::Lines(point) => mouse::ScrollDelta::Lines {
                x: point.x,
                y: point.y,
            },
        };
        Self {
            position: value.position,
            delta,
            modifiers: value.modifiers,
            touch_phase: value.touch_phase.into(),
        }
    }
}

impl From<&gpui::PinchEvent> for Pinch {
    fn from(value: &gpui::PinchEvent) -> Self {
        Self {
            position: value.position,
            delta: value.delta,
            modifiers: value.modifiers,
            phase: value.phase.into(),
        }
    }
}

impl From<&gpui::KeyDownEvent> for KeyDown {
    fn from(value: &gpui::KeyDownEvent) -> Self {
        Self {
            state: (&value.keystroke).into(),
            repeat: value.is_held,
            prefer_character_input: value.prefer_character_input,
        }
    }
}

impl From<&gpui::KeyUpEvent> for KeyUp {
    fn from(value: &gpui::KeyUpEvent) -> Self {
        Self {
            state: (&value.keystroke).into(),
        }
    }
}

impl From<&gpui::ModifiersChangedEvent> for ModifiersChanged {
    fn from(value: &gpui::ModifiersChangedEvent) -> Self {
        Self {
            modifiers: value.modifiers,
            capslock: value.capslock.on,
        }
    }
}

impl MouseDown {
    pub fn into_gpui(self) -> gpui::MouseDownEvent {
        gpui::MouseDownEvent {
            button: self.button.into(),
            position: self.position,
            modifiers: self.modifiers,
            click_count: self.click_count as usize,
            first_mouse: self.first_mouse,
        }
    }
}

impl MouseUp {
    pub fn into_gpui(self) -> gpui::MouseUpEvent {
        gpui::MouseUpEvent {
            button: self.button.into(),
            position: self.position,
            modifiers: self.modifiers,
            click_count: self.click_count as usize,
        }
    }
}

impl MouseMove {
    pub fn into_gpui(self) -> gpui::MouseMoveEvent {
        gpui::MouseMoveEvent {
            position: self.position,
            pressed_button: self.pressed_button.map(Into::into),
            modifiers: self.modifiers,
        }
    }
}

impl MouseExit {
    pub fn into_gpui(self) -> gpui::MouseExitEvent {
        gpui::MouseExitEvent {
            position: self.position,
            pressed_button: self.pressed_button.map(Into::into),
            modifiers: self.modifiers,
        }
    }
}

impl MousePressure {
    pub fn into_gpui(self) -> gpui::MousePressureEvent {
        gpui::MousePressureEvent {
            pressure: self.pressure,
            stage: match self.stage {
                PressureStage::Zero => gpui::PressureStage::Zero,
                PressureStage::Normal => gpui::PressureStage::Normal,
                PressureStage::Force => gpui::PressureStage::Force,
            },
            position: self.position,
            modifiers: self.modifiers,
        }
    }
}

impl ScrollWheel {
    pub fn into_gpui(self) -> gpui::ScrollWheelEvent {
        gpui::ScrollWheelEvent {
            position: self.position,
            delta: match self.delta {
                mouse::ScrollDelta::Pixels { x, y } => gpui::ScrollDelta::Pixels(point(x, y)),
                mouse::ScrollDelta::Lines { x, y } => gpui::ScrollDelta::Lines(gpui::point(x, y)),
            },
            modifiers: self.modifiers,
            touch_phase: self.touch_phase.into(),
        }
    }
}

impl Pinch {
    pub fn into_gpui(self) -> gpui::PinchEvent {
        gpui::PinchEvent {
            position: self.position,
            delta: self.delta,
            modifiers: self.modifiers,
            phase: self.phase.into(),
        }
    }
}

impl KeyDown {
    pub fn into_gpui(self) -> gpui::KeyDownEvent {
        gpui::KeyDownEvent {
            keystroke: (&self.state).into(),
            is_held: self.repeat,
            prefer_character_input: self.prefer_character_input,
        }
    }
}

impl KeyUp {
    pub fn into_gpui(self) -> gpui::KeyUpEvent {
        gpui::KeyUpEvent {
            keystroke: (&self.state).into(),
        }
    }
}

impl ModifiersChanged {
    pub fn into_gpui(self) -> gpui::ModifiersChangedEvent {
        gpui::ModifiersChangedEvent {
            modifiers: self.modifiers,
            capslock: gpui::Capslock { on: self.capslock },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_gpui_key_name_survives_the_wire() {
        let names = [
            "enter",
            "tab",
            "space",
            "escape",
            "backspace",
            "delete",
            "insert",
        ]
        .into_iter()
        .chain([
            "up", "down", "left", "right", "home", "end", "pageup", "pagedown",
        ])
        .chain([
            "shift", "control", "alt", "platform", "function", "back", "forward",
        ])
        .chain(["a", "é"])
        .map(str::to_owned)
        .chain((1..=35).map(|n| format!("f{n}")));
        for name in names {
            let printable = name.chars().count() == 1;
            let keystroke = gpui::Keystroke {
                modifiers: gpui::Modifiers {
                    platform: !printable,
                    ..Default::default()
                },
                key: name.clone(),
                key_char: printable.then(|| name.clone()),
            };
            let state = keyboard::KeyState::from(&keystroke);
            assert_eq!(
                matches!(state.key, keyboard::Key::Named(_)),
                !printable,
                "{name}"
            );
            assert_eq!(gpui::Keystroke::from(&state), keystroke, "{name}");
        }
    }
}
