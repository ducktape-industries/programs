//! Members: every account the `identity` program holds, grouped people,
//! then agents, then programs, and one account read in full: its bio, its
//! devices, the agents it manages and what its keys signed lately.
//!
//! Two trees draw it over this one model, and the root picks one per frame
//! by the device's pointer (`window.touch()`, in `Render::render`), never
//! by width:
//!
//! - under a pointer, the desk (`ui`): the list, and the chosen account
//!   docked beside it or floating over it when the window is narrow;
//! - under a finger, the phone (`ui::phone`): the Expo app's two screens,
//!   the roster and one member, a page at a time with Back.
//!
//! The model holds what both trees read (the roster's state, the page
//! stack, the links, the badges and the face's look), and `words` every
//! sentence they show; neither tree imports from the other.
//!
//! The screen is read-only: suspending, revoking, renaming and keys live in
//! Settings. Nothing here asks a program anything the list does not: one
//! `identity` list gives every account whole, `valset` the seats of the
//! keys, and the recent activity is the `activity` program's index of
//! what the account did (`activity`).
//!
//! The contracts are borsh and this view's state is a serde snapshot, so a
//! reply is folded to `Row`s as it lands: only what the screen shows is
//! kept across a snapshot, an account's kind and its valset seat as their
//! borsh bytes (`view::borsh_bytes`).
mod activity;
mod ui;
mod words;

use programs::PageRequest;
use serde::{Deserialize, Serialize};
use view::design::{self, Ladder, Pages};
use view::prelude::*;
use view::{Div, Stateful};

use identity::Identity;
use valset::Valset;

pub use activity::Recent;

#[derive(Serialize, Deserialize)]
pub struct Members {
    rows: Loadable<Vec<Row>>,
    /// what the reader typed into the filter; the rows are never refetched
    /// for it, since the program has no search
    filter: TextField,
    /// the kind chip that is on; `None` is All. It and the filter narrow the
    /// list together and never touch the page shown.
    only: Option<Group>,
    /// where the reader is: the roster, or one member over it. The member
    /// is the detail the desktop docks or floats beside the list, kept while
    /// the filter hides its row; on a phone it is the page shown, and Back
    /// pops it.
    pages: Pages<Page>,
    /// the phone's filter field is open in the title row, in the title's
    /// place
    filtering: bool,
    /// the row the arrows rest on in the phone's roster, where moving them
    /// opens nothing; `None` until they move
    #[serde(skip)]
    cursor: Option<usize>,
    /// the reader's own account, from the session
    #[serde(skip)]
    me: Option<u64>,
    /// the key this device signs with (hex), from the session: its device
    /// row says it is this phone
    #[serde(skip)]
    signer: String,
    /// the chain links are minted on, from the session
    #[serde(skip)]
    chain: String,
    /// what the selected account signed lately; read again on restore
    #[serde(skip)]
    activity: Loadable<Recent>,
    /// each account's activity as last read: an account chosen again shows
    /// it at once (the arrows run down the list and back) while `rereading`
    /// reads it anew
    #[serde(skip)]
    seen: std::collections::HashMap<u64, Recent>,
    #[serde(skip)]
    rereading: Option<Task<()>>,
    /// the roster's re-read while the rows on screen stay
    #[serde(skip)]
    rereading_rows: Option<Task<()>>,
    /// the pane's width, read from the window each render
    #[serde(skip)]
    width: f32,
    /// the device's pointer is a finger, read from the window each render:
    /// a phone's activity rows are titled by what each transaction did
    #[serde(skip)]
    touch: bool,
    /// the list's dragged width; `None` until the first drag
    #[serde(skip)]
    list: Option<f32>,
    // each card's picture, read once per blob as a face first draws it
    #[serde(skip)]
    pictures: Option<Entity<view::pictures::Pictures>>,
}

impl Default for Members {
    fn default() -> Self {
        Self {
            rows: Loadable::default(),
            filter: TextField::default(),
            only: None,
            pages: Pages::root(Page::Roster),
            filtering: false,
            cursor: None,
            me: None,
            signer: String::new(),
            chain: String::new(),
            activity: Loadable::default(),
            seen: Default::default(),
            rereading: None,
            rereading_rows: None,
            width: 0.,
            touch: false,
            list: None,
            pictures: None,
        }
    }
}

/// A page of the view's stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Page {
    /// every account
    Roster,
    /// one account, by its number
    Member(u64),
}

