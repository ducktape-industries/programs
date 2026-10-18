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

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use view::methods::{ClipboardWrite, Submitted};
use view::prelude::*;

use create::Draft;
use harness::{Asking, ThreadRow};
use identity::Identity;
use models::ModelDraft;
use read::{Detail, Model, Offer, Roster, Transcript};

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
    #[serde(skip)]
    title: Option<String>,
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
            Ok(_) => view.refresh(cx),
            Err(refusal) => cx.log_refused("harness's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<Identity>>((), |view, change, cx| match change {
            Ok(_) => view.refresh(cx),
            Err(refusal) => cx.log_refused("identity's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<egress::Egress>>((), |view, change, cx| match change {
            Ok(_) => view.refresh(cx),
            Err(refusal) => cx.log_refused("egress's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<files::Files>>((), |view, change, cx| match change {
            Ok(_) => view.refresh(cx),
            Err(refusal) => cx.log_refused("files's live heads", &refusal),
        })
        .detach();
        cx.follow::<Changes<access::Access>>((), |view, change, cx| match change {
            Ok(_) => view.refresh(cx),
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
        self.read_screen(cx);
    }

    fn read_roster(&mut self, cx: &mut Context<Self>) {
        cx.load(self, read::roster(cx.host(), self.me), |view| {
            &mut view.roster
        });
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
            Screen::Agent(agent) | Screen::Part(agent, _) => {
                cx.load(self, read::detail(host, agent), |view| &mut view.detail)
            }
            Screen::Thread(thread) | Screen::Usage(thread) => {
                cx.load(self, read::transcript(host, thread), |view| {
                    &mut view.transcript
                })
            }
        }
    }

    pub(crate) fn go(&mut self, screen: Screen, cx: &mut Context<Self>) {
        // what the next screen reads of the same agent or thread stays on screen while it is
        // read again
        if screen.agent() != self.screen.agent() {
            self.detail = Loadable::Idle;
        }
        if screen.thread() != self.screen.thread() {
            self.transcript = Loadable::Idle;
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
