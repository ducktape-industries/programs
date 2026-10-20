//! Forge: repositories, code, commits, refs and the Changes a reviewer lives in, on the view
//! `View` shape. The forge program answers everything this screen shows, in borsh, through one
//! method (`program.query`); conversation is chat's, through the method chat-view uses.
//
// Reads are a cache keyed by the query itself: `sync` asks what the current screen needs,
// issues what is missing, and drops what the reader has navigated away from. `render` writes
// only what the frame learns, the pane's width and the window's title last sent — what an event
// changes lands in `actions`.
//
// - `state`: what the view holds; `select`: what the screens read out of it.
// - `sync`: which reads the screen needs; `queries`: how one read is asked.
// - `navigate`, `actions`, `review`, `tree`: what an event changes.
// - `words`: every sentence both designs say; `markdown`: where a document's links go; `blob`:
//   the open file's lines, tokens and blocks, read once for either design (the SDK's `markdown`
//   crate parses, highlights and draws them); `list`: a list as one Tab stop.
// - `ui`: the desktop's screens; `ui::phone`: the phone's (the Expo app's).
//
// What becomes a notice is forge's NOTIFIER's (`notifier.rs`): the host runs it once per
// network, open panes or none. The `notifier` feature builds it alone in place of the view.
mod notifier;
#[cfg(not(feature = "notifier"))]
mod policies;
pub use notifier::ForgeNotices;
#[cfg(feature = "notifier")]
view::export_notifier!(ForgeNotices);

#[cfg(not(feature = "notifier"))]
mod api;
#[cfg(not(feature = "notifier"))]
mod blob;
#[cfg(not(feature = "notifier"))]
mod list;
#[cfg(not(feature = "notifier"))]
mod markdown;
#[cfg(not(feature = "notifier"))]
mod queries;
#[cfg(not(feature = "notifier"))]
mod select;
#[cfg(not(feature = "notifier"))]
mod state;
#[cfg(not(feature = "notifier"))]
mod sync;
#[cfg(not(feature = "notifier"))]
mod tree;
#[cfg(not(feature = "notifier"))]
mod words;

#[cfg(not(feature = "notifier"))]
mod actions;
#[cfg(not(feature = "notifier"))]
mod navigate;
#[cfg(not(feature = "notifier"))]
mod review;
#[cfg(not(feature = "notifier"))]
mod ui;

#[cfg(not(feature = "notifier"))]
use view::prelude::*;

#[cfg(not(feature = "notifier"))]
use identity::Identity;
#[cfg(not(feature = "notifier"))]
pub(crate) use select::Stage;
#[cfg(not(feature = "notifier"))]
pub use state::Forge;

#[cfg(not(feature = "notifier"))]
impl View for Forge {
    const NAME: &'static str = "Forge";
    const DESCRIPTION: &'static str =
        "Repositories, code, commits and the changes waiting on your judgment.";
    const ICON: &'static str = "icons/hammer.svg";
    const TINT: Tint = Tint::Orange;
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Program,
        Capability::Op,
        Capability::Host,
        Capability::Link,
        Capability::Clipboard,
        // each card's picture (`blob.get`)
        Capability::Blob,
    ];
    const MIN_WINDOW_WIDTH: u32 = 640;

    /// Every stream this view follows, on a first mount and after a
    /// snapshot. A refused item says so in the notice.
    fn attach(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        cx.follow::<HostSession>((), |forge, item, cx| match item {
            Ok(session) => forge.session_changed(session, cx),
            Err(refusal) => {
                forge.notice = format!("Couldn’t read the session: {}", design::sentence(&refusal));
                cx.notify();
            }
        })
        .detach();
        cx.follow::<HostRoute>((), |forge, route, cx| match route {
            Ok(route) => forge.open_route(&route, cx),
            Err(refusal) => {
                forge.notice = format!("Couldn’t follow the link: {}", design::sentence(&refusal));
                cx.notify();
            }
        })
        .detach();
        cx.follow::<Changes<forge::Forge>>((), |forge, change, cx| match change {
            Ok(change) => forge.reconcile(change.as_ref(), cx),
            Err(refusal) => cx.log_refused("forge's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<::chat::Chat>>((), |forge, change, cx| match change {
            Ok(change) => forge.chat_changed(change.as_ref(), cx),
            Err(refusal) => cx.log_refused("chat's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<Identity>>((), |forge, change, cx| match change {
            Ok(_) => forge.reread_names(cx),
            Err(refusal) => cx.log_refused("identity's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<access::Access>>((), |forge, change, cx| match change {
            Ok(_) => forge.reread_writers(cx),
            Err(refusal) => cx.log_refused("access's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<ownership::Ownership>>((), |forge, change, cx| match change {
            Ok(change) => forge.ownership_moved(change, cx),
            Err(refusal) => cx.log_refused("ownership's live heads", &refusal),
        })
        .detach();
        cx.follow::<HostVisible>((), |forge, shown, cx| match shown {
            Ok(true) => {
                forge.hidden = false;
                forge.refresh(cx)
            }
            Ok(false) => forge.hidden = true,
            Err(refusal) => cx.log_refused("visibility", &refusal),
        })
        .detach();
        if self.names.is_idle() {
            cx.load(self, queries::roster(cx.host()), |forge| &mut forge.names);
        }
        self.read_avatars(cx);
        self.sync(cx);
    }
}

#[cfg(not(feature = "notifier"))]
impl Render for Forge {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        self.layout.width = viewport.width.into();
        self.layout.height = viewport.height.into();
        self.settle_title(cx);
        self.settle_showing(cx);
        self.settle_policies(cx);
        if self.touch != window.touch() {
            self.touch = window.touch();
            self.sync(cx);
        }
        if self.touch {
            ui::phone::render(self, cx)
        } else {
            ui::render(self, cx).into_any_element()
        }
    }
}

#[cfg(not(feature = "notifier"))]
pub(crate) fn id(text: impl Into<String>) -> ElementId {
    ElementId::Name(text.into().into())
}

#[cfg(not(feature = "notifier"))]
impl Forge {
    fn settle_title(&mut self, cx: &mut Context<Self>) {
        let title = match (&self.nav.repo, self.nav.change) {
            (None, _) => "Repositories".to_owned(),
            (Some(repo), None) => repo.clone(),
            (Some(repo), Some(n)) => match self.change() {
                Some((change, ..)) => format!("{repo} · #{n} {}", change.title),
                None => format!("{repo} · #{n}"),
            },
        };
        if self.title.as_deref() != Some(title.as_str()) {
            cx.host().notify::<HostTitle>(title.clone());
            self.title = Some(title);
        }
    }

    fn settle_showing(&mut self, cx: &mut Context<Self>) {
        let showing = match (&self.nav.repo, self.nav.change) {
            (None, _) => String::new(),
            (Some(repo), None) => repo.clone(),
            (Some(repo), Some(n)) => format!("{repo}/{n}"),
        };
        if self.showing.as_ref() != Some(&showing) {
            cx.host().notify::<HostShowing>(showing.clone());
            self.showing = Some(showing);
        }
    }
}

#[cfg(not(feature = "notifier"))]
export_view!(Forge);

#[cfg(test)]
#[cfg(not(feature = "notifier"))]
mod tests;
