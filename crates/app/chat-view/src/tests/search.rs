//! Message search a screenful at a time: More results under the hits reads the page after the
//! cursor the last page answered, on the desktop and on the phone, and goes once none follows.
use super::phone::phone;
use super::*;

// Three hits in two pages: the first answers a cursor, the page after it none.
fn two_pages(cx: &TestAppContext) {
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::TextSearch {
                text, page: asked, ..
            } => {
                assert_eq!(text, "hello");
                match asked.after {
                    None => Reply::TextHits(::chat::PageResponse {
                        next: Some(b"two".to_vec()),
                        ..page(vec![row(3, 7, "hello again"), row(2, 8, "hello there")])
                    }),
                    Some(cursor) => {
                        assert_eq!(cursor, b"two", "the cursor the first page answered");
                        Reply::TextHits(page(vec![row(1, 7, "hello")]))
                    }
                }
            }
            query => chat_reply(query),
        })
    }));
}

fn shown(cx: &TestAppContext) -> Vec<u64> {
    (1..=3)
        .rev()
        .filter(|seq| cx.find(&format!("chat-search-hit-general-{seq}")).is_some())
        .collect()
}

#[test]
fn more_results_on_the_desktop_reads_the_next_page_and_goes_at_the_last() {
    let (mut cx, _) = opened();
    two_pages(&cx);
    cx.simulate_input("chat-sidebar-search", "hello");
    cx.simulate_submit("chat-sidebar-search");
    cx.run_until_parked();
    assert_eq!(shown(&cx), [3, 2]);
    cx.simulate_click("chat-search-more");
    cx.run_until_parked();
    assert_eq!(shown(&cx), [3, 2, 1]);
    assert!(cx.find("chat-search-more").is_none(), "the last page");
}

#[test]
fn more_results_out_when_a_new_search_is_made_land_in_none_of_its_hits() {
    let (mut cx, view) = opened();
    two_pages(&cx);
    cx.simulate_input("chat-sidebar-search", "hello");
    cx.simulate_submit("chat-sidebar-search");
    cx.run_until_parked();
    assert_eq!(shown(&cx), [3, 2]);
    cx.host().never::<Ask<::chat::Chat>>();
    cx.simulate_click("chat-search-more");
    cx.run_until_parked();
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::TextSearch { page: asked, .. } if asked.after.is_some() => {
                Reply::TextHits(page(vec![row(1, 7, "hello")]))
            }
            Query::TextSearch { .. } => Reply::TextHits(page(vec![row(5, 7, "bye")])),
            query => chat_reply(query),
        })
    }));
    cx.simulate_input("chat-sidebar-search", "bye");
    cx.simulate_submit("chat-sidebar-search");
    cx.run_until_parked();
    cx.host().answer_held::<Ask<::chat::Chat>>();
    cx.run_until_parked();
    let hits = view.read(|chat| {
        let hits = chat.search.hits.ready().expect("the new search landed");
        hits.rows.iter().map(|row| row.seq).collect::<Vec<_>>()
    });
    assert_eq!(hits, [5], "the old search's page stays out");
}

#[test]
fn more_results_on_the_phone_reads_the_next_page_and_goes_at_the_last() {
    let (mut cx, _) = phone();
    two_pages(&cx);
    cx.simulate_click("Search");
    cx.simulate_input("search", "hello");
    cx.simulate_submit("search");
    cx.run_until_parked();
    assert_eq!(shown(&cx), [3, 2]);
    assert!(cx.has_text("2 · newest first"));
    cx.simulate_click("chat-search-more");
    cx.run_until_parked();
    assert_eq!(shown(&cx), [3, 2, 1]);
    assert!(cx.has_text("3 · newest first"));
    assert!(cx.find("chat-search-more").is_none(), "the last page");
}

#[test]
fn a_restored_search_keeps_its_hits_and_asks_no_search_again() {
    let (mut cx, _) = opened();
    two_pages(&cx);
    cx.simulate_input("chat-sidebar-search", "hello");
    cx.simulate_submit("chat-sidebar-search");
    cx.run_until_parked();
    assert_eq!(shown(&cx), [3, 2]);
    let (restored, view) = restored(&cx);
    let searched = restored
        .host()
        .requests::<Ask<::chat::Chat>>()
        .into_iter()
        .filter(|query| matches!(query, Query::TextSearch { .. }))
        .count();
    assert_eq!(searched, 0, "the hits came back with the snapshot");
    let hits = view.read(|chat| chat.search.hits.ready().map(|hits| hits.rows.len()));
    assert_eq!(hits, Some(2), "the restored view shows the hits it held");
}
