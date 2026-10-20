//! gpui's handle on a div that scrolls, tracked by the div's whole path.
use crate::{slots, wire};
use gpui::{Pixels, Point};
use std::cell::RefCell;
use std::rc::Rc;

/// What a view holds to move a scrolling div (`.id(..).overflow_y_scroll()
/// .track_scroll(&handle)`): to its end, or to an offset. It moves the div
/// it was tracked on in the last frame drawn; a handle no frame has drawn
/// moves nothing.
#[derive(Clone, Default)]
pub struct ScrollHandle(Rc<RefCell<Option<Tracked>>>);

struct Tracked {
    slots: slots::Context,
    path: wire::WidgetTarget,
}

impl std::fmt::Debug for ScrollHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = self.0.borrow().as_ref().map(|tracked| tracked.path.clone());
        f.debug_tuple("ScrollHandle").field(&path).finish()
    }
}

impl ScrollHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scrolls to the end of the content.
    pub fn scroll_to_bottom(&self) {
        self.send(|target| wire::WidgetCommand::SnapEnd { target });
    }

    /// Puts the content at `offset` from the top left, as gpui counts it:
    /// negative as the content moves up.
    pub fn set_offset(&self, offset: Point<Pixels>) {
        self.send(|target| wire::WidgetCommand::ScrollTo {
            target,
            x: -f32::from(offset.x),
            y: -f32::from(offset.y),
        });
    }

    fn send(&self, command: impl FnOnce(wire::WidgetTarget) -> wire::WidgetCommand) {
        if let Some(tracked) = &*self.0.borrow() {
            slots::widget(&tracked.slots, command(tracked.path.clone()));
        }
    }

    pub(crate) fn track(&self, slots: &slots::Context, path: &[wire::ElementIdWire]) {
        *self.0.borrow_mut() = Some(Tracked {
            slots: slots.clone(),
            path: path.to_vec(),
        });
    }
}
