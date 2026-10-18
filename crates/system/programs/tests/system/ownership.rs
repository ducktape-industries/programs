//! The kernel's ownership guard on ducktape's host, the ownership program bound to its role: the
//! probe declares `o/` owned (`probe.item`) and `s/` shared, so a narrowed policy refuses another
//! account's write to an owned key and leaves a shared one alone.
use super::*;

const ADA: u64 = 21;
const BOB: u64 = 22;

fn set(key: &[u8]) -> Vec<Step> {
    vec![Step::Op(HostOp::Set {
        key: key.to_vec(),
        value: b"v".to_vec(),
    })]
}

#[test]
fn a_narrowed_policy_refuses_another_accounts_write_to_an_owned_key() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let apps = vec![founding(ownership::PROGRAM, &program("ownership"))];
        let mut net = Net::found_with(context, dir.path(), apps, Vec::new(), MEMBER_CAP).await;
        for (seed, name) in [(ADA, "Ada"), (BOB, "Bob")] {
            let create = identity::Op::Create {
                name: name.into(),
                scheme: Scheme::Ed25519,
            };
            net.apply(&public(seed), identity::PROGRAM, &create).await;
        }
        let strict = ownership::Op::Create {
            name: "mine".into(),
            write: ownership::Writers::Owner,
        };
        let made = net.apply(&public(ADA), ownership::PROGRAM, &strict).await;
        let policy: ownership::PolicyId = abi::decode(&made).unwrap();
        let global = ownership::Op::SetDefault {
            scope: ownership::Scope::Global,
            policy: Some(policy),
        };
        net.apply(&public(ADA), ownership::PROGRAM, &global).await;
        net.apply(&public(ADA), "probe", &set(b"o/ada")).await;
        let asked = ownership::Query::Record {
            program: "probe".into(),
            key: b"o/ada".to_vec(),
        };
        let ownership::Reply::Record(Some(record)) = net.ask(ownership::PROGRAM, &asked).await
        else {
            panic!("the guard recorded ada's new key")
        };
        assert!(matches!(record.owner, ownership::Party::Account(_)));
        assert_eq!(
            (record.policy, record.kind.as_deref()),
            (policy, Some("probe.item"))
        );
        let refused = net.refuse(&public(BOB), "probe", &set(b"o/ada")).await;
        assert_eq!(refused, reason::UNAUTHORIZED);
        net.apply(&public(BOB), "probe", &set(b"s/bob")).await;
        net.apply(&public(ADA), "probe", &set(b"o/ada")).await;
    });
}
