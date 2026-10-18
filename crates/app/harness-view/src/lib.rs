//! Agents: the program accounts whose script calls `harness`, and how each one worked. The
//! list shows every agent with its state; an agent shows its conversations, then its setup (its
//! configuration file, what harness resolves from it, whom it answers) and what its turns used;
//! a conversation shows its transcript, read only, a burst of tool calls folded to one line; the
//! models page lists harness's model records and sets one. People talk to an agent in chat; the
//! writes here are a manager's (create, edit its configuration, take it back from harness,
//! start, fork or close a conversation) and a model record's owner's.
//!
//! - `read`: every read the screens make; `links`: the links in and out.
//! - `create`: the create form and the ops it sends in order; `models`: the models form.
//! - `timeline`: a transcript's frames grouped into what a person reads.
//! - `ui`: the screens, the desktop's beside the phone's (`ui/phone`); the root picks one by
//!   the device.
mod create;
mod links;
mod models;
mod read;
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
use models::ModelDraft;
use read::{Detail, Model, Offer, Roster, Touched, Transcript};

#[derive(Default, Serialize, Deserialize)]
pub struct Agents {
    screen: Screen,
    draft: Draft,
    model_draft: ModelDraft,
    /// a manager's write waiting for its second press
    arming: Option<Arm>,
    /// the folds turned from how they start: a transcript's rows, an agent's setup
    open: BTreeSet<String>,
    /// the phone's sheet over the page
    sheet: Option<Sheet>,
    #[serde(skip)]
    me: Option<u64>,
    #[serde(skip)]
    chain: String,
    #[serde(skip)]
    roster: Loadable<Roster>,
    #[serde(skip)]
    budget: Loadable<harness::BudgetReport>,
    #[serde(skip)]
    detail: Loadable<Detail>,
    #[serde(skip)]
    transcript: Loadable<Option<Transcript>>,
    #[serde(skip)]
    models: Loadable<Vec<Model>>,
    #[serde(skip)]
    offers: Loadable<Vec<Offer>>,
    /// why a write or a link did not land
    #[serde(skip)]
    notice: Option<String>,
    #[serde(skip)]
    acting: Option<Task<()>>,
    /// what the write in flight does, in words: "Taking Scout back"
    #[serde(skip)]
    doing: Option<String>,
    #[serde(skip)]
    routing: Option<Task<()>>,
    // the status reads a Refresh press sent, while they land
    #[serde(skip)]
    reading: Option<Task<()>>,
    #[serde(skip)]
    title: Option<String>,
    // a change heard while the roster or the agent is read reads it once more when it lands
    #[serde(skip)]
    again: Again,
    // what blocks wrote to the open conversation since its last read of them, and that read
    #[serde(skip)]
    touched: Touched,
    #[serde(skip)]
    following: Option<Task<()>>,
    // the conversation's frames above those read, and its calls on screen, being read
    #[serde(skip)]
    earlier: Option<Task<()>>,
    #[serde(skip)]
    calling: Option<Task<()>>,
    // what the list asked for as it drew: the frames above those read, the calls it drew
    #[serde(skip)]
    wanted: RefCell<Wanted>,
    // the open conversation's list: its thread and the rows it holds, a row a frame
    #[serde(skip)]
    rows: RefCell<Option<(u64, ListState)>>,
}

#[derive(Default)]
struct Again {
    roster: Rc<Cell<bool>>,
    detail: Rc<Cell<bool>>,
    budget: Rc<Cell<bool>>,
}

#[derive(Default)]
pub(crate) struct Wanted {
    from: Option<u64>,
    calls: BTreeSet<String>,
}

/// Where the reader is. The desktop draws each screen whole; the phone draws them as a stack of
/// pages, its depth fixed by the screen, and adds the pages a desktop screen holds inline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Screen {
    #[default]
    List,
    Create,
    Agent(u64),
    Thread(u64),
    Models,
    /// the phone's page of one part of an agent's setup
    Part(u64, Part),
    /// the phone's page of a conversation's usage and context
    Usage(u64),
    /// the phone's page of the form that sets a model
    SetModel,
}

