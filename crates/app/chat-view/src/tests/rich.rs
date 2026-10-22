use super::*;

#[test]
fn rich_message_keeps_styles_and_dispatches_each_link_by_value() {
    let (mut cx, view) = opened();
    let spans = vec![
        chat::Span {
            text: "bold ".into(),
            marks: vec![chat::Mark::Bold],
        },
        chat::Span {
            text: "italic ".into(),
            marks: vec![chat::Mark::Italic],
        },
        chat::Span {
            text: "first ".into(),
            marks: vec![chat::Mark::Link("https://one.example".into())],
        },
        chat::Span {
            text: "reviewer".into(),
            marks: vec![chat::Mark::Mention(chat::Principal::Account(8))],
        },
        chat::Span {
            text: " code".into(),
            marks: vec![chat::Mark::Code],
        },
    ];
    cx.update(&view, |chat, _, cx| {
        chat.room
            .as_mut()
            .unwrap()
            .messages
            .ready_mut()
            .unwrap()
            .push(MsgRow {
                channel_id: "general".into(),
                seq: 3,
                message_id: "rich".into(),
                height: 2,
                blocks: vec![
                    chat::Block::Paragraph(spans),
                    chat::Block::Code {
                        lang: Some("rust".into()),
                        text: "fn main() {}".into(),
                    },
                    chat::Block::Quote(vec![chat::Span {
                        text: "quoted".into(),
                        marks: vec![chat::Mark::Italic],
                    }]),
                ],
                text: "rich".into(),
                ..MsgRow::by(Principal::Account(7))
            });
        cx.notify();
    });
    cx.run_until_parked();
    let key = "chat-message-rich-block-0";
    let Some(wire::Node::RichText {
        text,
        runs,
        clickable_ranges,
        font_family_overrides,
        ..
    }) = cx.find(key)
    else {
        panic!("message paragraph is one rich text node");
    };
    assert_eq!(text, "bold italic first @reviewer  code ");
    let wire::RichTextRuns::Highlights(highlights) = runs else {
        panic!("chat authors highlight ranges");
    };
    assert_eq!(highlights.len(), 5);
    assert!(highlights[0].1.font_weight.is_some());
    assert!(highlights[1].1.font_style.is_some());
    // a code span: the fenced block's ground, in the mono face
    assert!(highlights[4].1.background_color.is_some());
    assert_eq!(
        *font_family_overrides,
        vec![(
            highlights[4].0.clone(),
            view::design::fonts::FAMILY_MONO.into()
        )]
    );
    assert_eq!(clickable_ranges.len(), 2);
    assert!(cx.has_text("rust"));
    assert!(cx.has_text("fn main() {}"));
    assert!(cx.has_text("quoted"));
    let seq = view.read(|chat| {
        chat.shown(Pane::Timeline)
            .find(|row| row.message_id == "rich")
            .map(|row| row.seq)
            .expect("the rich row")
    });
    super::message::hover(&mut cx, &view, seq);
    assert!(matches!(
        cx.find("chat-message-rich-more"),
        Some(wire::Node::Container (view::wire::ContainerNode { interactivity: Some(interactivity), .. }))
            if interactivity.aria.label.as_deref() == Some("More message actions")
    ));

    cx.simulate_rich_click(key, 0);
    cx.simulate_rich_click(key, 1);
    let opened = cx.host().requests::<view::methods::LinkOpen>();
    assert_eq!(
        opened[opened.len().saturating_sub(2)..],
        ["https://one.example", "duck://testnet-0a1b2c3d/identity/8"]
    );
}

// A link to a room draws as the room's name now and opens it here, not
// through the app; a plain `#word` naming one room links to it too.
#[test]
fn a_room_link_names_the_room_and_opens_it_in_place() {
    let (mut cx, view) = opened();
    let text = "see [#old](duck://testnet-0a1b2c3d/chat/forge%3Aweb%3A3) and #general";
    cx.update(&view, |chat, _, cx| {
        let mut linked = parsed_row(3, 7, text);
        linked.message_id = "rooms".into();
        let room = chat.room.as_mut().unwrap();
        room.messages.ready_mut().unwrap().push(linked);
        cx.notify();
    });
    cx.run_until_parked();
    let key = "chat-message-rooms-block-0";
    let Some(wire::Node::RichText {
        text,
        clickable_ranges,
        ..
    }) = cx.find(key)
    else {
        panic!("the paragraph is one rich text node");
    };
    assert_eq!(text, "see #Review web#3 and #general");
    assert_eq!(clickable_ranges.len(), 2);
    cx.simulate_rich_click(key, 0);
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "forge:web:3"));
    assert!(cx.host().requests::<view::methods::LinkOpen>().is_empty());
}

