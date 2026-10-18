//! What the screens read out of the view: the replies that have landed,
//! picked by the query that asked them, and the reader's own identity.
use std::rc::Rc;
use view::prelude::*;

use crate::queries::PAGE;
use crate::state::{self, Anchor, ChangeTab, Forge, Menu, Nav, Progress, change_key};
use crate::words::{self, ref_label};
use forge::Principal;
use forge::{
    Bounds, Change, ChangeState, CommitInfo, Comparison, FileDiff, PageRequest, PageResponse,
    Query, RefInfo, Reply, RepoInfo, Review, Revision, Side, Verdict,
};
use identity::{Ownership, Party};

/// The open change, as its screens read it: the record, its two current
/// endpoints (either can be gone) and the reviews landed so far.
pub(crate) type OpenChange<'a> = (
    &'a Change,
    &'a Option<String>,
    &'a Option<String>,
    &'a PageResponse<Review>,
);

/// A diff file's path: its new one, or the old one of a file it deletes.
pub(crate) fn path_of(file: &FileDiff) -> Option<Vec<u8>> {
    file.new_path.clone().or_else(|| file.old_path.clone())
}

/// A revision's name as bytes: a ref's full name, or a commit's hex.
pub(crate) fn revision_name(revision: &Revision) -> Vec<u8> {
    match revision {
        Revision::Ref(name) => name.clone(),
        Revision::Oid(oid) => oid.clone().into_bytes(),
    }
}

/// The lines `files` add and remove, all told.
pub(crate) fn totals<'a>(files: impl IntoIterator<Item = &'a FileDiff>) -> (u64, u64) {
    files.into_iter().fold((0, 0), |(added, deleted), file| {
        (added + file.additions, deleted + file.deletions)
    })
}

