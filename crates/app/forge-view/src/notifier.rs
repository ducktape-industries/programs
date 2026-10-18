//! What forge hands the host's notification centre, decided here and
//! nowhere else, from what forge says the reader owes (`Query::Judgment`):
//! a change that asks the reader's review ("Your review is requested") and
//! a reply landing in a thread the reader started on a change ("New reply
//! to you"). Its body is the change, `<repo> #<n>: <title>`; its tag the
//! change, `<repo>#<n>`, so both fold together; its link the change. No
//! badge: the Changes tab's "Needs my judgment" is where the reader looks.
//!
//! This is forge's NOTIFIER: the host runs it once per network, whether or
//! not a forge pane is open. A change a pane shows on screen (`host.screen`)
//! is in front of the reader: nothing about it is announced.
//!
//! What it announced it keeps under `notified/<signer>`: per change, whether
//! a review was asked and the newest reply. A run that starts posts, once,
//! what moved while nothing ran; the very first run on a device has nothing
//! kept, and what the reader owes then is history, not news. A change that
//! stops asking (reviewed, closed) drops out, so a push that asks again is
//! announced again.
use std::collections::{BTreeMap, BTreeSet};

use forge::{Judgment, PageRequest, Principal, Query, Reply};
use view::host::{all_pages, malformed};
use view::prelude::*;
use view::{Notifier, store};

/// Forge's one read method.
type Ask = view::methods::Query<forge::Forge>;

/// Where the notifier keeps what it announced, change by change.
fn notified_key(signer: &str) -> String {
    format!("notified/{signer}")
}

/// What was announced of one change: a review asked, and the seq of the
/// newest reply to the reader (0 for none).
type Announced = (bool, u64);

/// Forge's notifier.
#[derive(Default)]
pub struct ForgeNotices {
    session: Session,
    /// Moves with every new reader: what lands for an earlier one is dropped.
    run: u64,
    /// What was announced; `None` until read off the store, `Some(None)`
    /// when nothing was ever kept on this device.
    announced: Option<Option<BTreeMap<String, Announced>>>,
    /// What the reader owes, as the last read said.
    owed: Option<Vec<Judgment>>,
    /// A read is out; `again` when a block landed meanwhile.
    reading: bool,
    again: bool,
    /// The changes a forge pane shows on screen now.
    screen: BTreeSet<(String, u64)>,
}

impl Notifier for ForgeNotices {
    const NAME: &'static str = "Forge";
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Program,
        Capability::Host,
        Capability::Notify,
        Capability::Store,
    ];

    fn new(cx: &mut Context<Self>) -> Self {
        cx.follow::<HostSession>((), |notices, session, cx| match session {
            Ok(session) => notices.session_changed(session, cx),
            Err(refusal) => log_refused(cx, "the session", &refusal),
        })
        .detach();
        // what the reader owes is forge's, and the threads it waits on chat's
        cx.follow::<Changes<forge::Forge>>((), |notices, change, cx| match change {
            Ok(change) => {
                let touched = notices.owed_query().is_some_and(|query| {
                    change.is_none_or(|change| change.touches::<forge::Forge, _>(&query))
                });
                if touched {
                    notices.read(cx);
                }
            }
            Err(refusal) => log_refused(cx, "forge's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<chat::Chat>>((), |notices, change, cx| match change {
            Ok(change) => {
                let touched = notices.owed_query().is_some_and(|query| {
                    change.is_none_or(|change| change.touches::<chat::Chat, _>(&query))
                });
                if touched {
                    notices.read(cx);
                }
            }
            Err(refusal) => log_refused(cx, "chat's live heads", &refusal),
        })
        .detach();
        cx.follow::<HostScreen>((), |notices, routes, cx| match routes {
            Ok(routes) => {
                notices.screen = routes.iter().filter_map(|route| on_screen(route)).collect();
                notices.settle(cx);
            }
            Err(refusal) => log_refused(cx, "the screen", &refusal),
        })
        .detach();
        Self::default()
    }
}

fn log_refused(cx: &mut Context<ForgeNotices>, what: &str, refusal: &Error) {
    cx.host()
        .log(format!("{}: {what} refused: {refusal}", ForgeNotices::NAME));
}

/// The change a forge route shows: `<repo>/<n>`.
fn on_screen(route: &str) -> Option<(String, u64)> {
    let (repo, n) = route.split_once('/')?;
    Some((repo.to_owned(), n.parse().ok()?))
}

impl ForgeNotices {
    fn me(&self) -> Option<Principal> {
        Principal::writer(self.session.account)
    }

