use ::program::Env;
use commonware_cryptography::Digestible as _;

use super::*;
use crate::state::HOME;

const FOUNDED: u64 = 1_700_000_000_000;

fn env_at(height: u64) -> Env {
    Env {
        height,
        time: FOUNDED,
        chain_id: b"sealpond".to_vec(),
        ..MockHost::env(PROGRAM).root()
    }
}

fn home(host: &MockHost, env: Env) -> Option<NetworkId> {
    HOME.get(&host.query(env)).unwrap()
}

#[test]
fn at_founding_crosspond_takes_its_networks_genesis_digest_as_its_id() {
    let host = MockHost::default();
    Crosspond::init(&host.exec(env_at(0)), &[]).unwrap();
    let genesis = Block::genesis(b"sealpond", FOUNDED).digest();
    assert_eq!(home(&host, env_at(0)), Some(genesis.0));
}

#[test]
fn at_founding_params_naming_another_network_are_refused() {
    let host = MockHost::default();
    let refused = Crosspond::init(&host.exec(env_at(0)), &abi::encode(&THERE)).unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT);
    let genesis = Block::genesis(b"sealpond", FOUNDED).digest().0;
    Crosspond::init(&host.exec(env_at(0)), &abi::encode(&genesis)).unwrap();
    assert_eq!(home(&host, env_at(0)), Some(genesis));
}

#[test]
fn published_after_founding_crosspond_takes_its_id_from_its_params() {
    let host = MockHost::default();
    let refused = Crosspond::init(&host.exec(env_at(5)), &[]).unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT);
    let refused = Crosspond::init(&host.exec(env_at(5)), &[1, 2, 3]).unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT);
    Crosspond::init(&host.exec(env_at(5)), &abi::encode(&HERE)).unwrap();
    assert_eq!(home(&host, env_at(5)), Some(HERE));
}
