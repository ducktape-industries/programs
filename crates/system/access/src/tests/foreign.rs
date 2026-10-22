//! A list naming a foreign user: it admits that user, sent through the program bound to the
//! crosspond role, and no other foreign user or account.

use ::program::Foreign;

use super::*;

const USER: Foreign = Foreign {
    network: [7; 32],
    account: 18,
};

fn crosspond_chain() -> MockChain {
    chain_with(MockHost::roles_with(abi::role::CROSSPOND, "crosspond"))
}

fn write_as(chain: &MockChain, foreign: Foreign, key: &str) -> Result<Vec<u8>, Error> {
    chain.submit_foreign(foreign, "notes", &Note::Write { key: key.into() })
}

#[test]
fn a_list_naming_a_foreign_user_admits_that_user_and_no_other() {
    let chain = crosspond_chain();
    own(&chain, ADA, "docs", None);
    let listed = Ownership::Listed(vec![Party::Foreign(USER.into())]);
    set(&chain, ADA, "docs", Some(listed)).unwrap();
    write_as(&chain, USER, "docs").unwrap();
    let elsewhere = Foreign {
        network: [8; 32],
        ..USER
    };
    let another = Foreign {
        account: 19,
        ..USER
    };
    for stranger in [elsewhere, another] {
        assert_eq!(
            refused(write_as(&chain, stranger, "docs")),
            code::UNAUTHORIZED,
            "{stranger}"
        );
    }
    assert_eq!(refused(write(&chain, BEA, "docs")), code::UNAUTHORIZED);
    write(&chain, ADA, "docs").unwrap();
}
