//! What Files follows while it is open: the session (who reads, on which
//! chain), the links opened into it, and every block that wrote to `files`,
//! access or identity. Every follower says what a refusal means to it; none
//! ends on one.
use identity::Identity;
use view::prelude::*;

use crate::Files;

impl Files {
    /// Follows each of them for as long as the view runs.
    pub(crate) fn watch(&mut self, cx: &mut Context<Self>) {
        cx.follow::<HostSession>((), |view, session, cx| match session {
            Ok(session) => view.session_changed(session, cx),
            Err(refusal) => cx.log_refused("the session", &refusal),
        })
        .detach();
        // `duck://<chain>/files/<path>`: a link opened into this view
        cx.follow::<HostRoute>((), |view, route, cx| match route {
            Ok(route) => view.open_link(&route, cx),
            Err(refusal) => cx.log_refused("the route", &refusal),
        })
        .detach();
        // a commit or a pin, by anyone: everything on screen reads again
        cx.follow::<Changes<files::Files>>((), |view, change, cx| match change {
            Ok(_) => view.read_all(cx),
            Err(refusal) => cx.log_refused("files' live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<access::Access>>((), |view, change, cx| match change {
            Ok(_) => view.read_all(cx),
            Err(refusal) => cx.log_refused("access's live heads", &refusal),
        })
        .detach();
        // an account named or renamed: its home and its snapshots' author
        cx.follow::<Changes<Identity>>((), |view, change, cx| match change {
            Ok(_) => {
                view.read_reader(cx);
                view.read_names(cx);
            }
            Err(refusal) => cx.log_refused("identity's live heads", &refusal),
        })
        .detach();
    }
}
