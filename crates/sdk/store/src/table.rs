//! Typed tables over a context: `const` descriptors whose methods take the
//! context. `const ACCOUNTS: Map<AccountNumber, Account> = Map::new("a");`

use std::marker::PhantomData;

use borsh::{BorshDeserialize, BorshSerialize};
use program::{Error, Range};
use program::{ExecCtx, QueryCtx, corrupt};

use crate::key::KeyCodec;
use crate::page::{Listing, PageRequest, PageResponse};

/// A table of `V` rows keyed by `K`, every key under one `prefix` of this
/// program's state. Declare it once as a `const`; each method takes the
/// context. Two tables' prefixes must not be prefixes of each other
/// (`"m/"` beside `"m/x/"` would read each other's rows).
pub struct Map<K, V> {
    prefix: &'static str,
    _types: PhantomData<fn() -> (K, V)>,
}

// What a table's rows are: the value each holds, and the kind a write claims each row as (none for
// a plain value, whose rows are shared).
pub trait Row {
    type Value: BorshSerialize + BorshDeserialize;
    const KIND: Option<&'static str>;
}

impl<V: BorshSerialize + BorshDeserialize> Row for V {
    type Value = V;
    const KIND: Option<&'static str> = None;
}

// A resource kind: the stable name a user's defaults and policies know a table's rows by
// (`chat.message`).
pub trait Kind {
    const KIND: &'static str;
}

// The rows of a table of owned `T`s: `Map<K, Owned<T>>` reads and writes `T` itself, the same
// bytes, and each put claims its row as `T`'s kind, so the row a put creates is its writer's. A row type another crate defines (an abi
// role's) names its kind through a marker of the program's own: `Owned<ServiceRow, Service>`.
pub struct Owned<T, K = T>(PhantomData<fn() -> (T, K)>);

impl<T: BorshSerialize + BorshDeserialize, K: Kind> Row for Owned<T, K> {
    type Value = T;
    const KIND: Option<&'static str> = Some(K::KIND);
}

impl<K: KeyCodec, V: Row> Map<K, V> {
    pub const fn new(prefix: &'static str) -> Self {
        Map {
            prefix,
            _types: PhantomData,
        }
    }

