//! What Account follows while it is open: the session, and the live heads
//! of identity, valset, access and ownership. Every follower says what a refusal means to it;
//! none ends on one.
use view::prelude::*;

use crate::Settings;
use access::Access;
use identity::Identity;
use ownership::Ownership;
use valset::Valset;

impl Settings {
    pub(crate) fn watch(&mut self, cx: &mut Context<Self>) {
        cx.follow::<HostSession>((), |view, session, cx| match session {
            Ok(session) => view.session_changed(session, cx),
            Err(refusal) => {
                view.account = Loadable::Failed(refusal);
                cx.notify();
            }
        })
        .detach();
        cx.follow::<Changes<Identity>>((), |view, head, cx| match head {
            Ok(_) => view.refresh_account(cx),
            Err(refusal) => cx.log_refused("identity's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<Valset>>((), |view, head, cx| match head {
            Ok(_) => view.refresh_account(cx),
            Err(refusal) => cx.log_refused("valset's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<Access>>((), |view, head, cx| match head {
            Ok(_) => view.refresh_account(cx),
            Err(refusal) => cx.log_refused("access's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<Ownership>>((), |view, head, cx| match head {
            Ok(change) => view.ownership_moved(change, cx),
            Err(refusal) => cx.log_refused("ownership's live heads", &refusal),
        })
        .detach();
    }
}
