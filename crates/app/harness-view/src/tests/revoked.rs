//! A revoked agent on the desktop list: out of its group, in a closed fold below the rest. The
//! phone's root is held to the same in `phone`.
use super::*;

pub(super) fn revoke_in(mut net: Net, number: u64) -> Net {
    for setup in net.setups.iter_mut().filter(|setup| setup.agent == number) {
        setup.standing = Standing::Revoked;
    }
    let accounts = net.accounts.iter_mut();
    for account in accounts.filter(|account| account.number == number) {
        if let identity::Control::Program { life, .. } = &mut account.control {
            *life = identity::Life::Revoked;
        }
    }
    net
}

#[test]
fn a_revoked_agent_waits_in_a_closed_fold_below_the_rest() {
    let (mut cx, _) = seated(revoke_in(Net::default(), ATLAS), EDDY);
    assert!(cx.find("agent-12").is_some(), "{:?}", cx.texts());
    assert!(cx.find("agent-24").is_none(), "{:?}", cx.texts());
    assert!(!cx.has_text("Other agents"), "{:?}", cx.texts());
    assert!(cx.has_text("Revoked"), "{:?}", cx.texts());
    cx.simulate_click("agents-revoked");
    cx.run_until_parked();
    assert!(cx.find("agent-24").is_some(), "{:?}", cx.texts());
    cx.simulate_click("agents-revoked");
    cx.run_until_parked();
    assert!(cx.find("agent-24").is_none(), "{:?}", cx.texts());

    let (cx, _) = seated(Net::default(), EDDY);
    assert!(cx.find("agents-revoked").is_none(), "{:?}", cx.texts());
}

#[test]
fn the_chosen_agent_revoked_beside_its_page_keeps_its_row_in_an_open_fold() {
    let (mut cx, shared) = seated(Net::default(), EDDY);
    cx.simulate_resize(2400., 900.);
    cx.run_until_parked();
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    let revoked = revoke_in(std::mem::take(&mut *shared.borrow_mut()), SCOUT);
    *shared.borrow_mut() = revoked;
    cx.host().stream::<Changes<Identity>>().send(Some(Change {
        height: 9011,
        keys: Vec::new(),
    }));
    cx.run_until_parked();
    assert!(cx.find("agents-revoked").is_some(), "{:?}", cx.texts());
    assert!(cx.find("agent-12").is_some(), "{:?}", cx.texts());
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.find("agent-12").is_none(), "{:?}", cx.texts());
}
