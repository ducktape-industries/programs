//! Files a message carries: an attachment block keeps its tag after every older variant, a
//! message of attachments alone posts, search finds it by a file's name, and mentions and tags
//! read nothing in it.
use super::*;
use crate::Block;
use crate::message::Attachment;

fn attachment(name: &str) -> Block {
    Block::Attachment(Attachment {
        path: format!("/chat/general/m1/{name}"),
        snapshot: [7; 32],
        name: name.into(),
        size: 12,
        content_type: "text/plain".into(),
    })
}

#[test]
fn block_variants_only_append() {
    assert_eq!(
        describe::variants::<Block>(),
        ["Paragraph", "Code", "Quote", "Divider", "Attachment"]
    );
    assert_eq!(borsh::to_vec(&Block::Divider).unwrap(), [3]);
    assert_eq!(borsh::to_vec(&attachment("a")).unwrap()[0], 4);
}

#[test]
fn a_message_of_files_alone_posts_and_search_finds_it_by_name() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    let blocks = vec![attachment("report.pdf"), attachment("@user2 #ship.txt")];
    chat.ok(
        &ADA,
        Op::PostMessage {
            channel_id: "general".into(),
            message_id: "m1".into(),
            blocks: blocks.clone(),
            thread: None,
        },
    );
    let row = chat.message(1);
    assert_eq!(row.blocks, blocks, "a typed name in a file name is text");
    assert_eq!(row.text, "report.pdf @user2 #ship.txt");
    assert!(row.tags.is_empty());
    assert_eq!(chat.search("report"), [1]);
}

#[test]
fn an_edit_keeps_the_attachments_it_sends() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    let file = attachment("plan.txt");
    chat.ok(
        &ADA,
        Op::PostMessage {
            channel_id: "general".into(),
            message_id: "m1".into(),
            blocks: vec![Block::paragraph("first"), file.clone()],
            thread: None,
        },
    );
    chat.ok(
        &ADA,
        Op::EditMessage {
            channel_id: "general".into(),
            seq: 1,
            blocks: vec![Block::paragraph("second"), file.clone()],
            base_rev: Some(0),
        },
    );
    let row = chat.message(1);
    assert_eq!(row.blocks, [Block::paragraph("second"), file]);
    assert_eq!(chat.search("plan"), [1]);
    assert!(chat.search("first").is_empty());
}
