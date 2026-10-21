//! Which reads the screen on display needs, and keeping them fresh. Every
//! read is cached under the `Query` that asked it; `sync` issues what the
//! screen lacks and drops what it no longer shows. A log is held a page at
//! a time (`logs`), every other read whole (`data`).
use std::collections::BTreeSet;

use view::Paged;
use view::prelude::*;

use crate::queries::{self, PAGE};
use crate::state::{ChangeTab, Filter, Forge, Progress, RepoTab, Talk};
use forge::{ChangeFilter, ChangeState, PageRequest, Query, Reply};
use view::methods::HostBadge;

impl Forge {
    pub(crate) fn session_changed(&mut self, next: Session, cx: &mut Context<Self>) {
        if next == self.session {
            return;
        }
        let reader_changed = next.signer != self.session.signer;
        self.session = next;
        if reader_changed {
            self.names = Loadable::Idle;
            cx.load(self, queries::roster(cx.host()), |forge| &mut forge.names);
            self.data.clear();
            self.writers.clear();
            self.logs.clear();
            self.rereading.clear();
        }
        cx.notify();
        self.sync(cx);
    }

    pub(crate) fn read(&mut self, query: Query, cx: &mut Context<Self>) {
        if self.data.contains_key(&query) || self.logs.contains_key(&query) {
            return;
        }
        if matches!(query, Query::Log { .. }) {
            let (host, asked) = (cx.host(), query.clone());
            let page = move |after| queries::log_page(host.clone(), asked.clone(), after);
            let log = cx.new(|cx| Paged::new(page, cx));
            let landed = cx.observe(&log, |forge, _, cx| forge.sync(cx));
            self.logs.insert(query, (log, landed));
            return;
        }
        let landing = query.clone();
        let asked = query.clone();
        let fetch = queries::fetch(cx.host(), asked);
        let task = cx.land(fetch, move |forge, result, cx| {
            forge.data.insert(landing, Loadable::from(result));
            cx.notify();
            forge.sync(cx);
        });
        self.data.insert(query, Loadable::Loading(task));
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.reread(|_| true, cx);
        self.reread_conversations(None, cx);
        self.reread_writers(cx);
    }

    pub(crate) fn reread_writers(&mut self, cx: &mut Context<Self>) {
        if self.hidden {
            return;
        }
        let again: Vec<String> = self
            .writers
            .iter()
            .filter(|(_, slot)| !matches!(slot, Loadable::Idle | Loadable::Loading(_)))
            .map(|(repo, _)| repo.clone())
            .collect();
        for repo in again {
            let list = queries::writers(cx.host(), repo.clone());
            cx.load(self, list, move |forge| {
                forge.writers.entry(repo.clone()).or_default()
            });
        }
    }

    pub(crate) fn retry_writers(&mut self, cx: &mut Context<Self>) {
        self.writers.clear();
        self.sync(cx);
        cx.notify();
    }

    pub(crate) fn reconcile(&mut self, change: Option<&Change>, cx: &mut Context<Self>) {
        if let Some(change) = change {
            self.block = self.block.max(change.height);
        }
        let pending = self.pending.len();
        self.pending
            .retain(|op| !matches!(op.progress, Progress::Accepted));
        if self.pending.len() != pending {
            cx.notify();
        }
        self.reread(
            |query| change.is_none_or(|change| change.touches::<forge::Forge, _>(query)),
            cx,
        );
    }

    pub(crate) fn chat_changed(&mut self, change: Option<&Change>, cx: &mut Context<Self>) {
        self.reread(
            |query| change.is_none_or(|change| change.touches::<chat::Chat, _>(query)),
            cx,
        );
        self.reread_conversations(change, cx);
    }

    // Asks again for the reads on screen that `touched` names, keeping what is there until the
    // fresh answer lands; a read that lands what is there draws nothing. Each read lives in its
    // slot, or beside the refusal it shows, so a screen the reader leaves drops its reads with it.
    // A read still out is left to land: its landing runs `sync`, and the next block asks it again.
    // A refusal stays on screen, saying why, until a block writes what it reads or the reader
    // retries: a block that wrote elsewhere does not ask it again. Out of sight nothing is asked.
    // A log reads the pages it holds again, not its whole history.
    fn reread(&mut self, touched: impl Fn(&Query) -> bool, cx: &mut Context<Self>) {
        if self.hidden {
            return;
        }
        for (query, (log, _)) in &self.logs {
            let out = log.read(|log| log.is_loading());
            if !out && touched(query) {
                log.update(cx, |log, cx| log.reread(cx));
            }
        }
        let again: Vec<(Query, bool)> = self
            .data
            .iter()
            .filter_map(|(query, slot)| match slot {
                Loadable::Idle | Loadable::Loading(_) => None,
                Loadable::Failed(_) => touched(query).then(|| (query.clone(), true)),
                Loadable::Ready(_) | Loadable::Reloading(..) => {
                    touched(query).then(|| (query.clone(), false))
                }
            })
            .collect();
        for (query, refused) in again {
            let work = queries::fetch(cx.host(), query.clone());
            let landing = query.clone();
            if refused {
                let task = cx.land(work, move |forge, result, cx| {
                    if replaces_refusal(forge.data.get_mut(&landing), result) {
                        cx.notify();
                        forge.sync(cx);
                    }
                });
                self.rereading.insert(query, task);
            } else {
                cx.load(self, work, move |forge| {
                    forge.data.entry(landing.clone()).or_insert(Loadable::Idle)
                });
            }
        }
        self.sync(cx);
    }

