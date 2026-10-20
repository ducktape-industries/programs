use std::collections::{BTreeMap, BTreeSet};

use crate::{Holdings, KINDS, PROGRAMS, scope, writers};
use ownership::{Party, Policy, Scope, ScopeDefault, Writers};

#[test]
fn the_kinds_are_the_ones_the_programs_claim() {
    let claimed = [
        ("chat", chat::tables::CHANNELS.kind()),
        ("chat", chat::tables::MESSAGE_IDS.kind()),
        ("forge", forge::tables::REVIEWS.kind()),
        ("forge", forge::tables::AUTHORSHIP.kind()),
        ("forge", forge::tables::CUSTODY.kind()),
        ("harness", harness::tables::PROVIDER_ROUTES.kind()),
        ("harness", harness::tables::CONVERSATIONS.kind()),
        ("egress", egress::tables::SERVICES.kind()),
        ("files", files::tables::GUARDED.kind()),
        ("files", files::tables::PINS.kind()),
    ];
    let declared: BTreeSet<_> = claimed
        .into_iter()
        .map(|(program, kind)| (program, kind.expect("an owned table")))
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

#[test]
fn an_item_says_whose_it_is_and_whether_its_policy_admits_the_reader() {
    use crate::item::{BEFORE, about, facts};
    use crate::{Editing, Item, Standing};
    let maya = Party::Account(7);
    let ada = Party::Account(9);
    let names = BTreeMap::from([(7, "Maya".to_owned()), (9, "Ada".to_owned())]);
    let team = Policy {
        id: 3,
        owner: maya.clone(),
        name: "Team".into(),
        write: Writers::Listed(vec![ada.clone()]),
        retired: false,
        switched_to: None,
    };
    let standing = |owns| Standing {
        record: Some(ownership::Record {
            owner: maya.clone(),
            policy: 3,
            kind: Some("chat.channel".into()),
        }),
        policy: Some(team.clone()),
        names: names.clone(),
        owns,
        admits: Some(true),
    };
    assert_eq!(about(&standing(true), Some(&maya)), "You and Ada");
    assert_eq!(about(&standing(false), Some(&ada)), "Maya and Ada");
    assert_eq!(
        facts(&standing(true), Some(&maya)),
        ["Owner: You", "Policy: Team", "Can edit: You and Ada"]
    );
    assert_eq!(
        facts(&standing(false), Some(&ada)),
        ["Owner: Maya", "Policy: Team", "Can edit: Maya and Ada"]
    );
    let before = Standing {
        record: None,
        policy: None,
        names: BTreeMap::new(),
        owns: false,
        admits: None,
    };
    assert_eq!(about(&before, Some(&ada)), BEFORE);
    assert_eq!(facts(&before, Some(&ada)), [BEFORE]);
    let item = Item {
        program: "chat".into(),
        key: vec![1],
        label: "#general".into(),
    };
    let editing = Editing {
        item: Some(item.clone()),
        standing: view::Loadable::Ready(standing(false)),
        ..Editing::default()
    };
    assert_eq!(editing.admits(&item), Some(true));
    let other = Item {
        key: vec![2],
        ..item.clone()
    };
    assert_eq!(editing.admits(&other), None, "another item is not read");
}
