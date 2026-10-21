use super::*;

#[test]
fn a_schedule_is_refused_until_a_quorum_of_validators_holds_the_code() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let code = net.stored(programs::CODE_KIND, &program("chat"));
        let id = code.id;
        let bundle = net.put(vec![code]).await;
        let schedule = programs::Op::Schedule(programs::Scheduled {
            height: net.height + 10,
            change: programs::Change::Set(programs::Entry {
                program: "o/identity2".into(),
                code: id,
                params: Vec::new(),
            }),
        });
        assert_eq!(net.blob(&id).await, None, "a put blob is no held one");
        assert_eq!(refusal_of(&net.govern(&schedule).await), reason::NOT_FOUND);

        net.receipted(bundle, &FOUNDERS[..1]).await;
        assert_eq!(net.blob(&id).await, None, "one receipt of two is no quorum");
        assert_eq!(refusal_of(&net.govern(&schedule).await), reason::NOT_FOUND);

        net.receipted(bundle, &FOUNDERS[1..]).await;
        assert!(net.blob(&id).await.is_some(), "the host serves held code");
        output_of(&net.govern(&schedule).await);

        let pack = net.hold("pack", b"PACK not code").await;
        let not_code = programs::Op::Schedule(programs::Scheduled {
            height: net.height + 10,
            change: programs::Change::Set(programs::Entry {
                program: "packed".into(),
                code: pack,
                params: Vec::new(),
            }),
        });
        assert_eq!(
            refusal_of(&net.govern(&not_code).await),
            reason::INVALID_INPUT
        );
    });
}

#[test]
fn a_held_program_is_scheduled_by_the_validators_vote_and_seated_at_its_height() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let code = net.hold(programs::CODE_KIND, &program("identity")).await;
        let entry = programs::Entry {
            program: "o/identity2".into(),
            code,
            params: Vec::new(),
        };
        let unheld = net
            .govern(&programs::Op::Schedule(programs::Scheduled {
                height: net.height + 3,
                change: programs::Change::Set(programs::Entry {
                    program: "ghost".into(),
                    code: BlobId::Sha256([9; 32]),
                    params: Vec::new(),
                }),
            }))
            .await;
        assert_eq!(refusal_of(&unheld), reason::NOT_FOUND);
        let past = programs::Op::Schedule(programs::Scheduled {
            height: net.height,
            change: programs::Change::Set(entry.clone()),
        });
        assert_eq!(refusal_of(&net.govern(&past).await), reason::INVALID_INPUT);
        let lands_at = net.height + 6;
        let scheduled = net
            .govern(&programs::Op::Schedule(programs::Scheduled {
                height: lands_at,
                change: programs::Change::Set(entry.clone()),
            }))
            .await;
        output_of(&scheduled);
        let taken = net
            .govern(&programs::Op::Schedule(programs::Scheduled {
                height: lands_at,
                change: programs::Change::Remove("o/identity2".into()),
            }))
            .await;
        assert_eq!(refusal_of(&taken), reason::ALREADY_EXISTS);
        let programs::Reply::Scheduled(pending) = net
            .ask(
                programs::PROGRAM,
                &programs::Query::Scheduled {
                    page: PageRequest::default(),
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(pending.height, net.height);
        assert_eq!(pending.next, None);
        let pending = pending.items;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].height, lands_at);
        assert_eq!(pending[0].change, programs::Change::Set(entry.clone()));
        let programs::Reply::Programs(later) = net
            .ask(programs::PROGRAM, &programs::Query::At(lands_at))
            .await
        else {
            panic!()
        };
        assert!(later.iter().any(|entry| entry.program == "o/identity2"));
        while net.height + 1 < lands_at {
            let applied = net.tick().await;
            assert!(applied.admissions.is_empty());
        }
        let applied = net.tick().await;
        assert_eq!(applied.height, lands_at);
        let admitted: Vec<&str> = applied
            .admissions
            .iter()
            .map(|receipt| receipt.program.as_str())
            .collect();
        assert_eq!(admitted, [identity::PROGRAM, "o/identity2"]);
        assert!(net.host.programs().unwrap().contains_key("o/identity2"));
        let programs::Reply::Roster {
            height, programs, ..
        } = net.ask(programs::PROGRAM, &programs::Query::Roster).await
        else {
            panic!()
        };
        assert_eq!(height, net.height);
        assert!(programs.iter().any(|entry| entry.program == "o/identity2"));
        net.apply(
            &public(7),
            "o/identity2",
            &identity::Op::Create {
                name: "Seven".into(),
                scheme: Scheme::Ed25519,
            },
        )
        .await;
        let programs::Reply::Program {
            height,
            entry: Some(seated),
        } = net
            .ask(
                programs::PROGRAM,
                &programs::Query::Program("o/identity2".into()),
            )
            .await
        else {
            panic!()
        };
        assert_eq!(height, net.height);
        assert_eq!(seated, entry);
        let removal_at = net.height + 5;
        let removal = net
            .govern(&programs::Op::Schedule(programs::Scheduled {
                height: removal_at,
                change: programs::Change::Remove("o/identity2".into()),
            }))
            .await;
        output_of(&removal);
        let cancelled = net
            .govern(&programs::Op::Cancel {
                height: removal_at,
                program: "o/identity2".into(),
            })
            .await;
        output_of(&cancelled);
        net.ticks(3).await;
        assert!(net.host.programs().unwrap().contains_key("o/identity2"));
        let removal_at = net.height + 3;
        let removal = net
            .govern(&programs::Op::Schedule(programs::Scheduled {
                height: removal_at,
                change: programs::Change::Remove("o/identity2".into()),
            }))
            .await;
        output_of(&removal);
        net.ticks(2).await;
        assert!(!net.host.programs().unwrap().contains_key("o/identity2"));
    });
}