    pub(crate) fn reread_conversations(&mut self, change: Option<&Change>, cx: &mut Context<Self>) {
        if self.hidden {
            return;
        }
        let (host, viewer) = (cx.host(), self.viewer());
        let touched = |talk: &Talk| {
            change.is_none_or(|change| {
                change.touches::<chat::Chat, _>(&chat::ask::Roots {
                    channel_id: talk.channel.clone(),
                    viewer: viewer.clone(),
                    page: PageRequest::default(),
                })
            })
        };
        // named before any is asked, as in `reread`
        let again: Vec<(Talk, bool)> = self
            .messages
            .iter()
            .filter_map(|(channel, slot)| match slot {
                Loadable::Idle | Loadable::Loading(_) => None,
                Loadable::Failed(_) => touched(channel).then(|| (channel.clone(), true)),
                Loadable::Ready(_) | Loadable::Reloading(..) => {
                    touched(channel).then(|| (channel.clone(), false))
                }
            })
            .collect();
        for (channel, refused) in again {
            let rows = queries::conversation(host.clone(), channel.clone(), viewer.clone());
            let landing = channel.clone();
            if refused {
                let task = cx.land(rows, move |forge, result, cx| {
                    if replaces_refusal(forge.messages.get_mut(&landing), result) {
                        cx.notify();
                    }
                });
                self.rereading_messages.insert(channel, task);
            } else {
                cx.load(self, rows, move |forge| {
                    forge.messages.entry(landing.clone()).or_default()
                });
            }
        }
    }

    pub(crate) fn reread_names(&mut self, cx: &mut Context<Self>) {
        if self.hidden {
            return;
        }
        cx.load(self, queries::roster(cx.host()), |forge| &mut forge.names);
        self.read_avatars(cx);
    }

    // Each account's picture, read again; what is on screen stays until a
    // different set lands.
    pub(crate) fn read_avatars(&mut self, cx: &mut Context<Self>) {
        if self.pictures.is_none() {
            self.pictures = Some(cx.new(|_| view::pictures::Pictures::default()));
        }
        let host = cx.host();
        let read = identity::view::avatars(move |ask| host.query(ask));
        cx.land(read, |forge, read, cx| match read {
            Ok(avatars) => {
                if forge.avatars != avatars {
                    forge.avatars = avatars;
                    cx.notify();
                }
            }
            Err(refusal) => cx.log_refused("the pictures", &refusal),
        })
        .detach();
    }

    pub(crate) fn retry_conversation(&mut self, cx: &mut Context<Self>) {
        if let Some(channel) = self.open_talk() {
            self.rereading_messages.remove(&channel);
            self.messages.remove(&channel);
        }
        self.sync(cx);
        cx.notify();
    }

    pub(crate) fn retry(&mut self, query: Query, cx: &mut Context<Self>) {
        self.rereading.remove(&query);
        self.data.remove(&query);
        self.logs.remove(&query);
        self.read(query, cx);
        cx.notify();
    }

    pub(crate) fn sync(&mut self, cx: &mut Context<Self>) {
        if self.hidden {
            return;
        }
        self.land_link(cx);
        self.land_goto();
        self.land_reveal();
        let needed = self.needed();
        let keys: BTreeSet<&Query> = needed.iter().collect();
        self.data.retain(|query, _| keys.contains(query));
        self.diff_cache.drop_unshared();
        self.logs.retain(|query, _| keys.contains(query));
        self.rereading.retain(|query, _| keys.contains(query));
        for query in needed {
            self.read(query, cx);
        }
        self.settle_badge(cx);
        self.sync_writers(cx);
        let Some(channel) = self.open_talk() else {
            self.messages.clear();
            self.rereading_messages.clear();
            return;
        };
        if !self.messages.contains_key(&channel) {
            let conversation = queries::conversation(cx.host(), channel.clone(), self.viewer());
            let landing = channel.clone();
            cx.load(self, conversation, move |forge| {
                forge.messages.entry(landing.clone()).or_default()
            });
        }
    }

    fn sync_writers(&mut self, cx: &mut Context<Self>) {
        let open = self.nav.repo.clone();
        self.writers.retain(|repo, _| open.as_ref() == Some(repo));
        let Some(repo) = open else {
            return;
        };
        if self.writers.contains_key(&repo) {
            return;
        }
        let list = queries::writers(cx.host(), repo.clone());
        cx.load(self, list, move |forge| {
            forge.writers.entry(repo.clone()).or_default()
        });
    }