    /// What the reader owes, asked of forge from its first page.
    fn owed_query(&self) -> Option<Query> {
        Some(Query::Judgment {
            principal: self.me()?,
            page: PageRequest::default(),
        })
    }

    fn session_changed(&mut self, next: Session, cx: &mut Context<Self>) {
        let reader = (&next.signer, &next.chain_id, next.account, next.connected);
        let was = (
            &self.session.signer,
            &self.session.chain_id,
            self.session.account,
            self.session.connected,
        );
        if reader == was {
            self.session = next;
            return;
        }
        // another reader, network or account: nothing known carries over
        *self = Self {
            session: next,
            run: self.run + 1,
            screen: std::mem::take(&mut self.screen),
            ..Self::default()
        };
        if !self.session.connected || self.me().is_none() {
            return;
        }
        let kept = store::get::<BTreeMap<String, Announced>>(
            &cx.host(),
            &notified_key(&self.session.signer),
        );
        let run = self.run;
        cx.land(kept, move |notices, kept, cx| {
            if notices.run != run {
                return;
            }
            let kept = kept.unwrap_or_else(|refusal| {
                log_refused(cx, "what was announced", &refusal);
                None
            });
            notices.announced = Some(kept);
            notices.settle(cx);
        })
        .detach();
        self.read(cx);
    }

    /// Reads what the reader owes, every page; one read at a time, and a
    /// block that lands meanwhile reads again after it.
    fn read(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.me() else { return };
        if std::mem::replace(&mut self.reading, true) {
            self.again = true;
            return;
        }
        let host = cx.host();
        let owed = all_pages(move |after| {
            let ask = host.ask::<Ask>(Query::Judgment {
                principal: me.clone(),
                page: PageRequest { after, limit: None },
            });
            async move {
                match ask.await? {
                    Reply::Judgment { page, .. } => Ok((page.items, page.next)),
                    _ => Err(malformed(
                        "forge answered a judgment with another reply".into(),
                    )),
                }
            }
        });
        let run = self.run;
        cx.land(owed, move |notices, owed, cx| {
            if notices.run != run {
                return;
            }
            notices.reading = false;
            match owed {
                Ok(owed) => {
                    notices.owed = Some(owed);
                    notices.settle(cx);
                }
                Err(refusal) => log_refused(cx, "what the reader owes", &refusal),
            }
            if std::mem::take(&mut notices.again) {
                notices.read(cx);
            }
        })
        .detach();
    }

    /// Both known: what moved since it was announced is posted, unless its
    /// change is on screen, and what is announced is kept.
    fn settle(&mut self, cx: &mut Context<Self>) {
        let (Some(owed), Some(kept)) = (&self.owed, &self.announced) else {
            return;
        };
        // the first run on this device: what is owed now is history
        let first = kept.is_none();
        let mut announced = BTreeMap::new();
        let mut posts = Vec::new();
        for judgment in owed {
            let change = &judgment.change;
            let key = tag(&change.repo, change.n);
            let now = (
                judgment.requested,
                judgment
                    .replies
                    .as_ref()
                    .map_or(0, |reply| reply.last_reply_seq),
            );
            let was = kept
                .as_ref()
                .and_then(|kept| kept.get(&key))
                .copied()
                .unwrap_or_default();
            announced.insert(key, now);
            if first || self.screen.contains(&(change.repo.clone(), change.n)) {
                continue;
            }
            if now.0 && !was.0 {
                posts.push(self.notice(judgment, "Your review is requested"));
            }
            if now.1 > was.1 {
                posts.push(self.notice(judgment, "New reply to you"));
            }
        }
        let moved = Some(&announced) != kept.as_ref();
        self.announced = Some(Some(announced));
        for post in posts {
            cx.host().notify::<NotifyPost>(post);
        }
        if moved && let Some(Some(announced)) = &self.announced {
            store::set(
                &cx.host(),
                &notified_key(&self.session.signer),
                Some(announced),
            );
        }
    }

    fn notice(&self, judgment: &Judgment, title: &str) -> Notification {
        let change = &judgment.change;
        let n = change.n.to_string();
        Notification {
            title: title.into(),
            body: format!("{} #{n}: {}", change.repo, change.title),
            tag: tag(&change.repo, change.n),
            // a notice without a link is one the centre cannot open, not a bad one
            link: ducklink::mint(&self.session.chain_id, forge::PROGRAM, &[&change.repo, &n])
                .unwrap_or_default(),
            ..Notification::default()
        }
    }
}

/// The tag a change's notices go under.
fn tag(repo: &str, n: u64) -> String {
    format!("{repo}#{n}")
}

#[cfg(test)]
mod tests;
