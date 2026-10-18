//! Chat: channels, direct messages, threads and search.
//!
//! The view's state is one struct per pane (`state.rs`). Rows come from the
//! `chat` program through its own types (`queries.rs`), are folded with the
//! name directory into what a frame draws (`message.rs`, `names.rs`) at
//! render time, and `render` (`ui/`) never mutates: what an event changes
//! lands in the handlers of `room`, `actions`, `search` and `compose`. The
//! rich editor is the composer (`composer.rs`, `composer/`), over the SDK's
//! text field.
//!
//! What becomes a notice is chat's NOTIFIER's (`notifier.rs`): the host runs
//! it once per network, open panes or none, and a pane posts nothing. The
//! `notifier` feature builds it alone, with the modules it reads
//! (`links`, `message`, `names`), in place of the view.
#![cfg_attr(feature = "notifier", allow(dead_code))]
#[cfg(not(feature = "notifier"))]
mod actions;
#[cfg(not(feature = "notifier"))]
mod compose;
#[cfg(not(feature = "notifier"))]
mod composer;
#[cfg(not(feature = "notifier"))]
mod emoji;
#[cfg(not(feature = "notifier"))]
mod kept;
mod links;
#[cfg(not(feature = "notifier"))]
mod listing;
mod message;
mod names;
#[cfg(not(feature = "notifier"))]
mod notices;
mod notifier;
#[cfg(not(feature = "notifier"))]
mod queries;
#[cfg(not(feature = "notifier"))]
mod room;
#[cfg(not(feature = "notifier"))]
mod room_list;
#[cfg(not(feature = "notifier"))]
mod search;
#[cfg(not(feature = "notifier"))]
mod session;
#[cfg(not(feature = "notifier"))]
mod state;
#[cfg(not(feature = "notifier"))]
mod ui;
#[cfg(not(feature = "notifier"))]
mod watch;
mod words;

pub use notifier::ChatNotices;
#[cfg(feature = "notifier")]
view::export_notifier!(ChatNotices);

#[cfg(not(feature = "notifier"))]
pub use state::*;

#[cfg(not(feature = "notifier"))]
use view::prelude::*;

/// Rows asked per page.
#[cfg(not(feature = "notifier"))]
const PAGE: usize = 64;
/// Rows a room keeps on screen before paging older.
#[cfg(not(feature = "notifier"))]
const WINDOW: usize = 256;
#[cfg(not(feature = "notifier"))]
const SPAN: usize = WINDOW;

#[cfg(not(feature = "notifier"))]
impl View for Chat {
    const NAME: &'static str = words::CHAT;
    const DESCRIPTION: &'static str =
        "Channels, direct messages, threads, search and the live call of this workspace.";
    const ICON: &'static str = "icons/messages-square.svg";
    const TINT: Tint = Tint::Blue;
    const CAPABILITIES: &'static [Capability] = &[
        // the heads' time: which day "Today" is
        Capability::Chain,
        Capability::Program,
        Capability::Op,
        Capability::Host,
        Capability::Link,
        Capability::Clipboard,
        Capability::Notify,
        Capability::Store,
    ];
    const TARGETS: &'static [&'static str] = &[chat::PROGRAM, identity::PROGRAM, access::PROGRAM];
    // at 480 an agent message's head line is cut at the room's edge
    const MIN_WINDOW_WIDTH: u32 = 560;
    // the room list, the room and a thread open beside it, none squeezed
    const WINDOW_WIDTH: Option<u32> = Some(1200);

    /// Follows the host and reads what the screen shows: on a first mount,
    /// and again after a snapshot, whose in-flight work it parks.
    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        // a send in flight when the snapshot was taken never came back:
        // park its body as a failed send the composer can restore
        for draft in self.drafts.values_mut() {
            draft.retire_device_requests();
        }
        self.menu = None;
        self.copy = None;
        self.watch(cx);
        if self.names.is_idle() {
            self.load_names(cx);
        }
        if self.channels.is_idle() {
            self.load_channels(cx);
        }
        if let Some(room) = &self.room {
            let (id, thread) = (room.id.clone(), room.thread.as_ref().map(|t| t.root));
            self.open(id, cx);
            if let Some(root) = thread {
                self.reply_in_thread(root, cx);
            }
        }
        if !self.search.query.is_empty() {
            self.search_now(cx);
        }
    }
}

#[cfg(not(feature = "notifier"))]
impl Render for Chat {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // the pane's size, as the host lays the frame out: the panes
        // clamp to it before the frame that shows them
        let size = window.viewport_size();
        self.layout.viewport = (size.width.into(), size.height.into());
        self.seat_drafts();
        // names land async: the frame is the one place that sees each
        self.settle_title(cx);
        self.settle_showing(cx);
        // the one pick: a phone draws the Expo app's own screens, every
        // other window the desktop's (STRUCTURE v2)
        if window.touch() {
            ui::phone::render(self, cx)
        } else {
            ui::render(self, cx).into_any_element()
        }
    }
}

#[cfg(not(feature = "notifier"))]
export_view!(Chat);

#[cfg(all(test, not(feature = "notifier")))]
mod tests;
