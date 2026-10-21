//! Standing questions against the test host: each answer the node pushes lands with no read,
//! and a host that stands none reads as before.
use super::Live;
use crate::host::Error;
use crate::methods::{Capability, Query, Watch};
use crate::testing::TestAppContext;
use crate::{Context, IntoElement, ParentElement, Render, View, Window};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;

pub struct Tally;
impl program::Program for Tally {
    const NAME: &'static str = "tally";
    type Op = ();
    type Query = Asked;
    type Reply = Said;

    fn execute(_: &program::ExecCtx, _: Self::Op) -> Result<(), program::Error> {
        unreachable!()
    }

    fn query(_: &program::QueryCtx, _: Self::Query) -> Result<Self::Reply, program::Error> {
        unreachable!()
    }
}

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize, ::program::Ask)]
#[ask(Tally)]
pub enum Asked {
    #[ask(Said::Count(u64))]
    Count { of: String },
}

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub enum Said {
    Count(u64),
}

#[derive(Serialize, Deserialize)]
struct Counted {
    of: String,
    count: Live<u64>,
}

impl Default for Counted {
    fn default() -> Self {
        Self {
            of: "tea".to_owned(),
            count: Live::default(),
        }
    }
}

impl View for Counted {
    const NAME: &'static str = "Counted";
    const CAPABILITIES: &'static [Capability] = &[Capability::Program];
    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.stand(cx);
    }
}

impl Counted {
    fn stand(&mut self, cx: &mut Context<Self>) {
        let of = self.of.clone();
        cx.live(
            self,
            |view| &mut view.count,
            ask::Count { of },
            Some,
            Self::read,
        );
    }

    fn read(&mut self, cx: &mut Context<Self>) {
        let of = self.of.clone();
        cx.reread(
            self,
            |view| &mut *view.count,
            move |_, cx| cx.host().query(ask::Count { of }),
            Self::read,
        );
    }

    fn ask_of(&mut self, of: &str, cx: &mut Context<Self>) {
        self.of = of.to_owned();
        cx.notify();
        self.stand(cx);
    }
}

impl Render for Counted {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let shown = match (self.count.ready(), self.count.failed()) {
            (Some(count), _) => count.to_string(),
            (None, Some(refusal)) => refusal.message.clone(),
            (None, None) => "…".to_owned(),
        };
        crate::div().child(shown)
    }
}

#[derive(Default)]
struct Counts {
    by_kind: RefCell<std::collections::BTreeMap<String, u64>>,
    refusing: RefCell<Option<String>>,
}

impl Counts {
    fn set(&self, of: &str, count: u64) {
        self.by_kind.borrow_mut().insert(of.to_owned(), count);
    }

    fn refuse(&self, of: Option<&str>) {
        *self.refusing.borrow_mut() = of.map(str::to_owned);
    }

    fn answer(&self, Asked::Count { of }: Asked) -> Result<Said, Error> {
        let refused = self.refusing.borrow().as_deref() == Some(of.as_str());
        if refused {
            return Err(Error::new("not_found", format!("no {of} counted")));
        }
        let count = self.by_kind.borrow().get(&of).copied().unwrap_or_default();
        Ok(Said::Count(count))
    }
}

fn counted(counts: &Rc<Counts>) -> TestAppContext {
    let cx = TestAppContext::new();
    let answering = counts.clone();
    cx.host()
        .handle::<Query<Tally>>(move |asked| answering.answer(asked));
    cx
}

fn reads(cx: &TestAppContext) -> usize {
    cx.host().requests::<Query<Tally>>().len()
}

fn watches(cx: &TestAppContext) -> usize {
    cx.host().requests::<Watch<Tally>>().len()
}

#[test]
fn each_answer_the_node_pushes_lands_and_nothing_is_read() {
    let counts = Rc::new(Counts::default());
    counts.set("tea", 5);
    let mut cx = counted(&counts);
    let node = cx.host().stand::<Tally>();
    let view = cx.open::<Counted>();
    cx.run_until_parked();
    assert!(cx.has_text("5"), "{:?}", cx.texts());
    let renders = cx.renders();
    node.moved();
    cx.run_until_parked();
    assert_eq!(
        cx.renders(),
        renders,
        "an answer a block left as it was is not sent"
    );
    counts.set("tea", 6);
    node.moved();
    cx.run_until_parked();
    assert!(cx.has_text("6"), "{:?}", cx.texts());
    assert_eq!(cx.renders(), renders + 1, "a moved answer draws once");
    assert_eq!(watches(&cx), 1, "asked once");
    assert_eq!(reads(&cx), 0, "and never read");
    view.read(|view| assert!(view.count.stands(), "{:?}", view.count));
}

