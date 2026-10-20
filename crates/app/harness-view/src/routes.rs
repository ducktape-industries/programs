//! The routes form: a provider's route set, edited or removed. Every press goes to
//! `Agents::press_route`, which hands it to `RouteDraft::press` and sends the op it names once the
//! person said yes. Who may change a route is harness's to say.
use serde::{Deserialize, Serialize};
use view::prelude::*;

use harness::{Configured, ProviderRouteRow, RouteService, Wire};

use crate::read::Roster;
use crate::{Agents, Screen, applied};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct RouteDraft {
    pub provider: TextField,
    // the wire the next service picked speaks, as its place in `Wire::ALL`
    pub wire: usize,
    // the services that reach it, in the order harness tries them, each with its wire's place
    pub services: Vec<(String, usize)>,
    // the route as the form opened it; none for a new one
    pub was: Option<Vec<(String, usize)>>,
    // the desktop's form is open under the list
    pub shown: bool,
    pub asking: Option<Yes>,
    pub problem: Option<String>,
    #[serde(skip)]
    pub sending: Option<Task<()>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Yes {
    Save,
    Remove,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Press {
    Toggle,
    New,
    Open(ProviderRouteRow),
    Wire(usize),
    Service(String),
    Ask(Yes),
    Yes,
    Unask,
    Sent(Option<String>),
}

pub(crate) enum Then {
    Stay,
    Send(harness::Op),
    Done,
}

pub(crate) fn wire_at(place: usize) -> Wire {
    Wire::ALL.get(place).copied().unwrap_or(Wire::Harness)
}

fn place_of(wire: Wire) -> usize {
    Wire::ALL
        .iter()
        .position(|listed| *listed == wire)
        .unwrap_or_default()
}

impl RouteDraft {
    pub fn provider(&self) -> String {
        self.provider.text().trim().to_owned()
    }

    pub fn route(&self) -> Vec<RouteService> {
        let services = self.services.iter().map(|(service, wire)| RouteService {
            service: service.clone(),
            wire: wire_at(*wire),
        });
        services.collect()
    }

    pub fn press(&mut self, press: Press) -> Then {
        if self.sending.is_some() && !matches!(press, Press::Sent(_)) {
            return Then::Stay;
        }
        match press {
            Press::Toggle => {
                let shown = !self.shown;
                *self = RouteDraft {
                    shown,
                    ..RouteDraft::default()
                };
            }
            Press::New => {
                *self = RouteDraft {
                    shown: true,
                    ..RouteDraft::default()
                }
            }
            Press::Open(row) => *self = RouteDraft::opened(row),
            Press::Wire(place) => self.wire = place,
            Press::Service(name) => {
                let at = self.services.iter().position(|(picked, _)| *picked == name);
                match at {
                    Some(at) => {
                        self.services.remove(at);
                    }
                    None => self.services.push((name, self.wire)),
                }
            }
            Press::Ask(yes) => {
                let checked = match yes {
                    Yes::Save => self.op().map(|_| ()),
                    Yes::Remove => Ok(()),
                };
                match checked {
                    Ok(()) => {
                        self.problem = None;
                        self.asking = Some(yes);
                    }
                    Err(problem) => self.problem = Some(problem),
                }
            }
            Press::Unask => self.asking = None,
            Press::Yes => {
                let op = match self.asking {
                    Some(Yes::Save) => self.op(),
                    Some(Yes::Remove) => Ok(harness::Op::RemoveRoute {
                        provider: self.provider(),
                    }),
                    None => return Then::Stay,
                };
                match op {
                    Ok(op) => return Then::Send(op),
                    Err(problem) => {
                        self.asking = None;
                        self.problem = Some(problem);
                    }
                }
            }
            Press::Sent(None) => {
                *self = RouteDraft::default();
                return Then::Done;
            }
            Press::Sent(Some(problem)) => {
                self.sending = None;
                self.asking = None;
                self.problem = Some(problem);
            }
        }
        Then::Stay
    }

    fn opened(row: ProviderRouteRow) -> RouteDraft {
        let services: Vec<(String, usize)> = row
            .services
            .iter()
            .map(|entry| (entry.service.clone(), place_of(entry.wire)))
            .collect();
        RouteDraft {
            provider: TextField::new(row.provider),
            wire: services.first().map_or(0, |(_, wire)| *wire),
            services: services.clone(),
            was: Some(services),
            shown: true,
            ..RouteDraft::default()
        }
    }

    pub fn op(&self) -> Result<harness::Op, String> {
        let provider = self.provider();
        if provider.is_empty() {
            return Err(
                "Write the provider agents name in their configuration, like anthropic.".into(),
            );
        }
        if self.services.is_empty() {
            return Err("Pick the services that reach its models.".into());
        }
        if self.was.as_ref() == Some(&self.services) {
            return Err("Nothing changed.".into());
        }
        Ok(harness::Op::SetRoute {
            provider,
            services: self.route(),
        })
    }

    // what the yes shows: its title, and a line for what it does
    pub fn asked(&self, roster: Option<&Roster>) -> Option<(String, Vec<String>)> {
        let provider = self.provider();
        Some(match self.asking? {
            Yes::Save => (
                format!("Save {provider}'s route"),
                vec![route_words(&self.route())],
            ),
            Yes::Remove => {
                let naming = agents_naming(roster, &provider);
                (
                    format!("Remove {provider}'s route?"),
                    vec![removes_words(naming)],
                )
            }
        })
    }
}

pub(crate) fn route_words(route: &[RouteService]) -> String {
    let listed: Vec<String> = route
        .iter()
        .map(|entry| format!("{} ({})", entry.service, entry.wire.label()))
        .collect();
    format!("Reach its models through {}", listed.join(", then "))
}

fn agents_naming(roster: Option<&Roster>, provider: &str) -> Option<Vec<String>> {
    let roster = roster?;
    let names = roster.agents.iter().filter(|card| {
        matches!(
            &card.setup.configured,
            Configured::Routed { provider: named, .. } | Configured::NoRoute { provider: named, .. }
                if named == provider
        )
    });
    Some(names.map(|card| card.name.clone()).collect())
}

fn removes_words(naming: Option<Vec<String>>) -> String {
    let Some(names) = naming else {
        return "Reading which agents name it…".into();
    };
    match names.as_slice() {
        [] => "No agent's configuration names it.".into(),
        [one] => format!(
            "{one}'s configuration still names it: {one} asks no model until a route reaches it again."
        ),
        [rest @ .., last] => format!(
            "{} and {last} still name it in their configuration: each asks no model until a route reaches it again.",
            rest.join(", ")
        ),
    }
}

impl Agents {
    pub(crate) fn press_route(&mut self, press: Press, cx: &mut Context<Self>) {
        match self.route_draft.press(press) {
            Then::Stay => {}
            Then::Send(op) => {
                let submit = cx.host().ask::<Submit<harness::Harness>>(op);
                let task = cx.land(submit, |view, answer, cx| {
                    let sent = applied(answer).err();
                    view.press_route(Press::Sent(sent), cx)
                });
                self.route_draft.sending = Some(task);
            }
            Then::Done => match self.screen {
                Screen::SetRoute => self.go(Screen::Routes, cx),
                _ => self.read_screen(cx),
            },
        }
        cx.notify();
    }
}
