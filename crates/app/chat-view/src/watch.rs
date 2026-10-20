//! What chat follows while it is open: the session, routes opened into
//! it, whether it is on screen, and the live heads of chat, identity and ownership.
//! Every follower says what a refusal means to it; none ends on one.
use view::prelude::*;

use crate::{Chat, links};
use identity::Identity;

impl Chat {
    pub(crate) fn watch(&mut self, cx: &mut Context<Self>) {
        cx.follow::<HostSession>((), |chat, props, cx| match props {
            Ok(next) => chat.session_changed(next, cx),
            Err(refusal) => {
                chat.notice = format!("Couldn’t read the session: {}", design::sentence(&refusal));
                cx.notify();
            }
        })
        .detach();
        cx.follow::<Changes<::chat::Chat>>((), |chat, change, cx| match change {
            Ok(change) => chat.changed(change.as_ref(), cx),
            Err(refusal) => cx.log_refused("chat's live heads", &refusal),
        })
        .detach();
        cx.follow::<HostRoute>((), |chat, route, cx| match route {
            Ok(route) => {
                if let Some((channel, seq)) = links::route_target(&route) {
                    chat.search_clear();
                    chat.open_at(channel, seq, cx);
                    cx.notify();
                }
            }
            Err(refusal) => cx.log_refused("the route", &refusal),
        })
        .detach();
        cx.follow::<ChainHeads>((), |chat, head, cx| match head {
            Ok(head) => {
                chat.widgets_ticked(head.time, cx);
                let today = Some(crate::message::day_of(head.time));
                if chat.today != today {
                    chat.today = today;
                    cx.notify();
                }
            }
            Err(refusal) => cx.log_refused("the chain's heads", &refusal),
        })
        .detach();
        cx.follow::<HostVisible>((), |chat, visible, cx| match visible {
            Ok(visible) => chat.visibility_changed(visible, cx),
            Err(refusal) => cx.log_refused("visibility", &refusal),
        })
        .detach();
        cx.follow::<Changes<Identity>>((), |chat, head, cx| match head {
            Ok(_) => {
                chat.load_names(cx);
                chat.load_avatars(cx);
            }
            Err(refusal) => cx.log_refused("identity's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<ownership::Ownership>>((), |chat, head, cx| match head {
            Ok(change) => chat.ownership_moved(change, cx),
            Err(refusal) => cx.log_refused("ownership's live heads", &refusal),
        })
        .detach();
    }
}
