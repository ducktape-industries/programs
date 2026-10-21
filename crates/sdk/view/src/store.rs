//! What a view keeps on this device between runs (`store`): its own keys,
//! borsh-encoded, apart per program, per network and per seated key; no
//! other program reads them. `cx.store()` is the handle.
use std::future::Future;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::host::{Error, Host, malformed};
use crate::methods::{self, StoreGet, StoreList, StoreSecretGet, StoreSecretSet, StoreSet};

/// The value kept under `key`, or `None` where nothing is.
pub fn get<T: BorshDeserialize>(
    host: &Host,
    key: &str,
) -> impl Future<Output = Result<Option<T>, Error>> + 'static + use<T> {
    let asked = host.ask::<StoreGet>(key.to_owned());
    async move {
        asked
            .await?
            .map(|bytes| methods::decode(&bytes).map_err(malformed))
            .transpose()
    }
}

/// Keep `value` under `key`; `None` drops it. Nobody waits for the answer.
pub fn set<T: BorshSerialize>(host: &Host, key: &str, value: Option<&T>) {
    host.notify::<StoreSet>((key.to_owned(), value.map(methods::encode)));
}

// Drop what is kept under `key`. Nobody waits for the answer.
pub fn remove(host: &Host, key: &str) {
    host.notify::<StoreSet>((key.to_owned(), None));
}

// The keys kept under `prefix` (empty: every key), in byte order. A host
// older than `store.list` refuses it `unknown_request`: read that as
// nothing kept, and go on with the defaults.
pub fn list(
    host: &Host,
    prefix: &str,
) -> impl Future<Output = Result<Vec<String>, Error>> + 'static + use<> {
    host.ask::<StoreList>(prefix.to_owned())
}

pub fn secret_get<T: BorshDeserialize>(
    host: &Host,
    key: &str,
) -> impl Future<Output = Result<Option<T>, Error>> + 'static + use<T> {
    let asked = host.ask::<StoreSecretGet>(key.to_owned());
    async move {
        asked
            .await?
            .map(|bytes| methods::decode(&bytes).map_err(malformed))
            .transpose()
    }
}

pub fn secret_set<T: BorshSerialize>(
    host: &Host,
    key: &str,
    value: Option<&T>,
) -> impl Future<Output = Result<(), Error>> + 'static + use<T> {
    host.ask::<StoreSecretSet>((key.to_owned(), value.map(methods::encode)))
}

// The view's store, as `cx.store()` hands it: the functions above, on the
// view's host.
#[derive(Clone)]
pub struct Store(pub(crate) Host);

impl Store {
    pub fn get<T: BorshDeserialize>(
        &self,
        key: &str,
    ) -> impl Future<Output = Result<Option<T>, Error>> + 'static + use<T> {
        get(&self.0, key)
    }

    pub fn set<T: BorshSerialize>(&self, key: &str, value: &T) {
        set(&self.0, key, Some(value));
    }

    pub fn remove(&self, key: &str) {
        remove(&self.0, key);
    }

    pub fn list(
        &self,
        prefix: &str,
    ) -> impl Future<Output = Result<Vec<String>, Error>> + 'static + use<> {
        list(&self.0, prefix)
    }

    pub fn secret_get<T: BorshDeserialize>(
        &self,
        key: &str,
    ) -> impl Future<Output = Result<Option<T>, Error>> + 'static + use<T> {
        secret_get(&self.0, key)
    }

    pub fn secret_set<T: BorshSerialize>(
        &self,
        key: &str,
        value: Option<&T>,
    ) -> impl Future<Output = Result<(), Error>> + 'static + use<T> {
        secret_set(&self.0, key, value)
    }
}
