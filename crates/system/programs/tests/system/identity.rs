use super::*;

fn consent(
    authorizer: &ed25519::PrivateKey,
    account: AccountNumber,
    new_key: &[u8],
    generation: u64,
    expires_at: u64,
) -> identity::Consent {
    let admission = identity::Admission {
        network: NETWORK.to_vec(),
        scheme: Scheme::Ed25519,
        key: new_key.to_vec(),
        generation,
        account,
        expires_at,
    };
    identity::Consent {
        key: authorizer.public_key().as_ref().to_vec(),
        account,
        expires_at,
        proof: testkit::ed25519_proof(
            authorizer,
            identity::CONSENT_NAMESPACE,
            &admission.preimage(),
        ),
    }
}

const FOUNDED: [&str; 6] = [
    programs::PROGRAM,
    valset::PROGRAM,
    identity::PROGRAM,
    blobs::PROGRAM,
    "access",
    "probe",
];

const ALICE: AccountNumber = 10;

impl Net {
    async fn account(&self, number: AccountNumber) -> identity::Account {
        let identity::Reply::Account(Some(account)) = self
            .ask(identity::PROGRAM, &identity::Query::Get { number })
            .await
        else {
            panic!("account {number} exists")
        };
        account
    }

    async fn of_program(&self, program: &str) -> Option<AccountNumber> {
        let asked = identity::Query::OfProgram {
            program: program.into(),
        };
        let identity::Reply::Number(number) = self.ask(identity::PROGRAM, &asked).await else {
            panic!()
        };
        number
    }

    async fn create(&mut self, seed: u64, name: &str) -> AccountNumber {
        let op = identity::Op::Create {
            name: name.into(),
            scheme: Scheme::Ed25519,
        };
        let output = self.apply(&public(seed), identity::PROGRAM, &op).await;
        abi::decode(&output).unwrap()
    }
}

#[test]
fn identity_founds_accounts_and_admits_keys_by_consent() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        assert_eq!(net.create(1, " Alice ").await, ALICE);
        let twice = net
            .refuse(
                &public(1),
                identity::PROGRAM,
                &identity::Op::Create {
                    name: "Alice again".into(),
                    scheme: Scheme::Ed25519,
                },
            )
            .await;
        assert_eq!(twice, reason::ALREADY_EXISTS);
        let phone = public(11);
        let expires_at = TIME + 600_000;
        net.apply(
            &phone,
            identity::PROGRAM,
            &identity::Op::AddKey {
                scheme: Scheme::Ed25519,
                label: Some("phone".into()),
                consent: consent(&key(1), ALICE, &phone, 0, expires_at),
            },
        )
        .await;
        let account = net.account(ALICE).await;
        assert_eq!(account.card.name, "Alice");
        assert_eq!(account.keys().len(), 2);
        let identity::Reply::Number(of_phone) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::OfKey { key: phone.clone() },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(of_phone, Some(ALICE));
        let replayed = net
            .refuse(
                &public(12),
                identity::PROGRAM,
                &identity::Op::AddKey {
                    scheme: Scheme::Ed25519,
                    label: None,
                    consent: consent(&key(1), ALICE, &phone, 0, expires_at),
                },
            )
            .await;
        assert_eq!(replayed, reason::UNAUTHORIZED);
        let expired = net
            .refuse(
                &public(12),
                identity::PROGRAM,
                &identity::Op::AddKey {
                    scheme: Scheme::Ed25519,
                    label: None,
                    consent: consent(&key(1), ALICE, &public(12), 0, TIME - 1),
                },
            )
            .await;
        assert_eq!(expired, reason::UNAUTHORIZED);
        let remove = |key: Vec<u8>| identity::Op::RemoveKey {
            account: ALICE,
            key,
        };
        let senior = net
            .refuse(&phone, identity::PROGRAM, &remove(public(1)))
            .await;
        assert_eq!(senior, reason::UNAUTHORIZED);
        net.apply(&public(1), identity::PROGRAM, &remove(phone.clone()))
            .await;
        let last = net
            .refuse(&public(1), identity::PROGRAM, &remove(public(1)))
            .await;
        assert_eq!(last, reason::WRONG_STATE);
        let identity::Reply::Generation(generation) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::Generation { key: phone.clone() },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(generation, 1);
        net.apply(
            &phone,
            identity::PROGRAM,
            &identity::Op::AddKey {
                scheme: Scheme::Ed25519,
                label: Some("phone again".into()),
                consent: consent(&key(1), ALICE, &phone, 1, expires_at),
            },
        )
        .await;
        net.apply(
            &public(1),
            identity::PROGRAM,
            &identity::Op::SetName {
                account: ALICE,
                name: "Alice B".into(),
            },
        )
        .await;
        let identity::Reply::Resolved(resolved) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::Resolve {
                    references: vec![
                        identity::Reference::Account(ALICE),
                        identity::Reference::Key(phone.clone()),
                        identity::Reference::Account(99),
                    ],
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(resolved, vec![Some(ALICE), Some(ALICE), None]);
    });
}

