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
    // An answer and its states: idle, loading, ready, reloading, failed.
    agents: Loadable<Vec<Agent>>,
    // The field's text. The host does the editing; what is typed lands here.
    name: TextField,
    // The reader's account. `None` until the session says, and for a key
    // that holds no account.
    me: Option<u64>,
    // Why the last create did not land, in the words every view uses.
    failed: Option<String>,
    // Never saved: a `Task` holds only while this instance runs. `Some`
    // while a create waits for its answer; dropping it cancels the wait.
    #[serde(skip)]
    creating: Option<Task<()>>,
}

// One row: what the screen draws of an agent's account. `PartialEq` is for
// `cx.load`, which compares the answer with the rows on screen.
#[derive(PartialEq, Serialize, Deserialize)]
struct Agent {
    number: u64,
    name: String,
    // A type of the program that derives borsh only, saved as its bytes.
    #[serde(with = "view::borsh_bytes")]
    kind: Kind,
}

// What the host reads about the view. A method outside these capabilities
// is refused; any program may be asked.
impl View for Agents {
    const NAME: &'static str = "Agents";
    const DESCRIPTION: &'static str = "The agents your account manages.";
    // One per prefix of the methods' kinds: `host.session` and `host.log`,
    // `program.query` and `program.changes`, `op.submit`.
    const CAPABILITIES: &'static [Capability] =
        &[Capability::Host, Capability::Program, Capability::Op];
    // The app never lays the view out narrower than this, in px. Unset, 480.
    const MIN_WINDOW_WIDTH: u32 = 320;

    // Runs on every mount, first or restored. What the view follows starts
    // here; `detach` keeps it running as long as the view does.
    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        // Who is reading, and an item again whenever that changes. The
        // closure runs per item; it never renders by itself.
        cx.follow::<HostSession>((), |view, session, cx| match session {
            Ok(session) if session.account != view.me => {
                view.me = session.account;
                // Another reader: the rows go, and a fresh read starts.
                view.agents = Loadable::Idle;
                view.reread(cx);
            }
            // The same account: another field of the session changed.
            Ok(_) => view.reread(cx),
            // The rows wait for the session, so its refusal is shown.
            Err(refusal) => {
                view.agents = Loadable::Failed(refusal);
                cx.notify();
            }
        })
        .detach();
        // One item per block that wrote to identity.
        cx.follow::<Changes<Identity>>((), |view, change, cx| match change {
            Ok(_) => view.reread(cx),
            // Nothing on screen waits for this one: to the host's log.
            Err(refusal) => cx.log_refused("the changes", &refusal),
        })
        .detach();
    }
}

impl Agents {
    // The read, into its slot. Rows on screen stay until the answer lands,
    // and the view renders only if the answer differs.
    fn reread(&mut self, cx: &mut Context<Self>) {
        let read = managed(cx.host(), self.me);
        cx.load(self, read, |view| &mut view.agents);
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        let name = self.name.text().trim().to_owned();
        if name.is_empty() || self.creating.is_some() {
            return;
        }
        self.failed = None;
        // A write to the program. It is sent here; the task only waits.
        let submit = cx
            .host()
            .ask::<Submit<Identity>>(identity::Op::CreateAgent { name });
        // Hands the answer, the value or the refusal, to the closure.
        self.creating = Some(cx.land(submit, |view, answer, cx| {
            view.creating = None;
            // Why a write did not land, in the words every view uses:
            // refused, not sent, or sent with no answer back.
            match design::failure(&answer) {
                // The node took it and predicts it applies: a read from
                // now on sees the agent.
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

// A typed question to the program: `ask::Managed` is answered with a page
// of accounts and nothing else. `query_all` asks the closure for the page
// after each cursor, `None` first, until the listing ends.
async fn managed(host: Host, by: Option<u64>) -> Result<Vec<Agent>, Error> {
    // A key that holds no account manages no agents.
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
    Ok(accounts.into_iter().map(agent).collect())
}

// Draws the state as a tree of elements. The host lays it out and paints
// it. Runs on the tick after a `cx.notify()`, never by itself.
impl Render for Agents {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The host's colors, light or dark.
        let theme = *cx.global::<Theme>();
        // The device's pointer: a finger takes the touch ladder.
        let ladder = &design::ladder(window.touch());
        let creating = self.creating.is_some();
        // A text field over `name`: its id, the field, then the name a
        // screen reader says. The host owns the text; each change lands in
        // `name` and renders the view.
        let name = Input::new("name", &self.name, "Name of the new agent")
            .flex_1()
            .h(design::size::CONTROL)
            .px(design::space::SM)
            .border_1()
            .border_color(theme.border_strong)
            .placeholder("Agent name")
            .disabled(creating)
            // Enter in the field.
            .on_submit(cx.listener(|view: &mut Agents, _: &(), _, cx| view.create(cx)));
        // A press. `cx.listener` turns a method of the view into a handler.
        let pressed = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.create(cx));
        // What a pressable `div` owes a screen reader and the keyboard: a
        // role, a Tab stop, and its state. Its text is its name. The test
        // host fails a frame that lacks one of them.
        // `design::button(id, label, &theme, click).enabled(..)` is all of
        // it ready-made.
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
        let agents = match &self.agents {
            // Idle until the first session item.
            Loadable::Idle | Loadable::Loading(_) => {
                design::quiet("Reading…", &theme).into_any_element()
            }
            // A refused read: the reason and a Retry button, under the ids
            // `agents-refused` and `agents-retry`.
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
                // Rows from data. Each carries its own id, unique in the
                // list: the host keeps a row's focus by it.
                .children(agents.iter().map(|agent| {
                    div()
                        .id(format!("agent-{}", agent.number))
                        .flex()
                        .justify_between()
                        .py(design::space::SM)
                        .child(agent.name.clone())
                        // "suspended" or "revoked", for an agent that is.
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
            // Why the create did not land, under the field.
            .children(
                self.failed
                    .clone()
                    .map(|why| div().text_color(theme.danger).child(why)),
            )
            .child(agents)
    }
}

// Writes the wasm exports the host calls and the manifest it reads.
export_view!(Agents);

// The view on a fake host, natively. Every frame it sends is held to the
// host's sanitizer and the accessibility audit.
#[cfg(test)]
mod tests {
    use super::*;
    use view::testing::TestAppContext;

    /// Scout, an agent account 7 manages.
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

    /// identity's answer to "the agents account 7 manages": one page.
    fn managed_by_7() -> identity::Reply {
        identity::Reply::Accounts(identity::PageResponse {
            // The block the answer was read at.
            height: 1,
            items: vec![scout()],
            next: None,
        })
    }

    /// The view open and seated as account 7. A test says what the host
    /// answers: an ask no handler answers fails it.
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
        // From the snapshot: nothing was read.
        assert!(cx.has_text("Scout"), "{:?}", cx.texts());
        view.read(|view| assert_eq!(view.name.text(), "Rover"));
        assert_eq!(cx.host().requests::<Query<Identity>>().len(), reads);
        // `attach` ran again: a change reads again.
        cx.host().stream::<Changes<Identity>>().send(None);
        cx.run_until_parked();
        assert_eq!(cx.host().requests::<Query<Identity>>().len(), reads + 1);
    }
}
