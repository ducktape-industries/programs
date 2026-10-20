//! Bounded, resumable query pages over `program`'s `Range`: the one paging
//! vocabulary every program's queries speak.

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use program::{Error, QueryCtx};
use serde::{Deserialize, Serialize};

use program::{invalid, stale};

/// What `PageRequest::after` and `PageResponse::next` carry, opaque to clients: the
/// listing the cursor belongs to (`scope`), what it is pinned to (the height
/// that answered it, unless the program pinned it to something of its own:
/// see [`Listing::pinned`]), and the raw position to resume after.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Cursor {
    /// the pin: the answering height unless the program set another
    pub height: u64,
    pub scope: Vec<u8>,
    pub after: Vec<u8>,
}

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
)]
pub struct PageRequest {
    pub after: Option<Vec<u8>>,
    pub limit: Option<u64>,
}

impl PageRequest {
    pub const fn first(limit: u64) -> PageRequest {
        PageRequest::resume(None, limit)
    }

    /// The page after the one that answered `next`
    /// ([`PageResponse::next`]; `None` asks the first page again).
    pub const fn resume(next: Option<Vec<u8>>, limit: u64) -> PageRequest {
        PageRequest {
            after: next,
            limit: Some(limit),
        }
    }

    pub fn limit(&self) -> u64 {
        self.limit.unwrap_or(u64::MAX).max(1)
    }

    /// The same page, its limit capped at `max` (a program's own bound).
    pub fn bounded(&self, max: u64) -> PageRequest {
        PageRequest::resume(self.after.clone(), self.limit().min(max.max(1)))
    }

    /// The scope a listing binds its cursors to: the borsh of the query's
    /// identifying arguments (a prefix, a channel, a repo and ref).
    pub fn scope_of(args: &impl BorshSerialize) -> Vec<u8> {
        abi::encode(args)
    }

    /// The cursor `after` carries, refused when it belongs to another
    /// listing than `scope`.
    pub fn open(&self, scope: &[u8]) -> Result<Option<Cursor>, Error> {
        let Some(bytes) = &self.after else {
            return Ok(None);
        };
        let cursor: Cursor =
            abi::decode(bytes).map_err(|_| invalid("a page cursor does not decode"))?;
        if cursor.scope != scope {
            return Err(stale("the cursor belongs to another listing"));
        }
        Ok(Some(cursor))
    }

    /// This page over one listing: its cursor opened against `scope`, and
    /// every `next` it answers bound to `scope` and pinned to the height of
    /// `ctx`, which answers it.
    pub fn listing(&self, ctx: &QueryCtx, scope: Vec<u8>) -> Result<Listing, Error> {
        let height = ctx.env().height;
        let cursor = self.open(&scope)?;
        Ok(Listing {
            limit: self.limit(),
            cursor_pin: cursor.as_ref().map(|c| c.height),
            after: cursor.map(|c| c.after),
            scope,
            height,
            pin: height,
        })
    }
}

/// A `PageRequest` opened over one listing (see `PageRequest::listing`).
#[derive(Debug)]
pub struct Listing {
    limit: u64,
    after: Option<Vec<u8>>,
    scope: Vec<u8>,
    height: u64,
    pin: u64,
    /// What the cursor was pinned to, for a program whose listings are
    /// rewritten (forge refuses a cursor pinned to another write count).
    pub cursor_pin: Option<u64>,
}

impl Listing {
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Pins every `next` to `pin` instead of the answering height: a program
    /// whose listings are rewritten by its own ops, and by nothing else,
    /// pins to a count of them, so a walk outlives the blocks that wrote
    /// nothing there and a write that shares the answering height (one of
    /// several in a block) still tells.
    pub fn pinned(mut self, pin: u64) -> Self {
        self.pin = pin;
        self
    }

    fn next(&self, after: Vec<u8>) -> Vec<u8> {
        abi::encode(&Cursor {
            height: self.pin,
            scope: self.scope.clone(),
            after,
        })
    }

    pub fn offset(&self) -> Result<usize, Error> {
        match &self.after {
            None => Ok(0),
            Some(bytes) => <[u8; 8]>::try_from(bytes.as_slice())
                .map(|b| u64::from_be_bytes(b) as usize)
                .map_err(|_| invalid("a list cursor is an 8-byte offset")),
        }
    }

    pub fn scan(&self, prefix: &[u8]) -> program::Range {
        let mut scan = program::Range::prefix(prefix);
        if let Some(after) = &self.after {
            let start = scan.start.clone();
            scan = scan.after(after);
            scan.start = scan.start.max(start);
        }
        scan.limit = Some(self.limit);
        scan
    }

    pub fn scan_ahead(&self, prefix: &[u8]) -> program::Range {
        self.scan(prefix).limit(self.limit.saturating_add(1))
    }

    /// Consume ordered rows, retaining one lookahead to detect the final page.
    pub fn reply<T>(&self, rows: impl IntoIterator<Item = (Vec<u8>, T)>) -> PageResponse<T> {
        let mut rows = rows
            .into_iter()
            .filter(|(key, _)| self.after.as_ref().is_none_or(|after| key > after));
        let mut items = Vec::new();
        let mut last = None;
        for (key, item) in rows.by_ref().take(self.limit as usize) {
            last = Some(key);
            items.push(item);
        }
        let next = rows.next().and(last).map(|key| self.next(key));
        PageResponse {
            height: self.height,
            items,
            next,
        }
    }

