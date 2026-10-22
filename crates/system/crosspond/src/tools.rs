//! What crosspond tells an agent of its ops and queries. Its Op and Query are the crosspond role's,
//! defined in abi, so markers carry each variant's words and a working value of it.
use ::program::tools::{Described, Tool, example};

use crate::{CertificateKind, Foreign, Header, NetworkId, Op, Query, StateProof};

pub(crate) struct Ops;

pub(crate) struct Queries;

const NETWORK: NetworkId = [7; 32];

fn op(variant: &'static str, description: &'static str, value: Op) -> Tool {
    Tool {
        variant,
        description,
        example: Some(example(value)),
        reply: "",
    }
}

fn query(variant: &'static str, description: &'static str, value: Query) -> Tool {
    Tool {
        variant,
        description,
        example: Some(example(value)),
        reply: variant,
    }
}

fn validators() -> Vec<Vec<u8>> {
    vec![vec![1; 32], vec![2; 32]]
}

impl Described for Ops {
    const ABOUT: &'static str = "Links this network to other ducktape networks, so an account here can run an op on a program there and an account there can run one here: one row per link, the packets this network owes each link, the receipts of the packets it took, and a light client of each linked network that checks every packet delivered from it before it runs.";

    fn tools() -> Vec<Tool> {
        vec![
            op(
                "Open",
                "Link this network to another, by its id (the digest of its genesis block), its name (the one its validators sign under), its epoch length in blocks, and the epoch, certificate kind and validator keys trusted from now on. The one kind is Ed25519SimplexFinalize: ed25519 signatures over `<name>_FINALIZE` in a simplex finalization certificate. The validators' vote alone; refused already_exists for a network linked and open, and invalid_input for an empty or doubled validator set or an epoch of no blocks. On a closed link it opens the link again from the trust given, holding no header, and keeps its sequence numbers, the packets it owes and the receipts it holds.",
                Op::Open {
                    network: NETWORK,
                    name: "othernet".into(),
                    epoch_length: 64,
                    epoch: 0,
                    kind: CertificateKind::Ed25519SimplexFinalize,
                    validators: validators(),
                },
            ),
            op(
                "Repin",
                "Trust this validator set, signing with this certificate kind, at this epoch of a linked network from now on, for a link whose headers stall because that network's set changed too much. The validators' vote alone.",
                Op::Repin {
                    network: NETWORK,
                    epoch: 3,
                    kind: CertificateKind::Ed25519SimplexFinalize,
                    validators: validators(),
                },
            ),
            op(
                "Close",
                "Close a link: it takes no send, header or delivery until an Open opens it again. The validators' vote alone.",
                Op::Close { network: NETWORK },
            ),
            op(
                "Port",
                "Set the networks whose packets may run on the program bound to role here, kept sorted and once each; an empty from closes the port. The validators' vote alone.",
                Op::Port {
                    role: "chat".into(),
                    from: vec![NETWORK],
                },
            ),
            op(
                "Send",
                "Send op, the borsh of an op of the program bound to port on the network to, as the sending account: it runs there as this account of this network, unless deadline (unix ms) passes first. Any account, as itself; a user of another network is refused. Refused capacity for a packet larger than the link takes. Returns the packet's sequence number, which Status follows.",
                Op::Send {
                    to: NETWORK,
                    port: "chat".into(),
                    op: vec![24, 0],
                    deadline: None,
                },
            ),
            op(
                "SendFor",
                "Send as Send does, for one of the accounts of the program bound to the port role here; the packet names that program as via. That program alone. Returns the packet's sequence number.",
                Op::SendFor {
                    to: NETWORK,
                    port: "chat".into(),
                    op: vec![24, 0],
                    deadline: None,
                    account: 18,
                },
            ),
            op(
                "Pump",
                "Ask a node serving crosspond to relay what this network owes to, and take back the other network's acks: its settled rows, each taken only with an op proof under that network's crosspond root at a header the link holds or the answer carries, and only for the very packet still owed. crosspond's own scheduled run alone.",
                Op::Pump { to: NETWORK },
            ),
            op(
                "Header",
                "Move a link's trust to a later epoch of the other network, by the first block of that epoch, its validators' finalization of the certificate kind the link trusts, the roots it settles and the validator set its state records, with that record's proof. Anyone; the light client's checks decide, and a header signed by a third or less of the set the link trusts is not_yet. A kind this network's crosspond does not name does not decode.",
                Op::Header {
                    from: NETWORK,
                    header: Header {
                        kind: CertificateKind::Ed25519SimplexFinalize,
                        block: Vec::new(),
                        certificate: Vec::new(),
                        roots: Vec::new(),
                        validators: validators(),
                        validators_proof: StateProof {
                            location: 0,
                            proof: Vec::new(),
                        },
                    },
                },
            ),
            op(
                "DeliverProven",
                "Deliver packets another network committed, each with its proof at the epoch of the header the link holds, in rising sequence order, with low: the other network's low-water row for this network and its proof under the same header, or null. Each next packet expected runs on the program bound to its port as the foreign user that sent it. A proven n prunes the receipts below it whose result is written, once the link was proven no higher n. Anyone; a proof that does not hold refuses the frame, which may carry no packets.",
                Op::DeliverProven {
                    from: NETWORK,
                    epoch: 3,
                    low: None,
                    packets: Vec::new(),
                },
            ),
        ]
    }
}

impl Described for Queries {
    fn tools() -> Vec<Tool> {
        vec![
            query(
                "Link",
                "A link's row: the other network's name and epoch length, the epoch trusted and the header held, the next sequence numbers in and out, the reason the last relay was refused, and whether it is closed; null for a network not linked.",
                Query::Link { network: NETWORK },
            ),
            query(
                "Outbound",
                "The packets this network owes to a network, after a sequence number, in order.",
                Query::Outbound {
                    to: NETWORK,
                    after: 0,
                },
            ),
            query(
                "Receipts",
                "The receipts kept for packets taken from a network, after a sequence number, in order; a receipt whose run has not replied holds no result.",
                Query::Receipts {
                    from: NETWORK,
                    after: 0,
                },
            ),
            query(
                "Status",
                "A sent packet's status: Pending, Refused with the reason, or Done with the other network's result; null for none.",
                Query::Status {
                    to: NETWORK,
                    seq: 1,
                },
            ),
            query(
                "Ports",
                "Each open port, by role, and the networks it takes packets from.",
                Query::Ports,
            ),
            query(
                "Name",
                "The name last heard for a user of another network; null while none was heard.",
                Query::Name(Foreign {
                    network: NETWORK,
                    account: 18,
                }),
            ),
            query(
                "Named",
                "The accounts of a network last heard under a name.",
                Query::Named {
                    network: NETWORK,
                    name: "alice".into(),
                },
            ),
            query(
                "Home",
                "This network's id, the digest of its genesis block.",
                Query::Home,
            ),
            query(
                "Linked",
                "The ids of every link whose last Open named it so, closed links too.",
                Query::Linked {
                    name: "othernet".into(),
                },
            ),
        ]
    }
}