    fn settle_badge(&mut self, cx: &mut Context<Self>) {
        let waiting = self
            .judgment_query()
            .and_then(|query| match self.ready(&query)? {
                Reply::Judgment { page, .. } => Some(!page.items.is_empty()),
                _ => None,
            })
            .unwrap_or(false);
        let badge = i64::from(waiting);
        if self.badge != Some(badge) {
            self.badge = Some(badge);
            cx.host().notify::<HostBadge>(badge);
        }
    }

    fn needed(&self) -> Vec<Query> {
        let mut wanted = vec![Query::Repos { page: PAGE }];
        wanted.extend(self.judgment_query());
        let Some(repo) = self.nav.repo.clone() else {
            return wanted;
        };
        wanted.push(self.repo_query());
        wanted.push(self.archived_query());
        wanted.push(self.refs_query());
        wanted.extend(self.open_changes_query());
        if self.touch {
            wanted.extend(self.form_compare_query());
        }
        match self.nav.change {
            Some(n) => wanted.extend(self.change_reads(&repo, n)),
            None => wanted.extend(self.tab_reads(repo)),
        }
        wanted
    }

    fn tab_reads(&self, repo: String) -> Vec<Query> {
        let blob = |oid: String| Query::Blob {
            repo: repo.clone(),
            oid,
            range: None,
        };
        match self.nav.tab {
            RepoTab::Readme => {
                let readme = self.readme().map(|(_, oid)| blob(oid));
                self.tree_query(Vec::new())
                    .into_iter()
                    .chain(readme)
                    .collect()
            }
            RepoTab::Code => {
                let open = self.nav.blob.as_ref().map(|(_, oid)| blob(oid.clone()));
                self.tree_queries().into_iter().chain(open).collect()
            }
            RepoTab::Commits if self.tree_query(Vec::new()).is_none() => Vec::new(),
            RepoTab::Commits => {
                let open = self.nav.commit.as_deref();
                std::iter::once(self.log_query())
                    .chain(open.into_iter().flat_map(|oid| self.commit_reads(oid)))
                    .collect()
            }
            RepoTab::Changes => self.changes_query(&repo).into_iter().collect(),
            RepoTab::Refs => self.compares_query().into_iter().collect(),
            RepoTab::Settings => Vec::new(),
        }
    }

    fn commit_reads(&self, oid: &str) -> Vec<Query> {
        let mut wanted = Vec::new();
        let shown = self.shown_log().and_then(|log| self.logs.get(&log));
        let held = shown.is_none_or(|(log, _)| {
            log.read(|log| log.is_loading() || log.rows().iter().any(|commit| commit.oid == oid))
        });
        if !held {
            wanted.push(self.commit_query(oid));
        }
        wanted.extend(self.commit_diff_query(oid));
        wanted
    }

    fn change_reads(&self, repo: &str, n: u64) -> Vec<Query> {
        let mut wanted = vec![Query::Change {
            repo: repo.to_owned(),
            n,
            page: PAGE,
        }];
        let (Some(_), Some(_)) = (self.change(), self.endpoints()) else {
            return wanted;
        };
        wanted.extend(self.compare_query());
        let talking = self.touch || self.nav.change_tab == ChangeTab::Conversation;
        if talking {
            wanted.extend(self.check_reads());
        }
        if self.nav.change_tab == ChangeTab::Commits {
            wanted.extend(self.shown_log());
            let open = self.nav.commit.as_deref();
            wanted.extend(open.into_iter().flat_map(|oid| self.commit_reads(oid)));
        }
        if self.touch || self.nav.change_tab == ChangeTab::Files {
            wanted.extend(self.diff_query());
        }
        wanted
    }

    pub(crate) fn changes_query(&self, repo: &str) -> Option<Query> {
        let me = self.me_principal();
        let state = match self.filter {
            Filter::Judgment => return self.judgment_query(),
            Filter::Merged => Some(ChangeState::Merged),
            Filter::Closed => Some(ChangeState::Closed),
            Filter::Open => Some(ChangeState::Open),
            Filter::Authored | Filter::Involves => None,
        };
        let personal = matches!(self.filter, Filter::Authored | Filter::Involves);
        if personal && me.is_none() {
            return None;
        }
        Some(Query::Changes {
            repo: repo.to_owned(),
            filter: ChangeFilter {
                state,
                author: me.clone().filter(|_| self.filter == Filter::Authored),
                involves: me.filter(|_| self.filter == Filter::Involves),
            },
            page: PAGE,
        })
    }
}

fn replaces_refusal<T>(slot: Option<&mut Loadable<T>>, result: Result<T, Error>) -> bool {
    let Some(slot) = slot.filter(|slot| slot.failed().is_some()) else {
        return false;
    };
    if slot.failed() == result.as_ref().err() {
        return false;
    }
    *slot = Loadable::from(result);
    true
}