    /// A page of an in-memory list: the cursor's position is the big-endian
    /// offset of the next item.
    pub fn slice<T: Clone>(&self, values: &[T]) -> Result<PageResponse<T>, Error> {
        let start = self.offset()?;
        if start > values.len() {
            return Err(invalid("cursor past the end of this listing"));
        }
        let end = start.saturating_add(self.limit as usize).min(values.len());
        Ok(PageResponse {
            height: self.height,
            items: values[start..end].to_vec(),
            next: (end < values.len()).then(|| self.next((end as u64).to_be_bytes().to_vec())),
        })
    }
}

/// A bounded query result. Resume with `next` as `PageRequest::after` on the same query.
/// Height identifies the answering state; pagination does not pin a snapshot.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Serialize,
    Deserialize,
)]
pub struct PageResponse<T> {
    pub height: u64,
    pub items: Vec<T>,
    pub next: Option<Vec<u8>>,
}

impl<T> PageResponse<T> {
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> PageResponse<U> {
        PageResponse {
            height: self.height,
            items: self.items.into_iter().map(f).collect(),
            next: self.next,
        }
    }

    pub fn try_map<U>(
        self,
        f: impl FnMut(T) -> Result<U, Error>,
    ) -> Result<PageResponse<U>, Error> {
        Ok(PageResponse {
            height: self.height,
            items: self.items.into_iter().map(f).collect::<Result<_, _>>()?,
            next: self.next,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(height: u64) -> QueryCtx {
        program::MockHost::default().query(program::Env {
            height,
            ..program::MockHost::env("test")
        })
    }

    fn listing(page: &PageRequest) -> Listing {
        page.listing(&at(7), b"p/".to_vec()).unwrap()
    }

    #[test]
    fn a_page_scans_after_its_cursor_and_a_cursor_stays_in_its_listing() {
        let first = listing(&PageRequest::first(2)).reply([
            (b"p/3".to_vec(), 3u8),
            (b"p/4".to_vec(), 4),
            (b"p/5".to_vec(), 5),
        ]);
        let page = PageRequest::resume(first.next, 2);
        let scan = listing(&page).scan(b"p/");
        assert!(!scan.admits(b"p/4"));
        assert!(scan.admits(b"p/5"));
        assert_eq!(scan.limit, Some(2));
        assert_eq!(
            listing(&PageRequest::default()).scan(b"p/").limit,
            Some(u64::MAX)
        );
        assert_eq!(PageRequest::first(500).bounded(10).limit(), 10);
        let other = page.listing(&at(7), b"q/".to_vec()).unwrap_err();
        assert_eq!(other.code, program::code::STALE);
        assert_eq!(listing(&page).cursor_pin, Some(7));
        let pinned = listing(&PageRequest::first(2)).pinned(3).reply([
            (b"p/3".to_vec(), 3u8),
            (b"p/4".to_vec(), 4),
            (b"p/5".to_vec(), 5),
        ]);
        let page = PageRequest::resume(pinned.next, 2);
        assert_eq!(listing(&page).cursor_pin, Some(3));
        assert_eq!(pinned.height, 7);
        let garbage = PageRequest {
            after: Some(vec![1]),
            limit: None,
        };
        assert_eq!(
            garbage.open(b"p/").unwrap_err().code,
            program::code::INVALID_INPUT
        );
    }

    #[test]
    fn pages_are_bounded_round_trip_and_resume_without_duplicates() {
        let rows: Vec<_> = (0..=255u8)
            .map(|n| (vec![n], n))
            .chain([(vec![255, 0], 0u8)])
            .collect();
        let total = rows.len();
        for limit in [None, Some(u64::MAX), Some(100), Some(0), Some(1)] {
            let page = PageRequest { after: None, limit };
            let encoded = abi::encode(&page);
            let page: PageRequest = abi::decode(&encoded).unwrap();
            let first = listing(&page).reply(rows.clone());
            assert_eq!(first.items.len(), (page.limit() as usize).min(total));
            assert_eq!(first.height, 7);
            let decoded: PageResponse<u8> = abi::decode(&abi::encode(&first)).unwrap();
            assert_eq!(decoded, first);
            let mut all = first.items;
            let mut next = first.next;
            while let Some(after) = next {
                let reply = listing(&PageRequest {
                    after: Some(after),
                    limit,
                })
                .reply(rows.clone());
                assert!(!reply.items.is_empty());
                all.extend(reply.items);
                next = reply.next;
            }
            assert_eq!(all.len(), total);
        }
        assert_eq!(listing(&PageRequest::default()).reply::<u8>([]).next, None);
    }

    #[test]
    fn a_cursor_cannot_escape_its_prefix() {
        let past = PageRequest::default()
            .listing(&at(1), b"c/1/".to_vec())
            .unwrap();
        let page = PageRequest {
            after: Some(past.next(b"a/".to_vec())),
            limit: None,
        };
        let scan = page
            .listing(&at(1), b"c/1/".to_vec())
            .unwrap()
            .scan(b"c/1/");
        assert!(!scan.admits(b"b/2"));
        assert!(scan.admits(b"c/1/2"));
        assert!(!scan.admits(b"c/2/2"));
    }

    #[test]
    fn a_list_pages_by_offset() {
        let values: Vec<u32> = (0..5).collect();
        let at = |after| {
            listing(&PageRequest {
                after,
                limit: Some(2),
            })
        };
        let first = at(None).slice(&values).unwrap();
        assert_eq!(first.items, [0, 1]);
        let second = at(first.next).slice(&values).unwrap();
        assert_eq!(second.items, [2, 3]);
        let last = at(second.next).slice(&values).unwrap();
        assert_eq!((last.items, last.next), (vec![4], None));
        let bad = listing(&PageRequest::default()).next(vec![1]);
        assert!(at(Some(bad)).slice(&values).is_err());
    }
}