/// What the roster has to show.
pub(crate) enum Roster<'a> {
    /// the rows the filter and the chip let through, grouped and in order
    Rows(Vec<&'a Row>),
    /// the first read is on its way
    Reading,
    Refused(&'a Error),
    /// nobody to show
    Empty {
        /// the empty state's element id
        id: &'static str,
        /// what is missing
        title: &'static str,
        /// and why
        detail: String,
    },
}

/// The list's three groups, in the order they are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Group {
    People,
    Agents,
    Programs,
}

impl Group {
    pub const ALL: [Group; 3] = [Group::People, Group::Agents, Group::Programs];

    pub fn label(self) -> &'static str {
        match self {
            Group::People => "People",
            Group::Agents => "Agents",
            Group::Programs => "Programs",
        }
    }

    fn of(kind: &identity::Kind) -> Group {
        match kind {
            identity::Kind::User => Group::People,
            identity::Kind::Program {
                category: identity::Category::Agent,
                ..
            } => Group::Agents,
            identity::Kind::System(_) => Group::Programs,
        }
    }

    /// The roster's one word for an account of this group, its row's tail
    /// (`index.tsx:69`).
    pub fn word(self) -> &'static str {
        match self {
            Group::People => words::USER,
            Group::Agents => words::AGENT,
            Group::Programs => words::PROGRAM,
        }
    }

    fn face(self, avatar: Div, pictured: bool, theme: &Theme) -> Div {
        match self {
            Group::People => avatar,
            Group::Agents => design::agent_tint(avatar, pictured, theme),
            Group::Programs => avatar
                .rounded_none()
                .font_family(design::fonts::FAMILY_MONO),
        }
    }
}

/// One account as this screen shows it.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Row {
    number: u64,
    name: String,
    bio: Option<String>,
    /// what the account is; labelled as it is drawn ([`identity::view::kind`])
    #[serde(with = "view::borsh_bytes")]
    kind: identity::Kind,
    devices: Vec<Device>,
    /// the valset seat of a key this account holds, where it holds one
    #[serde(with = "view::borsh_bytes")]
    seat: Option<valset::Role>,
    // the card's picture; a snapshot from before it reads as none
    #[serde(default, with = "view::borsh_bytes")]
    avatar: Option<abi::BlobId>,
}

impl Row {
    fn group(&self) -> Group {
        Group::of(&self.kind)
    }

    fn manager(&self) -> Option<u64> {
        match self.kind {
            identity::Kind::Program { manager, .. } => Some(manager),
            _ => None,
        }
    }

    /// An agent's standing: active, suspended or revoked. `None` for a
    /// person or a program, which have none.
    fn standing(&self) -> Option<identity::Standing> {
        match self.kind {
            identity::Kind::Program { standing, .. } => Some(standing),
            identity::Kind::User | identity::Kind::System(_) => None,
        }
    }

    /// What the account is, in words: `User` for a person, `Agent` for an
    /// agent, `Program` for a program named after its program (it says so
    /// once), else `Program · chat`.
    fn kind_word(&self) -> String {
        match &self.kind {
            identity::Kind::User => words::USER.into(),
            kind @ identity::Kind::Program { .. } => match design::runs(kind) {
                Some(runs) => format!("{} · {runs}", words::AGENT),
                None => words::AGENT.into(),
            },
            identity::Kind::System(program) if *program == self.name => words::PROGRAM.into(),
            identity::Kind::System(program) => format!("{} · {program}", words::PROGRAM),
        }
    }

    /// An agent's standing word: `active`, else why it no longer acts.
    fn life(&self) -> &'static str {
        design::note(&self.kind).unwrap_or(words::ACTIVE)
    }
}

/// One key an account acts with.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Device {
    label: Option<String>,
    key: Vec<u8>,
    /// when it joined, in milliseconds
    added_at: u64,
}

impl Device {
    /// Its label, else `Device N` by its place in the account's keys.
    fn title(&self, index: usize) -> String {
        self.label.clone().unwrap_or_else(|| words::device(index))
    }

