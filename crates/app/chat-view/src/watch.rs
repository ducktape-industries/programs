//! What chat follows while it is open: the session, routes opened into
//! it, whether it is on screen, and the live heads of chat, identity, access and ownership.
//! Every follower says what a refusal means to it; none ends on one.
use view::prelude::*;

use crate::{Chat, links};
use identity::Identity;

impl Chat {
    /// Follows each of them for as long as the view runs.
    pub(crate) fn watch(&mut self, cx: &mut Context<Self>) {
        cx.follow::<HostSession>((), |chat, props, cx| match props {
            Ok(next) => chat.session_changed(next, cx),
            Err(refusal) => {
                chat.notice = format!("Couldn’t read the session: {}", design::sentence(&refusal));
                cx.notify();
            }
        })
        .detach();
        // a block re-reads what it wrote to, as the program declares it
        cx.follow::<Changes<::chat::Chat>>((), |chat, change, cx| match change {
            Ok(change) => chat.changed(change.as_ref(), cx),
            Err(refusal) => cx.log_refused("chat's live heads", &refusal),
        })
        .detach();
        // `duck://<chain>/chat/<channel>[/<seq>]`: a link opened into
        // this view (a notice's, say) names the room and the message
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
        // the chain's clock: which day "Today" is (a day separator's word
        // under a finger); a new frame only when the day turns
        cx.follow::<ChainHeads>((), |chat, head, cx| match head {
            Ok(head) => {
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
        // re-read the roster on identity's heads, so a name another
        // signer claims replaces its "account N" fallback
        cx.follow::<Changes<Identity>>((), |chat, head, cx| match head {
            Ok(_) => {
                chat.load_names(cx);
                chat.load_avatars(cx);
            }
            Err(refusal) => cx.log_refused("identity's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<access::Access>>((), |chat, head, cx| match head {
            Ok(_) => chat.load_names(cx),
            Err(refusal) => cx.log_refused("access's live heads", &refusal),
        })
        .detach();
        // who can edit the open room or message may be among what it wrote
        cx.follow::<Changes<ownership::Ownership>>((), |chat, head, cx| match head {
            Ok(change) => chat.ownership_moved(change, cx),
            Err(refusal) => cx.log_refused("ownership's live heads", &refusal),
        })
        .detach();
    }
}
