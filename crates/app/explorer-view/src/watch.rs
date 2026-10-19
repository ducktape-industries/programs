//! What Explorer follows while it is open: the chain's heads, the session,
//! links opened into it, and the live heads of the programs whose lists it
//! shows. Every follower says what a refusal means to it; none ends on one.
use activity::Activity;
use identity::Identity;
use programs::Programs;
use valset::Valset;
use view::prelude::*;

use crate::Explorer;

impl Explorer {
    /// Follows each of them for as long as the view runs.
    pub(crate) fn watch(&mut self, cx: &mut Context<Self>) {
        // A head is not drawn on its own: it is drawn with the page it
        // brings (`at_head`). The host keeps the stream across a
        // reconnect, opened again at the new node's tip.
        cx.follow::<ChainHeads>((), |view, head, cx| match head {
            Ok(head) => view.at_head(head, cx),
            Err(refusal) => cx.log_refused("the chain's heads", &refusal),
        })
        .detach();
        // the chain links are minted in, and the reader the Programs page
        // offers its presses to
        cx.follow::<HostSession>((), |view, session, cx| match session {
            Ok(session) => view.session_changed(session, cx),
            Err(refusal) => cx.log_refused("the session", &refusal),
        })
        .detach();
        // `duck://<chain>/explorer/<route>`
        cx.follow::<HostRoute>((), |view, route, cx| match route {
            Ok(route) => view.open_route(&route, cx),
            Err(refusal) => cx.log_refused("the route", &refusal),
        })
        .detach();
        // an account made, renamed or re-keyed, by anyone: a program's
        // `RegisterProgram` included, which no transaction targets
        cx.follow::<Changes<Identity>>((), |view, head, cx| match head {
            Ok(_) => view.read_accounts(cx),
            Err(refusal) => cx.log_refused("identity's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<Valset>>((), |view, head, cx| match head {
            Ok(_) => {
                view.read_validators(cx);
                view.read_voices(cx);
            }
            Err(refusal) => cx.log_refused("valset's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<Programs>>((), |view, head, cx| match head {
            Ok(_) => view.read_network(cx),
            Err(refusal) => cx.log_refused("the programs role's live heads", &refusal),
        })
        .detach();
        // a write to activity: the open account's listing asks its first
        // page again, read in place or telling it is behind
        // (`Explorer::reread_first`); the counts are read again
        cx.follow::<Changes<Activity>>((), |view, head, cx| match head {
            Ok(_) => {
                view.read_used(cx);
                view.read_acted(cx);
            }
            Err(refusal) => cx.log_refused("activity's live heads", &refusal),
        })
        .detach();
    }
}