    /// The key in hex, as the session names the key this device signs with.
    fn hex(&self) -> String {
        self.key.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// An agent's standing as a badge: its word, active in the success
/// colours, suspended in the warning ones, revoked in danger
/// (`[number].tsx:141`). `None` for a person or a program.
fn standing_badge(
    id: &'static str,
    row: &Row,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    let (foreground, background) = match row.standing()? {
        identity::Standing::Active => (theme.success, theme.success_soft),
        identity::Standing::Suspended => (theme.warning, theme.warning_soft),
        identity::Standing::Revoked => (theme.danger, theme.danger_soft),
    };
    Some(design::badge(
        id,
        row.life(),
        ladder,
        foreground,
        background,
    ))
}

/// A key's valset seat as a badge: a validator in the success colours, a
/// resident quiet (Nodes' memberships drew them so).
fn valset_badge(seat: valset::Role, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    let (word, foreground, background) = match seat {
        valset::Role::Validator => (words::VALIDATOR, theme.success, theme.success_soft),
        valset::Role::Resident => (words::RESIDENT, theme.muted, theme.surface_raised),
    };
    design::badge("members-validator", word, ladder, foreground, background)
}

/// A refused roster read: its sentence, and Retry, which reads it again.
/// Each tree sets it in its own padding.
fn refused(
    refusal: &Error,
    cx: &mut Context<Members>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let retry = cx.listener(|view, _: &ClickEvent, _, cx| view.read(cx));
    design::refused("members", design::sentence(refusal), ladder, theme, retry)
}

/// A press that opens `link` through the host.
fn opens(link: String) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| cx.host().open_link(&link)
}

/// Account `number`'s page in Explorer.
fn explorer_account(number: u64) -> String {
    format!("duck://explorer/account/{number}")
}

/// The block at `height` in Explorer.
fn explorer_block(height: u64) -> String {
    format!("duck://explorer/block/{height}")
}

/// The frame at `position` in the block at `height`, in Explorer.
fn explorer_tx(height: u64, position: u32) -> String {
    format!("duck://explorer/tx/{height}/{position}")
}

/// The name of account `number` in `rows`.
fn name(rows: &[Row], number: u64) -> Option<String> {
    rows.iter()
        .find(|row| row.number == number)
        .map(|row| row.name.clone())
}

/// The name of account `number` in `rows`, else `#N`.
fn name_or_number(rows: &[Row], number: u64) -> String {
    name(rows, number).unwrap_or_else(|| format!("#{number}"))
}

impl View for Members {
    const NAME: &'static str = "Members";
    const DESCRIPTION: &'static str =
        "Every account of this network: who it is, what it runs and what it signed lately.";
    const ICON: &'static str = "icons/users.svg";
    const TINT: Tint = Tint::Teal;
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Chain,
        Capability::Program,
        Capability::Host,
        Capability::Link,
        // each card's picture (`blob.get`)
        Capability::Blob,
    ];
    // the list and the detail fit at 320, the desk's smallest window
    const MIN_WINDOW_WIDTH: u32 = 320;

    fn attach(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pictures.is_none() {
            self.pictures = Some(cx.new(|_| view::pictures::Pictures::default()));
        }
        cx.follow::<HostSession>((), |view, session, cx| match session {
            Ok(session) => {
                let now = (session.account, &session.chain_id, &session.signer);
                if (view.me, &view.chain, &view.signer) != now {
                    view.me = session.account;
                    view.chain = session.chain_id;
                    view.signer = session.signer;
                    cx.notify();
                }
            }
            Err(refusal) => cx.log_refused("the session", &refusal),
        })
        .detach();
        // the roster joins two programs: a block of either re-reads it
        cx.follow::<Changes<Identity>>((), |view, bump, cx| match bump {
            Ok(_) => view.read(cx),
            Err(refusal) => cx.log_refused("identity's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<Valset>>((), |view, bump, cx| match bump {
            Ok(_) => view.read(cx),
            Err(refusal) => cx.log_refused("valset's live heads", &refusal),
        })
        .detach();
        self.read(cx);
        self.read_activity(cx);
    }
}

impl Render for Members {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // the pane's width, as the host lays the frame out: the detail
        // docks or floats by it in the frame that shows it
        self.width = window.viewport_size().width.into();
        // the pointer is in hand from the first tick, after `attach` read a
        // restored member's activity: a finger reads it again, titled
        // SDK gap: attach's window carries no pointer yet; the SDK builds it later
        if self.touch != window.touch() {
            self.touch = window.touch();
            self.seen.clear();
            self.read_activity(cx);
        }
        // a finger draws the phone's screens (the Expo app's), a pointer
        // the desk's: two trees over the one model, picked here once
        if self.touch {
            ui::phone::render(self, cx).into_any_element()
        } else {
            ui::render(self, cx).into_any_element()
        }
    }
}

impl Members {
    // `row`'s picture, once read.
    fn picture(&self, row: &Row, cx: &mut Context<Self>) -> Option<std::sync::Arc<view::Image>> {
        let blob = row.avatar.as_ref()?;
        let pictures = self.pictures.as_ref()?;
        pictures.update(cx, |pictures, cx| pictures.get(blob, cx))
    }

