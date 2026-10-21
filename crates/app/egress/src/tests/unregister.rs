//! Removing a service keeps its name with its last registrant and leaves pending calls alone.
use super::*;

fn register() -> Op {
    Op::Register {
        service: SERVICE.into(),
    }
}

fn unregister() -> Op {
    Op::Unregister {
        service: SERVICE.into(),
    }
}

fn assign(nodes: Vec<Vec<u8>>) -> Op {
    Op::Assign {
        service: SERVICE.into(),
        nodes,
    }
}

#[test]
fn unregister_requires_its_owner_and_an_empty_assignment() {
    let chain = chain();
    submit(&chain, OWNER, &register()).unwrap();
    assert_eq!(
        submit(&chain, OTHER, &unregister()).unwrap_err().code,
        code::UNAUTHORIZED
    );
    submit(&chain, OWNER, &assign(vec![ADA.to_vec()])).unwrap();
    assert_eq!(
        submit(&chain, OWNER, &unregister()).unwrap_err().code,
        code::WRONG_STATE
    );
    submit(&chain, OWNER, &assign(Vec::new())).unwrap();
    submit(&chain, OWNER, &unregister()).unwrap();
    let Reply::Service(row) = chain
        .query(
            PROGRAM,
            &Query::Service {
                name: SERVICE.into(),
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(row, None);
    let Reply::Services(rows) = chain
        .query(
            PROGRAM,
            &Query::Services {
                page: PageRequest::default(),
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert!(rows.items.is_empty());
    assert_eq!(
        submit(&chain, OTHER, &register()).unwrap_err().code,
        code::UNAUTHORIZED
    );
    submit(&chain, OWNER, &register()).unwrap();
}

#[test]
fn an_unknown_service_cannot_be_unregistered() {
    let chain = chain();
    assert_eq!(
        submit(&chain, OWNER, &unregister()).unwrap_err().code,
        code::NOT_FOUND
    );
    let vote = Origin::Program(MockHost::role(abi::role::VALIDATORS));
    assert_eq!(
        chain.submit(vote, PROGRAM, &unregister()).unwrap_err().code,
        code::NOT_FOUND
    );
}

#[test]
fn the_vote_can_restore_a_retired_name_to_its_last_owner() {
    let chain = chain();
    submit(&chain, OWNER, &register()).unwrap();
    let Reply::Service(Some(before)) = chain
        .query(
            PROGRAM,
            &Query::Service {
                name: SERVICE.into(),
            },
        )
        .unwrap()
    else {
        panic!()
    };
    submit(&chain, OWNER, &unregister()).unwrap();
    let vote = Origin::Program(MockHost::role(abi::role::VALIDATORS));
    chain.submit(vote, PROGRAM, &register()).unwrap();
    let Reply::Service(Some(after)) = chain
        .query(
            PROGRAM,
            &Query::Service {
                name: SERVICE.into(),
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(after.owner, before.owner);
}

#[test]
fn unregister_does_not_cancel_or_prevent_answering_an_open_request() {
    let chain = chain();
    submit(&chain, OWNER, &register()).unwrap();
    submit(&chain, OWNER, &assign(vec![ADA.to_vec()])).unwrap();
    serve(&chain, ADA, &[SERVICE]).unwrap();
    ask(&chain, http_request(SERVICE, "/", Vec::new()));
    let pending = request_row(&chain, 1).unwrap();
    submit(&chain, OWNER, &assign(Vec::new())).unwrap();
    submit(&chain, OWNER, &unregister()).unwrap();
    assert_eq!(request_row(&chain, 1), Some(pending));
    ask(&chain, http_request(SERVICE, "/", Vec::new()));
    assert_eq!(
        answers(&chain)[0].as_ref().unwrap_err().code,
        code::NOT_FOUND
    );
    chain.at(2, 0);
    answer(&chain, ADA, 1, Answer::Output(b"finished".to_vec())).unwrap();
    assert_eq!(answers(&chain)[1].as_deref().unwrap(), b"finished");
    assert!(request_row(&chain, 1).is_none());
}

#[test]
fn a_guarded_service_can_be_removed_and_registered_again() {
    let mut chain = chain();
    chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    submit(&chain, OWNER, &register()).unwrap();
    submit(&chain, OWNER, &unregister()).unwrap();
    assert_eq!(
        submit(&chain, OTHER, &register()).unwrap_err().code,
        code::UNAUTHORIZED
    );
    submit(&chain, OWNER, &register()).unwrap();
    submit(&chain, OWNER, &unregister()).unwrap();
}