#[test]
fn schedule_pages_and_missing_programs_report_the_answering_height() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let code = net.hold(programs::CODE_KIND, &program("identity")).await;
        for program in ["o/z", "o/a", "o/m"] {
            let receipt = net
                .govern(&programs::Op::Schedule(programs::Scheduled {
                    height: 100,
                    change: programs::Change::Set(programs::Entry {
                        program: program.into(),
                        code,
                        params: Vec::new(),
                    }),
                }))
                .await;
            output_of(&receipt);
        }
        let mut after = None;
        let mut programs = Vec::new();
        loop {
            let programs::Reply::Scheduled(reply) = net
                .ask(
                    programs::PROGRAM,
                    &programs::Query::Scheduled {
                        page: PageRequest {
                            after,
                            limit: Some(2),
                        },
                    },
                )
                .await
            else {
                panic!()
            };
            assert_eq!(reply.height, net.height);
            assert!(reply.items.len() <= 2);
            for scheduled in reply.items {
                assert_eq!(scheduled.height, 100);
                programs.push(scheduled.change.program().to_owned());
            }
            after = reply.next;
            if after.is_none() {
                break;
            }
        }
        assert_eq!(programs, ["o/a", "o/m", "o/z"]);
        let reply: programs::Reply = net
            .ask(
                programs::PROGRAM,
                &programs::Query::Program("missing".into()),
            )
            .await;
        assert_eq!(
            reply,
            programs::Reply::Program {
                height: net.height,
                entry: None
            }
        );
    });
}

#[test]
fn a_schedule_from_a_key_with_no_account_or_under_the_notice_is_refused() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let code = net.hold(programs::CODE_KIND, &program("identity")).await;
        let schedule = programs::Op::Schedule(programs::Scheduled {
            height: net.height + 5,
            change: programs::Change::Set(programs::Entry {
                program: "o/identity2".into(),
                code,
                params: Vec::new(),
            }),
        });
        for signer in [public(1), public(7)] {
            let refused = net.refuse(&signer, programs::PROGRAM, &schedule).await;
            assert_eq!(refused, reason::UNAUTHORIZED, "a key with no account");
        }
        let by_program = net.as_anyone(programs::PROGRAM, &schedule).await;
        assert_eq!(
            refusal_of(&by_program),
            reason::INVALID_INPUT,
            "a program's own account, under the notice"
        );
        let programs::Reply::Scheduled(pending) = net
            .ask(
                programs::PROGRAM,
                &programs::Query::Scheduled {
                    page: PageRequest::default(),
                },
            )
            .await
        else {
            panic!()
        };
        assert!(pending.items.is_empty());
        output_of(&net.govern(&schedule).await);
    });
}

