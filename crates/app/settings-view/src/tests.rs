use crate::Settings;
use identity::Identity;
use valset::Valset;
use view::methods::{Change, Changes, ClipboardWrite, Query};
use view::methods::{HostSession, Invite, InviteCreate, Session, Submit};
use view::testing::{StreamSender, TestAppContext};
use view::{Theme, wire};

mod phone;
mod photo;
mod policies;

fn press(cx: &TestAppContext, key: &str) -> wire::Event {
    let handler = cx.interactivity(key).on_click.expect("a click route");
    wire::Event::Click {
        handler,
        event: (&view::ClickEvent::default()).into(),
    }
}

fn account(number: u64, name: &str, control: identity::Control) -> identity::Account {
    identity::Account {
        number,
        card: identity::Card {
            name: name.into(),
            avatar: None,
            bio: None,
            updated_at: 1,
        },
        control,
    }
}
fn maya(number: u64, label: &str) -> identity::Account {
    let keys = vec![identity::Key {
        scheme: abi::Scheme::Ed25519,
        key: vec![0xab, 0xcd],
        label: Some(label.into()),
        added_at: 1,
    }];
    account(number, "Maya", identity::Control::User { keys })
}
fn scout() -> identity::Account {
    account(
        12,
        "Scout",
        identity::Control::Program {
            script: None,
            manager: 7,
            category: identity::Category::Agent,
            life: identity::Life::Active { keys: Vec::new() },
        },
    )
}

fn scout_is(cx: &TestAppContext, life: identity::Life) {
    cx.host().handle::<Query<Identity>>(move |q| {
        let life = life.clone();
        Ok(match q {
            identity::Query::Get { number } => {
                identity::Reply::Account(Some(maya(number, "Laptop key")))
            }
            identity::Query::Managed { by: 7, .. } => {
                let identity::Control::Program {
                    manager, category, ..
                } = scout().control
                else {
                    panic!()
                };
                let control = identity::Control::Program {
                    script: None,
                    manager,
                    category,
                    life,
                };
                identity::Reply::Accounts(identity::PageResponse {
                    height: 42,
                    items: vec![identity::Account { control, ..scout() }],
                    next: None,
                })
            }
            identity::Query::Narrowed { .. } => identity::Reply::Narrowed(identity::PageResponse {
                height: 42,
                items: vec![(12, identity::Ownership::Owner)],
                next: None,
            }),
            q => panic!("unexpected query: {q:?}"),
        })
    });
}

