//! One-time (J-011 ⑥): `AdoptOwners` records an owner for each channel and message chat kept
//! before the ownership role was seated, linked to the policy matching what the access program
//! enforces for it today. Goes once duckpond has run it.
use ::program::{Error, ExecCtx, Reply, invalid};
use ownership::{Adopted, adopt, by_itself, owner_of, rule_of, writers_of};

use crate::gate::{message_key, resource};
use crate::state::{CHANNELS, MESSAGE_IDS, message};
use crate::{Op, PROGRAM};

// A batch of `kind`'s rows after `after`. Sent by anyone, it answers where the next batch starts
// and sends chat the same op, whose run records them.
pub(crate) fn adopt_owners(
    ctx: &ExecCtx,
    kind: &str,
    after: Option<Vec<u8>>,
    limit: u64,
) -> Result<(), Error> {
    let at = after.as_deref();
    if !by_itself(ctx)? {
        let next = match kind {
            "chat.channel" => CHANNELS.batch(ctx, at, limit)?.next,
            "chat.message" => MESSAGE_IDS.batch(ctx, at, limit)?.next,
            other => return Err(invalid(format!("chat keeps no owned kind {other}"))),
        };
        ctx.set_return_data(abi::encode(&next));
        let again = Op::AdoptOwners {
            kind: kind.to_owned(),
            after,
            limit,
        };
        ctx.emit(PROGRAM, &again, Reply::None);
        return Ok(());
    }
    let today = |key: &str| writers_of(ctx, PROGRAM, key, |at| resource(ctx, at));
    let mut items = Vec::new();
    match kind {
        "chat.channel" => {
            for (id, row) in CHANNELS.batch(ctx, at, limit)?.rows {
                let owner = owner_of(&row.owner, PROGRAM);
                let rule = rule_of(&owner, &today(&id)?);
                items.push(Adopted {
                    key: CHANNELS.key(&id),
                    owner,
                    kind: Some(kind.to_owned()),
                    rule,
                });
            }
        }
        "chat.message" => {
            for (id, (channel, seq)) in MESSAGE_IDS.batch(ctx, at, limit)?.rows {
                let row = message(ctx, &channel, seq)?;
                let owner = owner_of(&row.author, PROGRAM);
                let rule = rule_of(&owner, &today(&message_key(&channel, seq))?);
                items.push(Adopted {
                    key: MESSAGE_IDS.key(&id),
                    owner,
                    kind: Some(kind.to_owned()),
                    rule,
                });
            }
        }
        other => return Err(invalid(format!("chat keeps no owned kind {other}"))),
    }
    adopt(ctx, items)
}
