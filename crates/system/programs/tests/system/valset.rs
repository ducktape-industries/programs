use super::*;

fn voted(receipt: &Receipt) -> valset::Voted {
    abi::decode(output_of(receipt)).unwrap()
}

async fn next_epoch(net: &mut Net) -> u64 {
    let epoch = (net.height + EPOCH_LENGTH) / EPOCH_LENGTH;
    while net.host.epoch_members(epoch).unwrap().is_none() {
        net.tick().await;
    }
    epoch
}

#[test]
fn a_stranger_with_an_invite_enrolls_as_a_resident_the_next_epoch_tracks() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let uninvited = net
            .refuse(&public(3), valset::PROGRAM, &enroll(3, None))
            .await;
        assert_eq!(uninvited, reason::UNAUTHORIZED);
        let ticket = invite(1, 7, net.time() + 3_600_000);
        net.apply(
            &public(3),
            valset::PROGRAM,
            &enroll(3, Some(ticket.clone())),
        )
        .await;
        let mut expected = vec![
            membership(1, valset::Role::Validator),
            membership(2, valset::Role::Validator),
            membership(3, valset::Role::Resident),
        ];
        expected.sort_by(|a, b| a.key.cmp(&b.key));
        assert_eq!(net.memberships().await, expected);
        let reused = net
            .refuse(&public(4), valset::PROGRAM, &enroll(4, Some(ticket)))
            .await;
        assert_eq!(reused, reason::WRONG_STATE);
        let epoch = next_epoch(&mut net).await;
        let members = net.host.epoch_members(epoch).unwrap().unwrap();
        assert_eq!(members.len(), 3);
        assert!(members.iter().any(|member| member.key == public(3)));
        let seated = net.host.epoch_validators(epoch).unwrap().unwrap();
        assert_eq!(seated, net.validators().await);
        assert_eq!(seated.len(), 2);
        assert!(!seated.contains(&public(3)));
    });
}

#[test]
fn a_founding_validator_enrolling_again_moves_its_address() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let moved = valset::Op::Enroll {
            address: Some("elsewhere:1".into()),
            halo: halo(1),
            invite: None,
        };
        net.apply(&public(1), valset::PROGRAM, &moved).await;
        let valset::Reply::Membership(Some(held)) = net
            .ask(
                valset::PROGRAM,
                &valset::Query::Membership { key: public(1) },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(held.address.as_deref(), Some("elsewhere:1"));
        assert_eq!(held.role, valset::Role::Validator);
    });
}

#[test]
fn an_invite_signed_by_a_stranger_or_for_another_network_is_refused() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let expires = net.time() + 3_600_000;
        let by_stranger = invite(9, 1, expires);
        let refused = net
            .refuse(&public(3), valset::PROGRAM, &enroll(3, Some(by_stranger)))
            .await;
        assert_eq!(refused, reason::UNAUTHORIZED);
        let mut forged = invite(1, 2, expires);
        forged.grant.nonce = [3; 16];
        let refused = net
            .refuse(&public(3), valset::PROGRAM, &enroll(3, Some(forged)))
            .await;
        assert_eq!(refused, reason::UNAUTHORIZED);
        let expired = invite(1, 4, net.time());
        let refused = net
            .refuse(&public(3), valset::PROGRAM, &enroll(3, Some(expired)))
            .await;
        assert_eq!(refused, reason::INVALID_INPUT);
        assert_eq!(net.memberships().await.len(), 2);
    });
}

#[test]
fn a_member_cannot_promote_itself_and_a_program_cannot_write() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let promote = valset::Op::Vote(valset::Motion::Promote { key: public(3) });
        let stranger = net.refuse(&public(3), valset::PROGRAM, &promote).await;
        assert_eq!(stranger, reason::NOT_FOUND);
        let ticket = invite(1, 1, net.time() + 3_600_000);
        net.apply(&public(3), valset::PROGRAM, &enroll(3, Some(ticket)))
            .await;
        let resident = net.refuse(&public(3), valset::PROGRAM, &promote).await;
        assert_eq!(resident, reason::UNAUTHORIZED);
        let by_program = net.as_anyone(valset::PROGRAM, &promote).await;
        assert_eq!(refusal_of(&by_program), reason::UNAUTHORIZED);
        assert_eq!(net.validators().await.len(), 2);
    });
}

#[test]
fn the_validators_vote_promotes_a_resident_and_the_next_epoch_seats_it() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let ticket = invite(2, 1, net.time() + 3_600_000);
        net.apply(&public(3), valset::PROGRAM, &enroll(3, Some(ticket)))
            .await;
        let promoted = net.pass(valset::Motion::Promote { key: public(3) }).await;
        assert_eq!(voted(&promoted), valset::Voted::Enacted);
        assert_eq!(net.validators().await.len(), 3);
        let epoch = next_epoch(&mut net).await;
        let seated = net.host.epoch_validators(epoch).unwrap().unwrap();
        assert!(seated.contains(&public(3)));
        let counted = net
            .submit(
                &public(1),
                valset::PROGRAM,
                &valset::Op::Vote(valset::Motion::Remove { key: public(2) }),
            )
            .await;
        assert_eq!(
            voted(&counted),
            valset::Voted::Counted {
                votes: 1,
                needed: 3
            }
        );
        let left = net
            .submit(&public(3), valset::PROGRAM, &valset::Op::Leave)
            .await;
        output_of(&left);
        assert_eq!(net.validators().await.len(), 2);
    });
}

#[test]
fn a_newcomer_past_the_cap_is_refused_on_the_host() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found_with(context, dir.path(), Vec::new(), Vec::new(), 2).await;
        let ticket = invite(1, 1, net.time() + 3_600_000);
        let refused = net
            .refuse(&public(3), valset::PROGRAM, &enroll(3, Some(ticket)))
            .await;
        assert_eq!(refused, reason::CAPACITY);
        assert_eq!(net.memberships().await.len(), 2);
    });
}
