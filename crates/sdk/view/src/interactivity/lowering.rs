use super::*;
use crate::slots::Kind;

impl Interactivity {
    pub(crate) fn into_wire(
        self: Box<Self>,
        lowering: &Lowering<'_>,
    ) -> Option<Box<wire::Interactivity>> {
        let identified = self.id.is_some();
        self.interactions
            .map(|interactions| interactions.into_wire(identified, lowering))
    }
}

impl Interactions {
    fn into_wire(
        self: Box<Self>,
        identified: bool,
        lowering: &Lowering<'_>,
    ) -> Box<wire::Interactivity> {
        if let Some(handle) = &self.scroll_handle
            && identified
        {
            handle.track(lowering.slots(), lowering.current_path());
        }
        if self.dense && identified {
            lowering.mark_dense();
        }
        let mut aria = self.aria;
        if !self.a11y_actions.is_empty() {
            let offered: std::rc::Rc<[i32]> =
                aria.custom_actions.iter().map(|(id, _)| *id).collect();
            aria.actions = self
                .a11y_actions
                .into_iter()
                .map(|(action, listener)| {
                    let listener = std::cell::RefCell::new(listener);
                    let offered = offered.clone();
                    let route = lowering.route(
                        Kind::Action(action),
                        move |data: &Option<wire::ActionData>,
                              window: &mut Window,
                              app: &mut App| {
                            if let Some(wire::ActionData::CustomAction(id)) = data
                                && !offered.contains(id)
                            {
                                return;
                            }
                            (listener.borrow_mut())(data.as_ref(), window, app)
                        },
                    );
                    (action, route)
                })
                .collect();
        }
        let tooltip = self.tooltip.map(|tooltip| {
            let request = lowering.tooltip(tooltip.build);
            wire::Tooltip {
                request,
                hoverable: tooltip.hoverable,
                delay_ms: self
                    .tooltip_show_delay
                    .unwrap_or(Duration::from_millis(500))
                    .as_millis()
                    .min(u64::MAX as u128) as u64,
            }
        });
        Box::new(wire::Interactivity {
            role: self.role,
            aria,
            focusable: self.focusable,
            tab_stop: self.tab_stop.or(self.focusable.then_some(true)),
            tab_index: self.tab_index,
            tab_group: self.tab_group,
            focus: self.focus.map(|style| lowering.style(&style)),
            in_focus: self.in_focus.map(|style| lowering.style(&style)),
            focus_visible: match self.focus_visible {
                Some(style) => Some(lowering.style(&style)),
                None => self
                    .focusable
                    .then(|| lowering.style(&crate::design::focus_ring(lowering.theme().accent))),
            },
            focus_handle: self.focus_handle.map(|handle| handle.id),
            occlude: self.occlude,
            block_mouse_except_scroll: self.block_mouse_except_scroll,
            hover_listener_mode: match self.hover_listener_mode {
                gpui::HoverListenerMode::InputModalityAware => {
                    wire::HoverListenerMode::InputModalityAware
                }
                gpui::HoverListenerMode::InputModalityIndependent => {
                    wire::HoverListenerMode::InputModalityIndependent
                }
            },
            group: self.group,
            hover: self.hover.map(|style| lowering.style(&style)),
            file_drag_over: self.file_drag_over.map(|style| lowering.style(&style)),
            active: self.active.map(|style| lowering.style(&style)),
            group_hover: self
                .group_hover
                .map(|(group, style)| wire::GroupRefinement {
                    group,
                    style: lowering.style(&style),
                }),
            group_active: self
                .group_active
                .map(|(group, style)| wire::GroupRefinement {
                    group,
                    style: lowering.style(&style),
                }),
            on_click: route_plain(self.on_click, lowering, Kind::Click),
            on_aux_click: route_plain(self.on_aux_click, lowering, Kind::AuxClick),
            consumes_click: self.consumes_click,
            on_mouse_down: route_buttons(
                self.mouse_down,
                lowering,
                Kind::MouseDown,
                |e: &gpui::MouseDownEvent| e.button,
            ),
            capture_mouse_down: route_plain(
                self.capture_mouse_down,
                lowering,
                Kind::CaptureMouseDown,
            ),
            on_mouse_down_out: route_plain(self.mouse_down_out, lowering, Kind::MouseDownOut),
            on_mouse_up: route_buttons(
                self.mouse_up,
                lowering,
                Kind::MouseUp,
                |e: &gpui::MouseUpEvent| e.button,
            ),
            capture_mouse_up: route_plain(self.capture_mouse_up, lowering, Kind::CaptureMouseUp),
            on_mouse_up_out: route_buttons(
                self.mouse_up_out,
                lowering,
                Kind::MouseUpOut,
                |e: &gpui::MouseUpEvent| e.button,
            ),
            on_mouse_pressure: route_plain(self.mouse_pressure, lowering, Kind::MousePressure),
            capture_mouse_pressure: route_plain(
                self.capture_mouse_pressure,
                lowering,
                Kind::CaptureMousePressure,
            ),
            on_mouse_move: route_plain(self.mouse_move, lowering, Kind::MouseMove),
            on_mouse_exit: route_plain(self.mouse_exit, lowering, Kind::MouseExit),
            on_scroll_wheel: route_plain(self.scroll_wheel, lowering, Kind::ScrollWheel),
            on_pinch: route_plain(self.pinch, lowering, Kind::Pinch),
            capture_pinch: route_plain(self.capture_pinch, lowering, Kind::CapturePinch),
            on_key_down: route_plain(self.key_down, lowering, Kind::KeyDown),
            capture_key_down: route_plain(self.capture_key_down, lowering, Kind::CaptureKeyDown),
            on_key_up: route_plain(self.key_up, lowering, Kind::KeyUp),
            capture_key_up: route_plain(self.capture_key_up, lowering, Kind::CaptureKeyUp),
            on_modifiers_changed: route_plain(
                self.modifiers_changed,
                lowering,
                Kind::ModifiersChanged,
            ),
            consumes_keys: self.consumes_keys,
            on_hover: self
                .on_hover
                .map(|listener| lowering.route(Kind::Hover, listener)),
            on_file_drop_exit: route_plain(self.on_file_drop_exit, lowering, Kind::FileDropExit),
            on_file_drop: route_plain(self.on_file_drop, lowering, Kind::FileDrop),
            tooltip,
            on_back: self
                .on_back
                .map(|listener| lowering.route(Kind::Back, listener)),
            page: self.page,
        })
    }
}

fn route_plain<E: 'static>(
    listeners: Vec<EventListener<E>>,
    lowering: &Lowering<'_>,
    kind: Kind,
) -> Option<u32> {
    (!listeners.is_empty()).then(|| {
        lowering.route(kind, move |event: &E, window, app| {
            for listener in &listeners {
                listener(event, window, app);
            }
        })
    })
}

fn route_buttons<E: 'static>(
    listeners: Vec<ButtonBinding<E>>,
    lowering: &Lowering<'_>,
    kind: Kind,
    button: fn(&E) -> MouseButton,
) -> Option<u32> {
    (!listeners.is_empty()).then(|| {
        lowering.route(kind, move |event: &E, window, app| {
            for binding in &listeners {
                if binding.button.is_none_or(|wanted| wanted == button(event)) {
                    (binding.listener)(event, window, app);
                }
            }
        })
    })
}