#[test]
fn every_program_has_its_account_from_its_admission() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        for (at, program) in FOUNDED.into_iter().enumerate() {
            let number = at as AccountNumber + 1;
            assert_eq!(net.of_program(program).await, Some(number), "{program}");
            let account = net.account(number).await;
            assert_eq!(
                account.control,
                identity::Control::System {
                    program: program.into()
                }
            );
        }

        let code = net.hold(programs::CODE_KIND, PROBE).await;
        let lands_at = net.height + 3;
        let entry = programs::Entry {
            program: "late".into(),
            code,
            params: abi::encode(&Vec::<Step>::new()),
        };
        let scheduled = net
            .govern(&programs::Op::Schedule(programs::Scheduled {
                height: lands_at,
                change: programs::Change::Set(entry),
            }))
            .await;
        output_of(&scheduled);
        assert_eq!(net.of_program("late").await, None);
        net.ticks(lands_at - net.height).await;
        let late = net.of_program("late").await.expect("late has an account");
        assert_eq!(net.account(late).await.card.name, "late");

        let alice = net.create(1, "Alice").await;
        let probe = net.of_program("probe").await.unwrap();
        let rename = identity::Op::SetName {
            account: probe,
            name: "Probe".into(),
        };
        let by_person = net.refuse(&public(1), identity::PROGRAM, &rename).await;
        assert_eq!(by_person, reason::UNAUTHORIZED);
        output_of(&net.sent_by("probe", identity::PROGRAM, &rename).await);
        assert_eq!(net.account(probe).await.card.name, "Probe");
        let theirs = identity::Op::SetName {
            account: alice,
            name: "Probed".into(),
        };
        let other = net.sent_by("probe", identity::PROGRAM, &theirs).await;
        assert_eq!(refusal_of(&other), reason::UNAUTHORIZED);
    });
}

#[test]
fn a_programs_account_creates_and_manages_an_agent() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let probe = net.of_program("probe").await.unwrap();
        let create = identity::Op::CreateAgent {
            name: "Runner".into(),
        };
        let made = net.sent_by("probe", identity::PROGRAM, &create).await;
        let agent: AccountNumber = abi::decode(output_of(&made)).unwrap();
        assert_eq!(
            net.account(agent).await.control,
            identity::Control::Program {
                script: None,
                manager: probe,
                category: identity::Category::Agent,
                life: identity::Life::Active { keys: Vec::new() },
            }
        );
        let suspend = identity::Op::Suspend { account: agent };
        output_of(&net.sent_by("probe", identity::PROGRAM, &suspend).await);
        assert_eq!(
            design::note(&net.account(agent).await.kind()),
            Some("suspended")
        );
    });
}

