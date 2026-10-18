//! The program: the account the host resolved, then every op and every
//! query, each handed to its function (`ops.rs`, `changes.rs`,
//! `queries.rs`, `reads.rs`, `change_queries.rs`).

use ::program::schema::{Return, returns};
use ::program::{Error, ExecCtx, MessageId, Outcome, Program, QueryCtx};

use crate::change_queries::{change, changes, judgment};
use crate::changes::{
    Draft, Edit, MergeRequest, RebaseRequest, SquashRequest, close, edit, merge_heads, open,
    rebase, repoint, squash, submit_review,
};
use crate::contract::*;
use crate::ops::{
    configure, create, delete_branch, init, needs, protect, push, resource, retired, stage, touch,
};
use crate::queries::{
    advertise, closure, listing, protected_branches, ref_names, refs, repos, upload,
};
use crate::reads::{blob, comparison, diff, log, tree};
use crate::state::{load_bounds, load_repo};
use crate::watchers::watch;

pub struct Forge;

impl Program for Forge {
    const NAME: &'static str = crate::PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = &[
        returns::<OpReply>("Merge"),
        returns::<OpReply>("ChangeOpen"),
        returns::<OpReply>("ChangeEdit"),
        returns::<OpReply>("ChangeClose"),
        returns::<OpReply>("ReviewSubmit"),
        returns::<OpReply>("SquashMerge"),
        returns::<OpReply>("Rebase"),
        returns::<OpReply>("ChangeRepoint"),
    ];

    fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Error> {
        init(ctx, params)
    }

    // A watcher's refusal of what it heard is absorbed: it never fails forge's write.
    fn reply(_ctx: &ExecCtx, _id: &MessageId, _outcome: &Outcome) -> Result<(), Error> {
        Ok(())
    }

    /// Runs one op as the account the host resolved
    /// ([`ExecCtx::sender_account`]): a signed frame's key's account, a
    /// program's own, or the agent a program runs and sends it for (as
    /// harness does); never the chain itself, and never a key that holds no
    /// account. Every op names its repository; an accepted one marks it
    /// active.
    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        let repo = op.repo().map(str::to_owned);
        let actor = &Principal::Account(ctx.sender_account()?);
        let reply = match op {
            Op::Watch { deliver } => return watch(ctx, deliver),
            Op::Create { repo, hash } => create(ctx, actor, &repo, hash).map(|()| None),
            Op::Configure { repo, settings } => configure(ctx, &repo, settings).map(|()| None),
            Op::Grant { .. } | Op::Revoke { .. } => Err(retired()),
            Op::Push {
                repo,
                request,
                pack,
            } => push(ctx, &repo, &request, pack).map(|()| None),
            Op::Merge {
                repo,
                into,
                from,
                expected_into,
                expected_from,
                result,
                change,
            } => {
                let merge = MergeRequest {
                    into,
                    from,
                    expected_into,
                    expected_from,
                    result,
                    change,
                };
                merge_heads(ctx, actor, &repo, merge).map(Some)
            }
            Op::ChangeOpen {
                repo,
                from,
                into,
                title,
                body,
                reviewers,
            } => {
                let draft = Draft {
                    from,
                    into,
                    title,
                    body,
                    reviewers,
                };
                open(ctx, actor, &repo, draft).map(Some)
            }
            Op::ChangeEdit {
                repo,
                n,
                title,
                body,
                reviewers,
            } => {
                let fields = Edit {
                    title,
                    body,
                    reviewers,
                };
                edit(ctx, &repo, n, fields).map(Some)
            }
            Op::ChangeClose { repo, n } => close(ctx, actor, &repo, n).map(Some),
            Op::ReviewSubmit { repo, n, review } => {
                submit_review(ctx, actor, &repo, n, review).map(Some)
            }
            Op::Stage { repo, pack } => stage(ctx, &repo, pack).map(|()| None),
            Op::SquashMerge {
                repo,
                into,
                from,
                expected_into,
                expected_from,
                message,
                change,
            } => {
                let request = SquashRequest {
                    into,
                    from,
                    expected_into,
                    expected_from,
                    message,
                    change,
                };
                squash(ctx, actor, &repo, request).map(Some)
            }
            Op::Rebase {
                repo,
                n,
                onto,
                expected_head,
            } => {
                let request = RebaseRequest {
                    n,
                    onto,
                    expected_head,
                };
                rebase(ctx, actor, &repo, request).map(Some)
            }
            Op::ChangeRepoint { repo, n, from } => repoint(ctx, &repo, n, from).map(Some),
            Op::DeleteBranch {
                repo,
                branch,
                expected,
            } => delete_branch(ctx, &repo, &branch, &expected).map(|()| None),
            Op::Protect {
                repo,
                branch,
                protected,
            } => protect(ctx, &repo, &branch, protected).map(|()| None),
        }?;
        if let Some(reply) = reply {
            ctx.set_return_data(abi::encode(&reply));
        }
        match repo {
            Some(repo) => touch(ctx, &repo, ctx.env().height),
            None => Ok(()),
        }
    }

    /// Git's own wire: the ref advertisement and upload-pack answer with
    /// git's bytes, unframed, so a git client reads them off the node as is.
    /// Every other query is one height-bearing [`Reply`], as borsh.
    fn answer(ctx: &QueryCtx, query: Query) -> Result<Vec<u8>, Error> {
        match &query {
            Query::Advertise { repo, service } => advertise(ctx, repo, *service),
            Query::Upload { repo, request } => upload(ctx, repo, request),
            _ => Ok(abi::encode(&Self::query(ctx, query)?)),
        }
    }

    fn access(ctx: &QueryCtx, op: &Op) -> Result<Vec<::program::Need>, Error> {
        needs(ctx, op)
    }

    fn resource(ctx: &QueryCtx, key: &str) -> Result<::program::Resource, Error> {
        resource(ctx, key)
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        let height = ctx.env().height;
        let bounds = load_bounds(ctx)?;
        let scope = query.scope();
        let listing =
            |page: &PageRequest| listing(ctx, page.bounded(bounds.page_size as u64), scope.clone());
        Ok(match &query {
            // `answer` takes git's two queries before `query` sees them
            Query::Advertise { .. } | Query::Upload { .. } => {
                return Err(::program::invalid(
                    "a git-wire query answers git's bytes, not a Reply",
                ));
            }
            Query::Repos { page } => Reply::Repos {
                height,
                page: repos(ctx, &listing(page)?)?,
            },
            Query::Repo { repo } => Reply::Repo {
                height,
                repo: RepoInfo {
                    name: repo.clone(),
                    repo: load_repo(ctx, repo)?,
                },
                bounds,
            },
            Query::Refs { repo, page } => Reply::Refs {
                height,
                page: refs(ctx, repo, &listing(page)?)?,
            },
            Query::Activity { repo } => Reply::Activity {
                height,
                last_height: load_repo(ctx, repo)?.last_activity,
            },
            Query::Log {
                repo,
                from,
                exclude,
                page,
            } => log(
                ctx,
                height,
                &bounds,
                repo,
                from,
                exclude.as_ref(),
                &listing(page)?,
            )?,
            Query::Tree {
                repo,
                at,
                path,
                page,
            } => tree(ctx, height, &bounds, repo, at, path, &listing(page)?)?,
            Query::Blob { repo, oid, range } => blob(ctx, height, &bounds, repo, oid, *range)?,
            Query::Diff {
                repo,
                base,
                head,
                path,
                page,
            } => diff(
                ctx,
                height,
                &bounds,
                repo,
                base,
                head,
                path.as_deref(),
                &listing(page)?,
            )?,
            Query::Compare { repo, from, into } => {
                comparison(ctx, height, &bounds, repo, from, into)?
            }
            Query::Changes { repo, filter, page } => Reply::Changes {
                height,
                page: changes(ctx, repo, filter, &listing(page)?)?,
            },
            Query::Change { repo, n, page } => change(ctx, height, repo, *n, &listing(page)?)?,
            Query::Judgment { principal, page } => Reply::Judgment {
                height,
                page: judgment(ctx, principal, &listing(page)?)?,
            },
            Query::Closure { repo, walk } => closure(ctx, height, &bounds, repo, walk)?,
            Query::Protected { repo, page } => Reply::Protected {
                height,
                page: protected_branches(ctx, repo, &listing(page)?)?,
            },
            Query::RefNames { repo, prefix, page } => Reply::RefNames {
                height,
                page: ref_names(ctx, repo, prefix, &listing(page)?)?,
            },
            Query::DiffStat {
                repo,
                base,
                head,
                page,
            } => crate::diffs::stat(diff(
                ctx,
                height,
                &bounds,
                repo,
                base,
                head,
                None,
                &listing(page)?,
            )?)?,
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Forge);
