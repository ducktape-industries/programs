//! A room's tag filters: chips only where its `filters` meta names tags, a chip's list exactly chat's
//! `TagSearch` answer for the room a page at a time, All its own timeline again, and a block reading
//! a picked filter again only when it wrote the room, on the desktop and on the phone.
use super::*;

fn tagged_general() -> Vec<ChannelInfo> {
    let mut general = channel("general", "General", 3);
    general
        .meta
        .insert("filters".into(), "node, #app,,node".into());
    vec![general, channel("dm-7-8", "dm", 1)]
}

fn serve(cx: &TestAppContext) {
    let rooms = tagged_general();
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Channels { .. } => Reply::Channels(page(rooms.clone())),
                Query::TagSearch {
                    tag,
                    channel_id,
                    page: asked,
                    ..
                } => {
                    assert_eq!(channel_id.as_deref(), Some("general"));
                    let first = asked.after.is_none();
                    Reply::TagHits(match (tag.as_str(), first) {
                        ("node", true) => ::chat::PageResponse {
                            next: Some(b"two".to_vec()),
                            ..page(vec![row(9, 7, "nine"), row(7, 8, "seven")])
                        },
                        ("node", false) => {
                            assert_eq!(asked.after.as_deref(), Some(&b"two"[..]));
                            page(vec![row(4, 7, "four")])
                        }
                        _ => page(Vec::new()),
                    })
                }
                query => chat_reply(query),
            })
        }));
}

fn desk() -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    serve(&cx);
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    (cx, view)
}

fn asks(cx: &TestAppContext) -> Vec<(String, bool)> {
    let asked = cx.host().requests::<Ask<::chat::Chat>>().into_iter();
    asked
        .filter_map(|query| match query {
            Query::TagSearch { tag, page, .. } => Some((tag, page.after.is_some())),
            _ => None,
        })
        .collect()
}

fn shown(view: &Entity<Chat>) -> Vec<u64> {
    view.read(|chat| chat.shown(Pane::Timeline).map(|row| row.seq).collect())
}

fn pick(cx: &mut TestAppContext, chip: &str) {
    cx.simulate_click(chip);
    cx.run_until_parked();
}

#[test]
fn a_room_without_the_meta_draws_no_chips_and_asks_no_tags() {
    let (cx, view) = opened();
    assert!(cx.find("chat-room-filters").is_none());
    assert!(cx.find("chat-filter-all").is_none());
    assert!(asks(&cx).is_empty());
    assert_eq!(shown(&view), [1, 2]);
}

#[test]
fn a_chip_shows_exactly_its_tags_answer_oldest_at_the_top() {
    let (mut cx, view) = desk();
    assert_eq!(view.read(|chat| chat.room_filters()), ["node", "app"]);
    assert!(cx.find("chat-filter-all").is_some());
    assert!(cx.has_text("#node"), "{:?}", cx.texts());
    assert!(cx.has_text("#app"), "{:?}", cx.texts());
    assert!(asks(&cx).is_empty(), "All reads no tag");
    pick(&mut cx, "chat-filter-node");
    assert_eq!(asks(&cx), [("node".to_owned(), false)]);
    assert_eq!(shown(&view), [7, 9]);
    assert!(cx.has_text("nine"), "{:?}", cx.texts());
    assert!(!cx.has_text("hello"), "the room's own rows are out");
}

#[test]
fn the_first_row_in_view_reads_the_page_after_and_none_follows_the_last() {
    let (mut cx, view) = desk();
    pick(&mut cx, "chat-filter-node");
    view.read(|chat| assert!(chat.before_first(Pane::Timeline, 0)));
    reach_first(&mut cx, &view);
    assert_eq!(
        asks(&cx),
        [("node".to_owned(), false), ("node".to_owned(), true)]
    );
    assert_eq!(shown(&view), [4, 7, 9]);
    view.read(|chat| assert!(!chat.before_first(Pane::Timeline, 0), "no cursor, no page"));
}

#[test]
fn a_tag_with_no_posts_says_so() {
    let (mut cx, view) = desk();
    pick(&mut cx, "chat-filter-app");
    assert!(cx.has_text("No #app posts yet"), "{:?}", cx.texts());
    assert!(shown(&view).is_empty());
}

#[test]
fn all_shows_the_rooms_own_timeline_again_with_no_read() {
    let (mut cx, view) = desk();
    pick(&mut cx, "chat-filter-node");
    let asked = asks(&cx).len();
    pick(&mut cx, "chat-filter-all");
    assert_eq!(shown(&view), [1, 2]);
    assert!(view.read(|chat| chat.filtered().is_none()));
    assert_eq!(asks(&cx).len(), asked);
    assert!(cx.has_text("hello"), "{:?}", cx.texts());
}

#[test]
fn a_block_reads_the_filter_again_only_when_it_wrote_the_room() {
    let (mut cx, _view) = desk();
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    pick(&mut cx, "chat-filter-node");
    let asked = asks(&cx).len();
    changes.send(block(4, posted("dm-7-8")));
    cx.run_until_parked();
    assert_eq!(asks(&cx).len(), asked, "another room's post");
    changes.send(block(5, posted("general")));
    cx.run_until_parked();
    assert_eq!(asks(&cx).len(), asked + 1, "the pages held, read again");
}

#[test]
fn a_restored_view_opens_on_all() {
    let (mut cx, view) = desk();
    pick(&mut cx, "chat-filter-node");
    assert!(view.read(|chat| chat.filtered().is_some()));
    let (_restored, back) = restored(&cx);
    assert!(back.read(|chat| chat.filtered().is_none()));
}

#[test]
fn the_phone_draws_the_chips_over_its_room_and_filters_by_them() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    serve(&cx);
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    let view = cx.open::<Chat>();
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    seat(&mut cx);
    cx.simulate_click("chat-rooms-channel-general");
    cx.run_until_parked();
    assert!(cx.find("chat-room-filters").is_some());
    assert!(cx.has_text("node"), "{:?}", cx.texts());
    assert!(!cx.has_text("#node"), "a finger's chips name the bare tag");
    pick(&mut cx, "chat-filter-node");
    assert_eq!(shown(&view), [7, 9]);
    pick(&mut cx, "chat-filter-app");
    assert!(cx.has_text("No #app posts yet"), "{:?}", cx.texts());
}

#[test]
fn a_picked_tag_the_meta_drops_goes_back_to_all() {
    let (mut cx, view) = desk();
    pick(&mut cx, "chat-filter-app");
    assert!(view.read(|chat| chat.filtered().is_some()));
    let mut general = channel("general", "General", 3);
    general.meta.insert("filters".into(), "node".into());
    cx.update(&view, |chat, _, cx| {
        chat.channels_arrived(vec![general, channel("dm-7-8", "dm", 1)], cx)
    });
    cx.run_until_parked();
    assert!(view.read(|chat| chat.filtered().is_none()));
    assert!(cx.find("chat-filter-app").is_none());
    assert_eq!(shown(&view), [1, 2], "the room's own timeline");
    assert!(cx.has_text("hello"), "{:?}", cx.texts());
}