/// A part of an agent's setup the phone opens as its own page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Part {
    /// its model, thinking and tools
    Model,
    /// its configuration file, who may change it and whom it answers
    File,
    Prompt,
    Usage,
}

/// The phone's sheets: an agent's or a conversation's actions, and one frame's facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Sheet {
    Agent,
    Thread,
    Frame(u64),
}

impl Screen {
    /// The agent whose detail the screen reads.
    fn agent(self) -> Option<u64> {
        match self {
            Screen::Agent(agent) | Screen::Part(agent, _) => Some(agent),
            _ => None,
        }
    }

    /// The thread whose transcript the screen reads.
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
}

/// An agent's or a thread's state, as its badge says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Idle,
    Working,
    /// still asking while input waits behind it
    Stuck,
    /// no member node serves its model's service, or relay
    NoNode,
    Unconfigured,
}

/// A write's answer: the program's output once the node predicts it applies, else why not,
/// in every view's words (`design::failure`), shown where the write was made.
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

/// An agent's state: its configuration and a node before anything, then the busiest of its
/// open threads.
pub(crate) fn agent_state(card: &read::Card, roster: &Roster) -> State {
    let Some(serving) = card.serving else {
        return State::Unconfigured;
    };
    if serving == 0 || roster.relaying == 0 {
        return State::NoNode;
    }
    let states = card.threads.iter().map(thread_state);
    states.fold(State::Idle, |state, next| match (state, next) {
        (State::Stuck, _) | (_, State::Stuck) => State::Stuck,
        (State::Working, _) | (_, State::Working) => State::Working,
        _ => State::Idle,
    })
}

fn busy<T>(slot: &Loadable<T>) -> bool {
    matches!(slot, Loadable::Loading(_) | Loadable::Reloading(..))
}

fn shown_mut(slot: &mut Loadable<Option<Transcript>>) -> Option<&mut Transcript> {
    match slot {
        Loadable::Ready(Some(shown)) | Loadable::Reloading(Some(shown), _) => Some(shown),
        _ => None,
    }
}

// whether a key a block wrote moves the agent screen of `agent`
fn moves_detail(detail: &Loadable<Detail>, agent: u64, wrote: &Written) -> bool {
    match wrote {
        Written::Agent(number) => *number == agent,
        Written::Model => true,
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
        | Written::Armed(_) => false,
    }
}

// what the keys a block wrote touch of conversation `thread`, whose agent is `agent`
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
            Written::Model => touched.setup = true,
            Written::Budget => {}
            Written::Armed(id) => touched.armed |= *id == thread,
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
        // the newest block's time: how long ago a conversation moved
        Capability::Chain,
        // an id or a link, copied
        Capability::Clipboard,
    ];
    const TARGETS: &'static [&'static str] = &[
        harness::PROGRAM,
        identity::PROGRAM,
        egress::PROGRAM,
        files::PROGRAM,
        access::PROGRAM,
        chat::PROGRAM,
    ];
    const MIN_WINDOW_WIDTH: u32 = 480;

    fn attach(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        cx.follow::<HostSession>((), |view, session, cx| match session {
            Ok(session) => {
                if (view.me, &view.chain) != (session.account, &session.chain_id) {
                    let reader = view.me != session.account;
                    view.me = session.account;
                    view.chain = session.chain_id;
                    if reader {
                        // another reader's roster: the one being read is the last one's
                        view.roster = Loadable::Idle;
                        view.read_roster(cx);
                    }
                    cx.notify();
                }
            }
            Err(refusal) => cx.log_refused("the session", &refusal),
        })
        .detach();
        // `duck://<chain>/harness/<agent>/<place>`: a work-record link opens that thread
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
        cx.follow::<Changes<access::Access>>((), |view, change, cx| match change {
            Ok(_) => view.elsewhere(cx),
            Err(refusal) => cx.log_refused("access's live heads", &refusal),
        })
        .detach();
        self.refresh(cx);
    }
}

impl Render for Agents {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.settle_title(cx);
        // two designs, picked by the device: under a finger the phone's pages, under a
        // pointer the desktop's screens
        match window.touch() {
            true => ui::phone::render(self, cx),
            false => ui::render(self, cx),
        }
    }
}

