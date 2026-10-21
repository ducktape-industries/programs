//! Where a list stood, kept on this device under `scroll/<route>` in the view's store: the row at
//! its top (its id and seq) and how far into that row. A `Keeper` writes it as the list settles,
//! one write out at a time, and the newest anchor of each route after it.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::rc::Rc;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::host::{Error, Host};
use crate::methods::{self, StoreSet};
use crate::{App, AppContext};

#[derive(Clone, Debug, PartialEq, BorshSerialize, BorshDeserialize)]
pub struct Anchor {
    pub id: String,
    pub seq: u64,
    pub offset: f32,
}

pub fn key(route: &str) -> String {
    format!("scroll/{route}")
}

pub fn get(
    host: &Host,
    route: &str,
) -> impl Future<Output = Result<Option<Anchor>, Error>> + 'static + use<> {
    crate::store::get(host, &key(route))
}

#[derive(Clone, Default)]
pub struct Keeper(Rc<RefCell<Keeping>>);

#[derive(Default)]
struct Keeping {
    held: BTreeMap<String, Option<Anchor>>,
    writing: Option<(String, Option<Anchor>)>,
    due: BTreeMap<String, Option<Anchor>>,
}

impl Keeping {
    fn holds(&self, route: &str) -> Option<&Option<Anchor>> {
        match &self.writing {
            Some((writing, anchor)) if writing == route => Some(anchor),
            _ => self.held.get(route),
        }
    }
}

impl Keeper {
    pub fn known(&self, route: &str, anchor: Option<Anchor>) {
        let mut keeping = self.0.borrow_mut();
        let own = keeping.holds(route).is_some() || keeping.due.contains_key(route);
        if !own {
            keeping.held.insert(route.to_owned(), anchor);
        }
    }

    pub fn keep(&self, app: &App, route: &str, anchor: Option<Anchor>) {
        let mut keeping = self.0.borrow_mut();
        match keeping.holds(route) == Some(&anchor) {
            true => keeping.due.remove(route),
            false => keeping.due.insert(route.to_owned(), anchor),
        };
        drop(keeping);
        self.next(app);
    }

    fn next(&self, app: &App) {
        let mut keeping = self.0.borrow_mut();
        if keeping.writing.is_some() {
            return;
        }
        let Some((route, anchor)) = keeping.due.pop_first() else {
            return;
        };
        keeping.writing = Some((route.clone(), anchor.clone()));
        drop(keeping);
        let value = anchor.as_ref().map(methods::encode);
        let written = app.host().ask::<StoreSet>((key(&route), value));
        let keeper = self.clone();
        app.spawn(async move |cx| {
            let landed = written.await;
            let _ = cx.with_app(|app| keeper.landed(app, landed));
        })
        .detach();
    }

    fn landed(&self, app: &App, landed: Result<(), Error>) {
        let mut keeping = self.0.borrow_mut();
        let Some((route, anchor)) = keeping.writing.take() else {
            return;
        };
        match landed {
            Ok(()) => {
                keeping.held.insert(route, anchor);
            }
            Err(refusal) => {
                keeping.held.remove(&route);
                app.host()
                    .log(format!("where {route} stood is not kept: {refusal}"));
            }
        }
        drop(keeping);
        self.next(app);
    }
}

#[cfg(test)]
mod tests;
