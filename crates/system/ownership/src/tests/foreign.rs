//! A foreign user's run: the keys it claims are owned by that foreign user, under that user's
//! own "Only me" policy, and the guard reads it from the kernel's bytes.

use ::program::{Foreign, Principal};

use super::*;

const USER: Foreign = Foreign {
    network: [7; 32],
    account: 18,
};

fn user() -> Party {
    Party::Foreign(USER.into())
}

fn crosspond_chain() -> MockChain {
    chain_with(MockHost::roles_with(abi::role::CROSSPOND, "crosspond"))
}

fn note_as(chain: &MockChain, foreign: Foreign, key: &str, value: &str) -> Result<(), Error> {
    let writes: Writes = vec![(key.into(), Some(value.to_owned()))];
    chain.submit_foreign(foreign, "notes", &writes).map(drop)
}

#[test]
fn a_key_a_foreign_run_claims_is_owned_by_that_foreign_user() {
    let chain = crosspond_chain();
    note_as(&chain, USER, "n/f", "theirs").unwrap();
    let Reply::OnlyMe(Some(only_me)) = ask(&chain, Query::OnlyMe { owner: user() }) else {
        panic!("the foreign user's \"Only me\" policy was made")
    };
    assert_eq!(
        record(&chain, "n/f"),
        Some(Record {
            owner: user(),
            policy: only_me,
            kind: Some("test.note".to_owned()),
        })
    );
    assert_eq!(effective(&chain, "n/f").unwrap().owner, user());
    assert_eq!(record(&chain, "c/count"), None, "a shared key has no owner");
}

#[test]
fn a_narrowed_policy_on_a_foreign_users_key_admits_its_owner_and_refuses_others() {
    let chain = crosspond_chain();
    let mine = made(&chain, "ada", "mine", Writers::Owner);
    default(&chain, "ada", Scope::Global, Some(mine));
    note(&chain, "ada", "n/a", Some("hers")).unwrap();
    let refused = note_as(&chain, USER, "n/a", "theirs").unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    let listed = made(&chain, "ada", "listed", Writers::Listed(vec![user()]));
    let relink = Op::Relink {
        program: "notes".into(),
        key: b"n/a".to_vec(),
        policy: listed,
    };
    act(&chain, "ada", &relink).unwrap();
    note_as(&chain, USER, "n/a", "theirs").unwrap();
    let another = Foreign {
        account: 19,
        ..USER
    };
    let refused = note_as(&chain, another, "n/a", "not theirs").unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
}

#[test]
fn the_foreign_party_is_the_kernels_bytes_and_an_owner_read_from_a_stored_principal() {
    let foreign = abi::role::access::Party::Foreign(USER.into());
    assert_eq!(abi::encode(&user()), abi::encode(&foreign));
    assert_eq!(Party::from(foreign), user());
    assert_eq!(owner_of(&Principal::Foreign(USER), "notes"), user());
}
