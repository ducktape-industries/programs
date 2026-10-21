use view::testing::TestAppContext;

use super::*;

fn seated(total: u64) -> TestAppContext {
    let mut cx = TestAppContext::new();
    cx.host().handle::<Query<Tally>>(move |query| match query {
        tally::Query::Total => Ok(tally::Reply::Total(total)),
        other => panic!("unexpected tally query: {other:?}"),
    });
    cx.open::<Counter>();
    cx.run_until_parked();
    cx
}

#[test]
fn add_one_sends_add_and_a_change_reads_the_total_again() {
    let mut cx = seated(4);
    assert!(cx.has_text("Total: 4"), "{:?}", cx.texts());
    cx.host()
        .handle::<Submit<Tally>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.simulate_click("add");
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<Submit<Tally>>(),
        [tally::Op::Add { by: 1 }]
    );
    cx.host()
        .handle::<Query<Tally>>(|_| Ok(tally::Reply::Total(5)));
    cx.host().stream::<Changes<Tally>>().send(None);
    cx.run_until_parked();
    assert!(cx.has_text("Total: 5"), "{:?}", cx.texts());
}

#[test]
fn a_refused_add_says_why_where_it_was_pressed() {
    let mut cx = seated(4);
    cx.host()
        .refuse::<Submit<Tally>>("invalid_input", "adding 1 would overflow the count");
    cx.simulate_click("add");
    cx.run_until_parked();
    let said = "That didn’t go through: adding 1 would overflow the count";
    assert!(cx.has_text(said), "{:?}", cx.texts());
    assert!(cx.has_text("Total: 4"), "{:?}", cx.texts());
}

#[test]
fn a_refused_read_offers_a_retry_that_reads_again() {
    let mut cx = TestAppContext::new();
    cx.host()
        .refuse::<Query<Tally>>("unavailable", "The node is away.");
    cx.open::<Counter>();
    cx.run_until_parked();
    assert!(cx.has_text("The node is away."), "{:?}", cx.texts());
    cx.host()
        .handle::<Query<Tally>>(|_| Ok(tally::Reply::Total(7)));
    cx.simulate_click("total-retry");
    cx.run_until_parked();
    assert!(cx.has_text("Total: 7"), "{:?}", cx.texts());
}
