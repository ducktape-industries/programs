//! A wide window: the agents listed on the left, the model budget or the chosen agent beside them;
//! a narrower one keeps its one column.
use super::*;

const WIDE: (f32, f32) = (2400., 900.);
const NO_BUDGET: &str = "No model answer has named its usage windows yet.";
const USAGE: &str = "Usage · 39,926 tokens over 1 conversation shown";

fn wide() -> TestAppContext {
    let (mut cx, _) = seated(Net::default(), EDDY);
    cx.simulate_resize(WIDE.0, WIDE.1);
    cx.run_until_parked();
    cx
}

#[test]
fn a_wide_window_lists_the_agents_beside_the_budget() {
    let cx = wide();
    for text in [NO_BUDGET, "Scout", "Atlas"] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
}

#[test]
fn a_chosen_agent_opens_beside_the_list_and_pressed_again_gives_the_budget_back() {
    let mut cx = wide();
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.has_text(USAGE), "{:?}", cx.texts());
    assert!(cx.has_text("Atlas"), "the list stays: {:?}", cx.texts());
    assert!(!cx.has_text(NO_BUDGET), "{:?}", cx.texts());
    assert!(
        !cx.has_text("‹ Agents"),
        "no back beside the list: {:?}",
        cx.texts()
    );
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.has_text(NO_BUDGET), "{:?}", cx.texts());
    assert!(!cx.has_text(USAGE), "{:?}", cx.texts());
}

#[test]
fn the_model_budget_control_returns_from_an_open_agent() {
    let mut cx = wide();
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.has_text(USAGE), "{:?}", cx.texts());
    assert!(!cx.has_text(NO_BUDGET), "{:?}", cx.texts());
    assert!(cx.has_text("Model budget"), "{:?}", cx.texts());
    cx.simulate_click("agents-budget");
    cx.run_until_parked();
    assert!(cx.has_text(NO_BUDGET), "{:?}", cx.texts());
    assert!(!cx.has_text(USAGE), "{:?}", cx.texts());
    assert!(cx.has_text("Atlas"), "the list stays: {:?}", cx.texts());
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.has_text(USAGE), "the agent reopens: {:?}", cx.texts());
}

#[test]
fn a_narrower_window_keeps_one_column() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    assert!(cx.has_text(NO_BUDGET), "{:?}", cx.texts());
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.has_text("‹ Agents"), "{:?}", cx.texts());
    assert!(!cx.has_text(NO_BUDGET), "{:?}", cx.texts());
}
