//! Two interface changes whose Rust edits merge apart also merge in founding's committed
//! baselines, as forge's Rebase merges a file; two appends at one enum's end still conflict.
#![allow(dead_code)]

use borsh::BorshSchema;
use borsh::schema::BorshSchemaContainer;
use gitcore::merge::{Clash, trees};
use gitcore::{Hash, Kind, MemoryObjects, Mode, Oid, Tree, TreeEntry};

#[derive(BorshSchema)]
struct Interface<O, Q, R> {
    op: O,
    query: Q,
    returns: R,
}

mod base {
    use borsh::BorshSchema;

    #[derive(BorshSchema)]
    pub enum Op {
        Post { channel: String, text: String },
        Delete(u64),
    }

    #[derive(BorshSchema)]
    pub enum Query {
        Thread(u64),
        Head,
    }

    #[derive(BorshSchema)]
    #[allow(non_snake_case)]
    pub struct Returns {
        pub Post: u64,
        pub Delete: (),
    }
}

mod reacted {
    use borsh::BorshSchema;

    #[derive(BorshSchema)]
    pub enum Op {
        Post { channel: String, text: String },
        Delete(u64),
        React { seq: u64, emoji: String },
    }

    #[derive(BorshSchema)]
    #[allow(non_snake_case)]
    pub struct Returns {
        pub Post: u64,
        pub Delete: (),
        pub React: (),
    }
}

mod pinned {
    use borsh::BorshSchema;

    #[derive(BorshSchema)]
    pub enum Op {
        Post { channel: String, text: String },
        Delete(u64),
        Pin(u64),
    }

    #[derive(BorshSchema)]
    #[allow(non_snake_case)]
    pub struct Returns {
        pub Post: u64,
        pub Delete: (),
        pub Pin: (),
    }
}

mod counted {
    use borsh::BorshSchema;

    #[derive(BorshSchema)]
    pub enum Query {
        Thread(u64),
        Head,
        Count(String),
    }
}

fn baseline<O: BorshSchema, Q: BorshSchema, R: BorshSchema>() -> Vec<u8> {
    founding::baseline(&BorshSchemaContainer::for_type::<Interface<O, Q, R>>()).unwrap()
}

fn tree_holding(store: &mut MemoryObjects, baseline: &[u8]) -> Oid {
    let id = store.put(Kind::Blob, baseline).unwrap();
    let entries = vec![TreeEntry {
        mode: Mode::Regular,
        name: b"chat.schema".to_vec(),
        id,
    }];
    store
        .put(Kind::Tree, &Tree { entries }.serialize())
        .unwrap()
}

fn merged(base: &[u8], ours: &[u8], theirs: &[u8]) -> Result<Vec<u8>, Vec<Clash>> {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let [base, ours, theirs] = [base, ours, theirs].map(|bytes| tree_holding(&mut store, bytes));
    let cost = usize::try_from(founding::forge_bounds().merge_cost).unwrap();
    let merge = trees(&store, Some(&base), &ours, &theirs, cost).unwrap();
    if !merge.conflicts.is_empty() {
        return Err(merge
            .conflicts
            .iter()
            .map(|conflict| conflict.clash)
            .collect());
    }
    let root = Tree::parse(&merge.objects[&merge.tree].body, Hash::Sha1).unwrap();
    Ok(merge.objects[&root.entries[0].id].body.clone())
}

#[test]
fn appends_to_two_enums_of_one_interface_merge_into_the_baseline_of_both() {
    let base = baseline::<base::Op, base::Query, base::Returns>();
    let ops = baseline::<reacted::Op, base::Query, reacted::Returns>();
    let queries = baseline::<base::Op, counted::Query, base::Returns>();
    let both = baseline::<reacted::Op, counted::Query, reacted::Returns>();
    for (ours, theirs) in [(&ops, &queries), (&queries, &ops)] {
        let landed = merged(&base, ours, theirs);
        assert_eq!(landed.as_ref().err(), None);
        assert!(
            landed.ok() == Some(both.clone()),
            "the merged baseline is not the baseline of both appends"
        );
    }
}

#[test]
fn two_appends_at_one_enums_end_still_conflict() {
    let base = baseline::<base::Op, base::Query, base::Returns>();
    let react = baseline::<reacted::Op, base::Query, reacted::Returns>();
    let pin = baseline::<pinned::Op, base::Query, pinned::Returns>();
    assert_eq!(merged(&base, &react, &pin).err(), Some(vec![Clash::Lines]));
}
