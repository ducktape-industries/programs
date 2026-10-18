//! Typed reads of the system programs: accounts (identity), validators
//! (valset), programs (programs), and every transaction and what each
//! account did (activity), a page at a time. The window's blocks are the
//! host's (`load.rs`).
use view::host::Page;
use view::prelude::*;

use crate::state::{Accounts, Network, Used};

/// Every account.
pub(crate) async fn accounts(host: Host) -> Result<Accounts, Error> {
    let list = host
        .query_all(|after| identity::ask::List {
            page: identity::PageRequest { after, limit: None },
        })
        .await?;
    Ok(Accounts { list })
}

pub(crate) async fn validators(host: Host) -> Result<Vec<Vec<u8>>, Error> {
    host.query(valset::ask::Validators).await
}

pub(crate) async fn network(host: Host) -> Result<Network, Error> {
    let (height, programs, views) = host.query(programs::ask::Roster).await?;
    let hooks = host.query(programs::ask::Hooks(height)).await?;
    let roles = host.query(programs::ask::Roles).await?;
    let mut changes = host
        .query_all(|after| programs::ask::Scheduled {
            page: programs::PageRequest { after, limit: None },
        })
        .await?;
    changes.retain(|change| change.height > height);
    Ok(Network {
        programs,
        views,
        changes,
        hooks,
        roles: Some(roles),
    })
}

pub(crate) async fn used(host: Host) -> Result<Used, Error> {
    let list = host
        .query_all(|after| activity::ask::Used {
            page: activity::PageRequest { after, limit: None },
        })
        .await?;
    Ok(Used { list })
}

/// A page of activity's frames, newest first. One page holds the
/// Overview's latest and the phone's Activity rows (`load::ACTED`); an
/// unbounded ask is one page of every frame on the network (garage's 57k,
/// 104M fuel in one call).
pub(crate) const PAGE: u64 = 50;

/// The page of every transaction after `after`, as a [`Paged`](view::Paged)
/// reads it: its frames and the cursor of the page after them.
pub(crate) async fn frames(
    host: Host,
    after: Option<Vec<u8>>,
) -> Result<Page<activity::Frame>, Error> {
    let page = host
        .query(activity::ask::Frames {
            page: activity::PageRequest::resume(after, PAGE),
        })
        .await?;
    Ok((page.items, page.next))
}

/// The page of what account `account` did after `after`.
pub(crate) async fn acted(
    host: Host,
    account: u64,
    after: Option<Vec<u8>>,
) -> Result<Page<activity::Frame>, Error> {
    let page = host
        .query(activity::ask::Acted {
            account,
            page: activity::PageRequest::resume(after, PAGE),
        })
        .await?;
    Ok((page.items, page.next))
}
