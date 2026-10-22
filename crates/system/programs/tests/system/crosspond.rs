//! crosspond founded on ducktape's host: at founding it takes the digest of its network's genesis
//! block as its id, the digest the node's own genesis block has.
use super::*;

use commonware_cryptography::Digestible as _;

#[test]
fn founded_crosspond_takes_the_digest_of_its_networks_genesis_block_as_its_id() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let apps = vec![founding(crosspond::PROGRAM, &program("crosspond"))];
        let net = Net::found_with(context, dir.path(), apps, Vec::new(), MEMBER_CAP).await;
        let home = net
            .host
            .view(Layer::Confirmed)
            .get(crosspond::PROGRAM, b"home")
            .unwrap()
            .map(|id| abi::decode::<crosspond::NetworkId>(&id).unwrap());
        let genesis = node::Block::genesis(NETWORK, TIME).digest();
        assert_eq!(home, Some(genesis.0));
    });
}
