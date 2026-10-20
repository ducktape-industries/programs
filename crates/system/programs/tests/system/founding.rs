use super::*;

#[test]
fn founding_seats_the_validators_and_every_program_answers() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let net = Net::found(context, dir.path()).await;
        let programs = net.host.programs().unwrap();
        for program in [programs::PROGRAM, valset::PROGRAM, identity::PROGRAM] {
            assert!(programs.contains_key(program), "{program} is not rostered");
        }
        assert!(!programs.contains_key("lens"), "a view is never admitted");
        let programs::Reply::Roster { views, .. } =
            net.ask(programs::PROGRAM, &programs::Query::Roster).await
        else {
            panic!()
        };
        let names: Vec<&str> = views.iter().map(|view| view.name.as_str()).collect();
        assert_eq!(names, ["lens"]);
        let programs::Reply::Roles(roles) =
            net.ask(programs::PROGRAM, &programs::Query::Roles).await
        else {
            panic!()
        };
        assert_eq!(
            [
                abi::role::PROGRAMS,
                abi::role::VALIDATORS,
                abi::role::IDENTITY,
                abi::role::BLOBS,
                abi::role::HALO
            ]
            .map(|role| roles.get(role).map(String::as_str)),
            [
                Some(programs::PROGRAM),
                Some(valset::PROGRAM),
                Some(identity::PROGRAM),
                Some(blobs::PROGRAM),
                Some(halo::PROGRAM)
            ]
        );
        assert_eq!(
            net.blob(&views[0].view).await.as_deref(),
            Some(&b"program 6\0a view"[..]),
            "the founding stored the view's bytes"
        );
        let memberships = net.memberships().await;
        assert_eq!(memberships.len(), 2);
        assert!(
            memberships
                .iter()
                .all(|membership| membership.role == valset::Role::Validator)
        );
        let seated = net.host.epoch_members(0).unwrap().unwrap();
        assert_eq!(seated.len(), 2);
        assert!(seated.contains(&member(1)));
        assert!(seated.contains(&member(2)));
        let identity::Reply::Accounts(accounts) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::List {
                    page: PageRequest::default(),
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(accounts.height, net.height);
        assert_eq!(accounts.next, None);
        let programs: Vec<_> = accounts
            .items
            .iter()
            .map(|account| account.kind())
            .collect();
        let program = |program: &str| identity::Kind::System(program.into());
        assert_eq!(
            programs,
            [
                program(programs::PROGRAM),
                program(valset::PROGRAM),
                program(identity::PROGRAM),
                program(blobs::PROGRAM),
                program("access"),
                program("probe"),
                program(awaits::PROGRAM),
                program(halo::PROGRAM),
                program(scheduler::PROGRAM),
            ]
        );
    });
}