#[test]
fn an_account_upgrades_a_program_it_holds_with_no_vote() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let create = identity::Op::Create {
            name: "Seven".into(),
            scheme: Scheme::Ed25519,
        };
        let output = net.apply(&public(7), identity::PROGRAM, &create).await;
        let seven: AccountNumber = abi::decode(&output).unwrap();
        let code = net.hold(programs::CODE_KIND, &program("chat")).await;
        let upgrade = |height| {
            programs::Op::Schedule(programs::Scheduled {
                height,
                change: programs::Change::Set(programs::Entry {
                    program: "probe".into(),
                    code,
                    params: Vec::new(),
                }),
            })
        };
        let lands_at = net.height + 1 + programs::NOTICE;
        let a_role = programs::Op::Schedule(programs::Scheduled {
            height: lands_at,
            change: programs::Change::Set(programs::Entry {
                program: identity::PROGRAM.into(),
                code,
                params: Vec::new(),
            }),
        });
        let refused = net.refuse(&public(7), programs::PROGRAM, &a_role).await;
        assert_eq!(refused, reason::UNAUTHORIZED, "a role's program");
        let held = net
            .refuse(&public(7), programs::PROGRAM, &upgrade(lands_at))
            .await;
        assert_eq!(held, reason::UNAUTHORIZED, "probe is the vote's");
        let handed = programs::Op::Own {
            program: "probe".into(),
            holder: programs::Holder::Account(seven),
        };
        output_of(&net.govern(&handed).await);
        let programs::Reply::Owner { holder, .. } = net
            .ask(programs::PROGRAM, &programs::Query::Owner("probe".into()))
            .await
        else {
            panic!()
        };
        assert_eq!(holder, Some(programs::Holder::Account(seven)));
        let lands_at = net.height + 1 + programs::NOTICE;
        net.apply(&public(7), programs::PROGRAM, &upgrade(lands_at))
            .await;
        let programs::Reply::Scheduled(pending) = net
            .ask(
                programs::PROGRAM,
                &programs::Query::Scheduled {
                    page: PageRequest::default(),
                },
            )
            .await
        else {
            panic!()
        };
        let programs::Op::Schedule(scheduled) = upgrade(lands_at) else {
            unreachable!()
        };
        assert_eq!(
            pending.items,
            [programs::Pending {
                height: lands_at,
                change: scheduled.change,
                by: Some(seven),
            }]
        );
        let running = |net: &Net| net.host.programs().unwrap()["probe"];
        let before = running(&net);
        net.ticks(lands_at - 1 - net.height).await;
        assert_eq!(running(&net), before, "a block before");
        net.tick().await;
        assert_eq!(running(&net), code);
    });
}

#[test]
fn the_vote_overrules_an_accounts_change_at_its_own_key() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let create = identity::Op::Create {
            name: "Seven".into(),
            scheme: Scheme::Ed25519,
        };
        let output = net.apply(&public(7), identity::PROGRAM, &create).await;
        let seven: AccountNumber = abi::decode(&output).unwrap();
        let handed = programs::Op::Own {
            program: "probe".into(),
            holder: programs::Holder::Account(seven),
        };
        output_of(&net.govern(&handed).await);
        let code = net.hold(programs::CODE_KIND, &program("chat")).await;
        let lands_at = net.height + 1 + programs::NOTICE;
        let change = programs::Change::Set(programs::Entry {
            program: "probe".into(),
            code,
            params: Vec::new(),
        });
        let at = |change: &programs::Change| {
            programs::Op::Schedule(programs::Scheduled {
                height: lands_at,
                change: change.clone(),
            })
        };
        net.apply(&public(7), programs::PROGRAM, &at(&change)).await;
        let overruled = net.govern(&at(&change)).await;
        output_of(&overruled);
        let programs::Reply::Scheduled(pending) = net
            .ask(
                programs::PROGRAM,
                &programs::Query::Scheduled {
                    page: PageRequest::default(),
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(
            pending.items,
            [programs::Pending {
                height: lands_at,
                change,
                by: None,
            }]
        );
        let gone = net
            .govern(&programs::Op::Cancel {
                height: lands_at + 1,
                program: "probe".into(),
            })
            .await;
        output_of(&gone);
    });
}
