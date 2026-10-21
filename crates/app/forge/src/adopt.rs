//! One-time: `AdoptOwners` links each owned row forge keeps with no owner record, or on its
//! owner's built-in Open, to the policy matching what the access program enforces for it now, one
//! page a block until the last. Goes once duckpond has run it.
use ::program::{Error, ExecCtx, Reply, invalid};
use ownership::{Adopted, Rule, adopt, again_next_block, by_itself, owner_of, rule_of, writers_of};

use crate::PROGRAM;
use crate::contract::{Op, change_key, repo_key};
use crate::ops::resource;
use crate::state::{AUTHORSHIP, Authorship, CHANGES, CUSTODY, Custody, REPOS, REVIEWS};

pub(crate) fn adopt_owners(
    ctx: &ExecCtx,
    kind: &str,
    after: Option<Vec<u8>>,
    limit: u64,
) -> Result<(), Error> {
    let at = after.as_deref();
    if !by_itself(ctx)? {
        let next = match kind {
            "forge.review" => REVIEWS.batch(ctx, at, limit)?.next,
            "forge.change" => CHANGES.batch(ctx, at, limit)?.next,
            "forge.repo" => REPOS.batch(ctx, at, limit)?.next,
            other => return Err(invalid(format!("forge keeps no owned kind {other}"))),
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
    let next = match kind {
        "forge.review" => {
            let page = REVIEWS.batch(ctx, at, limit)?;
            for (row, review) in page.rows {
                items.push(Adopted {
                    key: REVIEWS.key(&row),
                    owner: owner_of(&review.author, PROGRAM),
                    kind: Some(kind.to_owned()),
                    rule: Rule::Open,
                });
            }
            page.next
        }
        "forge.change" => {
            let page = CHANGES.batch(ctx, at, limit)?;
            for ((repo, n), change) in page.rows {
                let row = (repo.clone(), n);
                if !AUTHORSHIP.has(ctx, &row) {
                    let author = change.author.clone();
                    AUTHORSHIP.put(ctx, &row, &Authorship { author, rev: 0 });
                }
                let owner = owner_of(&change.author, PROGRAM);
                let rule = rule_of(&owner, &today(&change_key(&repo, n))?);
                items.push(Adopted {
                    key: AUTHORSHIP.key(&row),
                    owner,
                    kind: Some(kind.to_owned()),
                    rule,
                });
            }
            page.next
        }
        "forge.repo" => {
            let page = REPOS.batch(ctx, at, limit)?;
            for (name, repo) in page.rows {
                if !CUSTODY.has(ctx, &name) {
                    let owner = repo.owner.clone();
                    CUSTODY.put(ctx, &name, &Custody { owner, rev: 0 });
                }
                let owner = owner_of(&repo.owner, PROGRAM);
                let rule = rule_of(&owner, &today(&repo_key(&name))?);
                items.push(Adopted {
                    key: CUSTODY.key(&name),
                    owner,
                    kind: Some(kind.to_owned()),
                    rule,
                });
            }
            page.next
        }
        other => return Err(invalid(format!("forge keeps no owned kind {other}"))),
    };
    adopt(ctx, items)?;
    let Some(after) = next else {
        return Ok(());
    };
    let again = Op::AdoptOwners {
        kind: kind.to_owned(),
        after: Some(after),
        limit,
    };
    again_next_block(ctx, &again)
}
