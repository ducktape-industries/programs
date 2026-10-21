//! The keeper: one write out at a time, the newest anchor of each route after it, nothing the
//! store holds already written again, and a refused write written again on the next move.
use super::*;
use crate::prelude::*;
use crate::testing::TestAppContext;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct Pane {
    #[serde(skip)]
    keeper: Keeper,
}

impl View for Pane {
    const NAME: &'static str = "Pane";
    const CAPABILITIES: &'static [Capability] = &[Capability::Store, Capability::Host];
}

impl Render for Pane {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn at(id: &str, offset: f32) -> Option<Anchor> {
    Some(Anchor {
        id: id.to_owned(),
        seq: 7,
        offset,
    })
}

fn keep(cx: &mut TestAppContext, view: &Entity<Pane>, route: &str, anchor: Option<Anchor>) {
    cx.update(view, |pane, _, cx| pane.keeper.keep(cx, route, anchor));
}

fn written(cx: &TestAppContext) -> Vec<(String, Option<Anchor>)> {
    let sets = cx.host().requests::<StoreSet>();
    let read = |value: Vec<u8>| methods::decode(&value).expect("an anchor");
    sets.into_iter()
        .map(|(key, value)| (key, value.map(read)))
        .collect()
}

fn set(route: &str, anchor: Option<Anchor>) -> (String, Option<Anchor>) {
    (key(route), anchor)
}

#[test]
fn one_write_is_out_at_a_time_and_the_newest_of_each_route_follows_it() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Pane>();
    cx.host().never::<StoreSet>();
    keep(&mut cx, &view, "general", at("m1", 0.));
    keep(&mut cx, &view, "general", at("m2", 4.));
    keep(&mut cx, &view, "random", at("r1", 0.));
    keep(&mut cx, &view, "general", at("m3", 9.));
    cx.run_until_parked();
    assert_eq!(
        written(&cx),
        [set("general", at("m1", 0.))],
        "one write out"
    );
    cx.host().handle::<StoreSet>(|_| Ok(()));
    cx.host().answer_held::<StoreSet>();
    cx.run_until_parked();
    assert_eq!(
        written(&cx),
        [
            set("general", at("m1", 0.)),
            set("general", at("m3", 9.)),
            set("random", at("r1", 0.)),
        ],
        "then the newest of each route, never m2"
    );
}

#[test]
fn what_the_store_holds_is_not_written_again_and_none_removes_it() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Pane>();
    view.read(|pane| pane.keeper.known("general", at("m1", 0.)));
    keep(&mut cx, &view, "general", at("m1", 0.));
    cx.run_until_parked();
    assert!(written(&cx).is_empty(), "the store holds it");
    keep(&mut cx, &view, "general", at("m2", 4.));
    cx.run_until_parked();
    assert_eq!(cx.host().kept::<Anchor>(&key("general")), at("m2", 4.));
    keep(&mut cx, &view, "general", at("m2", 4.));
    keep(&mut cx, &view, "general", None);
    cx.run_until_parked();
    assert_eq!(cx.host().kept::<Anchor>(&key("general")), None, "removed");
    keep(&mut cx, &view, "general", None);
    cx.run_until_parked();
    assert_eq!(
        written(&cx),
        [set("general", at("m2", 4.)), set("general", None)]
    );
}

#[test]
fn a_refused_write_is_said_and_written_again_on_the_next_move() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Pane>();
    cx.host()
        .refuse::<StoreSet>("host_fault", "the disk is full");
    keep(&mut cx, &view, "general", at("m1", 0.));
    cx.run_until_parked();
    keep(&mut cx, &view, "general", at("m1", 0.));
    cx.run_until_parked();
    assert_eq!(written(&cx).len(), 2, "the store never held it");
    let said = cx.host().requests::<methods::HostLog>();
    assert!(
        said.iter()
            .any(|line| line.contains("where general stood is not kept")),
        "{said:?}"
    );
}

#[test]
fn a_read_landing_after_a_write_never_hides_what_the_write_kept() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Pane>();
    keep(&mut cx, &view, "general", None);
    cx.run_until_parked();
    view.read(|pane| pane.keeper.known("general", at("m1", 0.)));
    keep(&mut cx, &view, "general", at("m1", 0.));
    cx.run_until_parked();
    assert_eq!(
        written(&cx),
        [set("general", None), set("general", at("m1", 0.))],
        "the read came from before the write: the keeper's own word stands"
    );
    assert_eq!(cx.host().kept::<Anchor>(&key("general")), at("m1", 0.));
}