fn respond(cx: &TestAppContext) {
    cx.host().handle::<Query<Identity>>(|q| {
        Ok(match q {
            identity::Query::Get { number } => {
                assert_eq!(number, 7);
                identity::Reply::Account(Some(maya(number, "Laptop key")))
            }
            identity::Query::Managed { by: 7, .. } => {
                identity::Reply::Accounts(identity::PageResponse {
                    height: 42,
                    items: vec![scout()],
                    next: None,
                })
            }
            identity::Query::Profile { number: 9 } => {
                identity::Reply::Profile(Some(policies::ada()))
            }
            identity::Query::Resolve { references }
                if references == [identity::Reference::Account(9)] =>
            {
                identity::Reply::Resolved(vec![Some(9)])
            }
            identity::Query::Narrowed { .. } => identity::Reply::Narrowed(identity::PageResponse {
                height: 42,
                items: vec![(12, identity::Ownership::Owner)],
                next: None,
            }),
            q => panic!("unexpected query: {q:?}"),
        })
    });
    policies::owning(cx);
    cx.host().handle::<Query<Valset>>(|q| {
        Ok(match q {
            valset::Query::Memberships { .. } => valset::Reply::Memberships(valset::PageResponse {
                height: 42,
                items: vec![valset::Membership {
                    key: vec![0xab, 0xcd],
                    address: Some("127.0.0.1:19001".into()),
                    halo: "node.example.org:9100".into(),
                    role: valset::Role::Validator,
                }],
                next: None,
            }),
            q => panic!("unexpected query: {q:?}"),
        })
    });
}
fn scout_seated(cx: &TestAppContext) {
    let keyed_scout = || {
        let keys = vec![identity::Key {
            scheme: abi::Scheme::Ed25519,
            key: vec![0xab, 0xcd],
            label: Some("Sandbox".into()),
            added_at: 1,
        }];
        identity::Account {
            control: identity::Control::Program {
                script: None,
                manager: 7,
                category: identity::Category::Agent,
                life: identity::Life::Active { keys },
            },
            ..scout()
        }
    };
    let relay = || {
        account(
            13,
            "Relay",
            identity::Control::Program {
                script: None,
                manager: 12,
                category: identity::Category::Agent,
                life: identity::Life::Active { keys: Vec::new() },
            },
        )
    };
    cx.host().handle::<Query<Identity>>(move |q| {
        Ok(match q {
            identity::Query::Get { number: 12 } => identity::Reply::Account(Some(keyed_scout())),
            identity::Query::Managed { by: 12, .. } => {
                identity::Reply::Accounts(identity::PageResponse {
                    height: 42,
                    items: vec![relay()],
                    next: None,
                })
            }
            identity::Query::Narrowed { .. } => identity::Reply::Narrowed(identity::PageResponse {
                height: 42,
                items: vec![(13, identity::Ownership::Owner)],
                next: None,
            }),
            q => panic!("unexpected query: {q:?}"),
        })
    });
}
fn fixture(state: &str, dark: bool) -> TestAppContext {
    seated(state, dark).0
}
fn seated(state: &str, dark: bool) -> (TestAppContext, StreamSender<HostSession>) {
    let mut cx = TestAppContext::new();
    let props = cx.host().stream::<HostSession>();
    respond(&cx);
    match state {
        "unregistered" => cx.host().handle::<Query<Identity>>(|q| match q {
            identity::Query::Resolve { references } => {
                Ok(identity::Reply::Resolved(vec![None; references.len()]))
            }
            q => panic!("an unregistered key asks identity who holds it, nothing more: {q:?}"),
        }),
        "suspended" => cx.host().handle::<Query<Identity>>(|q| {
            Ok(match q {
                identity::Query::Resolve { references } => {
                    assert_eq!(references, vec![identity::Reference::Key(vec![0xab, 0xcd])]);
                    identity::Reply::Resolved(vec![Some(12)])
                }
                identity::Query::Profile { number: 12 } => {
                    identity::Reply::Profile(Some(identity::Profile {
                        number: 12,
                        name: "Scout".into(),
                        kind: identity::Kind::Program {
                            script: None,
                            manager: 7,
                            category: identity::Category::Agent,
                            standing: identity::Standing::Suspended,
                        },
                    }))
                }
                q => panic!("unexpected query: {q:?}"),
            })
        }),
        "scout" => scout_seated(&cx),
        "loading" => {
            cx.host().never::<Query<Identity>>();
        }
        "refused" => {
            cx.host()
                .refuse::<Query<Identity>>("unavailable", "Account query refused.");
        }
        "no-file-read" => cx.host().handle::<view::methods::HostMethods>(|()| {
            Ok(view::methods::Hosted {
                level: view::wire::WIRE_LEVEL,
                kinds: Vec::new(),
            })
        }),
        _ => {}
    }
    cx.set_global(if dark { Theme::dark() } else { Theme::light() });
    cx.open::<Settings>();
    props.send(Session {
        signer: if state == "empty" {
            String::new()
        } else {
            "abcd".into()
        },
        account: match state {
            "empty" | "unregistered" | "suspended" => None,
            "scout" => Some(12),
            _ => Some(7),
        },
        endpoint: "http://127.0.0.1:19001".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    if state.starts_with("invite") {
        cx.simulate_click("settings/nav/invites");
        cx.run_until_parked();
        match state {
            "invite-loading" => cx.host().never::<InviteCreate>(),
            "invite-refused" => cx
                .host()
                .refuse::<InviteCreate>("forbidden", "This node does not allow minting invites."),
            _ => cx.host().handle::<InviteCreate>(|request| {
                assert_eq!(request.ttl_days, 7);
                Ok(Invite {
                    invite: "duck-invite:workshop-loopback-example".into(),
                    notes: vec![view::host::Error {
                        code: "expires".into(),
                        message: "This invite expires in 7 days.".into(),
                    }],
                })
            }),
        }
        cx.simulate_click("settings/invite/mint");
        cx.run_until_parked();
    }
    (cx, props)
}
#[test]
fn four_states_are_honest() {
    assert!(fixture("loading", false).has_text("Reading your account…"));
    assert!(fixture("refused", false).has_text("Account query refused."));
    assert!(fixture("empty", false).has_text("No account"));
    let cx = fixture("ready", false);
    for text in [
        "Maya",
        "account 7 · User",
        "Keys",
        "Laptop key",
        "abcd",
        "Validator",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(!cx.has_text("Node"));
    assert!(cx.find("settings/app/body").is_none());
}

#[test]
fn the_menu_opens_one_pane_at_a_time() {
    let mut cx = fixture("ready", false);
    for pane in ["account", "agents", "invites"] {
        assert!(cx.find(&format!("settings/nav/{pane}")).is_some(), "{pane}");
    }
    assert!(cx.find("settings/agents/12/suspend").is_none());
    cx.simulate_click("settings/nav/agents");
    cx.run_until_parked();
    assert!(cx.find("settings/agents/12/suspend").is_some());
    assert!(cx.find("settings/key/0").is_none());
    cx.simulate_click("settings/nav/invites");
    cx.run_until_parked();
    assert!(cx.find("settings/invite/mint").is_some());
    let cx = fixture("unregistered", false);
    assert!(cx.find("settings/nav/agents").is_none());
    assert!(cx.find("settings/nav/invites").is_some());
}
#[test]
fn an_arrow_on_the_menu_opens_the_next_section() {
    let mut cx = fixture("ready", false);
    let nav = cx.interactivity("settings/nav");
    assert_eq!(nav.role, Some(view::Role::TabList));
    assert_eq!(nav.aria.label.as_deref(), Some("Settings"));
    assert_eq!(
        nav.aria.orientation,
        Some(view::design::Orientation::Vertical)
    );
    assert!(nav.focusable && nav.tab_stop == Some(true));
    assert!(!cx.interactivity("settings/nav/account").focusable);
    assert!(
        cx.interactivity("settings/nav/account")
            .aria
            .active_descendant
    );
    cx.simulate_focus("settings/nav");
    cx.simulate_key_down("settings/nav", "down");
    cx.run_until_parked();
    assert!(cx.find("settings/agents/12/suspend").is_some());
    let agents = cx.interactivity("settings/nav/agents");
    assert_eq!(agents.aria.selected, Some(true));
    assert!(agents.aria.active_descendant);
    cx.simulate_key_down("settings/nav", "up");
    cx.simulate_key_down("settings/nav", "up");
    cx.run_until_parked();
    assert!(cx.find("settings/invite/mint").is_some());
    let mint = cx.interactivity("settings/invite/mint");
    let ring = &cx.styles()[mint.focus_visible.expect("a focus ring")];
    assert_eq!(ring.border_color, Some(Theme::light().primary_foreground));
}

#[test]
fn invite_ttl_copy_and_refusal() {
    let mut cx = fixture("invite-ready", false);
    cx.host().handle::<ClipboardWrite>(|text| {
        assert_eq!(text, "duck-invite:workshop-loopback-example");
        Ok(())
    });
    cx.simulate_click("settings/invite/copy");
    cx.run_until_parked();
    assert!(cx.has_text("Copied"));
    cx.host().handle::<InviteCreate>(|r| {
        assert_eq!(r.ttl_days, 30);
        Ok(Invite {
            invite: "long-lived".into(),
            notes: vec![],
        })
    });
    cx.simulate_click("settings/ttl/30");
    cx.simulate_click("settings/invite/mint");
    cx.run_until_parked();
    assert!(cx.has_text("long-lived"));
    assert!(fixture("invite-refused", false).has_text("This node does not allow minting invites."));
    assert!(fixture("invite-loading", false).has_text("Minting invite…"));
}
#[test]
fn the_invite_lifetime_checks_the_next_choice_on_an_arrow() {
    let mut cx = fixture("invite-ready", false);
    let group = cx.interactivity("settings/ttl");
    assert_eq!(group.role, Some(view::Role::RadioGroup));
    assert!(group.focusable && group.tab_stop == Some(true));
    assert!(!cx.interactivity("settings/ttl/1").focusable);
    let checked = |cx: &TestAppContext| {
        [1, 7, 30].map(|days| {
            let segment = cx.interactivity(&format!("settings/ttl/{days}"));
            (
                segment.aria.toggled == Some(view::Toggled::True),
                segment.aria.active_descendant,
            )
        })
    };
    assert_eq!(checked(&cx), [(false, false), (true, true), (false, false)]);
    cx.simulate_focus("settings/ttl");
    cx.simulate_key_down("settings/ttl", "right");
    assert_eq!(checked(&cx), [(false, false), (false, false), (true, true)]);
    cx.simulate_key_down("settings/ttl", "right");
    assert_eq!(checked(&cx), [(true, true), (false, false), (false, false)]);
    cx.simulate_key_down("settings/ttl", "left");
    assert_eq!(checked(&cx), [(false, false), (false, false), (true, true)]);
}

#[test]
fn a_refusal_retries_and_a_snapshot_restores() {
    let mut cx = fixture("refused", false);
    respond(&cx);
    cx.simulate_click("settings/account-retry");
    cx.run_until_parked();
    assert!(cx.has_text("account 7 · User"), "{:?}", cx.texts());
    let snapshot = cx.snapshot().unwrap();
    cx.restore::<Settings>(&snapshot).unwrap();
    cx.run_until_parked();
    assert!(cx.has_text("account 7 · User"));
}
#[test]
fn export_settings_screens() {
    let Some(out) = ::view::testing::screens("SETTINGS_SCREEN_EXPORT", "settings/fixtures") else {
        return;
    };
    let mut manifest = Vec::new();
    for (i, state) in [
        "loading",
        "refused",
        "empty",
        "ready",
        "agents",
        "invite-loading",
        "invite-refused",
        "invite-ready",
        "unregistered",
    ]
    .into_iter()
    .enumerate()
    {
        for dark in [false, true] {
            let cx = match state {
                "agents" => {
                    let mut cx = fixture("ready", dark);
                    cx.simulate_click("settings/nav/agents");
                    cx.run_until_parked();
                    cx
                }
                state => fixture(state, dark),
            };
            let theme = if dark { "dark" } else { "light" };
            let name = format!("{:02}-{state}-{theme}", i + 1);
            std::fs::write(
                out.join(format!("{name}.json")),
                serde_json::to_vec(&cx.whole_frame()).unwrap(),
            )
            .unwrap();
            manifest.push(serde_json::json!({"name":name,"theme":theme,"width":820,"height":1100,"how":"TestAppContext + FakeHost"}));
        }
    }
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn unregistered_key_keeps_its_standing() {
    let cx = fixture("unregistered", false);
    for text in ["Unregistered key", "Host key", "abcd", "Validator"] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
}

#[test]
fn no_key_offers_no_form() {
    let cx = fixture("empty", false);
    assert!(cx.has_text("No host key is selected. Sign in with a key to create an account."));
    assert!(cx.find("settings/account/create/name").is_none());
}

#[test]
fn a_suspended_agents_key_reads_as_such_and_creates_nothing() {
    let cx = fixture("suspended", false);
    assert!(cx.has_text("Scout"), "{:?}", cx.texts());
    assert!(cx.has_text("This key belongs to Scout, suspended by its manager."));
    assert!(cx.find("settings/account/create").is_none());
}
#[test]
fn unregistered_key_creates_an_account() {
    let (mut cx, props) = seated("unregistered", false);
    assert!(cx.has_text(
        "Your key isn't linked to an account yet. An account gives you a name others see."
    ));

    cx.simulate_click("settings/account/create/submit");
    cx.run_until_parked();
    assert!(cx.has_text("Enter an account name."));
    assert!(cx.host().requests::<Submit<Identity>>().is_empty());

    cx.host()
        .refuse::<Submit<Identity>>("invalid", "a name is not empty");
    cx.simulate_input("settings/account/create/name", "Maya");
    cx.simulate_submit("settings/account/create/name");
    cx.run_until_parked();
    assert!(cx.has_text("That didn’t go through: a name is not empty"));

    cx.host().handle::<Submit<Identity>>(|op| {
        assert!(matches!(
            op,
            identity::Op::Create { ref name, scheme: abi::Scheme::Ed25519 } if name == "Maya"
        ));
        Ok(view::testing::applied(Vec::new()))
    });
    cx.host().handle::<Query<Identity>>(|q| {
        Ok(match q {
            identity::Query::Get { number } => {
                assert_eq!(number, 9);
                identity::Reply::Account(Some(maya(number, "Host key")))
            }
            identity::Query::Managed { .. } => identity::Reply::Accounts(identity::PageResponse {
                height: 42,
                items: vec![],
                next: None,
            }),
            q => panic!("unexpected query: {q:?}"),
        })
    });
    cx.simulate_click("settings/account/create/submit");
    cx.run_until_parked();
    assert!(cx.has_text("Creating…"), "{:?}", cx.texts());
    props.send(Session {
        signer: "abcd".into(),
        account: Some(9),
        endpoint: "http://127.0.0.1:19001".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    assert!(cx.has_text("Maya"), "{:?}", cx.texts());
    assert!(cx.has_text("account 9 · User"), "{:?}", cx.texts());
    assert!(cx.find("settings/account/create/name").is_none());
}

#[test]
fn create_account_disables_controls_while_busy() {
    let mut cx = fixture("unregistered", false);
    cx.host().never::<Submit<Identity>>();
    cx.simulate_input("settings/account/create/name", "Maya");
    cx.simulate_click("settings/account/create/submit");
    cx.run_until_parked();
    assert!(cx.has_text("Creating…"));
    let Some(wire::Node::Container(wire::ContainerNode {
        interactivity: Some(interactivity),
        ..
    })) = cx.find("settings/account/create/submit")
    else {
        panic!("settings/account/create/submit button")
    };
    assert_eq!(interactivity.aria.disabled, Some(true));
    assert!(interactivity.on_click.is_none());
    let Some(wire::Node::Field { options, .. }) = cx.find("settings/account/create/name") else {
        panic!("settings/account/create/name input")
    };
    assert!(options.disabled);
}

#[test]
fn a_field_is_named_apart_from_its_hint() {
    fn named(cx: &TestAppContext, key: &str) -> (String, String) {
        let Some(wire::Node::Field {
            options,
            placeholder,
            ..
        }) = cx.find(key)
        else {
            panic!("no field {key}");
        };
        (options.label.clone(), placeholder.clone())
    }
    let pair = |name: &str, hint: &str| (name.to_owned(), hint.to_owned());
    let cx = fixture("unregistered", false);
    assert_eq!(
        named(&cx, "settings/account/create/name"),
        pair("Name the new account", "Account name")
    );
    let mut cx = fixture("ready", false);
    cx.simulate_click("settings/nav/agents");
    cx.simulate_click("settings/agents/12/rename");
    cx.run_until_parked();
    for (key, name, hint) in [
        (
            "settings/agents/create/name",
            "Name the new agent",
            "Agent name",
        ),
        (
            "settings/agents/key/request",
            "Paste an agent's key request",
            "Agent key request",
        ),
        ("settings/agents/12/name", "Rename Scout", "New name"),
    ] {
        assert_eq!(named(&cx, key), pair(name, hint));
    }
}

#[test]
fn long_host_key_is_truncated_and_non_validator_standing_is_quiet() {
    let mut cx = TestAppContext::new();
    let props = cx.host().stream::<HostSession>();
    let long_key = vec![0x11; 32];
    let long_hex = abi::hex(&long_key);
    cx.host().handle::<Query<Valset>>(|q| {
        Ok(match q {
            valset::Query::Memberships { .. } => valset::Reply::Memberships(valset::PageResponse {
                height: 1,
                items: Vec::new(),
                next: None,
            }),
            q => panic!("unexpected query: {q:?}"),
        })
    });
    cx.host().handle::<Query<Identity>>(|q| match q {
        identity::Query::Resolve { .. } => Ok(identity::Reply::Resolved(vec![None])),
        q => panic!("unexpected query: {q:?}"),
    });
    cx.set_global(Theme::light());
    cx.open::<Settings>();
    props.send(Session {
        signer: long_hex.clone(),
        endpoint: "http://127.0.0.1:19001".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    let truncated = format!("{}…{}", &long_hex[..8], &long_hex[long_hex.len() - 4..]);
    assert!(cx.has_text("Host key"), "{:?}", cx.texts());
    assert!(cx.has_text(&truncated), "{:?}", cx.texts());
    assert!(!cx.has_text("Validator"));
}

#[test]
fn a_person_creates_an_agent_and_adds_its_key() {
    let mut cx = fixture("ready", false);
    cx.simulate_click("settings/nav/agents");
    cx.run_until_parked();
    for text in ["Scout", "active", "account 12 · 0 keys"] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    cx.host().handle::<Submit<Identity>>(|op| {
        assert_eq!(op, identity::Op::CreateAgent { name: "Bot".into() });
        Ok(view::testing::applied(Vec::new()))
    });
    cx.simulate_input("settings/agents/create/name", " Bot ");
    cx.simulate_click("settings/agents/create/submit");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<Submit<Identity>>().len(), 1);

    cx.simulate_input("settings/agents/key/request", "zz");
    cx.simulate_click("settings/agents/key/submit");
    cx.run_until_parked();
    assert!(cx.has_text("That isn’t a key request for one of your agents."));
    let add = identity::Op::AddKey {
        scheme: abi::Scheme::Ed25519,
        label: Some("sandbox".into()),
        consent: identity::Consent {
            key: vec![0x51; 32],
            account: 12,
            expires_at: 500,
            proof: vec![0x52; 64],
        },
    };
    let expected = add.clone();
    cx.host().handle::<Submit<Identity>>(move |op| {
        assert_eq!(op, expected);
        Ok(view::testing::applied(Vec::new()))
    });
    cx.simulate_input("settings/agents/key/request", &abi::hex(&abi::encode(&add)));
    cx.simulate_click("settings/agents/key/submit");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<Submit<Identity>>().len(), 2);
    assert!(!cx.has_text("That isn’t a key request for one of your agents."));
}

fn scripted(calls: &str) -> identity::Account {
    let identity::Control::Program {
        manager,
        category,
        life,
        ..
    } = scout().control
    else {
        panic!()
    };
    identity::Account {
        control: identity::Control::Program {
            script: Some(identity::Script {
                calls: calls.into(),
                config: "/home/maya/agents/scout.md".into(),
            }),
            manager,
            category,
            life,
        },
        ..scout()
    }
}

fn with_scout(state: &str, agent: identity::Account) -> TestAppContext {
    let (mut cx, props) = seated(state, false);
    let listed = agent.clone();
    cx.host().handle::<Query<Identity>>(move |q| {
        Ok(match q {
            identity::Query::Get { number } => {
                identity::Reply::Account(Some(maya(number, "Laptop key")))
            }
            identity::Query::Managed { by: 7, .. } => {
                identity::Reply::Accounts(identity::PageResponse {
                    height: 42,
                    items: vec![listed.clone()],
                    next: None,
                })
            }
            identity::Query::Narrowed { .. } => identity::Reply::Narrowed(identity::PageResponse {
                height: 42,
                items: vec![(12, identity::Ownership::Owner)],
                next: None,
            }),
            q => panic!("unexpected query: {q:?}"),
        })
    });
    props.send(Session {
        signer: "abcd".into(),
        account: Some(7),
        chain_id: "garage#0a1b2c3d".into(),
        endpoint: "http://127.0.0.1:19001".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    cx.simulate_click("settings/nav/agents");
    cx.run_until_parked();
    cx
}

#[test]
fn a_harness_agent_keeps_its_photo_and_opens_in_agents() {
    let mut cx = with_scout("ready", scripted("harness"));
    assert!(cx.has_text("harness"), "{:?}", cx.texts());
    for gone in [
        "settings/agents/12/rename",
        "settings/agents/12/suspend",
        "settings/agents/12/revoke",
        "settings/agents/12/answering",
        "settings/agents/12/key/open",
        "settings/agents/12/harness",
    ] {
        assert!(cx.find(gone).is_none(), "{gone}: {:?}", cx.texts());
    }
    assert!(cx.has_text("Open in harness"), "{:?}", cx.texts());
    cx.simulate_click("settings/agents/12/open");
    assert_eq!(
        cx.host().requests::<view::methods::LinkOpen>(),
        ["duck://garage-0a1b2c3d/harness/12".to_owned()]
    );
}

#[test]
fn an_external_agent_takes_a_key_and_runs_on_harness_from_its_row() {
    let mut cx = with_scout("ready", scout());
    for text in [
        "external",
        "Add a key…",
        "Run it on harness…",
        "Who it answers",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    cx.simulate_click("settings/agents/12/harness");
    assert_eq!(
        cx.host().requests::<view::methods::LinkOpen>(),
        ["duck://garage-0a1b2c3d/harness/create/12".to_owned()]
    );
    cx.simulate_click("settings/agents/12/key/open");
    cx.run_until_parked();
    let asks = "On the agent's machine, sign a key request for account 12, then paste it here.";
    assert!(cx.has_text(asks), "{:?}", cx.texts());
    assert!(
        !cx.texts()
            .iter()
            .any(|text| text.contains("Created account")),
        "an agent that already existed: {:?}",
        cx.texts()
    );
}

#[test]
fn an_agent_another_program_runs_keeps_every_action_in_account() {
    let cx = with_scout("ready", scripted("forge"));
    assert!(cx.has_text("forge"), "{:?}", cx.texts());
    for kept in [
        "settings/agents/12/rename",
        "settings/agents/12/suspend",
        "settings/agents/12/revoke",
        "settings/agents/12/answering",
        "settings/agents/12/key/open",
    ] {
        assert!(cx.find(kept).is_some(), "{kept}: {:?}", cx.texts());
    }
    for gone in ["settings/agents/12/open", "settings/agents/12/harness"] {
        assert!(cx.find(gone).is_none(), "{gone}: {:?}", cx.texts());
    }
}

#[test]
fn a_connected_agent_asks_for_its_key_next() {
    let mut cx = fixture("ready", false);
    cx.simulate_click("settings/nav/agents");
    cx.run_until_parked();
    assert!(cx.has_text("Connect an external agent"), "{:?}", cx.texts());
    cx.host()
        .handle::<Submit<Identity>>(|_| Ok(view::testing::applied(view::methods::encode(&31u64))));
    cx.simulate_input("settings/agents/create/name", "Pager");
    cx.simulate_click("settings/agents/create/submit");
    cx.run_until_parked();
    let asks = "Created account 31. On the agent's machine, sign a key request for account 31, then paste it here.";
    assert!(cx.has_text(asks), "{:?}", cx.texts());
}

#[test]
fn every_key_is_judged_by_one_membership_read() {
    let cx = fixture("ready", false);
    let asked = cx.host().requests::<Query<Valset>>();
    assert!(!asked.is_empty());
    assert!(
        asked
            .iter()
            .all(|q| matches!(q, valset::Query::Memberships { .. })),
        "{asked:?}"
    );
    assert!(cx.has_text("Validator"), "{:?}", cx.texts());
}

#[test]
fn an_agents_account_lists_its_agents_and_creates_one() {
    let mut cx = fixture("scout", false);
    cx.simulate_click("settings/nav/agents");
    cx.run_until_parked();
    for text in ["Relay", "account 13 · 0 keys"] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    cx.host().handle::<Submit<Identity>>(|op| {
        assert_eq!(op, identity::Op::CreateAgent { name: "Bot".into() });
        Ok(view::testing::applied(Vec::new()))
    });
    cx.simulate_input("settings/agents/create/name", "Bot");
    cx.simulate_click("settings/agents/create/submit");
    cx.run_until_parked();
    assert_eq!(
        sent(&cx),
        [identity::Op::CreateAgent { name: "Bot".into() }]
    );
}

#[test]
fn a_manager_renames_suspends_and_revokes_an_agent() {
    let mut cx = fixture("ready", false);
    cx.simulate_click("settings/nav/agents");
    cx.run_until_parked();
    let sent = |cx: &TestAppContext| cx.host().requests::<Submit<Identity>>().len();
    assert!(cx.find("settings/agents/12/name").is_none());
    cx.simulate_click("settings/agents/12/rename");
    cx.run_until_parked();
    let Some(wire::Node::Field { value, .. }) = cx.find("settings/agents/12/name") else {
        panic!("the rename field")
    };
    assert_eq!(value, "Scout");
    assert!(cx.has_text("Save"));
    cx.simulate_input("settings/agents/12/name", " ");
    cx.simulate_click("settings/agents/12/rename");
    cx.run_until_parked();
    assert!(
        cx.has_text("Enter the agent's new name."),
        "{:?}",
        cx.texts()
    );
    assert_eq!(sent(&cx), 0);
    cx.host().handle::<Submit<Identity>>(|op| {
        assert_eq!(
            op,
            identity::Op::SetName {
                account: 12,
                name: "Scout II".into()
            }
        );
        Ok(view::testing::applied(Vec::new()))
    });
    cx.simulate_input("settings/agents/12/name", " Scout II ");
    cx.simulate_click("settings/agents/12/rename");
    cx.run_until_parked();
    assert_eq!(sent(&cx), 1);
    assert!(!cx.has_text("Enter the agent's new name."));
    assert!(
        cx.find("settings/agents/12/name").is_none(),
        "a rename that landed closes its field"
    );

    assert!(cx.find("settings/agents/12/suspend").is_some());
    assert!(cx.find("settings/agents/12/resume").is_none());
    cx.host().handle::<Submit<Identity>>(|op| {
        assert_eq!(op, identity::Op::Suspend { account: 12 });
        Ok(view::testing::applied(Vec::new()))
    });
    scout_is(&cx, identity::Life::Suspended { keys: Vec::new() });
    cx.simulate_click("settings/agents/12/suspend");
    cx.run_until_parked();
    assert_eq!(sent(&cx), 2);
    assert!(cx.has_text("suspended"), "{:?}", cx.texts());
    assert!(cx.find("settings/agents/12/resume").is_some());
    let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let logged = log.clone();
    cx.host().handle::<Submit<Identity>>(move |op| {
        logged.borrow_mut().push(op);
        Ok(view::testing::applied(Vec::new()))
    });
    cx.simulate_click("settings/agents/12/resume");
    cx.run_until_parked();
    scout_is(&cx, identity::Life::Revoked);
    cx.simulate_click("settings/agents/12/revoke");
    cx.run_until_parked();
    assert!(
        !cx.has_text("Revoke for good"),
        "the view asks no confirmation of its own: the host does"
    );
    assert_eq!(
        *log.borrow(),
        [
            identity::Op::Resume { account: 12 },
            identity::Op::Revoke { account: 12 },
        ]
    );
    assert!(
        cx.find("settings/agents/12/label").is_none(),
        "a revoked agent leaves the list: {:?}",
        cx.texts()
    );
    for action in ["rename", "suspend", "resume", "revoke"] {
        assert!(cx.find(&format!("settings/agents/12/{action}")).is_none());
    }
}

#[test]
fn an_identity_head_re_reads_the_account_in_place() {
    let mut cx = fixture("ready", false);
    let heads = cx.host().stream::<Changes<Identity>>();
    let head = || {
        Some(Change {
            height: 43,
            keys: Vec::new(),
        })
    };
    cx.host().never::<Query<Identity>>();
    heads.send(head());
    cx.run_until_parked();
    assert!(cx.has_text("Maya"), "{:?}", cx.texts());
    assert!(!cx.has_text("Reading your account…"));
    cx.host().handle::<Query<Identity>>(|q| {
        Ok(match q {
            identity::Query::Get { number } => {
                let mut renamed = maya(number, "Laptop key");
                renamed.card.name = "Maya R".into();
                identity::Reply::Account(Some(renamed))
            }
            identity::Query::Managed { .. } => {
                let mut scout = scout();
                scout.control = identity::Control::Program {
                    script: None,
                    manager: 7,
                    category: identity::Category::Agent,
                    life: identity::Life::Suspended { keys: Vec::new() },
                };
                identity::Reply::Accounts(identity::PageResponse {
                    height: 43,
                    items: vec![scout],
                    next: None,
                })
            }
            identity::Query::Narrowed { .. } => identity::Reply::Narrowed(identity::PageResponse {
                height: 42,
                items: vec![(12, identity::Ownership::Owner)],
                next: None,
            }),
            q => panic!("unexpected query: {q:?}"),
        })
    });
    cx.host().answer_held::<Query<Identity>>();
    heads.send(head());
    cx.run_until_parked();
    assert!(cx.has_text("Maya R"), "{:?}", cx.texts());
    cx.simulate_click("settings/nav/agents");
    cx.run_until_parked();
    assert!(
        cx.find("settings/agents/12/resume").is_some(),
        "{:?}",
        cx.texts()
    );
}

fn profile(number: u64, name: &str, kind: identity::Kind) -> identity::Profile {
    identity::Profile {
        number,
        name: name.into(),
        kind,
    }
}

fn person(number: u64, name: &str) -> identity::Profile {
    profile(number, name, identity::Kind::User)
}

type Answers = std::rc::Rc<std::cell::RefCell<identity::Ownership>>;

fn listing(profiles: &[identity::Profile]) -> identity::Reply {
    let control = |kind: &identity::Kind| match kind {
        identity::Kind::System(program) => identity::Control::System {
            program: program.clone(),
        },
        identity::Kind::Program {
            manager,
            category,
            standing: identity::Standing::Revoked,
            script,
        } => identity::Control::Program {
            manager: *manager,
            category: *category,
            life: identity::Life::Revoked,
            script: script.clone(),
        },
        identity::Kind::User | identity::Kind::Program { .. } => {
            identity::Control::User { keys: Vec::new() }
        }
    };
    let items = profiles
        .iter()
        .map(|profile| account(profile.number, &profile.name, control(&profile.kind)))
        .collect();
    identity::Reply::Accounts(identity::PageResponse {
        height: 42,
        items,
        next: None,
    })
}

fn listed(numbers: &[u64]) -> identity::Ownership {
    identity::Ownership::Listed(
        numbers
            .iter()
            .copied()
            .map(identity::Party::Account)
            .collect(),
    )
}

fn answering(ownership: identity::Ownership) -> (TestAppContext, Answers) {
    let mut cx = fixture("ready", false);
    let answers: Answers = std::rc::Rc::new(std::cell::RefCell::new(ownership));
    let read = answers.clone();
    let directory = [
        person(7, "Maya"),
        person(8, "Bob"),
        person(9, "Eve"),
        person(10, "Sam"),
        person(11, "sam"),
        profile(
            13,
            "Gone",
            identity::Kind::Program {
                script: None,
                manager: 7,
                category: identity::Category::Agent,
                standing: identity::Standing::Revoked,
            },
        ),
        profile(3, "forge", identity::Kind::System("forge".into())),
    ];
    cx.host().handle::<Query<Identity>>(move |q| {
        Ok(match q {
            identity::Query::Get { number } => {
                identity::Reply::Account(Some(maya(number, "Laptop key")))
            }
            identity::Query::Managed { by: 7, .. } => {
                identity::Reply::Accounts(identity::PageResponse {
                    height: 42,
                    items: vec![scout()],
                    next: None,
                })
            }
            identity::Query::Profile { number } => {
                identity::Reply::Profile(directory.iter().find(|p| p.number == number).cloned())
            }
            identity::Query::List { .. } => listing(&directory),
            identity::Query::Resolve { references } => identity::Reply::Resolved(
                references
                    .iter()
                    .map(|reference| match reference {
                        identity::Reference::Account(n) => {
                            directory.iter().any(|p| p.number == *n).then_some(*n)
                        }
                        identity::Reference::Key(_) => None,
                    })
                    .collect(),
            ),
            identity::Query::Narrowed { .. } => {
                let items = match read.borrow().clone() {
                    identity::Ownership::Anyone => Vec::new(),
                    narrowed => vec![(12, narrowed)],
                };
                identity::Reply::Narrowed(identity::PageResponse {
                    height: 42,
                    items,
                    next: None,
                })
            }
            q => panic!("unexpected query: {q:?}"),
        })
    });
    let landed = answers.clone();
    cx.host().handle::<Submit<Identity>>(move |op| {
        let identity::Op::SetOwnership {
            account: 12,
            ownership,
        } = op
        else {
            panic!("unexpected op: {op:?}");
        };
        *landed.borrow_mut() = ownership;
        Ok(view::testing::applied(Vec::new()))
    });
    let heads = cx.host().stream::<Changes<Identity>>();
    heads.send(Some(Change {
        height: 43,
        keys: Vec::new(),
    }));
    cx.simulate_click("settings/nav/agents");
    cx.run_until_parked();
    (cx, answers)
}

fn picked(cx: &TestAppContext) -> Vec<&'static str> {
    ["manager", "specific", "anyone"]
        .into_iter()
        .filter(|choice| {
            let segment = cx.interactivity(&format!("settings/agents/12/answers/{choice}"));
            segment.aria.toggled == Some(view::Toggled::True)
        })
        .collect()
}

fn sent(cx: &TestAppContext) -> Vec<identity::Op> {
    cx.host().requests::<Submit<Identity>>()
}

fn answers_sent(cx: &TestAppContext) -> Vec<identity::Op> {
    let answers = |op: &identity::Op| matches!(op, identity::Op::SetOwnership { .. });
    sent(cx).into_iter().filter(answers).collect()
}

fn scout_answers(ownership: identity::Ownership) -> identity::Op {
    identity::Op::SetOwnership {
        account: 12,
        ownership,
    }
}

const WARNING: &str = "Anyone on the network can tell this agent what to do.";

#[test]
fn an_agent_reads_as_answering_its_manager_some_people_or_anyone() {
    let (cx, _) = answering(identity::Ownership::Owner);
    assert_eq!(picked(&cx), ["manager"]);
    assert!(cx.has_text("Who it answers"), "{:?}", cx.texts());
    assert!(!cx.has_text(WARNING));
    assert!(cx.find("settings/agents/12/answers/people").is_none());

    let (cx, _) = answering(listed(&[8, 9]));
    assert_eq!(picked(&cx), ["specific"]);
    for text in ["Bob", "account 8", "Eve", "account 9", "Add someone"] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(cx.find("settings/agents/12/answers/8/remove").is_some());
    assert!(!cx.has_text(WARNING));

    let with_forge = identity::Ownership::Listed(vec![
        identity::Party::Account(8),
        identity::Party::Program("forge".into()),
    ]);
    let (cx, _) = answering(with_forge);
    assert_eq!(picked(&cx), ["specific"]);
    for text in ["Bob", "forge", "program"] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(
        cx.find("settings/agents/12/answers/program-forge/remove")
            .is_some()
    );

    let (cx, _) = answering(identity::Ownership::Anyone);
    assert_eq!(picked(&cx), ["anyone"]);
    assert!(cx.has_text(WARNING), "{:?}", cx.texts());
    assert!(cx.find("settings/agents/12/answers/people").is_none());
}

#[test]
fn the_people_every_agent_answers_are_named_by_one_read() {
    let mut cx = fixture("ready", false);
    let relay = account(
        13,
        "Relay",
        identity::Control::Program {
            script: None,
            manager: 7,
            category: identity::Category::Agent,
            life: identity::Life::Active { keys: Vec::new() },
        },
    );
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = asked.clone();
    cx.host().handle::<Query<Identity>>(move |q| {
        Ok(match q {
            identity::Query::Get { number } => {
                identity::Reply::Account(Some(maya(number, "Laptop key")))
            }
            identity::Query::Managed { by: 7, .. } => {
                log.borrow_mut().push("managed");
                identity::Reply::Accounts(identity::PageResponse {
                    height: 42,
                    items: vec![scout(), relay.clone()],
                    next: None,
                })
            }
            identity::Query::Narrowed { .. } => identity::Reply::Narrowed(identity::PageResponse {
                height: 42,
                items: vec![(12, listed(&[8])), (13, listed(&[8, 9]))],
                next: None,
            }),
            identity::Query::List { .. } => {
                log.borrow_mut().push("list");
                listing(&[person(8, "Bob"), person(9, "Eve")])
            }
            q => panic!("unexpected query: {q:?}"),
        })
    });
    let heads = cx.host().stream::<Changes<Identity>>();
    heads.send(Some(Change {
        height: 43,
        keys: Vec::new(),
    }));
    cx.simulate_click("settings/nav/agents");
    cx.run_until_parked();
    for text in ["Bob", "Eve", "account 9"] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    let asked = asked.borrow();
    assert!(!asked.is_empty());
    assert!(
        asked.chunks(2).all(|read| read == ["managed", "list"]),
        "each read of two agents names their people in one List: {asked:?}"
    );
}

#[test]
fn only_me_and_anyone_each_take_one_press() {
    let (mut cx, answers) = answering(identity::Ownership::Owner);
    cx.simulate_click("settings/agents/12/answers/anyone");
    cx.run_until_parked();
    assert_eq!(*answers.borrow(), identity::Ownership::Anyone);
    assert_eq!(picked(&cx), ["anyone"]);
    assert!(cx.has_text(WARNING), "{:?}", cx.texts());

    cx.simulate_click("settings/agents/12/answers/manager");
    cx.run_until_parked();
    assert_eq!(picked(&cx), ["manager"]);
    assert!(!cx.has_text(WARNING));
    cx.simulate_click("settings/agents/12/answers/manager");
    cx.run_until_parked();
    assert_eq!(
        answers_sent(&cx),
        [
            scout_answers(identity::Ownership::Anyone),
            scout_answers(identity::Ownership::Owner),
        ],
        "the choice it already has sends nothing"
    );
}

#[test]
fn specific_people_are_added_by_name_or_number_and_removed() {
    let (mut cx, answers) = answering(identity::Ownership::Owner);
    cx.simulate_click("settings/agents/12/answers/specific");
    cx.run_until_parked();
    assert_eq!(picked(&cx), ["specific"]);
    assert!(cx.has_text("Add people by name or account number, or a program by its name."));
    assert!(answers_sent(&cx).is_empty(), "nobody is on the list yet");

    cx.simulate_input("settings/agents/12/answers/name", " bob ");
    cx.simulate_click("settings/agents/12/answers/add");
    cx.run_until_parked();
    assert_eq!(*answers.borrow(), listed(&[8]));
    assert!(cx.has_text("account 8"), "{:?}", cx.texts());

    cx.simulate_input("settings/agents/12/answers/name", "Forge");
    cx.simulate_click("settings/agents/12/answers/add");
    cx.run_until_parked();
    let forge = identity::Party::Program("forge".into());
    assert_eq!(
        *answers.borrow(),
        identity::Ownership::Listed(vec![identity::Party::Account(8), forge.clone()]),
        "a program's own account adds the program"
    );

    cx.simulate_input("settings/agents/12/answers/name", "#9");
    cx.simulate_submit("settings/agents/12/answers/name");
    cx.run_until_parked();
    assert_eq!(
        *answers.borrow(),
        identity::Ownership::Listed(vec![
            identity::Party::Account(8),
            forge.clone(),
            identity::Party::Account(9)
        ])
    );
    assert!(cx.has_text("Eve"), "{:?}", cx.texts());

    for (typed, problem) in [
        ("Nobody", "No account or program has that name or number."),
        ("#99", "No account or program has that name or number."),
        (
            "Sam",
            "More than one account has that name. Add it by its number.",
        ),
        ("", "Enter a name, an account number or a program."),
    ] {
        cx.simulate_input("settings/agents/12/answers/name", typed);
        cx.simulate_click("settings/agents/12/answers/add");
        cx.run_until_parked();
        assert!(cx.has_text(problem), "{typed}: {:?}", cx.texts());
        assert!(cx.find("settings/agents/12/answers/refused").is_some());
    }
    assert_eq!(answers_sent(&cx).len(), 3);

    cx.simulate_click("settings/agents/12/answers/8/remove");
    cx.run_until_parked();
    assert_eq!(
        *answers.borrow(),
        identity::Ownership::Listed(vec![forge.clone(), identity::Party::Account(9)])
    );
    cx.simulate_click("settings/agents/12/answers/program-forge/remove");
    cx.run_until_parked();
    assert_eq!(*answers.borrow(), listed(&[9]));
    assert!(!cx.has_text("account 8"), "{:?}", cx.texts());
    assert!(cx.find("settings/agents/12/answers/refused").is_none());
}

fn typed(cx: &TestAppContext) -> &str {
    let Some(wire::Node::Field { value, .. }) = cx.find("settings/agents/12/answers/name") else {
        panic!("the person field: {:?}", cx.texts())
    };
    value
}

#[test]
fn a_person_typed_goes_with_a_choice_that_lands() {
    let (mut cx, _) = answering(listed(&[8]));
    cx.simulate_click("settings/agents/12/answers/add");
    cx.simulate_input("settings/agents/12/answers/name", "Sam");
    cx.simulate_click("settings/agents/12/answers/anyone");
    cx.run_until_parked();
    cx.simulate_click("settings/agents/12/answers/specific");
    assert_eq!(typed(&cx), "");

    cx.simulate_input("settings/agents/12/answers/name", "Eve");
    cx.host()
        .refuse::<Submit<Identity>>("unauthorized", "account 7 does not manage account 12");
    cx.simulate_click("settings/agents/12/answers/manager");
    cx.run_until_parked();
    cx.simulate_click("settings/agents/12/answers/specific");
    assert_eq!(typed(&cx), "Eve");
}

#[test]
fn the_person_field_waits_for_a_choice_that_is_out() {
    let (mut cx, _) = answering(listed(&[8]));
    cx.simulate_click("settings/agents/12/answers/add");
    let anyone = press(&cx, "settings/agents/12/answers/anyone");
    cx.tick(vec![anyone]);
    let Some(wire::Node::Field { options, .. }) = cx.find("settings/agents/12/answers/name") else {
        panic!("the person field: {:?}", cx.texts())
    };
    assert!(options.disabled);
    assert!(
        cx.interactivity("settings/agents/12/answers/add")
            .on_click
            .is_none()
    );
}

#[test]
fn a_landed_remove_takes_a_refused_choices_sentence() {
    let (mut cx, _) = answering(listed(&[8, 9]));
    cx.host()
        .refuse::<Submit<Identity>>("unauthorized", "account 7 does not manage account 12");
    cx.simulate_click("settings/agents/12/answers/anyone");
    cx.run_until_parked();
    let refused = "That didn’t go through: account 7 does not manage account 12";
    assert!(cx.has_text(refused));
    cx.host()
        .handle::<Submit<Identity>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.simulate_click("settings/agents/12/answers/8/remove");
    cx.run_until_parked();
    assert!(!cx.has_text(refused), "{:?}", cx.texts());
}

#[test]
fn a_refused_choice_shows_in_its_agents_row() {
    let (mut cx, answers) = answering(identity::Ownership::Owner);
    cx.host()
        .refuse::<Submit<Identity>>("unauthorized", "account 7 does not manage account 12");
    cx.simulate_click("settings/agents/12/answers/anyone");
    cx.run_until_parked();
    assert_eq!(*answers.borrow(), identity::Ownership::Owner);
    assert_eq!(picked(&cx), ["manager"]);
    assert!(!cx.has_text(WARNING));
    let Some(refusal) = cx.find("settings/agents/12/answers/refused") else {
        panic!("the refusal sits in Scout's row: {:?}", cx.texts());
    };
    assert!(
        format!("{refusal:?}").contains("account 7 does not manage account 12"),
        "{refusal:?}"
    );
    assert!(cx.has_text("That didn’t go through: account 7 does not manage account 12"));
}

#[test]
fn a_revoked_account_is_found_by_its_number_alone_and_reads_as_revoked_once_listed() {
    let (mut cx, answers) = answering(identity::Ownership::Owner);
    cx.simulate_click("settings/agents/12/answers/specific");
    cx.run_until_parked();
    cx.simulate_input("settings/agents/12/answers/name", "Gone");
    cx.simulate_click("settings/agents/12/answers/add");
    cx.run_until_parked();
    assert!(
        cx.has_text("No account or program has that name or number."),
        "{:?}",
        cx.texts()
    );
    assert!(answers_sent(&cx).is_empty());
    cx.simulate_input("settings/agents/12/answers/name", "#13");
    cx.simulate_click("settings/agents/12/answers/add");
    cx.run_until_parked();
    assert_eq!(*answers.borrow(), listed(&[13]));

    let (cx, _) = answering(listed(&[13]));
    assert!(cx.has_text("Gone (revoked)"), "{:?}", cx.texts());
    assert!(cx.find("settings/agents/12/answers/13/remove").is_some());
}
