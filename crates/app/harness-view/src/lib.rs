//! Agents: harness's agents and how each worked: the list, an agent (its conversations, its setup,
//! its front matter edited by `setup`), a transcript (`timeline`), the routes (`routes`), the create
//! and edit form (`create`), who may act (`policies`); `read` reads, `ui` draws desktop and phone.
mod create;
mod links;
mod policies;
mod read;
mod routes;
mod setup;
mod timeline;
mod ui;
mod words;

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use view::methods::{ClipboardWrite, Submitted};
use view::prelude::*;

use create::Draft;
use harness::{Asking, ThreadRow, Written};
use identity::Identity;
use read::{Detail, Offer, Roster, Route, Touched, Transcript};
use routes::RouteDraft;
use setup::SetupDraft;

#[derive(Default, Serialize, Deserialize)]
pub struct Agents {
    screen: Screen,
    draft: Draft,
    #[serde(default)]
    route_draft: RouteDraft,
    #[serde(default)]
    setup_draft: SetupDraft,
    arming: Option<Arm>,
    #[serde(default)]
    rename: TextField,
    #[serde(skip)]
    menu: Option<AgentMenu>,
    open: BTreeSet<String>,
    sheet: Option<Sheet>,
    #[serde(skip)]
    me: Option<u64>,
    #[serde(skip)]
    chain: String,
    #[serde(skip)]
    roster: Reread<Roster>,
    #[serde(skip)]
    budget: Reread<read::Budgeted>,
    #[serde(skip)]
    detail: Reread<Detail>,
    #[serde(skip)]
    transcript: Loadable<Option<Transcript>>,
    #[serde(skip)]
    routes: Loadable<Vec<Route>>,
    #[serde(skip)]
    offers: Loadable<Vec<Offer>>,
    #[serde(skip)]
    notice: Option<String>,
    #[serde(skip)]
    acting: Option<Task<()>>,
    #[serde(skip)]
    doing: Option<String>,
    #[serde(skip)]
    routing: Option<Task<()>>,
    #[serde(skip)]
    title: Option<String>,
    // the rooms named so far this session, and the newest head's time
    #[serde(skip)]
    rooms: read::Rooms,
    #[serde(skip)]
    now: Rc<Cell<u64>>,
    #[serde(skip)]
    touched: Touched,
    #[serde(skip)]
    following: Option<Task<()>>,
    #[serde(skip)]
    earlier: Option<Task<()>>,
    #[serde(skip)]
    calling: Option<Task<()>>,
    #[serde(skip)]
    wanted: RefCell<Wanted>,
    #[serde(skip)]
    rows: RefCell<Option<(u64, ListState)>>,
    #[serde(skip)]
    width: f32,
    #[serde(skip)]
    conversations_policy: policy_view::Editing,
    #[serde(skip)]
    route_policy: policy_view::Editing,
    #[serde(skip)]
    pictures: Option<Entity<view::pictures::Pictures>>,
}

pub(crate) type Picture = Option<std::sync::Arc<view::Image>>;