    /// One read of both programs — the boot, a retry, a restore, a live
    /// bump of either. Rows already on screen stay there while it runs, so a bump
    /// never blinks the list back to "Loading", and a bump that changed no
    /// row draws nothing; a refused bump is logged and leaves them. A newer
    /// read cancels the one before.
    fn read(&mut self, cx: &mut Context<Self>) {
        let task = cx.land(roster(cx.host()), |view, result, cx| {
            let rescan = view.activity.is_idle();
            match (result, view.rows.ready()) {
                (Ok(rows), Some(old)) if *old == rows => {}
                (Ok(rows), _) => {
                    let keys_moved = view
                        .selected()
                        .is_some_and(|number| keys(view.all(), number) != keys(&rows, number));
                    view.rows = Loadable::Ready(rows);
                    cx.notify();
                    if keys_moved {
                        return view.read_activity(cx);
                    }
                }
                (Err(refusal), Some(_)) => cx.log_refused("a refresh", &refusal),
                (Err(refusal), None) => {
                    view.rows = Loadable::Failed(refusal);
                    cx.notify();
                }
            }
            if rescan {
                view.read_activity(cx);
            }
        });
        match self.rows.ready() {
            Some(_) => self.rereading_rows = Some(task),
            None => {
                self.rows = Loadable::Loading(task);
                cx.notify();
            }
        }
    }

    /// Shows `number` in the detail, in place of the one shown: the list's
    /// choice (a press, the arrows), which never stacks deeper than one
    /// member over the roster.
    pub fn select(&mut self, number: u64, cx: &mut Context<Self>) {
        if self.selected() == Some(number) {
            return;
        }
        match self.pages.depth() {
            0 => self.pages.push(Page::Member(number)),
            _ => self.pages.replace(Page::Member(number)),
        }
        self.moved(cx);
    }

    /// Opens `number` over the page shown (the phone's roster rows, its
    /// manager link and its Manages rows): Back comes back here.
    fn open(&mut self, number: u64, cx: &mut Context<Self>) {
        if self.selected() == Some(number) {
            return;
        }
        self.pages.push(Page::Member(number));
        self.moved(cx);
    }

    /// Back: the member under the one shown, else the roster.
    fn back(&mut self, cx: &mut Context<Self>) {
        if self.pages.pop().is_some() {
            self.moved(cx);
        }
    }

    /// The desktop's Close on a floating detail: the roster again.
    fn close(&mut self, cx: &mut Context<Self>) {
        while self.pages.pop().is_some() {}
        self.moved(cx);
    }

    /// The page moved: read what the member shown signed lately.
    fn moved(&mut self, cx: &mut Context<Self>) {
        self.activity = Loadable::Idle;
        self.read_activity(cx);
        cx.notify();
    }

    /// The account the top page shows, if it is a member's.
    fn selected(&self) -> Option<u64> {
        match *self.pages.top() {
            Page::Member(number) => Some(number),
            Page::Roster => None,
        }
    }

    /// Reads the selected account's recent activity, once its keys are
    /// known; an account with no keys signs nothing and asks nothing.
    /// Notifies when the detail's activity changes.
    fn read_activity(&mut self, cx: &mut Context<Self>) {
        self.rereading = None;
        let Some(row) = self.selected_row() else {
            return;
        };
        if row.devices.is_empty() {
            if !self.activity.is_idle() {
                self.activity = Loadable::Idle;
                cx.notify();
            }
            return;
        }
        let number = row.number;
        let work = activity::recent(cx.host(), number, self.touch);
        // held in `activity` or `rereading`, so choosing someone else drops
        // it unfinished
        let task = cx.land(work, move |view, result, cx| {
            if let Ok(recent) = &result {
                view.seen.insert(number, recent.clone());
            }
            match result {
                // read anew and the same: nothing to draw
                Ok(recent) if view.activity.ready() == Some(&recent) => {}
                result => {
                    view.activity = Loadable::from(result);
                    cx.notify();
                }
            }
        });
        match self.seen.get(&number) {
            Some(recent) => {
                if self.activity.ready() != Some(recent) {
                    self.activity = Loadable::Ready(recent.clone());
                    cx.notify();
                }
                self.rereading = Some(task);
            }
            None => {
                self.activity = Loadable::Loading(task);
                cx.notify();
            }
        }
    }

    fn selected_row(&self) -> Option<&Row> {
        let number = self.selected()?;
        self.rows.ready()?.iter().find(|row| row.number == number)
    }

