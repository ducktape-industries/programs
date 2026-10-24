//! What valset tells an agent of its ops and queries. Its Op is the validators role's, defined in
//! abi, so a marker carries each variant's words and a working value of it.
use ::program::tools::{Described, Tool, example};

use crate::{Motion, Op, PageRequest, Query};

pub(crate) struct Ops;

const KEY: [u8; 32] = [7; 32];

fn told(variant: &'static str, description: &'static str, value: Op) -> Tool {
    Tool {
        variant,
        description,
        example: Some(example(value)),
        reply: "",
    }
}

impl Described for Ops {
    const ABOUT: &'static str = "Who validates and who resides on the network, by ed25519 key: a key enrolls as a resident through an open door or with a validator's invite, and the validators' vote, by more than two thirds of them, promotes, demotes or removes a member, opens or closes the door, or sends a role's program an op.";

    fn tools() -> Vec<Tool> {
        vec![
            told(
                "Enroll",
                "Enroll the signing key as a resident, with the address its node answers at and its halo, the host:port other members dial (no loopback or unspecified host); a member that enrolls again replaces its address and halo. While the door is closed it takes an invite a validator signed for this network, unexpired and not yet redeemed; refused once the network holds its founding cap of members.",
                Op::Enroll {
                    address: Some("wss://node.example:443".into()),
                    halo: "node.example:9100".into(),
                    invite: None,
                },
            ),
            told(
                "Leave",
                "The signing key leaves the network, its pending promote, demote and remove motions gone with it; the last validator cannot leave.",
                Op::Leave,
            ),
            told(
                "Vote",
                "Vote for a motion, as a validator's node key or as the account that manages the active program account a validator's node key sits on (each validator it manages votes). A motion passes once more than two thirds of the seated validators voted for it (n - (n - 1) / 3 of n), and is enacted at once: Promote a resident, Demote or Remove a member but never the last validator, open or close the Door, set which Queries the network answers (Signed or Open; a motion naming the current policy is refused), or send a Role's program op as the vote. Answers how it stands: counted with votes and needed, or enacted.",
                Op::Vote(Motion::Door { open: true }),
            ),
            told(
                "SetHalo",
                "Set the signing member's halo, the host:port other members dial.",
                Op::SetHalo {
                    halo: "node.example:9100".into(),
                },
            ),
            told(
                "ClearHalo",
                "The signing enrolled member explicitly has no direct halo address. Its registered address and membership stay unchanged; SetHalo or enrolling again undoes this, as do leaving or removal. Other members reach it through the existing relay or punch path.",
                Op::ClearHalo,
            ),
        ]
    }
}

pub(crate) fn validators() -> Query {
    Query::Validators
}

pub(crate) fn members() -> Query {
    Query::Members
}

pub(crate) fn memberships() -> Query {
    Query::Memberships {
        page: PageRequest::default(),
    }
}

pub(crate) fn membership() -> Query {
    Query::Membership { key: KEY.to_vec() }
}

pub(crate) fn motions() -> Query {
    Query::Motions {
        page: PageRequest::default(),
    }
}

pub(crate) fn voices() -> Query {
    Query::Voices { key: KEY.to_vec() }
}

pub(crate) fn query_policy() -> Query {
    Query::QueryPolicy
}

pub(crate) fn halo_cleared() -> Query {
    Query::HaloCleared
}
