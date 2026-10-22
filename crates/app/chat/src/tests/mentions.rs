//! A typed `@name`: which runs of a message are names, and what chat stores
//! for one that one account holds, that none holds, or that several hold.
use super::*;
use crate::mentions::{Typed, typed_names};
use crate::{Category, Kind, Profile, Standing};

fn names(text: &str) -> Vec<String> {
    (typed_names(&parse_message(text)).into_iter())
        .map(|typed| match typed {
            Typed::Local(name) => name,
            Typed::Foreign { name, network } => format!("{name}@{network}"),
        })
        .collect()
}

#[test]
fn a_name_is_the_run_of_name_characters_after_an_at_that_opens_a_word() {
    let cases: &[(&str, &[&str])] = &[
        ("@chief", &["chief"]),
        ("hi @chief, look", &["chief"]),
        ("thanks @chief.", &["chief"]),
        ("really @chief...", &["chief"]),
        ("@chief!", &["chief"]),
        ("(@chief)", &["chief"]),
        ("@chief: @dev-ops_2", &["chief", "dev-ops_2"]),
        ("@chief.bob is one name", &["chief.bob"]),
        ("@Chief and @CHIEF", &["chief"]),
        ("**@chief** and *@dev*", &["chief", "dev"]),
        ("> @chief in a quote", &["chief"]),
        ("- @chief in a list", &["chief"]),
        ("**a**@chief", &[]),
        ("mail a@chief.com", &[]),
        ("@@chief", &[]),
        ("@ chief", &[]),
        ("@.", &[]),
        ("`@chief` in code", &[]),
        ("```\n@chief\n```", &[]),
        ("[@chief](https://x.y)", &[]),
        ("<@7> is picked", &[]),
    ];
    for (text, expected) in cases {
        assert_eq!(names(text), *expected, "{text:?}");
    }
}

#[test]
fn a_name_one_account_holds_is_stored_as_the_mention_a_pick_makes() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&ADA, "m1", "hi @user2, and **@User3**.\n> @user4 too", None);
    chat.post(&ADA, "m2", "hi <@2>, and **<@3>**.\n> <@4> too", None);
    let (typed, picked) = (chat.message(1), chat.message(2));
    assert_eq!(typed.blocks, picked.blocks);
    assert_eq!(typed.text, picked.text);
}

#[test]
fn an_edit_resolves_a_typed_name_as_a_post_does() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&ADA, "m1", "first", None);
    chat.ok(&ADA, super::channels::edit(1, "now @user5 sees it"));
    assert_eq!(chat.message(1).blocks, parse_message("now <@5> sees it"));
}

#[test]
fn a_name_no_one_or_several_hold_stays_text_and_any_one_account_is_mentioned() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    let profile = |number, name: &str, kind| Profile {
        number,
        name: name.into(),
        kind,
    };
    let sleeper = Kind::Program {
        manager: 1,
        category: Category::Agent,
        standing: Standing::Suspended,
        script: None,
    };
    chat.store.identity(vec![
        profile(1, "Ada", Kind::User),
        profile(2, "Twin", Kind::User),
        profile(3, "twin", Kind::User),
        profile(4, "Sleeper", sleeper),
        profile(5, "forge", Kind::System("forge".into())),
        profile(6, "Zoe\u{308}", Kind::User),
    ]);
    let plain = "@twin @nobody";
    chat.post(&ADA, "m1", plain, None);
    assert_eq!(chat.message(1).blocks, parse_message(plain));
    chat.post(&ADA, "m2", "@ada and @Zoë", None);
    assert_eq!(chat.message(2).blocks, parse_message("<@1> and <@6>"));
    chat.post(&ADA, "m3", "@sleeper and @forge", None);
    assert_eq!(chat.message(3).blocks, parse_message("<@4> and <@5>"));
}

#[test]
fn a_message_with_no_typed_name_asks_identity_nothing() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.store.borrow_mut().siblings.clear();
    let text = "mail a@b.c, `@code` and <@2>";
    chat.post(&ADA, "m1", text, None);
    assert_eq!(chat.message(1).blocks, parse_message(text));
}
