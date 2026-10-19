//! The migration ops chat kept once every channel had moved to `Stored`: their bytes still
//! read, and they write nothing.
use super::*;

#[test]
fn the_migration_ops_write_nothing_and_unmigrated_lists_no_channel() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&ADA, "g1", "alpha #ship", None);
    let before = chat.store.borrow().state.clone();
    let migrate = |channel: &str| Op::Migrate {
        channel_id: channel.into(),
    };
    chat.ok(&BO, migrate("general"));
    chat.ok(&CY, Op::MigrateSome { limit: 5 });
    assert_eq!(chat.store.borrow().state, before);
    assert_eq!(chat.refused(&ADA, migrate("nowhere")), code::NOT_FOUND);
    let page = PageRequest::default();
    let Reply::ChannelIds(page) = chat.ask(Query::Unmigrated { page }) else {
        panic!("Unmigrated answers channel ids");
    };
    assert!(page.items.is_empty() && page.next.is_none());
    assert!(crate::gate::needs(&Op::MigrateSome { limit: 5 }).is_empty());
    let said = crate::describe(&migrate("general"));
    assert_eq!(said.title, "Migrate channel · #general");
}