    /// The roster's state: its rows, or why there are none to show (nobody
    /// on the network, or nobody the filter and the chip let through).
    fn roster(&self) -> Roster<'_> {
        let all = match &self.rows {
            Loadable::Idle | Loadable::Loading(_) => return Roster::Reading,
            Loadable::Failed(refusal) => return Roster::Refused(refusal),
            Loadable::Ready(rows) | Loadable::Reloading(rows, _) => rows,
        };
        if all.is_empty() {
            let why = words::NO_ACCOUNTS_WHY.into();
            return Roster::Empty {
                id: "members-empty",
                title: words::NO_ACCOUNTS,
                detail: why,
            };
        }
        let shown = self.shown();
        if shown.is_empty() {
            let only = self.only.map(Group::label);
            let detail = words::no_match(self.filter.text().trim(), only);
            return Roster::Empty {
                id: "members-no-match",
                title: words::NOTHING_MATCHES,
                detail,
            };
        }
        Roster::Rows(shown)
    }

    /// Every account, once read; none before.
    fn all(&self) -> &[Row] {
        self.rows.ready().map_or(&[], Vec::as_slice)
    }

    /// The chips: All, People, Agents, Programs, each with how many the
    /// network holds.
    fn choices(&self) -> [(Option<Group>, &'static str, usize); 4] {
        let count = |group: Option<Group>| {
            self.all()
                .iter()
                .filter(|row| group.is_none_or(|group| row.group() == group))
                .count()
        };
        [
            None,
            Some(Group::People),
            Some(Group::Agents),
            Some(Group::Programs),
        ]
        .map(|group| (group, group.map_or(words::ALL, Group::label), count(group)))
    }

    /// The network's label: the session's chain id before its salt
    /// (`testkit#0a1b` reads `testkit`).
    fn network(&self) -> &str {
        self.chain
            .split_once('#')
            .map_or(self.chain.as_str(), |(label, _)| label)
    }

    /// Open DM's link to `row`: any account but the reader.
    fn dm_link(&self, row: &Row) -> Option<String> {
        let me = self.me.filter(|me| *me != row.number)?;
        ducklink::mint(
            &self.chain,
            chat::PROGRAM,
            &[&chat::dm_channel_id(me, row.number)],
        )
    }

    /// The agents account `number` manages.
    fn managed(&self, number: u64) -> Vec<&Row> {
        self.all()
            .iter()
            .filter(|row| row.manager() == Some(number))
            .collect()
    }

    /// The rows the filter and the chip let through, grouped and in order.
    fn shown(&self) -> Vec<&Row> {
        let Some(rows) = self.rows.ready() else {
            return Vec::new();
        };
        let needle = self.filter.text().trim().to_lowercase();
        let mut shown: Vec<&Row> = rows
            .iter()
            .filter(|row| self.only.is_none_or(|only| row.group() == only))
            .filter(|row| {
                needle.is_empty()
                    || row.name.to_lowercase().contains(&needle)
                    || row.number.to_string().contains(&needle)
            })
            .collect();
        shown.sort_by_key(|row| (row.group(), row.number));
        shown
    }
}

/// The roster, with each account's valset standing joined on the keys it
/// holds.
///
/// Both programs answer in pages; the roster follows every `next` cursor to
/// the end, since the screen shows the whole network.
async fn roster(host: Host) -> Result<Vec<Row>, Error> {
    let page = |after| PageRequest { after, limit: None };
    let accounts = host
        .query_all(|after| identity::ask::List { page: page(after) })
        .await?;
    let members = host
        .query_all(|after| valset::ask::Memberships { page: page(after) })
        .await?;
    Ok(accounts
        .iter()
        .map(|account| row(account, &members))
        .collect())
}

/// The keys account `number` holds in `rows`, if it is there.
fn keys(rows: &[Row], number: u64) -> Option<Vec<&[u8]>> {
    let row = rows.iter().find(|row| row.number == number)?;
    Some(row.devices.iter().map(|device| &device.key[..]).collect())
}

fn row(account: &identity::Account, members: &[valset::Membership]) -> Row {
    Row {
        number: account.number,
        name: account.card.name.clone(),
        bio: account.card.bio.clone(),
        avatar: account.card.avatar,
        kind: account.kind(),
        devices: account
            .keys()
            .iter()
            .map(|key| Device {
                label: key.label.clone(),
                key: key.key.clone(),
                added_at: key.added_at,
            })
            .collect(),
        seat: members
            .iter()
            .find(|member| account.holds(&member.key))
            .map(|member| member.role),
    }
}

export_view!(Members);

#[cfg(test)]
mod tests;
