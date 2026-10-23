//! Agents: the agents the reader's account manages, and a field that
//! creates one. The smallest view that uses every core feature of the view
//! SDK once.
//!
//! Words: the host is the app that runs the view. A program is the code on
//! the node the view talks to, here identity. The reader is whoever looks
//! at the view. A refusal is an `Error` in place of an answer.
use identity::{Identity, Kind, PageRequest};
use serde::{Deserialize, Serialize};
use view::prelude::*;

// The state. The host saves it with serde before a redeploy and hands it to
// the new build, so what is here survives one.
#[derive(Default, Serialize, Deserialize)]
pub struct Agents {
    agents: Reread<Vec<Agent>>,
    name: TextField,
    me: Option<u64>,
    failed: Option<String>,
    #[serde(skip)]
    creating: Option<Task<()>>,
}

#[derive(PartialEq, Serialize, Deserialize)]
struct Agent {
    number: u64,
    name: String,
    #[serde(with = "view::borsh_bytes")]
    kind: Kind,
}

impl View for Agents {
    const NAME: &'static str = "Agents";
    const DESCRIPTION: &'static str = "The agents your account manages.";
    const CAPABILITIES: &'static [Capability] =
        &[Capability::Host, Capability::Program, Capability::Op];
    const MIN_WINDOW_WIDTH: u32 = 320;

    // Runs on every mount, first or restored. What the view follows starts
    // here; `detach` keeps it running as long as the view does.
    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        cx.follow::<HostSession>((), |view, session, cx| match session {
            Ok(session) if session.account != view.me => {
                view.me = session.account;
                *view.agents = Loadable::Idle;
                view.reread(cx);
            }
            Ok(_) => view.reread(cx),
            Err(refusal) => {
                *view.agents = Loadable::Failed(refusal);
                cx.notify();
            }
        })
        .detach();
        cx.follow::<Changes<Identity>>((), |view, change, cx| match change {
            Ok(_) => view.reread(cx),
            Err(refusal) => cx.log_refused("the changes", &refusal),
        })
        .detach();
    }
}

impl Agents {
    fn reread(&mut self, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.agents,
            |view, cx| managed_accounts_all_pages(cx.host(), view.me),
            Self::reread,
        );
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        let name = self.name.text().trim().to_owned();
        if name.is_empty() || self.creating.is_some() {
            return;
        }
        self.failed = None;
        let submit = cx
            .host()
            .ask::<Submit<Identity>>(identity::Op::CreateAgent { name });
        self.creating = Some(cx.land(submit, |view, answer, cx| {
            view.creating = None;
            match design::failure(&answer) {
                None => {
                    view.name.reset("");
                    view.reread(cx);
                }
                Some(said) => view.failed = Some(said),
            }
            cx.notify();
        }));
        cx.notify();
    }
}

async fn managed_accounts_all_pages(host: Host, by: Option<u64>) -> Result<Vec<Agent>, Error> {
    let Some(by) = by else {
        return Ok(Vec::new());
    };
    let accounts = host
        .query_all(|after| identity::ask::Managed {
            by,
            page: PageRequest { after, limit: None },
        })
        .await?;
    let agent = |account: identity::Account| Agent {
        number: account.number,
        kind: account.kind(),
        name: account.card.name,
    };
    Ok(accounts
        .into_iter()
        .filter(|account| identity::view::listed(&account.kind()))
        .map(agent)
        .collect())
}

impl Render for Agents {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let ladder = &design::ladder(window.touch());
        let creating = self.creating.is_some();
        let name = Input::new("name", &self.name, "Name of the new agent")
            .flex_1()
            .h(design::size::CONTROL)
            .px(design::space::SM)
            .border_1()
            .border_color(theme.border_strong)
            .placeholder("Agent name")
            .disabled(creating)
            .on_submit(cx.listener(|view: &mut Agents, _: &(), _, cx| view.create(cx)));
        let pressed = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.create(cx));
        let create = div()
            .id("create")
            .h(design::size::CONTROL)
            .px(design::space::MD)
            .flex()
            .items_center()
            .bg(theme.surface)
            .role(Role::Button)
            .focusable()
            .aria_disabled(creating)
            .when(!creating, |button| button.on_click(pressed))
            .child(if creating { "Creating…" } else { "Create" });
        let agents = match &*self.agents {
            Loadable::Idle | Loadable::Loading(_) => {
                design::quiet("Reading…", &theme).into_any_element()
            }
            Loadable::Failed(refusal) => {
                let retry = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.reread(cx));
                design::refused("agents", design::sentence(refusal), ladder, &theme, retry)
                    .into_any_element()
            }
            Loadable::Ready(agents) | Loadable::Reloading(agents, _) if agents.is_empty() => {
                design::empty_state(
                    "no-agents",
                    "bot",
                    "No agents yet",
                    "Name one above.",
                    ladder,
                    &theme,
                )
                .into_any_element()
            }
            Loadable::Ready(agents) | Loadable::Reloading(agents, _) => div()
                .id("agents")
                .flex_1()
                .overflow_y_scroll()
                .children(agents.iter().map(|agent| {
                    div()
                        .id(format!("agent-{}", agent.number))
                        .flex()
                        .justify_between()
                        .py(design::space::SM)
                        .child(agent.name.clone())
                        .children(design::note(&agent.kind).map(|note| design::quiet(note, &theme)))
                }))
                .into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(design::space::MD)
            .p(design::space::LG)
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(design::text::BODY)
            .child(
                div()
                    .flex()
                    .gap(design::space::SM)
                    .child(name)
                    .child(create),
            )
            .children(
                self.failed
                    .clone()
                    .map(|why| div().text_color(theme.danger).child(why)),
            )
            .child(agents)
    }
}

