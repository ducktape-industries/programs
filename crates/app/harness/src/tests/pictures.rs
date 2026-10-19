//! What an agent sees: an image a message carries reaches the model as an image block beside its
//! text and in every later request, another file stays its name, one past Anthropic's limits is a
//! sentence, a request refused over its images is asked once more with them as words, and `image_view`.
use base64::Engine;

use super::*;

const SHOT: &str = "/shared/qa/shot.png";
const NOTES: &str = "/shared/qa/notes.txt";

fn png(width: u32, height: u32, len: usize) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    let len = len.max(bytes.len());
    bytes.resize(len, 7);
    bytes
}

fn put(chain: &MockChain, path: &str, bytes: &[u8]) -> BlobId {
    let base = match chain.query(files::PROGRAM, &files::Query::Head).unwrap() {
        files::Reply::Head(head) => head.map(|row| row.id),
        other => panic!("{other:?}"),
    };
    let chunks = bytes
        .chunks(files::CHUNK_SIZE as usize)
        .map(|chunk| chain.put_blob(files::CHUNK, chunk.to_vec()).unwrap())
        .collect();
    let size = bytes.len() as u64;
    let put = files::Change::Put {
        path: path.into(),
        exec: false,
        meta: Default::default(),
        content: files::Content::Chunks { size, chunks },
    };
    let commit = files::Op::Commit {
        base,
        message: "attach".into(),
        changes: vec![put],
    };
    abi::decode(&chain.submit(signed(ADA), files::PROGRAM, &commit).unwrap()).unwrap()
}

fn attached(path: &str, at: BlobId, content_type: &str, size: usize) -> chat::Block {
    let BlobId::Sha256(snapshot) = at else {
        panic!("files names a snapshot by sha256");
    };
    chat::Block::Attachment(chat::message::Attachment {
        path: path.into(),
        snapshot,
        name: path.rsplit('/').next().unwrap().into(),
        size: size as u64,
        content_type: content_type.into(),
    })
}

// a mention of the agent carrying `files`: the conversation it opens, its model's window learned
fn mentioned(world: &World, files: Vec<chat::Block>) -> u64 {
    let mut blocks = chat::parse_message(&format!("<@{}> look at this", world.agent));
    blocks.extend(files);
    let op = chat::Op::PostMessage {
        channel_id: "general".into(),
        message_id: "m1".into(),
        blocks,
        thread: None,
    };
    world.chain.submit(signed(ADA), chat::PROGRAM, &op).unwrap();
    let thread = world.threads()[0].id;
    world.chain.at(2, 0);
    learn(&world.chain, Some(WINDOW));
    thread
}

fn last_content(chain: &MockChain) -> Vec<Value> {
    let messages = sent_messages(chain);
    let last = messages.last().expect("a message is sent");
    last["content"].as_array().unwrap().clone()
}

fn images(content: &[Value]) -> Vec<&Value> {
    content
        .iter()
        .filter(|block| block["type"] == "image")
        .collect()
}

fn texts(content: &[Value]) -> Vec<&str> {
    content
        .iter()
        .filter_map(|block| block["text"].as_str())
        .collect()
}

fn decoded(image: &Value) -> Vec<u8> {
    let data = image["source"]["data"].as_str().unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap()
}

fn refused_over_images() -> Vec<u8> {
    let message = "messages.0.content.2.image.source.base64.data: At least one of the image dimensions exceed max allowed size for many-image requests: 2000 pixels";
    let error = json!({
        "type": "error",
        "error": { "type": "invalid_request_error", "message": message },
    });
    response(400, error)
}

#[test]
fn a_png_a_message_carries_reaches_the_model_beside_its_text_and_another_file_stays_its_name() {
    let world = world();
    let chain = &world.chain;
    let bytes = png(640, 480, 14 * 1024);
    let shot = put(chain, SHOT, &bytes);
    let notes = put(chain, NOTES, b"it works");
    let files = vec![
        attached(SHOT, shot, "image/png", bytes.len()),
        attached(NOTES, notes, "text/plain", 8),
    ];
    mentioned(&world, files);
    let content = last_content(chain);
    let [image] = images(&content)[..] else {
        panic!("one image block: {content:?}");
    };
    assert_eq!(image["source"]["type"], "base64");
    assert_eq!(image["source"]["media_type"], "image/png");
    assert_eq!(decoded(image), bytes);
    let words = texts(&content);
    assert_eq!(
        content.len(),
        3,
        "the text, the image's line and the image: {content:?}"
    );
    assert!(words[0].contains("notes.txt"), "{words:?}");
    let at = abi::hex(shot.digest());
    assert_eq!(
        words[1],
        format!("shot.png (files {SHOT} at snapshot {at}): image/png, 14 KB, 640×480 px")
    );
    assert_eq!(content[2], *image, "the image follows the line naming it");

    chain.at(3, 0);
    let said = json!([{ "type": "text", "text": "Seen." }]);
    model_says(chain, said, "end_turn");
    chain.at(4, 0);
    world.mention("m2", "and now?");
    let messages = sent_messages(chain);
    let first = messages[0]["content"].as_array().unwrap();
    let [again] = images(first)[..] else {
        panic!("the image stays in the conversation: {first:?}");
    };
    assert_eq!(decoded(again), bytes);
}