/// A person's avatar opens their identity page, the page a mention of
/// them opens; the press is the avatar's, so the message stays unchosen.
/// (An agent's opens its card: `agents.rs`.)
#[test]
fn an_avatar_opens_its_authors_profile() {
    let (mut cx, view) = opened();
    cx.simulate_click("chat-message-m1-avatar");
    cx.run_until_parked();
    let opened = cx.host().requests::<view::methods::LinkOpen>();
    assert_eq!(
        opened.last().map(String::as_str),
        Some("duck://testnet-0a1b2c3d/identity/7")
    );
    assert_eq!(
        cx.interactivity("chat-message-m1-avatar")
            .aria
            .label
            .as_deref(),
        Some("Open eddy’s profile")
    );
    view.read(|chat| assert!(chat.menu.is_none() && chat.copy.is_none()));
}

#[test]
fn a_headers_block_number_opens_explorer_at_that_block() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        // Start a new author run so the header shows its block number.
        let mut late = row(3, 7, "late");
        late.message_id = "late".into();
        late.height = 12_345;
        chat.room
            .as_mut()
            .unwrap()
            .messages
            .ready_mut()
            .unwrap()
            .push(late);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("block 12,345"));
    cx.simulate_click("chat-message-late-height");
    let opened = cx.host().requests::<view::methods::LinkOpen>();
    assert_eq!(
        opened.last().map(String::as_str),
        Some("duck://explorer/block/12345")
    );
}