/// A commit's first line (Expo `parts.tsx` `summaryOf`).
pub(crate) fn summary_of(commit: &CommitInfo) -> String {
    String::from_utf8_lossy(&commit.message)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// What a line in the open change's room is, read from forge's records
/// rather than its text ([`Forge::change_event`]).
pub(crate) enum ChangeEvent<'a> {
    /// what a reader said there
    Reply,
    /// forge's line for a review
    Review(&'a Review),
    /// forge's first line: the change opened
    Opened,
    /// forge's line for the merge: who merged it, and its commit
    Merged {
        by: Option<&'a Principal>,
        oid: &'a str,
    },
    /// forge's line for the close: who closed it
    Closed { by: Option<&'a Principal> },
}

/// Who reviewed the open change: each reviewer's latest verdict, in the
/// place of their first review (Expo's `Map.set`), then each asked who
/// has not answered.
pub(crate) struct Standings<'a> {
    pub answered: Vec<(&'a Principal, Verdict)>,
    pub asked: Vec<&'a Principal>,
}

/// A published line comment of the open change, at its anchor.
#[derive(Clone)]
pub(crate) struct Published {
    pub anchor: Anchor,
    pub author: String,
    pub body: String,
    /// the block its review landed in
    pub height: u64,
    /// its review is pinned at a head the change has moved on from
    pub outdated: bool,
}

/// A read, in the three states a screen draws.
pub(crate) enum Stage<'a, T = Reply> {
    Loading,
    Failed(&'a Error),
    Ready(&'a T),
}

const UNLISTED: &Ownership = &Ownership::Anyone;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Writer {
    Party(Party),
    Everyone,
}

impl Writer {
    pub(crate) fn rows(list: &Ownership) -> Vec<Writer> {
        match list {
            Ownership::Owner => Vec::new(),
            Ownership::Listed(parties) => parties.iter().cloned().map(Writer::Party).collect(),
            Ownership::Anyone => vec![Writer::Everyone],
        }
    }

    pub(crate) fn key(&self) -> String {
        match self {
            Writer::Party(Party::Account(number)) => format!("acct-{number}"),
            Writer::Party(Party::Program(program)) => format!("program-{program}"),
            Writer::Everyone => "everyone".into(),
        }
    }
}

impl Forge {
    /// The reader's account number, as the host resolved it.
    pub(crate) fn my_account(&self) -> Option<u64> {
        self.session.account
    }

    /// The reader as a principal: their account; nobody while their seated key
    /// holds none. [`Principal::writer`] is the one rule every view gates its
    /// writes on, as `ExecCtx::sender` refuses them.
    pub(crate) fn me_principal(&self) -> Option<Principal> {
        Principal::writer(self.my_account())
    }

    /// Whether the reader writes at all: connected, and an account to write
    /// as. A key that holds none reads everything and writes nothing.
    pub(crate) fn may_write(&self) -> bool {
        self.session.connected && self.me_principal().is_some()
    }

    pub(crate) fn writes_repo(&self) -> bool {
        let (Some(me), Some((info, _))) = (self.me_principal(), self.repo()) else {
            return false;
        };
        let owner = info.repo.owner == me;
        let listed = self.writers().is_some_and(|list| match list {
            Ownership::Owner => false,
            Ownership::Listed(parties) => me
                .account()
                .is_some_and(|number| parties.contains(&Party::Account(number))),
            Ownership::Anyone => true,
        });
        self.may_write() && (owner || listed)
    }

    /// Whether the reader owns the open repository: only the owner changes
    /// its settings and writers.
    pub(crate) fn owns_repo(&self) -> bool {
        let (Some(me), Some((info, _))) = (self.me_principal(), self.repo()) else {
            return false;
        };
        self.may_write() && info.repo.owner == me
    }

    pub(crate) fn stage(&self, query: &Query) -> Stage<'_> {
        match self.data.get(query) {
            Some(Loadable::Ready(reply) | Loadable::Reloading(reply, _)) => Stage::Ready(reply),
            Some(Loadable::Failed(refusal)) => Stage::Failed(refusal),
            _ => Stage::Loading,
        }
    }

    /// A landed reply, shared: for a screen that keeps a layout of it.
    pub(crate) fn shared(&self, query: &Query) -> Option<Rc<Reply>> {
        self.data.get(query)?.ready().cloned()
    }

    pub(crate) fn ready(&self, query: &Query) -> Option<&Reply> {
        match self.stage(query) {
            Stage::Ready(reply) => Some(reply),
            _ => None,
        }
    }

    pub(crate) fn repo_name(&self) -> String {
        self.nav.repo.clone().unwrap_or_default()
    }

    /// `duck://<chain>/forge/<name>`: the repository's clone address and
    /// link, minted by ducklink from the session's chain id
    /// (`<label>#<salt>`); a placeholder `duck://<network>/forge/<name>`
    /// while no chain is known.
    pub(crate) fn repo_link(&self, name: &str) -> String {
        self.link(forge::PROGRAM, &[name])
            .unwrap_or_else(|| format!("duck://<network>/forge/{name}"))
    }

    /// `duck://<chain>/<program>/<segments…>`, minted by ducklink from the
    /// session's chain id; `None` while no chain is known, when a Copy
    /// link has nothing to copy.
    pub(crate) fn link(&self, program: &str, segments: &[&str]) -> Option<String> {
        ducklink::mint(&self.session.chain_id, program, segments)
    }

    pub(crate) fn repo_query(&self) -> Query {
        Query::Repo {
            repo: self.repo_name(),
        }
    }

    pub(crate) fn refs_query(&self) -> Query {
        Query::Refs {
            repo: self.repo_name(),
            page: PAGE,
        }
    }

    pub(crate) fn repo(&self) -> Option<(&RepoInfo, &Bounds)> {
        match self.ready(&self.repo_query())? {
            Reply::Repo { repo, bounds, .. } => Some((repo, bounds)),
            _ => None,
        }
    }

    pub(crate) fn writers_stage(&self) -> Stage<'_, Ownership> {
        let slot = self
            .nav
            .repo
            .as_ref()
            .and_then(|repo| self.writers.get(repo));
        match slot {
            Some(Loadable::Ready(list) | Loadable::Reloading(list, _)) => {
                Stage::Ready(list.as_ref().unwrap_or(UNLISTED))
            }
            Some(Loadable::Failed(refusal)) => Stage::Failed(refusal),
            Some(Loadable::Idle | Loadable::Loading(_)) | None => Stage::Loading,
        }
    }

    pub(crate) fn writers(&self) -> Option<&Ownership> {
        match self.writers_stage() {
            Stage::Ready(list) => Some(list),
            Stage::Loading | Stage::Failed(_) => None,
        }
    }

    pub(crate) fn writer_name(&self, writer: &Writer) -> String {
        match writer {
            Writer::Party(Party::Account(number)) => {
                self.principal_name(&Principal::Account(*number))
            }
            Writer::Party(Party::Program(program)) => program.clone(),
            Writer::Everyone => words::EVERYONE.to_owned(),
        }
    }

    pub(crate) fn refs(&self) -> Option<&[RefInfo]> {
        match self.ready(&self.refs_query())? {
            Reply::Refs { page, .. } => Some(&page.items),
            _ => None,
        }
    }

    pub(crate) fn branches(&self) -> Vec<Vec<u8>> {
        self.refs()
            .unwrap_or_default()
            .iter()
            .filter(|info| info.name.starts_with(b"refs/heads/"))
            .map(|info| info.name.clone())
            .collect()
    }

    /// The branches (or `tags`) a dropdown lists, in its order: the
    /// default branch first, each kept while its name holds what the
    /// dropdown's filter box does, in any case.
    pub(crate) fn menu_refs(&self, tags: bool) -> Vec<Vec<u8>> {
        let prefix: &[u8] = if tags { b"refs/tags/" } else { b"refs/heads/" };
        let needle = self.menu_search.text().trim().to_lowercase();
        let mut refs: Vec<Vec<u8>> = self
            .refs()
            .unwrap_or_default()
            .iter()
            .map(|info| info.name.clone())
            .filter(|name| name.starts_with(prefix))
            .filter(|name| ref_label(name).to_lowercase().contains(&needle))
            .collect();
        let default = self.default_head();
        refs.sort_by_key(|name| (*name != default, name.clone()));
        refs
    }

    /// What `menu` lists: its open tab of branches or tags. A default head
    /// is a branch, so its Tags tab lists none (Expo hands its sheet the
    /// branches alone).
    pub(crate) fn menu_list(&self, menu: Menu) -> Vec<Vec<u8>> {
        let tags = self.menu_offers_tags(menu) && self.menu_tags;
        if tags && menu == Menu::Head {
            return Vec::new();
        }
        self.menu_refs(tags)
    }

    /// Whether `menu` offers tags beside branches: every ref sheet on the
    /// phone (Expo's `RefSheet` always has Branches | Tags), every
    /// dropdown on the desktop but the default head's.
    pub(crate) fn menu_offers_tags(&self, menu: Menu) -> bool {
        menu != Menu::Head || self.touch
    }

    /// The ref `menu` has picked: the one browsed, the default head (the
    /// desktop's settings form, the saved one on the phone), or the change
    /// form's source or target.
    pub(crate) fn menu_picked(&self, menu: Menu) -> Vec<u8> {
        let form = self.form.as_ref();
        match menu {
            Menu::Ref => self.head_name(),
            Menu::Head if self.touch => self.default_head(),
            Menu::Head => self
                .repo_settings
                .as_ref()
                .map(|form| form.head.clone())
                .unwrap_or_default(),
            Menu::Source => form.map(|form| form.from.clone()).unwrap_or_default(),
            Menu::Target => form.map(|form| form.into.clone()).unwrap_or_default(),
        }
    }

    /// How a new change's source compares with its target, once a source
    /// is picked (`changes/new.tsx` `useCompare`).
    pub(crate) fn form_compare_query(&self) -> Option<Query> {
        let form = self.change_form()?;
        if form.edit.is_some() || form.from.is_empty() {
            return None;
        }
        Some(Query::Compare {
            repo: form.repo.clone(),
            from: Revision::Ref(form.from.clone()),
            into: Revision::Ref(form.into.clone()),
        })
    }

    /// The default head of the repo, or `refs/heads/main` before it lands.
    pub(crate) fn default_head(&self) -> Vec<u8> {
        self.repo()
            .map(|(info, _)| info.repo.settings.head.clone())
            .unwrap_or_else(|| b"refs/heads/main".to_vec())
    }

    /// The ref the reader is browsing.
    pub(crate) fn head_name(&self) -> Vec<u8> {
        self.nav.rev.clone().unwrap_or_else(|| self.default_head())
    }

    pub(crate) fn revision(&self) -> Revision {
        self.nav.revision(&self.default_head())
    }

    /// The commit the browsed ref points at, once refs have landed.
    pub(crate) fn head_oid(&self) -> Option<String> {
        let name = self.head_name();
        self.refs()?
            .iter()
            .find(|info| info.name == name)
            .map(|info| info.target.clone())
    }

    /// The README of the root tree: `README.md` over any other `README*`.
    pub(crate) fn readme(&self) -> Option<(Vec<u8>, String)> {
        let Reply::Tree { page, .. } = self.ready(&self.tree_query(Vec::new())?)? else {
            return None;
        };
        page.items
            .iter()
            .filter(|entry| {
                entry.kind != forge::EntryKind::Directory
                    && String::from_utf8_lossy(&entry.name)
                        .to_lowercase()
                        .starts_with("readme")
            })
            .min_by_key(|entry| !entry.name.eq_ignore_ascii_case(b"readme.md"))
            .map(|entry| (entry.name.clone(), entry.oid.clone()))
    }

    /// The log of the ref the reader browses.
    pub(crate) fn log_query(&self) -> Query {
        Query::Log {
            repo: self.repo_name(),
            from: self.revision(),
            exclude: None,
            page: PAGE,
        }
    }

    /// The commit `oid` by itself: the first row of the history from it.
    pub(crate) fn commit_query(&self, oid: &str) -> Query {
        Query::Log {
            repo: self.repo_name(),
            from: Revision::Oid(oid.to_owned()),
            exclude: None,
            page: PageRequest::first(1),
        }
    }

    /// The log on screen: inside a change its own commits (what its target
    /// does not reach), else the history of the ref the reader browses.
    pub(crate) fn shown_log(&self) -> Option<Query> {
        match self.nav.change {
            Some(_) => {
                let (from, into) = self.endpoints()?;
                Some(crate::ui::commits::query(self, from, Some(into)))
            }
            None => Some(self.log_query()),
        }
    }

    /// The commit `oid`, once a log holds it: the one on screen, else the
    /// commit's own row.
    pub(crate) fn commit(&self, oid: &str) -> Option<CommitInfo> {
        self.shown_log()
            .into_iter()
            .chain([self.commit_query(oid)])
            .filter_map(|query| self.logs.get(&query))
            .find_map(|(log, _)| {
                log.read(|log| log.rows().iter().find(|commit| commit.oid == oid).cloned())
            })
    }

    /// The open commit's diff against its first parent (the empty tree for
    /// a root commit), once the commit is known.
    pub(crate) fn commit_diff_query(&self, oid: &str) -> Option<Query> {
        Some(Query::Diff {
            repo: self.repo_name(),
            base: self.commit(oid)?.parents.first().cloned(),
            head: oid.to_owned(),
            path: None,
            page: PAGE,
        })
    }

    /// How many changes are open here, read for the tab bar on every
    /// repository screen (a list with more pages to come counts none).
    pub(crate) fn open_changes(&self) -> Option<u64> {
        match self.ready(&self.open_changes_query()?)? {
            Reply::Changes { page, .. } if page.next.is_none() => Some(page.items.len() as u64),
            _ => None,
        }
    }

    /// The open changes of the repository, the read the tab bar's count
    /// comes from.
    pub(crate) fn open_changes_query(&self) -> Option<Query> {
        Some(Query::Changes {
            repo: self.nav.repo.clone()?,
            filter: forge::ChangeFilter {
                state: Some(forge::ChangeState::Open),
                ..forge::ChangeFilter::default()
            },
            page: PAGE,
        })
    }

    /// The changes that wait on my judgment, across every repository: the
    /// Changes tab's judgment filter, and the repositories page's top
    /// section on the phone (board 20). Nobody (no account) has
    /// none to ask for.
    pub(crate) fn judgment_query(&self) -> Option<Query> {
        Some(Query::Judgment {
            principal: self.me_principal()?,
            page: PAGE,
        })
    }

    pub(crate) fn change_query(&self) -> Option<Query> {
        Some(Query::Change {
            repo: self.nav.repo.clone()?,
            n: self.nav.change?,
            page: PAGE,
        })
    }

    pub(crate) fn change(&self) -> Option<OpenChange<'_>> {
        match self.ready(&self.change_query()?)? {
            Reply::Change {
                change,
                source_head,
                target_head,
                reviews,
                ..
            } => Some((change, source_head, target_head, reviews)),
            _ => None,
        }
    }

    /// What the open change compares: its source and target, or, once
    /// merged, the two heads the merge joined (the refs move on after).
    pub(crate) fn endpoints(&self) -> Option<(Revision, Revision)> {
        let (change, _, _, _) = self.change()?;
        Some(match &change.merged_heads {
            Some(heads) => (
                Revision::Oid(heads.source.clone()),
                Revision::Oid(heads.target.clone()),
            ),
            None => (change.from.clone(), Revision::Ref(change.into.clone())),
        })
    }

    pub(crate) fn compare_query(&self) -> Option<Query> {
        let (from, into) = self.endpoints()?;
        Some(Query::Compare {
            repo: self.nav.repo.clone()?,
            from,
            into,
        })
    }

    pub(crate) fn compare(&self) -> Option<&Comparison> {
        match self.ready(&self.compare_query()?)? {
            Reply::Compare { comparison, .. } => Some(comparison),
            _ => None,
        }
    }

    pub(crate) fn diff_query(&self) -> Option<Query> {
        let (_, source_head, _, _) = self.change()?;
        Some(Query::Diff {
            repo: self.nav.repo.clone()?,
            base: self.compare()?.base.clone(),
            head: source_head.clone()?,
            path: None,
            page: PAGE,
        })
    }

    /// The principals chat marks as the reader's own: at most one.
    pub(crate) fn viewer(&self) -> Vec<Principal> {
        self.me_principal().into_iter().collect()
    }

    /// What a person or chat author is called: their account name once
    /// the roster has landed.
    pub(crate) fn principal_name(&self, principal: &Principal) -> String {
        match self.names.ready() {
            Some(names) => names.member(principal),
            None => chat::view::unnamed(principal),
        }
    }

    /// The hidden chat channel of the open change: read on its tab, and
    /// on every tab on the phone, whose strip counts its lines as Expo's
    /// does.
    pub(crate) fn open_channel(&self) -> Option<String> {
        let (change, _, _, _) = self.change()?;
        let read = self.touch || self.nav.change_tab == ChangeTab::Conversation;
        read.then(|| change.channel.clone())
    }

    pub(crate) fn review(&self) -> Option<&state::ReviewSession> {
        self.reviews
            .get(&change_key(self.nav.repo.as_deref()?, self.nav.change?))
    }

    /// The open change's review being written, to edit.
    pub(crate) fn review_mut(&mut self) -> Option<&mut state::ReviewSession> {
        let key = change_key(self.nav.repo.as_deref()?, self.nav.change?);
        self.reviews.get_mut(&key)
    }

    pub(crate) fn pending_in(&self, scope: &str) -> Vec<&state::Pending> {
        self.pending.iter().filter(|op| op.scope == scope).collect()
    }

    /// Whether a write of `scope` is still on its way: a second built from
    /// what is on screen could land twice. One that did not land is not, so
    /// a refusal never holds the controls off.
    pub(crate) fn busy(&self, scope: &str) -> bool {
        self.pending_in(scope)
            .iter()
            .any(|op| !matches!(op.progress, Progress::Refused(_)))
    }

    /// Where the open change's reply shows while it is on its way.
    pub(crate) fn reply_scope(&self) -> Option<String> {
        Some(format!("{}/reply", self.review_key()?))
    }

    // ------------------------------------------------------ the open change

    /// The open change's room, once read.
    pub(crate) fn conversation(&self) -> Option<&[chat::MsgRow]> {
        let (change, ..) = self.change()?;
        match self.messages.get(&change.channel)? {
            Loadable::Ready(rows) | Loadable::Reloading(rows, _) => Some(rows),
            _ => None,
        }
    }

    /// forge's own line in a change's room: written by the account the
    /// roster names forge's ([`chat::program_author`]).
    pub(crate) fn is_forge_line(&self, row: &chat::MsgRow) -> bool {
        let program = self
            .names
            .ready()
            .and_then(|names| names.program(&row.author));
        chat::program_author(row, program) == Some(forge::PROGRAM)
    }

    /// What `row` of the open change's room is: a reader's reply, or
    /// forge's own line for a review, the opening (forge's first line) or
    /// how the change ended. `None`: forge's line that names nothing the
    /// view holds yet (a review still paging in), or nothing at all.
    pub(crate) fn change_event(&self, row: &chat::MsgRow) -> Option<ChangeEvent<'_>> {
        if !self.is_forge_line(row) {
            return Some(ChangeEvent::Reply);
        }
        let (change, _, _, reviews) = self.change()?;
        if let Some(review) = reviews
            .items
            .iter()
            .find(|review| review.message_id == row.message_id)
        {
            return Some(ChangeEvent::Review(review));
        }
        let first = self
            .conversation()?
            .iter()
            .find(|row| self.is_forge_line(row))
            .map(|row| row.seq);
        if first == Some(row.seq) {
            return Some(ChangeEvent::Opened);
        }
        // a line matching no review yet may be one still paging in
        if reviews.next.is_some() {
            return None;
        }
        match (change.state, &change.merge_oid) {
            (ChangeState::Merged, Some(oid)) => Some(ChangeEvent::Merged {
                by: change.merged_by.as_ref(),
                oid,
            }),
            (ChangeState::Closed, _) => Some(ChangeEvent::Closed {
                by: change.closed_by.as_ref(),
            }),
            _ => None,
        }
    }

    /// What a change tab holds, where the view has read it: the lines of
    /// the conversation, the commits the target lacks, the files the diff
    /// touches.
    pub(crate) fn change_tab_count(&self, tab: ChangeTab) -> Option<u64> {
        match tab {
            ChangeTab::Conversation => self.conversation().map(|rows| rows.len() as u64),
            ChangeTab::Commits => self.compare().map(|comparison| comparison.ahead),
            ChangeTab::Files => match self.ready(&self.diff_query()?)? {
                Reply::Diff { total_files, .. } => Some(*total_files),
                _ => None,
            },
        }
    }

    /// Who reviewed the open change and who is still asked ([`Standings`]).
    pub(crate) fn standings(&self) -> Option<Standings<'_>> {
        let (change, _, _, reviews) = self.change()?;
        let mut answered: Vec<(&Principal, Verdict)> = Vec::new();
        for review in &reviews.items {
            match answered.iter_mut().find(|(who, _)| **who == review.author) {
                Some(held) => held.1 = review.draft.verdict,
                None => answered.push((&review.author, review.draft.verdict)),
            }
        }
        let asked = change
            .reviewers
            .iter()
            .filter(|asked| !answered.iter().any(|(who, _)| who == asked))
            .collect();
        Some(Standings { answered, asked })
    }

    /// Every published line comment of the open change, review by review.
    pub(crate) fn published_comments(&self) -> Vec<Published> {
        let Some((_, _, _, reviews)) = self.change() else {
            return Vec::new();
        };
        reviews
            .items
            .iter()
            .flat_map(|review| {
                let author = self.principal_name(&review.author);
                let outdated = self.outdated(&review.draft.commit_oid);
                review.draft.comments.iter().map(move |comment| Published {
                    anchor: (
                        comment.path.clone(),
                        comment.side == Side::New,
                        comment.line,
                    ),
                    author: author.clone(),
                    body: comment.body.clone(),
                    height: review.height,
                    outdated,
                })
            })
            .collect()
    }

    /// Whether a review can be pinned: the source head and the comparison
    /// are on screen (Expo `reviewStart`).
    pub(crate) fn reviewable(&self) -> bool {
        self.change().is_some_and(|(_, head, ..)| head.is_some()) && self.compare().is_some()
    }

    /// Whether the roster names `who` an agent.
    pub(crate) fn is_agent(&self, who: &Principal) -> bool {
        self.names.ready().is_some_and(|names| {
            matches!(
                names.kind(who),
                Some(chat::Kind::Program {
                    category: chat::Category::Agent,
                    ..
                })
            )
        })
    }

    // ------------------------------------------------------ refs

    /// How a ref stands against the browsed one: `browsing` for that ref
    /// itself, a tag nothing, a branch `N ahead · M behind · <word>` once
    /// compared.
    pub(crate) fn standing(&self, name: &[u8]) -> Option<String> {
        let head = self.head_name();
        if name == head {
            return Some(words::BROWSING.to_owned());
        }
        if !name.starts_with(b"refs/heads/") {
            return None;
        }
        let query = Query::Compare {
            repo: self.repo_name(),
            from: Revision::Ref(name.to_vec()),
            into: Revision::Ref(head),
        };
        Some(match self.ready(&query) {
            Some(Reply::Compare { comparison, .. }) => words::compared(comparison),
            _ => words::COMPARING.to_owned(),
        })
    }

    /// What the open repository's settings forbid a push, once read.
    pub(crate) fn forbids(&self) -> Option<String> {
        let (info, _) = self.repo()?;
        let settings = &info.repo.settings;
        words::forbids(settings.allow_force, settings.allow_delete)
    }

    /// The change form this screen shows: a draft on its repository's
    /// Changes and Refs tabs, an edit on its change's page. Anywhere else
    /// it waits, its text kept.
    pub(crate) fn change_form(&self) -> Option<&state::ChangeForm> {
        self.form.as_ref().filter(|form| {
            self.nav.repo.as_ref() == Some(&form.repo) && form.edit == self.nav.change
        })
    }

    pub(crate) fn nav(&self) -> &Nav {
        &self.nav
    }
}