    // The kind each put claims its row as: `Owned<T>`'s, none for a plain value.
    pub const fn kind(&self) -> Option<&'static str> {
        V::KIND
    }

    /// The table's prefix followed by `head`: a full key, or the leading
    /// elements of a tuple key.
    pub fn key<H: KeyCodec>(&self, head: &H) -> Vec<u8> {
        let mut bytes = self.prefix.as_bytes().to_vec();
        head.encode_key(&mut bytes);
        bytes
    }

    /// Every key whose leading elements are `head` (a tuple key's prefix).
    pub fn prefix_of<H: KeyCodec>(&self, head: &H) -> Range {
        Range::prefix(self.key(head))
    }

    /// Every key whose leading elements sort before `head`.
    pub fn below<H: KeyCodec>(&self, head: &H) -> Range {
        let mut hi = self.prefix.as_bytes().to_vec();
        head.encode_key(&mut hi);
        Range::new(self.prefix.as_bytes().to_vec(), Some(hi))
    }

    /// The row at `key`; a row that does not decode is `code::CORRUPT`.
    pub fn get(&self, ctx: &QueryCtx, key: &K) -> Result<Option<V::Value>, Error> {
        let bytes = self.key(key);
        ctx.get(&bytes)
            .map(|value| decode_value(self.prefix, &bytes, &value))
            .transpose()
    }

    /// Whether a row is at `key`, without decoding it.
    pub fn has(&self, ctx: &QueryCtx, key: &K) -> bool {
        ctx.get(self.key(key)).is_some()
    }

    /// Writes the row at `key`, replacing any.
    pub fn put(&self, ctx: &ExecCtx, key: &K, value: &V::Value) {
        ctx.set_with(self.key(key), abi::encode(value), V::KIND);
    }

    /// Removes the row at `key` (a missing row is no error).
    pub fn remove(&self, ctx: &ExecCtx, key: &K) {
        ctx.delete(self.key(key));
    }

    /// The rows a scan admits, keys decoded back. The scan comes from
    /// `prefix_of`, `below`, or `PageRequest::scan` over `self.prefix()`.
    pub fn scan(&self, ctx: &QueryCtx, scan: Range) -> Result<Vec<(K, V::Value)>, Error> {
        ctx.scan(scan)
            .into_iter()
            .map(|entry| {
                let key = decode_key(self.prefix, &entry.key)?;
                let value = decode_value(self.prefix, &entry.key, &entry.value)?;
                Ok((key, value))
            })
            .collect()
    }

    /// Every row, in key order. Unbounded: a query a client asks answers a
    /// page instead ([`Map::range`]).
    pub fn all(&self, ctx: &QueryCtx) -> Result<Vec<(K, V::Value)>, Error> {
        self.scan(ctx, Range::prefix(self.prefix))
    }

    /// One page of the table in key order, resumable through
    /// `PageResponse::next`, answered at the ctx's height.
    pub fn range(
        &self,
        ctx: &QueryCtx,
        page: &PageRequest,
    ) -> Result<PageResponse<(K, V::Value)>, Error> {
        self.range_of(ctx, &(), page)
    }

    /// One page of the keys whose leading elements are `head`.
    pub fn range_of<H: KeyCodec>(
        &self,
        ctx: &QueryCtx,
        head: &H,
        page: &PageRequest,
    ) -> Result<PageResponse<(K, V::Value)>, Error> {
        let listing = page.listing(ctx, self.key(head))?;
        self.page_of(ctx, head, &listing)
    }

    /// One page of the keys whose leading elements are `head`, over a
    /// listing the program opened itself (one whose cursors are bound to
    /// more than the prefix: the whole query, a height).
    pub fn page_of<H: KeyCodec>(
        &self,
        ctx: &QueryCtx,
        head: &H,
        listing: &Listing,
    ) -> Result<PageResponse<(K, V::Value)>, Error> {
        let rows = ctx
            .scan(listing.scan_ahead(&self.key(head)))
            .into_iter()
            .map(|entry| {
                let key = decode_key(self.prefix, &entry.key)?;
                let value = decode_value(self.prefix, &entry.key, &entry.value)?;
                Ok((entry.key, (key, value)))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(listing.reply(rows))
    }

    pub fn prefix(&self) -> &'static str {
        self.prefix
    }

    pub fn batch(
        &self,
        ctx: &QueryCtx,
        after: Option<&[u8]>,
        limit: u64,
    ) -> Result<Batch<K, V::Value>, Error> {
        let mut range = Range::prefix(self.prefix);
        if let Some(after) = after {
            range = range.after(after);
        }
        let rows = self.scan(ctx, range.limit(limit))?;
        let full = limit > 0 && rows.len() as u64 == limit;
        let next = rows.last().filter(|_| full).map(|(key, _)| self.key(key));
        Ok(Batch { rows, next })
    }

    /// Whether `key`, as a block's writes list it, is a row of this table.
    pub fn owns(&self, key: &[u8]) -> bool {
        key.starts_with(self.prefix.as_bytes())
    }
}

// One batch of a table's rows in key order after the raw key `after`, at most `limit` of them, and
// where the next batch starts: the last row's raw key when this one is full.
pub struct Batch<K, V> {
    pub rows: Vec<(K, V)>,
    pub next: Option<Vec<u8>>,
}

/// A [`Map`] with no values: which keys are present.
pub struct Set<K> {
    map: Map<K, ()>,
}

impl<K: KeyCodec> Set<K> {
    pub const fn new(prefix: &'static str) -> Self {
        Set {
            map: Map::new(prefix),
        }
    }

    pub fn key<H: KeyCodec>(&self, head: &H) -> Vec<u8> {
        self.map.key(head)
    }

    pub fn prefix_of<H: KeyCodec>(&self, head: &H) -> Range {
        self.map.prefix_of(head)
    }

    pub fn has(&self, ctx: &QueryCtx, key: &K) -> bool {
        self.map.has(ctx, key)
    }

    pub fn insert(&self, ctx: &ExecCtx, key: &K) {
        self.map.put(ctx, key, &());
    }

