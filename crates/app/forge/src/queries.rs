//! The repository answers [`Forge::query`](crate::Forge) names, and git's
//! own bytes for a git client.

use ::program::Error;
use ::program::{QueryCtx, invalid, stale};
use gitcore::wire::receive::advertise_refs;
use gitcore::wire::smart_http_service_header;
use gitcore::wire::upload::{Command, capability_advertisement, ls_refs_response, parse_command};
use store::{KeyCodec, Listing};

use gitcore::walk::{Closed, closure as close, history as walk_history};

use crate::contract::*;
use crate::objects::ObjectStore;
use crate::ops::{cap, refusal_of};
use crate::state::{
    ACTIVITY, ARCHIVED, PROTECTED, REFS, load_bounds, load_refs, load_repo, repo_hash, storage,
    writes,
};

const AGENT: &[u8] = b"ducktape-forge";

/// Repositories, the most recently active first.
pub(crate) fn repos(ctx: &QueryCtx, listing: &Listing) -> Result<PageResponse<RepoInfo>, Error> {
    ACTIVITY.page_of(ctx, &(), listing)?.try_map(|(_, name)| {
        Ok(RepoInfo {
            repo: load_repo(ctx, &name)?,
            name,
        })
    })
}

// Whether the repository `name` is archived; a name no repository has is not found.
pub(crate) fn archived(ctx: &QueryCtx, name: &str) -> Result<bool, Error> {
    load_repo(ctx, name)?;
    Ok(crate::state::archived(ctx, name))
}

// The archived repositories, in name order.
pub(crate) fn archived_repos(
    ctx: &QueryCtx,
    listing: &Listing,
) -> Result<PageResponse<RepoInfo>, Error> {
    ARCHIVED.page_of(ctx, &(), listing)?.try_map(|name| {
        Ok(RepoInfo {
            repo: load_repo(ctx, &name)?,
            name,
        })
    })
}

/// A repository's refs in byte-name order.
pub(crate) fn refs(
    ctx: &QueryCtx,
    name: &str,
    listing: &Listing,
) -> Result<PageResponse<RefInfo>, Error> {
    let hash = repo_hash(&load_repo(ctx, name)?);
    REFS.page_of(ctx, &name.to_owned(), listing)?
        .try_map(|((_, name), bytes)| {
            let oid = gitcore::Oid::from_bytes(hash, &bytes).map_err(|e| storage(e.to_string()))?;
            Ok(RefInfo {
                name,
                target: oid.to_hex(),
            })
        })
}

// A repository's protected branches in byte-name order; its head is protected besides.
pub(crate) fn protected_branches(
    ctx: &QueryCtx,
    name: &str,
    listing: &Listing,
) -> Result<PageResponse<Vec<u8>>, Error> {
    load_repo(ctx, name)?;
    Ok(PROTECTED
        .page_of(ctx, &name.to_owned(), listing)?
        .map(|(_, branch)| branch))
}