#[test]
fn an_agent_acts_until_its_manager_suspends_it() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let alice = net.create(1, "Alice").await;
        let bob = net.create(2, "Bob").await;
        let output = net
            .apply(
                &public(1),
                identity::PROGRAM,
                &identity::Op::CreateAgent {
                    name: "Scout".into(),
                },
            )
            .await;
        let agent: AccountNumber = abi::decode(&output).unwrap();
        assert_eq!(
            net.account(agent).await.control,
            identity::Control::Program {
                script: None,
                manager: alice,
                category: identity::Category::Agent,
                life: identity::Life::Active { keys: Vec::new() },
            }
        );

        let bot = key(21);
        let bot_key = public(21);
        let add = identity::Op::AddKey {
            scheme: Scheme::Ed25519,
            label: Some("sandbox".into()),
            consent: consent(&bot, agent, &bot_key, 0, TIME + 600_000),
        };
        let stranger = net.refuse(&public(2), identity::PROGRAM, &add).await;
        assert_eq!(stranger, reason::UNAUTHORIZED);
        net.apply(&public(1), identity::PROGRAM, &add).await;

        let spare_key = public(22);
        let own = identity::Op::AddKey {
            scheme: Scheme::Ed25519,
            label: Some("spare".into()),
            consent: consent(&bot, agent, &spare_key, 0, TIME + 600_000),
        };
        net.apply(&spare_key, identity::PROGRAM, &own).await;
        assert!(net.account(agent).await.holds(&spare_key));
        let remove_spare = identity::Op::RemoveKey {
            account: agent,
            key: spare_key.clone(),
        };
        net.apply(&bot_key, identity::PROGRAM, &remove_spare).await;

        let made = net
            .apply(
                &bot_key,
                identity::PROGRAM,
                &identity::Op::CreateAgent {
                    name: "Relay".into(),
                },
            )
            .await;
        let relay: AccountNumber = abi::decode(&made).unwrap();
        let standing = |standing| identity::Kind::Program {
            script: None,
            manager: agent,
            category: identity::Category::Agent,
            standing,
        };
        assert_eq!(
            net.account(relay).await.kind(),
            standing(identity::Standing::Active)
        );
        let relay_ops = [
            (
                identity::Op::Suspend { account: relay },
                identity::Standing::Suspended,
            ),
            (
                identity::Op::Resume { account: relay },
                identity::Standing::Active,
            ),
            (
                identity::Op::Revoke { account: relay },
                identity::Standing::Revoked,
            ),
        ];
        for (op, after) in relay_ops {
            let by_bob = net.refuse(&public(2), identity::PROGRAM, &op).await;
            assert_eq!(by_bob, reason::UNAUTHORIZED);
            net.apply(&bot_key, identity::PROGRAM, &op).await;
            assert_eq!(net.account(relay).await.kind(), standing(after));
        }
        let idle = Vec::<Step>::new();
        output_of(&net.submit(&bot_key, "probe", &idle).await);

        let rename = |name: &str| identity::Op::SetName {
            account: agent,
            name: name.into(),
        };
        net.apply(&bot_key, identity::PROGRAM, &rename("Me")).await;
        assert_eq!(net.account(agent).await.card.name, "Me");
        let by_bob = net
            .refuse(&public(2), identity::PROGRAM, &rename("Bob's"))
            .await;
        assert_eq!(by_bob, reason::UNAUTHORIZED);
        let take_its_card = access::Op::Set {
            program: identity::PROGRAM.into(),
            key: identity::account_key(agent),
            list: Some(identity::Ownership::Owner),
        };
        let listed_sets_no_list = net.refuse(&bot_key, access::PROGRAM, &take_its_card).await;
        assert_eq!(listed_sets_no_list, reason::UNAUTHORIZED);
        let narrow = identity::Op::SetOwnership {
            account: agent,
            ownership: identity::Ownership::Owner,
        };
        let by_bob = net.refuse(&public(2), identity::PROGRAM, &narrow).await;
        assert_eq!(by_bob, reason::UNAUTHORIZED);
        net.apply(&bot_key, identity::PROGRAM, &narrow).await;
        for (from, heard) in [(alice, true), (bob, false)] {
            let hears = identity::Query::Hears {
                account: agent,
                from,
            };
            let asked: identity::Reply = net.ask(identity::PROGRAM, &hears).await;
            assert_eq!(asked, identity::Reply::Hears(heard), "{from}");
        }
        let writers = access::Query::Writers {
            program: identity::PROGRAM.into(),
            key: identity::account_key(agent),
        };
        let access::Reply::Writers(writers) = net.ask(access::PROGRAM, &writers).await else {
            panic!()
        };
        let parties = |number| [identity::Party::Account(number)];
        assert!(writers.allows(&parties(agent)) && writers.allows(&parties(alice)));
        assert!(!writers.allows(&parties(bob)));
        net.apply(&public(1), identity::PROGRAM, &rename("Scout 2"))
            .await;
        let identity::Reply::Profile(Some(profile)) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::Profile { number: agent },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(profile.name, "Scout 2");

        let suspend = identity::Op::Suspend { account: agent };
        net.apply(&bot_key, identity::PROGRAM, &suspend).await;
        let suspended = net.refuse(&bot_key, "probe", &idle).await;
        assert_eq!(suspended, reason::UNAUTHORIZED);
        net.apply(
            &public(1),
            identity::PROGRAM,
            &identity::Op::Resume { account: agent },
        )
        .await;
        output_of(&net.submit(&bot_key, "probe", &idle).await);

        let by_bob = net.refuse(&public(2), identity::PROGRAM, &suspend).await;
        assert_eq!(by_bob, reason::UNAUTHORIZED);
        net.apply(&public(1), identity::PROGRAM, &suspend).await;
        let suspended = net.refuse(&bot_key, "probe", &idle).await;
        assert_eq!(suspended, reason::UNAUTHORIZED);
        assert_eq!(net.account(agent).await.keys().len(), 1, "it keeps its key");
        net.apply(
            &public(1),
            identity::PROGRAM,
            &identity::Op::Resume { account: agent },
        )
        .await;
        output_of(&net.submit(&bot_key, "probe", &idle).await);

        net.apply(
            &public(1),
            identity::PROGRAM,
            &identity::Op::Revoke { account: agent },
        )
        .await;
        let revived = net
            .refuse(
                &public(1),
                identity::PROGRAM,
                &identity::Op::Resume { account: agent },
            )
            .await;
        assert_eq!(revived, reason::WRONG_STATE);
        let account = net.account(agent).await;
        assert_eq!(account.card.name, "Scout 2");
        assert_eq!(
            account.kind(),
            identity::Kind::Program {
                script: None,
                manager: alice,
                category: identity::Category::Agent,
                standing: identity::Standing::Revoked,
            }
        );
    });
}