    pub fn remove(&self, ctx: &ExecCtx, key: &K) {
        self.map.remove(ctx, key);
    }

    pub fn scan(&self, ctx: &QueryCtx, scan: Range) -> Result<Vec<K>, Error> {
        Ok(self
            .map
            .scan(ctx, scan)?
            .into_iter()
            .map(|(k, ())| k)
            .collect())
    }

    pub fn all(&self, ctx: &QueryCtx) -> Result<Vec<K>, Error> {
        self.scan(ctx, Range::prefix(self.map.prefix))
    }

    pub fn range(&self, ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<K>, Error> {
        Ok(self.map.range(ctx, page)?.map(|(k, ())| k))
    }

    pub fn range_of<H: KeyCodec>(
        &self,
        ctx: &QueryCtx,
        head: &H,
        page: &PageRequest,
    ) -> Result<PageResponse<K>, Error> {
        Ok(self.map.range_of(ctx, head, page)?.map(|(k, ())| k))
    }

    pub fn page_of<H: KeyCodec>(
        &self,
        ctx: &QueryCtx,
        head: &H,
        listing: &Listing,
    ) -> Result<PageResponse<K>, Error> {
        Ok(self.map.page_of(ctx, head, listing)?.map(|(k, ())| k))
    }

    pub fn prefix(&self) -> &'static str {
        self.map.prefix
    }

    /// Whether `key`, as a block's writes list it, is a row of this set.
    pub fn owns(&self, key: &[u8]) -> bool {
        self.map.owns(key)
    }
}

/// One value at one key of this program's state: a counter, the params
/// genesis gave.
pub struct Item<T> {
    key: &'static str,
    _type: PhantomData<fn() -> T>,
}

impl<T: Row> Item<T> {
    pub const fn new(key: &'static str) -> Self {
        Item {
            key,
            _type: PhantomData,
        }
    }

    /// The value, if one was put.
    pub fn get(&self, ctx: &QueryCtx) -> Result<Option<T::Value>, Error> {
        ctx.get(self.key)
            .map(|value| decode_value(self.key, b"", &value))
            .transpose()
    }

    pub fn put(&self, ctx: &ExecCtx, value: &T::Value) {
        ctx.set_with(self.key.as_bytes().to_vec(), abi::encode(value), T::KIND);
    }

    /// Reads the value (or its default), lets `change` alter it, stores and returns it.
    pub fn update(
        &self,
        ctx: &ExecCtx,
        change: impl FnOnce(&mut T::Value),
    ) -> Result<T::Value, Error>
    where
        T::Value: Default,
    {
        let mut value = self.get(ctx)?.unwrap_or_default();
        change(&mut value);
        self.put(ctx, &value);
        Ok(value)
    }

    /// Whether `key`, as a block's writes list it, is this item.
    pub fn owns(&self, key: &[u8]) -> bool {
        key == self.key.as_bytes()
    }

    /// The one key this item is, as a block's writes list it.
    pub fn key(&self) -> Vec<u8> {
        self.key.as_bytes().to_vec()
    }
}

fn decode_key<K: KeyCodec>(table: &str, raw: &[u8]) -> Result<K, Error> {
    let mut rest = raw.get(table.len()..).unwrap_or_default();
    match K::decode_key(&mut rest) {
        Some(key) if rest.is_empty() => Ok(key),
        _ => Err(corrupt(table, raw, "the key does not decode")),
    }
}

fn decode_value<V: BorshDeserialize>(table: &str, key: &[u8], value: &[u8]) -> Result<V, Error> {
    borsh::from_slice(value).map_err(|fault| corrupt(table, key, fault))
}

#[cfg(test)]
mod tests {
    use super::*;
    use program::MockHost;

    fn exec() -> ExecCtx {
        MockHost::default().exec(MockHost::env("test"))
    }

    const NUMBERS: Map<u64, String> = Map::new("n/");
    const PAIRS: Map<(u64, String), u8> = Map::new("p/");
    const SEEN: Set<Vec<u8>> = Set::new("s/");
    const NEXT: Item<u64> = Item::new("next");

