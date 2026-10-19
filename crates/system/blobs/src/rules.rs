// The rules: an account puts a numbered bundle and indexes each blob to it, a current
// validator's key receipts it once, and the receipt that reaches a quorum marks it held.

use ::program::{
    Error, ExecCtx, QueryCtx, Range, already_exists, invalid, not_found, unauthorized,
    unexpected_reply, wrong_state,
};
use abi::BlobId;
use abi::role::validators;
use blob::contract::{
    Bundle, Custody, Entry, Holding, NEXT, bundle_key, index_key, index_prefix, indexed_bundle,
    quorum,
};

pub(crate) fn bundle(ctx: &QueryCtx, n: u64) -> Result<Option<Bundle>, Error> {
    ctx.record(bundle_key(n))
}

pub(crate) fn put(ctx: &ExecCtx, blobs: Vec<Entry>) -> Result<(), Error> {
    let putter = ctx.sender_account()?;
    if blobs.is_empty() {
        return Err(invalid("a put names at least one blob"));
    }
    let n: u64 = ctx.record(NEXT)?.unwrap_or(0);
    for entry in &blobs {
        ctx.set(index_key(&entry.id, n), Vec::new());
    }
    let bundle = Bundle {
        putter,
        height: ctx.env().height,
        blobs,
        custody: Custody::Receipted(Vec::new()),
    };
    ctx.put(bundle_key(n), &bundle);
    ctx.put(NEXT, &(n + 1));
    ctx.set_return_data(abi::encode(&n));
    Ok(())
}

pub(crate) fn receipt(ctx: &ExecCtx, n: u64) -> Result<(), Error> {
    let key = ctx.env().signer()?;
    let seated = validators(ctx)?;
    let current = seated.contains(&key);
    if !current {
        return Err(unauthorized(
            "only a current validator's key receipts a bundle",
        ));
    }
    let Some(mut bundle) = bundle(ctx, n)? else {
        return Err(not_found(format!("no bundle {n}")));
    };
    let Custody::Receipted(mut keys) = bundle.custody else {
        return Err(wrong_state(format!("bundle {n} is held")));
    };
    if keys.contains(&key) {
        return Err(already_exists(format!("this key receipted bundle {n}")));
    }
    keys.push(key);
    let counted = keys.iter().filter(|key| seated.contains(key)).count();
    let reached = counted >= quorum(seated.len());
    bundle.custody = if reached {
        Custody::Held {
            since: ctx.env().height,
        }
    } else {
        Custody::Receipted(keys)
    };
    ctx.put(bundle_key(n), &bundle);
    Ok(())
}

pub(crate) fn held(ctx: &QueryCtx, blob: BlobId) -> Result<Option<Holding>, Error> {
    for entry in ctx.scan(Range::prefix(index_prefix(&blob))) {
        let Some(n) = indexed_bundle(&entry.key, &blob) else {
            continue;
        };
        let Some(bundle) = bundle(ctx, n)? else {
            continue;
        };
        let Custody::Held { since } = bundle.custody else {
            continue;
        };
        let Some(named) = bundle.blobs.into_iter().find(|named| named.id == blob) else {
            continue;
        };
        return Ok(Some(Holding {
            bundle: n,
            len: named.len,
            kind: named.kind,
            since,
        }));
    }
    Ok(None)
}

fn validators(ctx: &QueryCtx) -> Result<Vec<Vec<u8>>, Error> {
    let role = ctx.env().role(abi::role::VALIDATORS)?.clone();
    match ctx.query(role.clone(), &validators::Query::Validators)? {
        validators::Reply::Validators(seated) => Ok(seated),
        other => Err(unexpected_reply(&role, "Validators", &other)),
    }
}
