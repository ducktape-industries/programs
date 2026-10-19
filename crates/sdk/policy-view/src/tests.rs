use std::collections::{BTreeMap, BTreeSet};

use ownership::{Party, Policy, Scope, ScopeDefault, Writers};
use program::{Held, Program};

use crate::{Holdings, KINDS, PROGRAMS, scope, writers};

// Each owned kind `P` declares, with the program's name.
fn owned<P: Program>() -> impl Iterator<Item = (&'static str, &'static str)> {
    P::TABLES.iter().filter_map(|table| match &table.held {
        Held::Owned(Some(kind)) => Some((P::NAME, *kind)),
        _ => None,
    })
}

#[test]
fn the_kinds_are_the_ones_the_programs_declare() {
    let declared: BTreeSet<_> = owned::<chat::Chat>()
        .chain(owned::<forge::Forge>())
        .chain(owned::<harness::Harness>())
        .chain(owned::<egress::Egress>())
        .collect();
    let listed: BTreeSet<_> = KINDS.iter().map(|kind| (kind.program, kind.name)).collect();
    assert_eq!(listed, declared);
    let mut programs: Vec<&str> = Vec::new();
    for kind in &KINDS {
        if !programs.contains(&kind.program) {
            programs.push(kind.program);
        }
    }
    assert_eq!(programs, PROGRAMS);
}

#[test]
fn a_policy_says_who_may_write_beside_its_owner() {
    let maya = Party::Account(7);
    let names = BTreeMap::from([(7, "Maya".to_owned()), (9, "Ada".to_owned())]);
    let to_maya = |write: Writers| writers(&write, &maya, Some(&maya), &names);
    assert_eq!(to_maya(Writers::Anyone), "Anyone");
    assert_eq!(to_maya(Writers::Owner), "Only you");
    let team = Writers::Listed(vec![Party::Account(9), Party::Program("board".into())]);
    assert_eq!(to_maya(team.clone()), "You, Ada and board");
    let stranger = Writers::Listed(vec![Party::Account(12)]);
    assert_eq!(to_maya(stranger), "You and account 12");
    // the owner listed again adds nobody
    assert_eq!(to_maya(Writers::Listed(vec![maya.clone()])), "Only you");
    let to_ada = writers(&team, &maya, Some(&Party::Account(9)), &names);
    assert_eq!(to_ada, "Maya, Ada and board");
    assert_eq!(writers(&Writers::Owner, &maya, None, &names), "Only Maya");
}

#[test]
fn a_scope_with_no_default_inherits_the_next_wider_one() {
    let policy = |id, name: &str| Policy {
        id,
        owner: Party::Account(7),
        name: name.into(),
        write: Writers::Owner,
        retired: false,
        switched_to: None,
    };
    let holdings = Holdings {
        owner: Party::Account(7),
        open: Some(1),
        policies: vec![policy(1, "Open"), policy(2, "Team"), policy(3, "Close")],
        defaults: vec![
            ScopeDefault {
                scope: Scope::Program("chat".into()),
                policy: 2,
            },
            ScopeDefault {
                scope: Scope::Kind("chat.message".into()),
                policy: 3,
            },
        ],
        names: BTreeMap::new(),
    };
    let message = Scope::Kind("chat.message".into());
    let channel = Scope::Kind("chat.channel".into());
    assert_eq!(holdings.default_of(&message), Some(3));
    assert_eq!(holdings.inherited(&message), Some(2));
    assert_eq!(holdings.default_of(&channel), None);
    assert_eq!(holdings.inherited(&channel), Some(2));
    assert_eq!(
        holdings.inherited(&Scope::Kind("forge.repo".into())),
        Some(1)
    );
    assert_eq!(scope(&message), "Chat messages");
    assert_eq!(
        scope(&Scope::Program("files".into())),
        "Everything in files"
    );
    assert_eq!(scope(&Scope::Global), "Everything");
}