impl Agents {
    /// Everything on screen read again: the roster, and what the screen reads besides.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.read_roster(cx);
        self.read_budget(cx);
        self.read_screen(cx);
    }

    // a block of harness: what it wrote is read again where the screen shows it, and nothing
    // else is
    fn heard(&mut self, change: &Change, cx: &mut Context<Self>) {
        let written: Vec<Written> = change
            .keys
            .iter()
            .filter_map(|key| harness::written(key.as_slice()))
            .collect();
        let roster = written.iter().any(|wrote| {
            matches!(
                wrote,
                Written::Thread(_) | Written::Agent(_) | Written::Topic(_) | Written::Model
            )
        });
        if roster {
            self.read_roster(cx);
        }
        if written.contains(&Written::Budget) {
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
            Screen::Create | Screen::Models | Screen::SetModel => {
                if written.contains(&Written::Model) {
                    self.read_screen(cx);
                }
            }
            Screen::List => {}
        }
    }

    // a block of another program the screens read: the roster, and the screen's reads besides
    // the conversation's own frames and calls
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

    fn read_roster(&mut self, cx: &mut Context<Self>) {
        if busy(&self.roster) {
            self.again.roster.set(true);
            return;
        }
        let (host, me, again) = (cx.host(), self.me, self.again.roster.clone());
        let read = async move {
            loop {
                again.set(false);
                let roster = read::roster(host.clone(), me).await;
                if !again.get() {
                    return roster;
                }
            }
        };
        cx.load(self, read, |view| &mut view.roster);
    }

    fn read_budget(&mut self, cx: &mut Context<Self>) {
        if busy(&self.budget) {
            self.again.budget.set(true);
            return;
        }
        let (host, again) = (cx.host(), self.again.budget.clone());
        let read = async move {
            loop {
                again.set(false);
                let budget = read::budget(&host).await;
                if !again.get() {
                    return budget;
                }
            }
        };
        cx.load(self, read, |view| &mut view.budget);
    }

    fn read_detail(&mut self, agent: u64, cx: &mut Context<Self>) {
        if busy(&self.detail) {
            self.again.detail.set(true);
            return;
        }
        let (host, again) = (cx.host(), self.again.detail.clone());
        let read = async move {
            loop {
                again.set(false);
                let detail = read::detail(host.clone(), agent).await;
                if !again.get() {
                    return detail;
                }
            }
        };
        cx.load(self, read, |view| &mut view.detail);
    }

    fn read_screen(&mut self, cx: &mut Context<Self>) {
        let host = cx.host();
        match self.screen {
            Screen::List => {}
            Screen::Create => cx.load(self, read::models(host), |view| &mut view.models),
            Screen::Models => {
                cx.load(self, read::models(host.clone()), |view| &mut view.models);
                cx.load(self, read::offers(host), |view| &mut view.offers);
            }
            Screen::SetModel => cx.load(self, read::offers(host), |view| &mut view.offers),
            Screen::Agent(agent) | Screen::Part(agent, _) => self.read_detail(agent, cx),
            Screen::Thread(thread) | Screen::Usage(thread) => self.open_transcript(thread, cx),
        }
    }

    // the conversation read anew, as far as its list held it; the phone's usage page holds
    // every turn
    fn open_transcript(&mut self, thread: u64, cx: &mut Context<Self>) {
        let from = match self.screen {
            Screen::Usage(_) => Some(1),
            _ => self
                .transcript
                .ready()
                .and_then(Option::as_ref)
                .map(|shown| shown.from),
        };
        let open = read::transcript(cx.host(), thread, from);
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

    // what blocks wrote to the open conversation since, read once the read before it lands
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

    // the list's row for frame `seq`, above the frames read: read from there on
    pub(crate) fn want_from(&self, seq: u64) {
        let mut wanted = self.wanted.borrow_mut();
        wanted.from = Some(wanted.from.map_or(seq, |from| from.min(seq)));
    }

    pub(crate) fn want_call(&self, call: &str) {
        self.wanted.borrow_mut().calls.insert(call.to_owned());
    }

    // what the list asked for as it drew, read
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

    // the open conversation's list, a row a frame and one after them, kept as its frames grow
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
        // what the next screen reads of the same agent or thread stays on screen while it is
        // read again
        if screen.agent() != self.screen.agent() {
            self.detail = Loadable::Idle;
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
        self.sheet = None;
        self.notice = None;
        self.read_screen(cx);
        cx.notify();
    }

    /// The screen this one was opened from.
    pub(crate) fn parent(&self) -> Screen {
        match self.screen {
            Screen::List => Screen::List,
            Screen::Agent(_) | Screen::Models => Screen::List,
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
            Screen::SetModel => Screen::Models,
        }
    }

    /// Back: the sheet on screen closes, else the screen this one was opened from opens.
    pub(crate) fn back(&mut self, cx: &mut Context<Self>) {
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

    /// `text` onto the clipboard: a frame's id, a link. A refusal says so where writes do.
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
        let Some((agent, place)) = links::route_target(route) else {
            self.notice = Some(format!("This view opens no link like “{route}”."));
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

    /// The manager's write, sent once its second press confirms it.
    pub(crate) fn act(&mut self, arm: Arm, cx: &mut Context<Self>) {
        if self.acting.is_some() {
            return;
        }
        self.doing = Some(match arm {
            Arm::Remove(agent) => format!("Taking {} back", self.agent_name(agent)),
            Arm::Close(_) => "Closing the conversation".to_owned(),
        });
        let acted =
            move |view: &mut Self, answer, cx: &mut Context<Self>| view.acted(arm, answer, cx);
        let task = match arm {
            Arm::Remove(account) => {
                let op = identity::Op::SetScript {
                    account,
                    script: None,
                };
                cx.land(cx.host().ask::<Submit<Identity>>(op), acted)
            }
            Arm::Close(thread) => {
                let op = harness::Op::Close { thread };
                cx.land(cx.host().ask::<Submit<harness::Harness>>(op), acted)
            }
        };
        self.acting = Some(task);
        cx.notify();
    }

    // each service's status of its model accounts, read on a press
    pub(crate) fn read_status(&mut self, cx: &mut Context<Self>) {
        if self.reading.is_some() {
            return;
        }
        let services = self.budget.ready().map(ui::budget::services);
        let host = cx.host();
        let reads = async move {
            for service in services.into_iter().flatten() {
                let op = harness::Op::ReadBudget { service };
                if let Err(problem) = applied(host.ask::<Submit<harness::Harness>>(op).await) {
                    return Ok::<_, Error>(Some(problem));
                }
            }
            Ok(None)
        };
        self.reading = Some(cx.land(reads, |view, read, cx| {
            view.reading = None;
            if let Ok(Some(problem)) = read {
                view.notice = Some(problem);
            }
            cx.notify();
        }));
        cx.notify();
    }

    pub(crate) fn converse(&mut self, op: harness::Op, cx: &mut Context<Self>) {
        if self.acting.is_some() {
            return;
        }
        self.doing = Some(
            match op {
                harness::Op::ForkThread { .. } => "Forking the conversation",
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
            (Ok(_), Arm::Close(_)) => self.read_screen(cx),
            (Err(problem), _) => self.notice = Some(problem),
        }
        self.read_roster(cx);
        cx.notify();
    }

    pub(crate) fn home(&self) -> Option<&str> {
        let me = self.me?;
        self.roster.ready()?.names.get(&me).map(String::as_str)
    }

    /// An account's name once the roster is read, else its number.
    pub(crate) fn agent_name(&self, agent: u64) -> String {
        self.roster
            .ready()
            .map_or_else(|| format!("account {agent}"), |roster| roster.name(agent))
    }

    /// The window's title: `Agents` over the list, an agent's name, a conversation's heading
    /// after its agent's name.
    fn settle_title(&mut self, cx: &mut Context<Self>) {
        let roster = self.roster.ready();
        let title = match self.screen {
            Screen::List => "Agents".to_owned(),
            Screen::Create if self.draft.again => format!("Edit {}", self.draft.name.text()),
            Screen::Create => "Create an agent".to_owned(),
            Screen::Models => "Models".to_owned(),
            Screen::SetModel => "Set a model".to_owned(),
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