#[derive(BorshDeserialize)]
enum Probed {
    #[allow(dead_code)]
    Host(abi::HostReply),
    Env(Box<abi::Env>),
}

impl Net {
    async fn sent_as(&mut self, account: AccountNumber) -> Receipt {
        let message = Message {
            target: "probe".into(),
            payload: abi::encode(&vec![Step::Env]),
            reply: false,
            acting: Some(account),
        };
        let script = vec![Step::Op(HostOp::Emit(message))];
        let submission = self.submission(&public(PROBE_SIGNER), "probe", abi::encode(&script));
        let applied = self.block(vec![submission]).await;
        applied.submissions[0].nested[0].clone()
    }
}

#[test]
fn a_program_sends_as_the_agent_whose_script_calls_it_while_the_agent_acts() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        net.create(1, "Alice").await;
        net.create(2, "Bob").await;
        let create = identity::Op::CreateAgent {
            name: "Scout".into(),
        };
        let agent: AccountNumber =
            abi::decode(&net.apply(&public(1), identity::PROGRAM, &create).await).unwrap();
        assert_eq!(refusal_of(&net.sent_as(agent).await), reason::UNAUTHORIZED);

        let script = identity::Op::SetScript {
            account: agent,
            script: Some(identity::Script {
                calls: "probe".into(),
                config: String::new(),
            }),
        };
        let by_bob = net.refuse(&public(2), identity::PROGRAM, &script).await;
        assert_eq!(by_bob, reason::UNAUTHORIZED);
        net.apply(&public(1), identity::PROGRAM, &script).await;
        let ran = net.sent_as(agent).await;
        let probed: Vec<Probed> = abi::decode(output_of(&ran)).unwrap();
        let [Probed::Env(env)] = probed.as_slice() else {
            panic!()
        };
        assert_eq!(env.sender, Some(abi::Principal::Account(agent)));
        assert_eq!(env.origin, abi::Origin::Program("probe".into()));

        let suspend = identity::Op::Suspend { account: agent };
        net.apply(&public(1), identity::PROGRAM, &suspend).await;
        assert_eq!(refusal_of(&net.sent_as(agent).await), reason::UNAUTHORIZED);
        let user = net.sent_as(ALICE).await;
        assert_eq!(refusal_of(&user), reason::UNAUTHORIZED);
    });
}