// The refs of a repository whose name starts with `prefix`, in byte-name order: one key range
// over `REFS`, since a name's key is its bytes escaped and the escape keeps a prefix a prefix.
pub(crate) fn ref_names(
    ctx: &QueryCtx,
    name: &str,
    prefix: &str,
    listing: &Listing,
) -> Result<PageResponse<RefName>, Error> {
    let hash = repo_hash(&load_repo(ctx, name)?);
    let mut start = REFS.key(&name.to_owned());
    let escaped = prefix.as_bytes().to_vec().key_bytes();
    start.extend_from_slice(&escaped[..escaped.len() - 2]);
    let rows = ctx
        .scan(listing.scan_ahead(&start))
        .into_iter()
        .map(|entry| {
            let mut key = &entry.key[REFS.prefix().len()..];
            let decoded = <(String, Vec<u8>)>::decode_key(&mut key)
                .ok_or_else(|| storage("a ref's key does not decode"))?;
            let bytes: Vec<u8> =
                borsh::from_slice(&entry.value).map_err(|e| storage(e.to_string()))?;
            let oid = gitcore::Oid::from_bytes(hash, &bytes).map_err(|e| storage(e.to_string()))?;
            let named = RefName {
                name: String::from_utf8_lossy(&decoded.1).into_owned(),
                target: oid.to_hex(),
            };
            Ok((entry.key, named))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(listing.reply(rows))
}

/// A forge listing is rewritten only by a forge op, so a cursor is pinned
/// to the count of ops accepted when it was answered and is good until
/// forge next writes: a walk crosses the blocks that wrote nothing here,
/// and a push mid-walk restarts it, even one in the block that answered
/// the cursor (the preconfirmed layer answers at the height its ops run
/// at, so a height alone would not tell).
pub(crate) fn listing(ctx: &QueryCtx, page: PageRequest, scope: Vec<u8>) -> Result<Listing, Error> {
    let writes = writes(ctx)?;
    let listing = page.listing(ctx, scope)?.pinned(writes);
    if listing.cursor_pin.is_some_and(|pinned| pinned != writes) {
        return Err(stale("the listing changed; restart it"));
    }
    Ok(listing)
}

pub(crate) fn advertise(ctx: &QueryCtx, name: &str, service: Service) -> Result<Vec<u8>, Error> {
    let repo = load_repo(ctx, name)?;
    let hash = repo_hash(&repo);
    let body = match service {
        Service::ReceivePack => {
            let refs = load_refs(ctx, name, hash)?;
            let object_format = format!("object-format={}", hash.name());
            let agent = format!("agent={}", String::from_utf8_lossy(AGENT));
            advertise_refs(
                &refs,
                hash,
                &[
                    b"report-status",
                    b"delete-refs",
                    b"side-band-64k",
                    b"ofs-delta",
                    object_format.as_bytes(),
                    agent.as_bytes(),
                ],
            )
        }
        Service::UploadPack => {
            let mut body = smart_http_service_header(b"git-upload-pack");
            body.extend_from_slice(&capability_advertisement(hash, AGENT));
            body
        }
    };
    Ok(body)
}

pub(crate) fn upload(ctx: &QueryCtx, name: &str, request: &[u8]) -> Result<Vec<u8>, Error> {
    let repo = load_repo(ctx, name)?;
    let bounds = load_bounds(ctx)?;
    let hash = repo_hash(&repo);
    let refs = load_refs(ctx, name, hash)?;
    let objects = ObjectStore::new(ctx);
    let Some(command) = parse_command(request, hash).map_err(refusal_of)? else {
        return Ok(Vec::new());
    };
    match command {
        Command::LsRefs(command) => ls_refs_response(
            &objects,
            &refs,
            Some(&repo.settings.head),
            &command,
            cap(bounds.fetch_walk),
        )
        .map_err(refusal_of),
        Command::Fetch(_) => Err(invalid(
            "a fetch lists its objects with Closure and reads their bytes from the node's blobs",
        )),
    }
}

pub(crate) fn closure(
    ctx: &QueryCtx,
    height: u64,
    bounds: &Bounds,
    name: &str,
    walk: &Walk,
) -> Result<Reply, Error> {
    walked(ctx, height, bounds, name, walk, true)
}

// `Closure` less the trees, which the client expands from tree bodies it reads itself.
pub(crate) fn history(
    ctx: &QueryCtx,
    height: u64,
    bounds: &Bounds,
    name: &str,
    walk: &Walk,
) -> Result<Reply, Error> {
    walked(ctx, height, bounds, name, walk, false)
}

fn walked(
    ctx: &QueryCtx,
    height: u64,
    bounds: &Bounds,
    name: &str,
    walk: &Walk,
    trees: bool,
) -> Result<Reply, Error> {
    let repo = load_repo(ctx, name)?;
    let store = ObjectStore::querying(ctx, bounds, bounds.tree_walk);
    let Some(asked) = walk.parse(repo.hash) else {
        return Err(invalid(
            "a walk names an id that is not an object id of this repository",
        ));
    };
    let closed = if trees {
        close(&store, asked, usize::MAX)
    } else {
        walk_history(&store, asked, usize::MAX)
    };
    let Closed { listed, next } = closed.map_err(refusal_of)?;
    Ok(Reply::Closure {
        height,
        listed: listed.into_iter().map(Listed::of).collect(),
        next: Walk::of(next),
    })
}
