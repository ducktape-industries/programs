//! A chat room's rules reach a conversation once, ahead of the first message it hears from the
//! room, as the meta's text or a named file's content, and again once they change, once the file
//! moves, and once a summary may have left them out.
use super::*;

const FRENCH: &str = "Write in French.";
const ENGLISH: &str = "Write in English.";
const FILE: &str = "/shared/rules.md";

fn set_rules(world: &World, value: &str) {
    let op = chat::Op::SetChannelMeta {
        channel_id: "general".into(),
        key: chat::RULES.into(),
        value: value.into(),
    };
    world.chain.submit(signed(ADA), chat::PROGRAM, &op).unwrap();
}

fn clear_rules(world: &World) {
    let op = chat::Op::RemoveChannelMeta {
        channel_id: "general".into(),
        key: chat::RULES.into(),
    };
    world.chain.submit(signed(ADA), chat::PROGRAM, &op).unwrap();
}

// every post the agent's conversations hold, framed or held, in order
fn posts(world: &World) -> Vec<String> {
    let chain = &world.chain;
    let threads = world.threads().into_iter();
    threads
        .flat_map(|row| frames(chain, row.id).into_iter().chain(held(chain, row.id)))
        .filter_map(|body| match body {
            Body::Post { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

fn as_text(rules: &str) -> String {
    format!("Rules of chat channel general (general), for how to talk there:\n{rules}")
}

fn from_file(path: &str, rules: &str) -> String {
    format!(
        "Rules of chat channel general (general), for how to talk there (from {path}):\n{rules}"
    )
}

const GONE: &str = "The chat channel general (general) has no rules now.";

fn told(world: &World) -> Vec<String> {
    let posts = posts(world).into_iter();
    posts
        .filter(|text| text.starts_with("Rules of ") || text == GONE)
        .collect()
}

#[test]
fn rules_come_once_ahead_of_the_message_and_again_when_they_change() {
    let world = world();
    let chain = &world.chain;
    set_rules(&world, FRENCH);
    world.opened("m1", "first", Some(WINDOW));
    let (request, _) = open_request(chain);
    let sent = body(chain, &request).to_string();
    assert!(sent.contains(FRENCH), "the model reads them: {sent}");
    world.mention("m2", "second");
    let heard = posts(&world);
    let [rules, first, second] = &heard[..] else {
        panic!("{heard:?}")
    };
    assert_eq!(rules, &as_text(FRENCH));
    assert!(first.ends_with("first"), "{first}");
    assert!(second.ends_with("second"), "{second}");

    set_rules(&world, ENGLISH);
    world.mention("m3", "third");
    clear_rules(&world);
    world.mention("m4", "fourth");
    world.mention("m5", "fifth");
    let heard = posts(&world);
    let at = |text: &str| heard.iter().position(|said| said == text).unwrap();
    assert!(heard[at(&as_text(ENGLISH)) + 1].ends_with("third"));
    assert!(heard[at(GONE) + 1].ends_with("fourth"));
    let rules = vec![as_text(FRENCH), as_text(ENGLISH), GONE.to_owned()];
    assert_eq!(told(&world), rules, "nothing again while they stay");
}

#[test]
fn a_path_brings_its_files_content_again_once_the_file_moves() {
    let world = world();
    let chain = &world.chain;
    commit(chain, ADA, FILE, "Be brief.");
    set_rules(&world, FILE);
    world.mention("m1", "first");
    world.mention("m2", "second");
    commit(chain, ADA, FILE, "Be briefer.");
    world.mention("m3", "third");
    set_rules(&world, "/nowhere/rules.md");
    world.mention("m4", "fourth");
    let rules = vec![
        from_file(FILE, "Be brief."),
        from_file(FILE, "Be briefer."),
        as_text("/nowhere/rules.md"),
    ];
    assert_eq!(told(&world), rules, "a path naming no file is the text");
}

#[test]
fn a_summary_brings_the_rules_back_with_the_next_message() {
    let world = world();
    let chain = &world.chain;
    set_rules(&world, FRENCH);
    world.opened("m1", "remember the plan", Some(2_000));
    chain.at(3, 0);
    let noted = json!([{ "type": "text", "text": "Noted." }]);
    model_says_after(chain, noted, "end_turn", 1000);
    world.mention("m2", "and now?");
    assert_eq!(told(&world).len(), 1, "once before the summary");
    chain.at(4, 0);
    summarized(chain, "Ada set out the plan.");
    chain.at(5, 0);
    let still = json!([{ "type": "text", "text": "Still on it." }]);
    model_says(chain, still, "end_turn");
    world.mention("m3", "once more");
    assert_eq!(told(&world), vec![as_text(FRENCH), as_text(FRENCH)]);
    let heard = posts(&world);
    let [.., rules, last] = &heard[..] else {
        panic!("{heard:?}")
    };
    assert_eq!(rules, &as_text(FRENCH));
    assert!(last.ends_with("once more"), "{last}");
}