export_view!(Agents);

#[cfg(test)]
mod tests {
    use super::*;
    use view::testing::TestAppContext;

    fn scout() -> identity::Account {
        identity::Account {
            number: 12,
            card: identity::Card {
                name: "Scout".into(),
                avatar: None,
                bio: None,
                updated_at: 1,
            },
            control: identity::Control::Program {
                script: None,
                manager: 7,
                category: identity::Category::Agent,
                life: identity::Life::Active { keys: Vec::new() },
            },
        }
    }

    fn managed_by_7() -> identity::Reply {
        identity::Reply::Accounts(identity::PageResponse {
            height: 1,
            items: vec![scout()],
            next: None,
        })
    }

    fn seated() -> TestAppContext {
        let mut cx = TestAppContext::new();
        cx.host().handle::<Query<Identity>>(|query| match query {
            identity::Query::Managed { by: 7, .. } => Ok(managed_by_7()),
            other => panic!("unexpected query: {other:?}"),
        });
        cx.open::<Agents>();
        cx.host().stream::<HostSession>().send(Session {
            account: Some(7),
            ..Session::default()
        });
        cx.run_until_parked();
        cx
    }

    #[test]
    fn a_typed_name_is_submitted_and_a_change_reads_again() {
        let mut cx = seated();
        assert!(cx.has_text("Scout"), "{:?}", cx.texts());
        cx.host()
            .handle::<Submit<Identity>>(|_| Ok(view::testing::applied(Vec::new())));
        cx.simulate_input("name", "Rover");
        cx.simulate_click("create");
        cx.run_until_parked();
        let rover = identity::Op::CreateAgent {
            name: "Rover".into(),
        };
        assert_eq!(cx.host().requests::<Submit<Identity>>(), [rover]);
        let reads = cx.host().requests::<Query<Identity>>().len();
        cx.host().stream::<Changes<Identity>>().send(None);
        cx.run_until_parked();
        assert_eq!(cx.host().requests::<Query<Identity>>().len(), reads + 1);
    }

    #[test]
    fn the_rows_land_though_a_head_comes_before_every_answer() {
        let mut cx = seated();
        let changes = cx.host().stream::<Changes<Identity>>();
        cx.host().never::<Query<Identity>>();
        changes.send(None);
        cx.run_until_parked();
        let mut rover = scout();
        rover.card.name = "Rover".into();
        for _ in 0..2 {
            changes.send(None);
            let rows = vec![rover.clone()];
            cx.host().handle::<Query<Identity>>(move |_| {
                Ok(identity::Reply::Accounts(identity::PageResponse {
                    height: 2,
                    items: rows.clone(),
                    next: None,
                }))
            });
            cx.host().answer_held::<Query<Identity>>();
            cx.host().never::<Query<Identity>>();
            cx.tick(Vec::new());
        }
        assert!(cx.has_text("Rover"), "{:?}", cx.texts());
    }

    #[test]
    fn a_refusal_is_shown_where_it_happened() {
        let mut cx = seated();
        cx.host()
            .refuse::<Submit<Identity>>("unauthorized", "account 7 is suspended");
        cx.simulate_input("name", "Rover");
        cx.simulate_submit("name");
        cx.run_until_parked();
        assert!(
            cx.has_text("That didn’t go through: account 7 is suspended"),
            "{:?}",
            cx.texts()
        );
        cx.host()
            .refuse::<Query<Identity>>("unavailable", "The node is away.");
        cx.host().stream::<Changes<Identity>>().send(None);
        cx.run_until_parked();
        assert!(cx.has_text("The node is away."), "{:?}", cx.texts());
        cx.host().handle::<Query<Identity>>(|_| Ok(managed_by_7()));
        cx.simulate_click("agents-retry");
        cx.run_until_parked();
        assert!(cx.has_text("Scout"), "{:?}", cx.texts());
    }

    #[test]
    fn a_refused_stream_is_shown_or_logged() {
        let mut cx = TestAppContext::new();
        cx.host()
            .refuse::<HostSession>("unavailable", "No session.");
        cx.host()
            .refuse::<Changes<Identity>>("unavailable", "No link.");
        cx.open::<Agents>();
        cx.run_until_parked();
        assert!(cx.has_text("No session."), "{:?}", cx.texts());
        let logged = "Agents: the changes refused: unavailable: No link.";
        assert_eq!(cx.host().requests::<HostLog>(), [logged]);
    }

    #[test]
    fn a_key_without_an_account_sees_no_agents_and_asks_nothing() {
        let mut cx = TestAppContext::new();
        cx.open::<Agents>();
        cx.host().stream::<HostSession>().send(Session::default());
        cx.run_until_parked();
        assert!(cx.has_text("No agents yet"), "{:?}", cx.texts());
    }

    #[test]
    fn a_restore_shows_what_was_saved_and_follows_again() {
        let mut cx = seated();
        cx.simulate_input("name", "Rover");
        let saved = cx.snapshot().unwrap();
        let reads = cx.host().requests::<Query<Identity>>().len();
        let view = cx.restore::<Agents>(&saved).unwrap();
        assert!(cx.has_text("Scout"), "{:?}", cx.texts());
        view.read(|view| assert_eq!(view.name.text(), "Rover"));
        assert_eq!(cx.host().requests::<Query<Identity>>().len(), reads);
        cx.host().stream::<Changes<Identity>>().send(None);
        cx.run_until_parked();
        assert_eq!(cx.host().requests::<Query<Identity>>().len(), reads + 1);
    }
}