#[test]
fn account_lists_resume_with_the_answering_height() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        for seed in 1..=3 {
            net.apply(
                &public(seed),
                identity::PROGRAM,
                &identity::Op::Create {
                    name: format!("User {seed}"),
                    scheme: Scheme::Ed25519,
                },
            )
            .await;
        }
        for _ in 0..3 {
            let op = identity::Op::CreateAgent {
                name: "Managed".into(),
            };
            net.apply(&public(1), identity::PROGRAM, &op).await;
        }
        for managed in [false, true] {
            let mut after = None;
            let mut numbers = Vec::new();
            loop {
                let page = PageRequest {
                    after,
                    limit: Some(2),
                };
                let query = if managed {
                    identity::Query::Managed { by: ALICE, page }
                } else {
                    identity::Query::List { page }
                };
                let identity::Reply::Accounts(reply) = net.ask(identity::PROGRAM, &query).await
                else {
                    panic!()
                };
                assert_eq!(reply.height, net.height);
                assert!(reply.items.len() <= 2);
                numbers.extend(reply.items.iter().map(|account| account.number));
                after = reply.next;
                if after.is_none() {
                    break;
                }
            }
            assert_eq!(
                numbers,
                if managed {
                    (ALICE + 3..=ALICE + 5).collect::<Vec<_>>()
                } else {
                    (1..=ALICE + 5).collect::<Vec<_>>()
                }
            );
        }
        let garbage = net
            .refused(
                identity::PROGRAM,
                &identity::Query::List {
                    page: PageRequest {
                        after: Some(vec![255]),
                        limit: Some(0),
                    },
                },
            )
            .await;
        assert_eq!(garbage.reason, abi::reason::INVALID_INPUT);
        let identity::Reply::Accounts(managed) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::Managed {
                    by: ALICE,
                    page: PageRequest::first(1),
                },
            )
            .await
        else {
            panic!()
        };
        let other_listing = net
            .refused(
                identity::PROGRAM,
                &identity::Query::List {
                    page: PageRequest {
                        after: managed.next,
                        limit: Some(1),
                    },
                },
            )
            .await;
        assert_eq!(other_listing.reason, abi::reason::STALE);
    });
}

#[test]
fn a_managers_device_votes_for_the_validator_on_its_agent_until_suspended() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        net.create(3, "Alice").await;
        let output = net
            .apply(
                &public(3),
                identity::PROGRAM,
                &identity::Op::CreateAgent {
                    name: "validator 1".into(),
                },
            )
            .await;
        let agent: AccountNumber = abi::decode(&output).unwrap();
        let node = identity::Op::AddKey {
            scheme: Scheme::Ed25519,
            label: Some("node".into()),
            consent: consent(&key(1), agent, &public(1), 0, TIME + 600_000),
        };
        net.apply(&public(3), identity::PROGRAM, &node).await;
        let valset::Reply::Voices(voices) = net
            .ask(valset::PROGRAM, &valset::Query::Voices { key: public(3) })
            .await
        else {
            panic!()
        };
        assert_eq!(voices, [public(1)]);

        let open = valset::Op::Vote(valset::Motion::Door { open: true });
        let receipt = net.submit(&public(3), valset::PROGRAM, &open).await;
        let voted: valset::Voted = abi::decode(output_of(&receipt)).unwrap();
        assert_eq!(
            voted,
            valset::Voted::Counted {
                votes: 1,
                needed: 2
            }
        );

        let suspend = identity::Op::Suspend { account: agent };
        net.apply(&public(3), identity::PROGRAM, &suspend).await;
        let refused = net.refuse(&public(3), valset::PROGRAM, &open).await;
        assert_eq!(refused, reason::UNAUTHORIZED);
    });
}
