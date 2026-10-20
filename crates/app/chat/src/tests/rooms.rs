//! How an op reads in the explorer (`describe`).
use super::channels::{meta, open_dm};
use super::*;
use crate::{describe, dm_channel_id};

#[test]
fn a_dm_title_names_no_account_numbers_and_its_accounts_are_fields() {
    use describe::Value;
    let dm = dm_channel_id(1, 3);
    let react = describe(&Op::AddReaction {
        channel_id: dm.clone(),
        seq: 4,
        emoji: "👍".into(),
    });
    assert_eq!(react.title, "React · DM");
    let between = react.fields.iter().find(|f| f.label == "between").unwrap();
    assert_eq!(
        between.value,
        Value::List(vec![Value::Account(1), Value::Account(3)])
    );
    let edit = describe(&Op::EditMessage {
        channel_id: dm,
        seq: 4,
        blocks: Vec::new(),
        base_rev: None,
    });
    assert_eq!(edit.title, "Edit message · DM");
    let channel = describe(&Op::DeleteMessage {
        channel_id: "old-launch".into(),
        seq: 1,
    });
    assert_eq!(channel.title, "Delete message · #old-launch");
    assert!(channel.fields.iter().all(|f| f.label != "between"));
    assert_eq!(describe(&open_dm(3)).title, "Open a DM");
    let created = describe(&Op::CreateChannel {
        channel_id: "channel-18d8db5b".into(),
        name: "[qa] probe".into(),
        post_policy: PostPolicy::Open,
    });
    assert_eq!(created.title, "Create channel · #[qa] probe");
    assert_eq!(
        describe(&post("design", "m", "hi", None)).title,
        "Post in #design"
    );
}

#[test]
fn a_channels_meta_reads_as_its_keys_and_values() {
    use describe::{Value, field};
    let created = describe(&Op::CreateChannelWithMeta {
        channel_id: "channel-1".into(),
        name: "j-020".into(),
        post_policy: PostPolicy::Open,
        meta: meta(&[("category", "Jobs")]),
    });
    assert_eq!(created.title, "Create channel · #j-020");
    let entries = field("meta", Value::List(vec![Value::text("category: Jobs")]));
    assert!(created.fields.contains(&entries));
    let set = describe(&Op::SetChannelMeta {
        channel_id: "j-020".into(),
        key: "category".into(),
        value: "Jobs".into(),
    });
    assert_eq!(set.title, "Set channel meta · #j-020");
    let value = field("value", Value::text("Jobs"));
    assert!(set.fields.contains(&value));
    let removed = describe(&Op::RemoveChannelMeta {
        channel_id: "j-020".into(),
        key: "category".into(),
    });
    assert_eq!(removed.title, "Remove channel meta · #j-020");
    let key = field("key", Value::text("category"));
    assert!(removed.fields.contains(&key));
}
