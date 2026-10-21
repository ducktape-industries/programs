//! Drafts: someone answering in a room before they post, a piece of text at a time, written where
//! they may post and taken down by their post or their own `Undraft`.
use super::*;
use crate::Draft;

fn drafting(text: &str, anew: bool, doing: Option<&str>) -> Op {
    Op::Drafting {
        channel_id: "general".into(),
        thread: None,
        text: text.into(),
        anew,
        doing: doing.map(Into::into),
    }
}

fn undraft() -> Op {
    Op::Undraft {
        channel_id: "general".into(),
    }
}

impl Chat {
    fn drafts(&self) -> Vec<Draft> {
        let Reply::Drafts(drafts) = self.ask(Query::Drafts {
            channel_id: "general".into(),
        }) else {
            panic!("Drafts answers drafts");
        };
        drafts
    }
}

fn said(drafts: &[Draft]) -> Vec<(Principal, &str, &str)> {
    let said = drafts.iter().map(|draft| {
        let (doing, text) = (draft.doing.as_str(), draft.text.as_str());
        (draft.author.clone(), doing, text)
    });
    said.collect()
}

#[test]
fn a_draft_grows_a_piece_at_a_time_and_its_authors_post_takes_its_place() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.ok(&BO, drafting("Hel", false, Some("")));
    let opened = chat.height;
    chat.ok(&BO, drafting("lo", false, Some("running forge_read")));
    let drafts = chat.drafts();
    assert_eq!(said(&drafts), [(BO, "running forge_read", "Hello")]);
    assert_eq!((drafts[0].height, drafts[0].thread), (opened, None));
    chat.ok(&BO, drafting("Again", true, None));
    assert_eq!(said(&chat.drafts()), [(BO, "running forge_read", "Again")]);
    chat.post(&CY, "c1", "unrelated", None);
    assert_eq!(chat.drafts().len(), 1, "another's post leaves it");
    chat.post(&BO, "b1", "Again", None);
    assert!(chat.drafts().is_empty(), "the post took its place");
    chat.ok(&BO, drafting("next", false, None));
    let [next] = chat.drafts().try_into().unwrap();
    assert_eq!((next.doing.as_str(), next.text.as_str()), ("", "next"));
    assert!(next.height > opened, "a draft after a post opens anew");
}

#[test]
fn a_draft_is_written_where_its_author_may_post_under_a_threads_root() {
    let mut chat = Chat::with_channel(PostPolicy::MembersOnly);
    assert_eq!(
        chat.refused(&BO, drafting("hi", false, None)),
        code::UNAUTHORIZED
    );
    chat.post(&ADA, "a1", "root", None);
    chat.post(&ADA, "a2", "reply", Some(1));
    let under = |root| Op::Drafting {
        channel_id: "general".into(),
        thread: Some(root),
        text: "hi".into(),
        anew: false,
        doing: None,
    };
    chat.ok(&ADA, under(2));
    let drafted = chat.drafts()[0].thread;
    assert_eq!(
        drafted,
        Some(1),
        "a draft under a reply answers under its root"
    );
    chat.ok(&ADA, drafting("there", false, None));
    let [moved] = chat.drafts().try_into().unwrap();
    assert_eq!((moved.thread, moved.text.as_str()), (None, "there"));
    chat.ok(&BO, undraft());
    assert_eq!(chat.drafts().len(), 1, "only its author takes it down");
    chat.ok(&ADA, undraft());
    assert!(chat.drafts().is_empty());
}