    #[test]
    fn tables_round_trip_scan_in_key_order_and_refuse_corrupt_rows() {
        let ctx = exec();
        NUMBERS.put(&ctx, &10, &"ten".into());
        NUMBERS.put(&ctx, &2, &"two".into());
        assert_eq!(NUMBERS.get(&ctx, &2).unwrap().as_deref(), Some("two"));
        assert!(NUMBERS.has(&ctx, &10) && !NUMBERS.has(&ctx, &3));
        let keys: Vec<u64> = NUMBERS
            .all(&ctx)
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, [2, 10], "numeric, not lexical");
        NUMBERS.remove(&ctx, &2);
        assert_eq!(NUMBERS.all(&ctx).unwrap().len(), 1);

        PAIRS.put(&ctx, &(1, "b".into()), &1);
        PAIRS.put(&ctx, &(1, "a".into()), &2);
        PAIRS.put(&ctx, &(2, "a".into()), &3);
        let under_one = PAIRS.scan(&ctx, PAIRS.prefix_of(&1u64)).unwrap();
        assert_eq!(under_one.len(), 2);
        let paged = PAIRS.range_of(&ctx, &1u64, &PageRequest::first(1)).unwrap();
        assert_eq!((paged.items.len(), paged.next.is_some()), (1, true));
        assert_eq!(under_one[0].0.1, "a");
        assert_eq!(PAIRS.scan(&ctx, PAIRS.below(&2u64)).unwrap().len(), 2);

        SEEN.insert(&ctx, &vec![7]);
        assert!(SEEN.has(&ctx, &vec![7]));
        assert_eq!(SEEN.all(&ctx).unwrap(), [vec![7]]);

        assert_eq!(NEXT.update(&ctx, |n| *n += 1).unwrap(), 1);
        assert_eq!(NEXT.get(&ctx).unwrap(), Some(1));

        ctx.set(b"n/short".to_vec(), abi::encode(&"x".to_string()));
        let refusal = NUMBERS.all(&ctx).unwrap_err();
        assert_eq!(refusal.code, program::code::CORRUPT);
        assert!(refusal.message.starts_with("n/["), "{refusal}");
    }

    #[test]
    fn string_keys_list_by_name_and_a_prefix_is_a_whole_element() {
        const NAMED: Map<(String, u64), ()> = Map::new("x/");
        let ctx = exec();
        for (name, n) in [
            ("general", 1),
            ("abc", 2),
            ("ab", 1),
            ("abc", 1),
            ("docs", 1),
            ("a\0b", 1),
        ] {
            NAMED.put(&ctx, &(name.into(), n), &());
        }
        let order = |scan| -> Vec<(String, u64)> {
            NAMED
                .scan(&ctx, scan)
                .unwrap()
                .into_iter()
                .map(|(k, ())| k)
                .collect()
        };
        let all = order(Range::prefix(NAMED.prefix()));
        let names: Vec<&str> = all.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["a\0b", "ab", "abc", "abc", "docs", "general"]);
        assert_eq!(all[2..4], [("abc".into(), 1), ("abc".into(), 2)]);
        assert_eq!(
            order(NAMED.prefix_of(&"ab".to_string())),
            [("ab".into(), 1)]
        );
        assert_eq!(order(NAMED.prefix_of(&"a".to_string())), []);
        assert_eq!(order(NAMED.below(&"abc".to_string())).len(), 2);
    }

    #[test]
    fn a_range_pages_with_lookahead() {
        let host = MockHost::default();
        let ctx = host.exec(program::Env {
            height: 3,
            ..MockHost::env("test")
        });
        for n in 0..5u64 {
            NUMBERS.put(&ctx, &n, &n.to_string());
        }
        let reply = NUMBERS.range(&ctx, &PageRequest::first(2)).unwrap();
        assert_eq!((reply.items.len(), reply.height), (2, 3));
        let page = PageRequest::resume(reply.next, 2);
        let reply = NUMBERS.range(&ctx, &page).unwrap();
        assert_eq!(reply.items[0].0, 2);
        assert!(reply.next.is_some());
        let page = PageRequest::resume(reply.next, 2);
        assert_eq!(NUMBERS.range(&ctx, &page).unwrap().next, None);
    }
}