/// A run's header says when it was posted beside its block, and a new day
/// opens under a divider naming it.
#[test]
fn a_header_tells_the_time_and_a_new_day_opens_under_a_divider() {
    let (mut cx, view) = opened();
    // 24 Sep 2026, 15:42 UTC, and the next morning
    let today = 1_790_264_527_000;
    let tomorrow = today + 18 * 3_600_000;
    cx.update(&view, |chat, _, cx| {
        let messages = chat.room.as_mut().unwrap().messages.ready_mut().unwrap();
        for (seq, time) in [(3, today), (4, tomorrow)] {
            let mut late = row(seq, 7, "late");
            late.height = 4_908;
            late.time = time;
            messages.push(late);
        }
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("3:42 PM ·"));
    assert!(cx.has_text("block 4,908"));
    assert!(cx.has_text("9:42 AM ·"), "a new day reopens the run");
    assert!(
        cx.find("chat-day-m3").is_some(),
        "the first dated row opens its day"
    );
    assert!(cx.has_text("24 Sep 2026"));
    assert!(cx.find("chat-day-m4").is_some());
    assert!(cx.has_text("25 Sep 2026"));
}

// Two rows an hour apart that UTC puts on one day: two hours east of it, the reader's midnight
// falls between them, and on the desktop the later opens Today under the earlier's Yesterday.
#[test]
fn a_day_turns_at_the_readers_midnight_and_says_today_and_yesterday() {
    let (mut cx, view) = opened();
    let hour = 3_600_000;
    let evening = 20 * 24 * hour + 21 * hour + hour / 2;
    let morning = evening + hour;
    cx.simulate_offset(120);
    cx.update(&view, |chat, _, cx| {
        let messages = chat.room.as_mut().unwrap().messages.ready_mut().unwrap();
        *messages = vec![
            MsgRow {
                time: evening,
                ..row(1, 7, "late")
            },
            MsgRow {
                time: morning,
                ..row(2, 7, "early")
            },
        ];
        chat.today = Some(crate::message::day_of(morning));
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-day-m1").is_some() && cx.find("chat-day-m2").is_some());
    assert!(
        cx.has_text("Yesterday") && cx.has_text("Today"),
        "{:?}",
        cx.texts()
    );
}

/// A line typed as `- x` or `1. x` renders as a list row: its marker in a
/// gutter and the item, marks and all, beside it, wrapping in what the
/// gutter leaves of the row. The room and the thread draw a message
/// through one card, so this holds in both panes.
#[test]
fn list_lines_render_as_list_rows() {
    use view::px;
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        let mut list = parsed_row(3, 7, "- **apples**\n2. pears\nplain");
        list.message_id = "list".into();
        chat.room
            .as_mut()
            .unwrap()
            .messages
            .ready_mut()
            .unwrap()
            .push(list);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-message-list-block-0-item").is_some());
    assert!(cx.has_text("•"));
    let Some(wire::Node::RichText { text, .. }) = cx.find("chat-message-list-block-0") else {
        panic!("the item is one rich text node");
    };
    assert_eq!(text, "apples", "the marker leaves the item's text");
    // as wide as the whole row (its `w_full`) beside the gutter, the item
    // would run past the pane's right edge by the gutter's width
    let item = cx.style("chat-message-list-block-0");
    assert_eq!(
        (item.flex_grow, item.min_size.width),
        (Some(1.), Some(px(0.).into())),
        "the item takes the row's rest and may wrap under its widest line"
    );
    assert!(cx.find("chat-message-list-block-1-item").is_some());
    assert!(cx.has_text("2."));
    assert!(
        cx.find("chat-message-list-block-2-item").is_none(),
        "a plain line stays a paragraph"
    );
}

/// The reader's offset landing after the rows moves the day dividers onto
/// the reader's midnights without a refresh.
#[test]
fn an_offset_landing_late_moves_the_day_dividers() {
    let (mut cx, view) = opened();
    // 24 Sep 2026, 13:00 and 16:00 UTC: one UTC day, two in Seoul
    let afternoon = 1_790_254_800_000;
    let evening = afternoon + 3 * 3_600_000;
    cx.update(&view, |chat, _, cx| {
        let messages = chat.room.as_mut().unwrap().messages.ready_mut().unwrap();
        for (seq, time) in [(3, afternoon), (4, evening)] {
            let mut late = row(seq, 7, "late");
            late.time = time;
            messages.push(late);
        }
        chat.watch(cx);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-day-m3").is_some());
    assert!(cx.find("chat-day-m4").is_none(), "one UTC day");
    cx.simulate_offset(540);
    assert!(
        cx.find("chat-day-m4").is_some(),
        "Seoul's midnight falls between"
    );
    assert!(cx.has_text("25 Sep 2026"));
}

// A table and a heading typed into a message draw as the shared ones (the
// markdown crate's), the table's marks kept and its separator gone, and the
// line after the table stays a paragraph.
#[test]
fn a_typed_table_and_heading_draw_as_the_shared_ones() {
    let (mut cx, view) = opened();
    let text =
        "## Footprint\n| share | idle |\n|---|---|\n| **total** | 104.8 |\n| heap | 30.9 |\nafter";
    cx.update(&view, |chat, _, cx| {
        let mut table = parsed_row(3, 7, text);
        table.message_id = "table".into();
        let room = chat.room.as_mut().unwrap();
        room.messages.ready_mut().unwrap().push(table);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        cx.find("chat-message-table-block-0-h").is_some(),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("chat-message-table-block-1-table").is_some());
    let Some(wire::Node::RichText { text, runs, .. }) = cx.find("chat-message-table-block-1-1-0")
    else {
        panic!("a body cell is one rich text node");
    };
    assert_eq!(text, "total");
    let wire::RichTextRuns::Highlights(highlights) = runs else {
        panic!("the cell's marks are highlight ranges");
    };
    assert!(highlights[0].1.font_weight.is_some(), "total stays bold");
    assert!(cx.has_text("30.9") && !cx.has_text("|---|---|"));
    assert!(cx.find("chat-message-table-block-5").is_some());
}

#[test]
fn a_page_link_alone_on_its_line_draws_the_page_and_any_other_link_stays_a_link() {
    let (mut cx, view) = opened();
    let page = "duck://testnet-0a1b2c3d/files/shared/artifacts/plan.html";
    let blob = format!("duck://testnet-0a1b2c3d/blobs/{}", "ab".repeat(32));
    let lines = [
        page.to_owned(),
        format!("see {page} here"),
        blob.clone(),
        "duck://testnet-0a1b2c3d/files/shared/notes.md".to_owned(),
        "duck://testnet-0a1b2c3e/files/shared/artifacts/plan.html".to_owned(),
        format!("[The plan]({page})"),
        "duck://testnet-0a1b2c3d/blobs/abab".to_owned(),
    ];
    cx.update(&view, |chat, _, cx| {
        let mut pages = parsed_row(3, 7, &lines.join("\n"));
        pages.message_id = "pages".into();
        let room = chat.room.as_mut().unwrap();
        room.messages.ready_mut().unwrap().push(pages);
        cx.notify();
    });
    cx.run_until_parked();
    let drawn: Vec<Option<String>> = (0..lines.len())
        .map(
            |index| match cx.find(&format!("chat-message-pages-block-{index}")) {
                Some(wire::Node::Artifact { link, .. }) => Some(link.clone()),
                Some(_) => None,
                None => panic!("block {index} is not drawn"),
            },
        )
        .collect();
    assert_eq!(
        drawn,
        [
            Some(page.to_owned()),
            None,
            Some(blob),
            None,
            None,
            Some(page.to_owned()),
            None,
        ]
    );
}

#[test]
fn a_page_link_another_network_sends_stays_a_link_and_ours_draws_the_page() {
    let (mut cx, view) = opened();
    let page = "duck://testnet-0a1b2c3d/files/shared/artifacts/plan.html";
    cx.update(&view, |chat, _, cx| {
        let ours = MsgRow {
            message_id: "ours".into(),
            ..parsed_row(3, 7, page)
        };
        let theirs = MsgRow {
            message_id: "theirs".into(),
            author: Principal::Foreign(chat::Foreign {
                network: [0x22; 32],
                account: 7,
            }),
            ..parsed_row(4, 7, page)
        };
        let room = chat.room.as_mut().unwrap();
        room.messages.ready_mut().unwrap().extend([ours, theirs]);
        cx.notify();
    });
    cx.run_until_parked();
    let drawn = |id: &str| match cx.find(id) {
        Some(wire::Node::Artifact { link, .. }) => Some(link.clone()),
        Some(_) => None,
        None => panic!("{id} is not drawn"),
    };
    assert_eq!(drawn("chat-message-ours-block-0"), Some(page.to_owned()));
    assert_eq!(drawn("chat-message-theirs-block-0"), None);
}

// `CHAT_SCREEN_EXPORT=1` writes a room holding a typed table, a heading and
// a fenced block for the app's node-less renderer, as the desktop draws it.
#[test]
fn export_markdown_screens() {
    let Some(out) = ::view::testing::screens("CHAT_SCREEN_EXPORT", "chat/fixtures") else {
        return;
    };
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        let room = chat.room.as_mut().unwrap();
        room.messages = Loadable::Ready(vec![parsed_row(1, 7, TYPED_TABLE)]);
        cx.notify();
    });
    cx.run_until_parked();
    std::fs::write(
        out.join("markdown-desktop.json"),
        serde_json::to_vec(&cx.whole_frame()).unwrap(),
    )
    .unwrap();
    let (width, height) = cx.viewport();
    let listed = serde_json::json!([{ "name": "markdown-desktop", "width": width as u32, "height": height as u32, "theme": "light" }]);
    std::fs::write(
        out.join("markdown-desktop-manifest.json"),
        serde_json::to_vec_pretty(&listed).unwrap(),
    )
    .unwrap();
}

#[test]
fn a_notice_draws_as_one_line_naming_whom_it_is_for() {
    let (mut cx, view) = opened();
    let notice = chat::Notice {
        kind: "pod".into(),
        group: "builds-1".into(),
        subject: "pod builds-1".into(),
        link: Some("https://pods.example/builds-1".into()),
        status: chat::message::Status::Failed,
        detail: "exit code 1".into(),
        concerns: vec![8],
    };
    cx.update(&view, |chat, _, cx| {
        let room = chat.room.as_mut().unwrap();
        room.messages.ready_mut().unwrap().push(MsgRow {
            channel_id: "general".into(),
            seq: 3,
            message_id: "notice".into(),
            height: 2,
            blocks: vec![chat::Block::Notice(notice)],
            ..MsgRow::by(Principal::Account(7))
        });
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-message-notice-block-0-notice").is_some());
    assert!(cx.has_text("●"));
    let key = "chat-message-notice-block-0";
    let Some(wire::Node::RichText {
        text,
        clickable_ranges,
        ..
    }) = cx.find(key)
    else {
        panic!("the notice is one rich text node");
    };
    assert_eq!(text, "pod builds-1 failed — exit code 1 · for @reviewer");
    assert_eq!(clickable_ranges.len(), 2);
    cx.simulate_rich_click(key, 0);
    cx.simulate_rich_click(key, 1);
    let opened = cx.host().requests::<view::methods::LinkOpen>();
    assert_eq!(
        opened[opened.len().saturating_sub(2)..],
        [
            "https://pods.example/builds-1",
            "duck://testnet-0a1b2c3d/identity/8"
        ]
    );
}

#[test]
fn a_notices_path_link_opens_on_the_sessions_own_chain() {
    let (mut cx, view) = opened();
    let notice = chat::Notice {
        kind: "change".into(),
        group: "merged programs 213".into(),
        subject: "programs #213: Fix".into(),
        link: Some("forge/programs/213".into()),
        status: chat::message::Status::Passed,
        detail: "merged into dev".into(),
        concerns: vec![8],
    };
    cx.update(&view, |chat, _, cx| {
        let room = chat.room.as_mut().unwrap();
        room.messages.ready_mut().unwrap().push(MsgRow {
            channel_id: "general".into(),
            seq: 3,
            message_id: "notice".into(),
            height: 2,
            blocks: vec![chat::Block::Notice(notice)],
            ..MsgRow::by(Principal::Account(9))
        });
        cx.notify();
    });
    cx.run_until_parked();
    let key = "chat-message-notice-block-0";
    let Some(wire::Node::RichText {
        clickable_ranges, ..
    }) = cx.find(key)
    else {
        panic!("the notice is one rich text node");
    };
    assert_eq!(clickable_ranges.len(), 2);
    cx.simulate_rich_click(key, 0);
    let opened = cx.host().requests::<view::methods::LinkOpen>();
    assert_eq!(
        opened[opened.len().saturating_sub(1)..],
        ["duck://testnet-0a1b2c3d/forge/programs/213"]
    );
}