#[test]
fn a_host_that_stands_no_question_reads_it_and_asks_no_more() {
    let counts = Rc::new(Counts::default());
    counts.set("tea", 5);
    counts.set("milk", 7);
    let mut cx = counted(&counts);
    let view = cx.open::<Counted>();
    cx.run_until_parked();
    assert!(cx.has_text("5"), "{:?}", cx.texts());
    assert_eq!((watches(&cx), reads(&cx)), (1, 1), "asked, refused, read");
    view.read(|view| assert!(!view.count.stands(), "{:?}", view.count));
    cx.update(&view, |view, _, cx| view.ask_of("milk", cx));
    cx.run_until_parked();
    assert!(cx.has_text("7"), "{:?}", cx.texts());
    assert_eq!(
        (watches(&cx), reads(&cx)),
        (1, 2),
        "the host's unsupported is kept for the session: read, never asked again"
    );
}

#[test]
fn a_question_the_program_refuses_is_read_and_the_next_one_still_stands() {
    let counts = Rc::new(Counts::default());
    counts.set("milk", 7);
    counts.refuse(Some("tea"));
    let mut cx = counted(&counts);
    let _node = cx.host().stand::<Tally>();
    let view = cx.open::<Counted>();
    cx.run_until_parked();
    assert!(cx.has_text("no tea counted"), "{:?}", cx.texts());
    assert_eq!((watches(&cx), reads(&cx)), (1, 1), "refused, then read");
    cx.update(&view, |view, _, cx| view.ask_of("milk", cx));
    cx.run_until_parked();
    assert!(cx.has_text("7"), "{:?}", cx.texts());
    assert_eq!(
        (watches(&cx), reads(&cx)),
        (2, 1),
        "a program's refusal is that question's"
    );
    view.read(|view| assert!(view.count.stands(), "{:?}", view.count));
}

#[test]
fn an_answer_the_node_could_not_run_is_read_once_and_the_question_stands() {
    let counts = Rc::new(Counts::default());
    counts.set("tea", 5);
    let mut cx = counted(&counts);
    let node = cx.host().stand::<Tally>();
    let view = cx.open::<Counted>();
    cx.run_until_parked();
    counts.set("tea", 6);
    node.could_not_answer();
    cx.run_until_parked();
    assert!(cx.has_text("6"), "{:?}", cx.texts());
    assert_eq!(reads(&cx), 1, "read once");
    view.read(|view| assert!(view.count.stands(), "{:?}", view.count));
}

#[test]
fn a_refused_run_lands_its_refusal_and_the_next_answer_lands_over_it() {
    let counts = Rc::new(Counts::default());
    counts.set("tea", 5);
    let mut cx = counted(&counts);
    let node = cx.host().stand::<Tally>();
    cx.open::<Counted>();
    cx.run_until_parked();
    counts.refuse(Some("tea"));
    node.moved();
    cx.run_until_parked();
    assert!(cx.has_text("no tea counted"), "{:?}", cx.texts());
    counts.refuse(None);
    counts.set("tea", 6);
    node.moved();
    cx.run_until_parked();
    assert!(cx.has_text("6"), "{:?}", cx.texts());
    assert_eq!((watches(&cx), reads(&cx)), (1, 0));
}

#[test]
fn a_new_question_stands_in_place_of_the_old_and_dropping_the_slot_lets_go() {
    let counts = Rc::new(Counts::default());
    counts.set("tea", 5);
    counts.set("milk", 7);
    let mut cx = counted(&counts);
    let node = cx.host().stand::<Tally>();
    let view = cx.open::<Counted>();
    cx.run_until_parked();
    cx.update(&view, |view, _, cx| view.stand(cx));
    cx.run_until_parked();
    assert_eq!(
        watches(&cx),
        1,
        "the same question asked again stands as it is"
    );
    cx.update(&view, |view, _, cx| view.ask_of("milk", cx));
    cx.run_until_parked();
    assert!(cx.has_text("7"), "{:?}", cx.texts());
    assert_eq!(watches(&cx), 2);
    assert_eq!(node.standing(), 1, "the old question was let go");
    cx.update(&view, |view, _, cx| {
        view.count = Live::default();
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(
        node.standing(),
        0,
        "dropping the slot let go of its question"
    );
    assert_eq!(reads(&cx), 0);
}