#[derive(Default)]
pub(crate) struct Wanted {
    from: Option<u64>,
    calls: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Screen {
    #[default]
    List,
    Create,
    Agent(u64),
    Thread(u64),
    #[serde(alias = "Models")]
    Routes,
    Part(u64, Part),
    Usage(u64),
    #[serde(alias = "SetModel")]
    SetRoute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Part {
    Model,
    File,
    Prompt,
    Usage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Sheet {
    Agent,
    Thread,
    Frame(u64),
}

impl Screen {
    fn agent(self) -> Option<u64> {
        match self {
            Screen::Agent(agent) | Screen::Part(agent, _) => Some(agent),
            _ => None,
        }
    }

    fn thread(self) -> Option<u64> {
        match self {
            Screen::Thread(thread) | Screen::Usage(thread) => Some(thread),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Arm {
    Remove(u64),
    Close(u64),
    Rename(u64),
    Suspend(u64),
    Resume(u64),
    Revoke(u64),
}

pub(crate) const AGENT_MENU: &str = "agent-menu";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AgentMenu {
    pub agent: u64,
    pub at: Option<view::Point<Pixels>>,
    pub active: usize,
}

pub(crate) fn standing_toggle(
    agent: u64,
    standing: identity::Standing,
) -> (&'static str, &'static str, Arm) {
    match standing {
        identity::Standing::Suspended => ("resume", "Resume", Arm::Resume(agent)),
        identity::Standing::Active | identity::Standing::Revoked => {
            ("suspend", "Suspend", Arm::Suspend(agent))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Idle,
    Working,
    Stuck,
    NoNode,
    Unconfigured,
}

pub(crate) fn applied(answer: Result<Submitted, Error>) -> Result<Vec<u8>, String> {
    match answer {
        Ok(Submitted::Applied { output, .. }) => Ok(output),
        answer => Err(design::failure(&answer).expect("only a landed write has no failure")),
    }
}

pub(crate) fn thread_state(thread: &ThreadRow) -> State {
    let asking = !thread.closed && thread.asking != Asking::Idle;
    match (asking, thread.held) {
        (false, _) => State::Idle,
        (true, 0) => State::Working,
        (true, _) => State::Stuck,
    }
}

pub(crate) fn agent_state(card: &read::Card, roster: &Roster) -> State {
    let Some(serving) = card.serving else {
        return State::Unconfigured;
    };
    if serving == 0 || roster.relaying == 0 {
        return State::NoNode;
    }
    let states = card.asking.iter().map(|said| thread_state(&said.thread));
    states.fold(State::Idle, |state, next| match (state, next) {
        (State::Stuck, _) | (_, State::Stuck) => State::Stuck,
        (State::Working, _) | (_, State::Working) => State::Working,
        _ => State::Idle,
    })
}

fn shown_mut(slot: &mut Loadable<Option<Transcript>>) -> Option<&mut Transcript> {
    match slot {
        Loadable::Ready(Some(shown)) | Loadable::Reloading(Some(shown), _) => Some(shown),
        _ => None,
    }
}

fn moves_detail(detail: &Loadable<Detail>, agent: u64, wrote: &Written) -> bool {
    match wrote {
        Written::Agent(number) => *number == agent,
        Written::Route => true,
        Written::Thread(id) | Written::Tally(id) | Written::Topic(id) => match detail.ready() {
            Some(detail) => detail.threads.iter().any(|(thread, _)| thread.id == *id),
            None => true,
        },
        Written::Frame { .. }
        | Written::Call { .. }
        | Written::Held(_)
        | Written::Withheld(_)
        | Written::Partial(_)
        | Written::Settling(_)
        | Written::Budget
        | Written::Armed(_)
        | Written::Runs(_) => false,
        Written::FinalError { agent: number, .. } => *number == agent,
    }
}

fn touched_of(thread: u64, agent: Option<u64>, written: &[Written]) -> Touched {
    let mut touched = Touched::default();
    for wrote in written {
        match wrote {
            Written::Thread(id) => touched.row |= *id == thread,
            Written::Frame { thread: id, .. } => touched.frames |= *id == thread,
            Written::Call { thread: id, call } => {
                if *id == thread {
                    touched.calls.insert(call.clone());
                }
            }
            Written::Held(id) => touched.held |= *id == thread,
            Written::Withheld(id) => touched.withheld |= *id == thread,
            Written::Partial(id) => touched.partial |= *id == thread,
            Written::Topic(id) => touched.topic |= *id == thread,
            Written::Tally(id) => touched.tally |= *id == thread,
            Written::Settling(id) => touched.settling |= *id == thread,
            Written::Agent(number) => touched.setup |= agent == Some(*number),
            Written::Route => touched.setup = true,
            Written::Budget => {}
            Written::Armed(id) => touched.armed |= *id == thread,
            Written::Runs(number) => touched.runs |= agent == Some(*number),
            Written::FinalError { thread: id, .. } => touched.failing |= *id == thread,
        }
    }
    touched
}

impl View for Agents {
    const NAME: &'static str = "Agents";
    const DESCRIPTION: &'static str =
        "The agents harness runs, and how each one worked: threads, tool calls and usage.";
    const ICON: &'static str = "icons/bot.svg";
    const TINT: Tint = Tint::Violet;
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Program,
        Capability::Op,
        Capability::Host,
        Capability::Link,
        Capability::Chain,
        Capability::Clipboard,
        Capability::Blob,
    ];
    const MIN_WINDOW_WIDTH: u32 = 480;

    fn attach(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pictures.is_none() {
            self.pictures = Some(cx.new(|_| view::pictures::Pictures::default()));
        }
        cx.follow::<HostSession>((), |view, session, cx| match session {
            Ok(session) => {
                if (view.me, &view.chain) != (session.account, &session.chain_id) {
                    let reader = view.me != session.account;
                    if view.chain != session.chain_id {
                        view.rooms.borrow_mut().clear();
                    }
                    view.me = session.account;
                    view.chain = session.chain_id;
                    if reader {
                        // another reader's roster: the one being read is the last one's
                        *view.roster = Loadable::Idle;
                        view.read_roster(cx);
                    }
                    cx.notify();
                }
            }
            Err(refusal) => cx.log_refused("the session", &refusal),
        })
        .detach();
        cx.follow::<HostRoute>((), |view, route, cx| match route {
            Ok(route) => view.open_route(&route, cx),
            Err(refusal) => {
                view.notice = Some(format!(
                    "Couldn’t follow the link: {}",
                    design::sentence(&refusal)
                ));
                cx.notify();
            }
        })
        .detach();
        cx.follow::<ChainHeads>((), |view, head, cx| match head {
            Ok(head) => view.ticked(head.time, cx),
            Err(refusal) => cx.log_refused("the chain's heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<harness::Harness>>((), |view, change, cx| match change {
            Ok(Some(change)) => view.heard(&change, cx),
            Ok(None) => view.refresh(cx),
            Err(refusal) => cx.log_refused("harness's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<Identity>>((), |view, change, cx| match change {
            Ok(_) => view.elsewhere(cx),
            Err(refusal) => cx.log_refused("identity's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<egress::Egress>>((), |view, change, cx| match change {
            Ok(_) => view.elsewhere(cx),
            Err(refusal) => cx.log_refused("egress's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<files::Files>>((), |view, change, cx| match change {
            Ok(_) => view.elsewhere(cx),
            Err(refusal) => cx.log_refused("files's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<ownership::Ownership>>((), |view, change, cx| match change {
            Ok(change) => view.ownership_moved(change, cx),
            Err(refusal) => cx.log_refused("ownership's live heads", &refusal),
        })
        .detach();
        self.refresh(cx);
    }
}

impl Render for Agents {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.width = window.viewport_size().width.into();
        self.settle_title(cx);
        self.settle_policies(cx);
        match window.touch() {
            true => ui::phone::render(self, cx),
            false => ui::render(self, cx),
        }
    }
}

impl Agents {
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.read_roster(cx);
        self.read_budget(cx);
        self.read_screen(cx);
    }

    fn heard(&mut self, change: &Change, cx: &mut Context<Self>) {
        let written: Vec<Written> = change
            .keys
            .iter()
            .filter_map(|key| harness::written(key.as_slice()))
            .collect();
        let roster = written.iter().any(|wrote| {
            matches!(
                wrote,
                Written::Thread(_) | Written::Agent(_) | Written::Topic(_) | Written::Route
            )
        });
        if roster {
            self.read_roster(cx);
        }
        let budget = written
            .iter()
            .any(|wrote| matches!(wrote, Written::Budget | Written::Route));
        if budget {
            self.read_budget(cx);
        }
        match self.screen {
            Screen::Agent(agent) | Screen::Part(agent, _) => {
                let moved = written
                    .iter()
                    .any(|wrote| moves_detail(&self.detail, agent, wrote));
                if moved {
                    self.read_detail(agent, cx);
                }
            }
            Screen::Thread(thread) | Screen::Usage(thread) => {
                let agent = self
                    .transcript
                    .ready()
                    .and_then(Option::as_ref)
                    .map(|shown| shown.thread.agent);
                self.follow_transcript(touched_of(thread, agent, &written), cx);
            }
            Screen::Create | Screen::Routes | Screen::SetRoute => {
                if written.contains(&Written::Route) {
                    self.read_screen(cx);
                }
            }
            Screen::List => {}
        }
    }

    fn elsewhere(&mut self, cx: &mut Context<Self>) {
        self.read_roster(cx);
        match self.screen {
            Screen::Thread(_) | Screen::Usage(_) => {
                let touched = Touched {
                    setup: true,
                    ..Touched::default()
                };
                self.follow_transcript(touched, cx);
            }
            _ => self.read_screen(cx),
        }
    }

    // a new head: its time is now, which says how long ago each conversation moved
    fn ticked(&mut self, time: u64, cx: &mut Context<Self>) {
        self.now.set(time);
        if let Some(roster) = self.roster.ready_mut()
            && roster.now != time
        {
            roster.now = time;
            cx.notify();
        }
    }

    // a change heard while the roster, the budget or the agent is read reads it once more when
    // that read lands
    fn read_roster(&mut self, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.roster,
            |view, cx| read::roster(cx.host(), view.rooms.clone(), view.now.clone()),
            Self::read_roster,
        );
    }

    fn read_budget(&mut self, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.budget,
            |_, cx| {
                let host = cx.host();
                async move { read::budget(&host).await }
            },
            Self::read_budget,
        );
    }

    fn read_detail(&mut self, agent: u64, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.detail,
            move |view, cx| read::detail(cx.host(), agent, view.rooms.clone()),
            Self::read_screen,
        );
    }

    fn read_screen(&mut self, cx: &mut Context<Self>) {
        let host = cx.host();
        let me = self.me;
        match self.screen {
            Screen::List => {}
            Screen::Create => cx.load(self, read::routes(host, me), |view| &mut view.routes),
            Screen::Routes => {
                cx.load(self, read::routes(host.clone(), me), |view| {
                    &mut view.routes
                });
                cx.load(self, read::offers(host), |view| &mut view.offers);
            }
            Screen::SetRoute => cx.load(self, read::offers(host), |view| &mut view.offers),
            Screen::Agent(agent) | Screen::Part(agent, _) => self.read_detail(agent, cx),
            Screen::Thread(thread) | Screen::Usage(thread) => self.open_transcript(thread, cx),
        }
    }

    fn open_transcript(&mut self, thread: u64, cx: &mut Context<Self>) {
        let from = match self.screen {
            Screen::Usage(_) => Some(1),
            _ => self
                .transcript
                .ready()
                .and_then(Option::as_ref)
                .map(|shown| shown.from),
        };
        let open = read::transcript(cx.host(), thread, from, self.rooms.clone());
        let task = cx.land(open, |view, opened, cx| {
            view.transcript = Loadable::from(opened);
            view.read_delta(cx);
            cx.notify();
        });
        self.transcript = match self.transcript.take() {
            Loadable::Ready(shown) | Loadable::Reloading(shown, _) => {
                Loadable::Reloading(shown, task)
            }
            Loadable::Idle | Loadable::Loading(_) | Loadable::Failed(_) => {
                cx.notify();
                Loadable::Loading(task)
            }
        };
    }

    fn follow_transcript(&mut self, touched: Touched, cx: &mut Context<Self>) {
        self.touched.add(touched);
        self.read_delta(cx);
    }

    fn read_delta(&mut self, cx: &mut Context<Self>) {
        if self.following.is_some() {
            return;
        }
        let Some(Some(shown)) = self.transcript.ready() else {
            return;
        };
        let (thread, agent, next) = (shown.thread.id, shown.thread.agent, shown.next());
        let mut touched = std::mem::take(&mut self.touched);
        touched.calls.retain(|call| shown.asked.contains(call));
        if !touched.any() {
            return;
        }
        let read = read::delta(cx.host(), thread, agent, next, touched);
        self.following = Some(cx.land(read, |view, delta, cx| {
            view.following = None;
            match delta {
                Ok(delta) => {
                    if let Some(shown) = shown_mut(&mut view.transcript) {
                        shown.moved(delta);
                    }
                    cx.notify();
                }
                Err(refusal) => cx.log_refused("the conversation's changes", &refusal),
            }
            view.read_delta(cx);
        }));
    }

    pub(crate) fn want_from(&self, seq: u64) {
        let mut wanted = self.wanted.borrow_mut();
        wanted.from = Some(wanted.from.map_or(seq, |from| from.min(seq)));
    }

    pub(crate) fn want_call(&self, call: &str) {
        self.wanted.borrow_mut().calls.insert(call.to_owned());
    }

    pub(crate) fn read_wanted(&mut self, cx: &mut Context<Self>) {
        self.read_earlier(cx);
        self.read_calls(cx);
    }

    fn read_earlier(&mut self, cx: &mut Context<Self>) {
        if self.earlier.is_some() {
            return;
        }
        let Some(Some(shown)) = self.transcript.ready() else {
            return;
        };
        let (thread, before) = (shown.thread.id, shown.from);
        let wanted = self.wanted.borrow_mut().from.take();
        let Some(from) = wanted.filter(|from| *from < before) else {
            return;
        };
        let read = read::earlier(cx.host(), thread, from, before);
        self.earlier = Some(cx.land(read, |view, earlier, cx| {
            view.earlier = None;
            match earlier {
                Ok(earlier) => {
                    if let Some(shown) = shown_mut(&mut view.transcript) {
                        shown.reached(earlier);
                    }
                    cx.notify();
                }
                Err(refusal) => cx.log_refused("the conversation's earlier frames", &refusal),
            }
            view.read_earlier(cx);
        }));
    }

    fn read_calls(&mut self, cx: &mut Context<Self>) {
        if self.calling.is_some() {
            return;
        }
        let Some(Some(shown)) = self.transcript.ready() else {
            return;
        };
        let thread = shown.thread.id;
        let wanted = std::mem::take(&mut self.wanted.borrow_mut().calls);
        let calls: Vec<String> = wanted
            .into_iter()
            .filter(|call| !shown.asked.contains(call))
            .collect();
        if calls.is_empty() {
            return;
        }
        let read = read::calls(cx.host(), thread, calls);
        self.calling = Some(cx.land(read, |view, calls, cx| {
            view.calling = None;
            match calls {
                Ok(calls) => {
                    if let Some(shown) = shown_mut(&mut view.transcript) {
                        shown.called(calls);
                    }
                    cx.notify();
                }
                Err(refusal) => cx.log_refused("the conversation's calls", &refusal),
            }
            view.read_calls(cx);
        }));
    }

    pub(crate) fn rows_for(&self, thread: &ThreadRow) -> ListState {
        let count = thread.frames as usize + 1;
        let mut rows = self.rows.borrow_mut();
        if let Some((id, state)) = rows.as_ref()
            && *id == thread.id
        {
            let held = state.item_count();
            match held.cmp(&count) {
                Ordering::Less => state.splice(held - 1..held - 1, count - held),
                Ordering::Greater => state.reset(count),
                Ordering::Equal => {}
            }
            return state.clone();
        }
        let state = ListState::new(count, ListAlignment::Bottom, px(0.));
        state.set_follow_mode(FollowMode::Tail);
        *rows = Some((thread.id, state.clone()));
        state
    }

    pub(crate) fn go(&mut self, screen: Screen, cx: &mut Context<Self>) {
        if screen.agent() != self.screen.agent() {
            *self.detail = Loadable::Idle;
            self.setup_draft = SetupDraft::default();
        }
        if screen.thread() != self.screen.thread() {
            self.transcript = Loadable::Idle;
            self.touched = Touched::default();
            self.following = None;
            self.earlier = None;
            self.calling = None;
            *self.wanted.borrow_mut() = Wanted::default();
            *self.rows.borrow_mut() = None;
        }
        self.screen = screen;
        self.arming = None;
        self.menu = None;
        self.sheet = None;
        self.conversations_policy.sheet = None;
        self.route_policy.sheet = None;
        self.notice = None;
        self.read_screen(cx);
        cx.notify();
    }

    pub(crate) fn parent(&self) -> Screen {
        match self.screen {
            Screen::List => Screen::List,
            Screen::Agent(_) | Screen::Routes => Screen::List,
            Screen::Create => match (self.draft.again, self.draft.created) {
                (true, Some(agent)) => Screen::Agent(agent),
                _ => Screen::List,
            },
            Screen::Thread(_) => match self.transcript.ready() {
                Some(Some(shown)) => Screen::Agent(shown.thread.agent),
                _ => Screen::List,
            },
            Screen::Part(agent, _) => Screen::Agent(agent),
            Screen::Usage(thread) => Screen::Thread(thread),
            Screen::SetRoute => Screen::Routes,
        }
    }

    pub(crate) fn back(&mut self, cx: &mut Context<Self>) {
        if self.policy_sheet_open() {
            return self.close_policy_sheet(cx);
        }
        if self.sheet.take().is_some() {
            self.arming = None;
            return cx.notify();
        }
        self.go(self.parent(), cx);
    }

    pub(crate) fn show_sheet(&mut self, sheet: Option<Sheet>, cx: &mut Context<Self>) {
        self.sheet = sheet;
        self.arming = None;
        cx.notify();
    }

    pub(crate) fn copy(&mut self, text: String, cx: &mut Context<Self>) {
        let ask = cx.host().ask::<ClipboardWrite>(text);
        cx.land(ask, |view, copied, cx| {
            if let Err(refusal) = copied {
                view.notice = Some(format!("Couldn’t copy: {}", design::sentence(&refusal)));
                cx.notify();
            }
        })
        .detach();
    }

    fn open_route(&mut self, route: &str, cx: &mut Context<Self>) {
        if let Some(agent) = links::create_target(route) {
            return self.continue_create(agent, cx);
        }
        let Some((agent, place)) = links::route_target(route) else {
            self.notice = Some(format!(
                "This view opens no link like “{route}”: open a harness conversation link"
            ));
            cx.notify();
            return;
        };
        let Some(place) = place else {
            self.routing = None;
            return self.go(Screen::Agent(agent), cx);
        };
        let finding = read::thread_at(cx.host(), agent, place.clone());
        self.routing = Some(cx.land(finding, move |view, found, cx| {
            view.routing = None;
            match found {
                Ok(Some(thread)) => view.go(Screen::Thread(thread), cx),
                Ok(None) => {
                    view.go(Screen::Agent(agent), cx);
                    let roster = view.roster.ready();
                    let name = roster.map_or_else(|| format!("Agent {agent}"), |r| r.name(agent));
                    let place = words::place_label(agent, &place, roster);
                    view.notice = Some(format!("{name} has no conversation in {place} yet."));
                }
                Err(refusal) => {
                    view.notice = Some(format!(
                        "Couldn’t open the link: {}",
                        design::sentence(&refusal)
                    ));
                    cx.notify();
                }
            }
        }));
    }

    fn continue_create(&mut self, agent: u64, cx: &mut Context<Self>) {
        let host = cx.host();
        let named = async move { host.query(identity::ask::Profile { number: agent }).await };
        self.routing = Some(cx.land(named, move |view, profile, cx| {
            view.routing = None;
            match profile {
                Ok(Some(profile)) => match &profile.kind {
                    identity::Kind::Program {
                        script: Some(script),
                        ..
                    } if script.calls == harness::PROGRAM => view.go(Screen::Agent(agent), cx),
                    identity::Kind::Program {
                        manager,
                        category: identity::Category::Agent,
                        script: None,
                        ..
                    } if Some(*manager) == view.me => view.resume(agent, &profile.name, cx),
                    _ => {
                        view.notice = Some(format!(
                            "{} isn't an agent of yours waiting for harness.",
                            profile.name
                        ));
                        cx.notify();
                    }
                },
                Ok(None) => {
                    view.notice = Some(format!("No account {agent} to hand to harness."));
                    cx.notify();
                }
                Err(refusal) => {
                    view.notice = Some(format!(
                        "Couldn’t open the link: {}",
                        design::sentence(&refusal)
                    ));
                    cx.notify();
                }
            }
        }));
    }

    pub(crate) fn begin_rename(&mut self, agent: u64, cx: &mut Context<Self>) {
        self.rename = TextField::new(self.agent_name(agent));
        self.arming = Some(Arm::Rename(agent));
        cx.notify();
    }

    pub(crate) fn open_menu(&mut self, agent: u64, cx: &mut Context<Self>) {
        self.menu = Some(AgentMenu {
            agent,
            at: None,
            active: 0,
        });
        cx.notify();
    }

    pub(crate) fn place_menu(
        &mut self,
        button: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = &mut self.menu else {
            return;
        };
        if menu.at.is_none() {
            menu.at = Some(button.bottom_left());
            window.focus(AGENT_MENU);
            cx.notify();
        }
    }

    pub(crate) fn move_in_menu(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.menu {
            menu.active = index;
            cx.notify();
        }
    }

    pub(crate) fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            window.focus("agent-more");
            cx.notify();
        }
    }

    pub(crate) fn run_menu(&mut self, arm: Arm, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(window, cx);
        match arm {
            Arm::Suspend(_) | Arm::Resume(_) => self.act(arm, cx),
            Arm::Remove(_) | Arm::Revoke(_) | Arm::Rename(_) | Arm::Close(_) => {
                self.arming = Some(arm);
                cx.notify();
            }
        }
    }

    pub(crate) fn act(&mut self, arm: Arm, cx: &mut Context<Self>) {
        if self.acting.is_some() {
            return;
        }
        let name = self.rename.text().trim().to_owned();
        let unnamed = matches!(arm, Arm::Rename(_)) && name.is_empty();
        if unnamed {
            self.notice = Some("Name the agent.".to_owned());
            return cx.notify();
        }
        self.doing = Some(match arm {
            Arm::Remove(agent) => format!("Taking {} back", self.agent_name(agent)),
            Arm::Close(_) => "Closing the conversation".to_owned(),
            Arm::Rename(agent) => format!("Renaming {}", self.agent_name(agent)),
            Arm::Suspend(agent) => format!("Suspending {}", self.agent_name(agent)),
            Arm::Resume(agent) => format!("Resuming {}", self.agent_name(agent)),
            Arm::Revoke(agent) => format!("Revoking {}", self.agent_name(agent)),
        });
        let acted =
            move |view: &mut Self, answer, cx: &mut Context<Self>| view.acted(arm, answer, cx);
        let op = match arm {
            Arm::Close(thread) => {
                let op = harness::Op::Close { thread };
                self.acting = Some(cx.land(cx.host().ask::<Submit<harness::Harness>>(op), acted));
                return cx.notify();
            }
            Arm::Remove(account) => identity::Op::SetScript {
                account,
                script: None,
            },
            Arm::Rename(account) => identity::Op::SetName { account, name },
            Arm::Suspend(account) => identity::Op::Suspend { account },
            Arm::Resume(account) => identity::Op::Resume { account },
            Arm::Revoke(account) => identity::Op::Revoke { account },
        };
        self.acting = Some(cx.land(cx.host().ask::<Submit<Identity>>(op), acted));
        cx.notify();
    }

    pub(crate) fn converse(&mut self, op: harness::Op, cx: &mut Context<Self>) {
        if self.acting.is_some() {
            return;
        }
        self.doing = Some(
            match op {
                harness::Op::ForkThread { .. } => "Forking the conversation",
                harness::Op::ClaimConversations { .. } => "Claiming who starts its conversations",
                _ => "Starting a new conversation",
            }
            .to_owned(),
        );
        self.sheet = None;
        let submit = cx.host().ask::<Submit<harness::Harness>>(op);
        self.acting = Some(cx.land(submit, |view, answer, cx| {
            view.acting = None;
            view.doing = None;
            match applied(answer) {
                Ok(_) => view.read_screen(cx),
                Err(problem) => view.notice = Some(problem),
            }
            view.read_roster(cx);
            cx.notify();
        }));
        cx.notify();
    }

    fn acted(&mut self, arm: Arm, answer: Result<Submitted, Error>, cx: &mut Context<Self>) {
        self.acting = None;
        self.doing = None;
        self.arming = None;
        self.sheet = None;
        match (applied(answer), arm) {
            (Ok(_), Arm::Remove(_)) => self.go(Screen::List, cx),
            (
                Ok(_),
                Arm::Close(_) | Arm::Rename(_) | Arm::Suspend(_) | Arm::Resume(_) | Arm::Revoke(_),
            ) => self.read_screen(cx),
            (Err(problem), _) => self.notice = Some(problem),
        }
        self.read_roster(cx);
        cx.notify();
    }

    pub(crate) fn home(&self) -> Option<&str> {
        let me = self.me?;
        self.roster.ready()?.names.get(&me).map(String::as_str)
    }

    pub(crate) fn picture(&self, number: u64, cx: &mut Context<Self>) -> Picture {
        let blob = self.roster.ready()?.avatars.get(&number)?;
        let pictures = self.pictures.as_ref()?;
        pictures.update(cx, |pictures, cx| pictures.get(blob, cx))
    }

    pub(crate) fn author_picture(&self, body: &harness::Body, cx: &mut Context<Self>) -> Picture {
        match body {
            harness::Body::Post {
                author: harness::Principal::Account(number),
                ..
            } => self.picture(*number, cx),
            _ => None,
        }
    }

    pub(crate) fn agent_name(&self, agent: u64) -> String {
        self.roster
            .ready()
            .map_or_else(|| format!("account {agent}"), |roster| roster.name(agent))
    }

    fn settle_title(&mut self, cx: &mut Context<Self>) {
        let roster = self.roster.ready();
        let title = match self.screen {
            Screen::List => "Agents".to_owned(),
            Screen::Create if self.draft.again => format!("Edit {}", self.draft.name.text()),
            Screen::Create => "Create an agent".to_owned(),
            Screen::Routes => "Routes".to_owned(),
            Screen::SetRoute => "Set a route".to_owned(),
            Screen::Agent(agent) | Screen::Part(agent, _) => self.agent_name(agent),
            Screen::Thread(id) | Screen::Usage(id) => match self.transcript.ready() {
                Some(Some(shown)) => format!(
                    "{} · {}",
                    self.agent_name(shown.thread.agent),
                    words::conversation_label(&shown.thread, roster)
                ),
                _ => format!("Conversation {id}"),
            },
        };
        if self.title.as_deref() != Some(title.as_str()) {
            cx.host().notify::<HostTitle>(title.clone());
            self.title = Some(title);
        }
    }
}

export_view!(Agents);

#[cfg(test)]
mod tests;