#[test]
fn an_image_past_anthropics_limits_or_no_image_at_all_is_a_sentence_saying_why() {
    let world = world();
    let chain = &world.chain;
    let large = png(640, 480, 4 * 1024 * 1024);
    let wide = png(9000, 300, 1024);
    let fake = b"not a picture".to_vec();
    let mut files = Vec::new();
    for (path, bytes) in [
        ("/shared/qa/large.png", &large),
        ("/shared/qa/wide.png", &wide),
        ("/shared/qa/fake.png", &fake),
    ] {
        let at = put(chain, path, bytes);
        files.push(attached(path, at, "image/png", bytes.len()));
    }
    mentioned(&world, files);
    let content = last_content(chain);
    assert!(images(&content).is_empty(), "{content:?}");
    let words = texts(&content);
    let said = |name: &str| {
        let line = words.iter().find(|line| line.starts_with(name));
        line.unwrap_or_else(|| panic!("no line for {name}: {words:?}"))
            .to_string()
    };
    assert!(
        said("large.png").ends_with(
            ": not shown, 4.0 MB, too large to show: Anthropic takes at most 5 MB of base64 per image"
        ),
        "{words:?}"
    );
    assert!(
        said("wide.png").ends_with(
            ": not shown, 9000×300 px, too large to show: Anthropic takes at most 8000 px on a side"
        ),
        "{words:?}"
    );
    assert!(
        said("fake.png").ends_with(": not shown, it is not a png, jpeg, gif or webp image"),
        "{words:?}"
    );
}

#[test]
fn a_request_refused_over_its_images_is_asked_once_more_with_them_as_words() {
    let world = world();
    let chain = &world.chain;
    let bytes = png(3000, 2000, 2048);
    let shot = put(chain, SHOT, &bytes);
    let thread = mentioned(&world, vec![attached(SHOT, shot, "image/png", 2048)]);
    assert_eq!(images(&last_content(chain)).len(), 1);
    let (request, _) = open_request(chain);
    chain.at(3, 0);
    answer(chain, request.id, Answer::Output(refused_over_images()));

    let content = last_content(chain);
    assert!(images(&content).is_empty(), "{content:?}");
    let words = texts(&content);
    assert!(
        words[1].contains(
            ": not shown, as the model refused a request with it: the model answered 400, invalid_request_error: messages.0.content.2.image"
        ),
        "{words:?}"
    );
    let told = notices(chain, thread);
    assert!(
        told.iter().any(|notice| notice.ends_with(
            "every image so far reaches the model as words from now on, and it is asked again"
        )),
        "{told:?}"
    );

    let (request, _) = open_request(chain);
    chain.at(4, 0);
    answer(chain, request.id, Answer::Output(refused_over_images()));
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "with no image left to hide, the refusal ends the turn"
    );
    let told = notices(chain, thread);
    let last = told.last().unwrap();
    assert!(last.starts_with("the model answered 400"), "{told:?}");
}

#[test]
fn image_view_shows_an_image_file_in_its_result_and_refuses_another_kind() {
    let world = world();
    let chain = &world.chain;
    let bytes = png(32, 16, 200);
    put(chain, SHOT, &bytes);
    let head = put(chain, NOTES, b"plain text");
    let thread = world.opened("m1", "look at the shot", Some(WINDOW));
    chain.at(3, 0);
    let calls = json!([
        tool_use("v1", "image_view", json!({ "path": SHOT })),
        tool_use("v2", "image_view", json!({ "path": NOTES })),
    ]);
    model_says(chain, calls, "tool_use");
    chain.at(4, 0);
    relay_back(chain, "v1");
    relay_back(chain, "v2");

    let (text, failed) = result_of(chain, thread, "v1");
    assert!(!failed, "{text}");
    let seen: Value = serde_json::from_str(&text).unwrap();
    let at = abi::hex(head.digest());
    let expected = json!({
        "path": SHOT,
        "snapshot": at,
        "size": 200,
        "media_type": "image/png",
        "width": 32,
        "height": 16,
    });
    assert_eq!(seen, expected);
    let (refused, failed) = result_of(chain, thread, "v2");
    assert!(failed, "{refused}");
    assert!(
        refused.ends_with(&format!("{NOTES} is not a png, jpeg, gif or webp image")),
        "{refused}"
    );

    let content = last_content(chain);
    let result = |call: &str| {
        let found = content.iter().find(|block| block["tool_use_id"] == call);
        found.unwrap_or_else(|| panic!("no result for {call}: {content:?}"))
    };
    let shown = result("v1")["content"].as_array().unwrap();
    assert_eq!(shown[0]["text"], text);
    let [image] = images(shown)[..] else {
        panic!("one image in the result: {shown:?}");
    };
    assert_eq!(decoded(image), bytes);
    assert!(result("v2")["content"].is_string());
}
