//! Forge's notifier: what forge hands the host's notification centre, from
//! the reviews forge says the reader is asked for (`Query::ReviewRequests`),
//! asked again only when a forge block can have moved them.
// One notice, "Your review is requested", for a change that asks the reader's
// review at its head. Its body is the change, `<repo> #<n>: <title>`; its tag
// the change, `<repo>#<n>`; its link the change. No badge: the Changes tab's
// "Needs my judgment" is where the reader looks. Replies are chat's to tell.
//
// This is forge's NOTIFIER: the host runs it once per network, whether or
// not a forge pane is open. A change a pane shows on screen (`host.screen`)
// is in front of the reader: nothing about it is announced.
//
// What it announced it keeps under `asked/<signer>`: the changes that asked.
// A run that starts posts, once, what moved while nothing ran; the very first
// run on a device has nothing kept, and what the reader is asked for then is
// history, not news. A change that stops asking (reviewed, closed) drops out,
// so a push that asks again is announced again.
use std::collections::BTreeSet;

use forge::{ChangeSummary, PageRequest, Principal, Query, Reply};
use view::host::{all_pages, malformed};
use view::prelude::*;
use view::{Notifier, store};

type Ask = view::methods::Query<forge::Forge>;

fn asked_key(signer: &str) -> String {
    format!("asked/{signer}")
}

/// Forge's notifier.
#[derive(Default)]
pub struct ForgeNotices {
    session: Session,
    run: u64,
    announced: Option<Option<BTreeSet<String>>>,
    asked: Option<Vec<ChangeSummary>>,
    reading: bool,
    again: bool,
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
        cx.follow::<Changes<forge::Forge>>((), |notices, change, cx| match change {
            Ok(change) => {
                let touched = notices.asked_query().is_some_and(|query| {
                    change.is_none_or(|change| change.touches::<forge::Forge, _>(&query))
                });
                if touched {
                    notices.read(cx);
                }
            }
            Err(refusal) => log_refused(cx, "forge's live heads", &refusal),
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

fn on_screen(route: &str) -> Option<(String, u64)> {
    let (repo, n) = route.split_once('/')?;
    Some((repo.to_owned(), n.parse().ok()?))
}

impl ForgeNotices {
    fn me(&self) -> Option<Principal> {
        Principal::writer(self.session.account)
    }

    fn asked_query(&self) -> Option<Query> {
        Some(Query::ReviewRequests {
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
        *self = Self {
            session: next,
            run: self.run + 1,
            screen: std::mem::take(&mut self.screen),
            ..Self::default()
        };
        if !self.session.connected || self.me().is_none() {
            return;
        }
        let kept = store::get::<BTreeSet<String>>(&cx.host(), &asked_key(&self.session.signer));
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

    fn read(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.me() else { return };
        if std::mem::replace(&mut self.reading, true) {
            self.again = true;
            return;
        }
        let host = cx.host();
        let asked = all_pages(move |after| {
            let ask = host.ask::<Ask>(Query::ReviewRequests {
                principal: me.clone(),
                page: PageRequest { after, limit: None },
            });
            async move {
                match ask.await? {
                    Reply::ReviewRequests { page, .. } => Ok((page.items, page.next)),
                    _ => Err(malformed(
                        "forge answered review requests with another reply".into(),
                    )),
                }
            }
        });
        let run = self.run;
        cx.land(asked, move |notices, asked, cx| {
            if notices.run != run {
                return;
            }
            notices.reading = false;
            match asked {
                Ok(asked) => {
                    notices.asked = Some(asked);
                    notices.settle(cx);
                }
                Err(refusal) => log_refused(cx, "the reviews the reader is asked for", &refusal),
            }
            if std::mem::take(&mut notices.again) {
                notices.read(cx);
            }
        })
        .detach();
    }

    fn settle(&mut self, cx: &mut Context<Self>) {
        let (Some(asked), Some(kept)) = (&self.asked, &self.announced) else {
            return;
        };
        let first = kept.is_none();
        let mut announced = BTreeSet::new();
        let mut posts = Vec::new();
        for change in asked {
            let key = tag(&change.repo, change.n);
            let was = kept.as_ref().is_some_and(|kept| kept.contains(&key));
            announced.insert(key);
            let shown = self.screen.contains(&(change.repo.clone(), change.n));
            let quiet = first || shown || was;
            if !quiet {
                posts.push(self.notice(change));
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
                &asked_key(&self.session.signer),
                Some(announced),
            );
        }
    }

    fn notice(&self, change: &ChangeSummary) -> Notification {
        let n = change.n.to_string();
        Notification {
            title: "Your review is requested".into(),
            body: format!("{} #{n}: {}", change.repo, change.title),
            tag: tag(&change.repo, change.n),
            link: ducklink::mint(&self.session.chain_id, forge::PROGRAM, &[&change.repo, &n])
                .unwrap_or_default(),
            ..Notification::default()
        }
    }
}

fn tag(repo: &str, n: u64) -> String {
    format!("{repo}#{n}")
}

#[cfg(test)]
mod tests;
