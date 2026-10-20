//! Where the reader goes: a tab or a row, a search, a link opened into the
//! explorer, and a page's link copied out of it.
use view::design::Pages;
use view::prelude::*;

use crate::Explorer;
use crate::chain::rows;
use crate::state::{Linked, Note, Route, hash_of};

enum Found {
    Block,
    Tx { position: u32 },
}

impl Explorer {
    pub(crate) fn go(&mut self, route: Route, cx: &mut Context<Self>) {
        if route.listed() {
            self.pages = Pages::root(route);
        } else {
            if !self.touch {
                self.pages = Pages::root(self.pages.first().clone());
            }
            self.pages.push(route);
        }
        self.arrive(cx);
    }

    pub(crate) fn step(&mut self, height: u64, cx: &mut Context<Self>) {
        self.pages.replace(Route::Block(height));
        self.arrive(cx);
    }

    pub(crate) fn dismiss_note(&mut self, cx: &mut Context<Self>) {
        self.note = None;
        cx.notify();
    }

    pub(crate) fn close_sheet(&mut self, cx: &mut Context<Self>) {
        self.sheet = None;
        self.acting.form = None;
        cx.notify();
    }

    pub(crate) fn leave_page(&mut self, cx: &mut Context<Self>) {
        self.acting.form = None;
        self.pages.pop();
        self.arrive(cx);
    }

    fn arrive(&mut self, cx: &mut Context<Self>) {
        let top = self.pages.top();
        self.left[top.tab()] = Some(top.clone());
        self.read_page(cx);
        self.start_over(cx);
        self.read_acted(cx);
        self.note = None;
        self.cursor = None;
        self.searching = false;
        self.sheet = None;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.search.reset("");
        cx.notify();
    }

    pub(crate) fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.text().trim().to_string();
        self.note = None;
        cx.notify();
        if query.is_empty() {
            return;
        }
        let digits: String = query.chars().filter(|c| *c != ',').collect();
        if let Ok(height) = digits.parse::<u64>() {
            return self.go(Route::Block(height), cx);
        }
        if let Some(hash) = hash_of(&query) {
            return self.find_hash(hash, None, cx);
        }
        let number = query
            .strip_prefix('#')
            .and_then(|n| n.trim().parse::<u64>().ok());
        let lower = query.to_lowercase();
        let accounts = self.account_list();
        let named = |account: &&identity::Account| account.card.name.to_lowercase() == lower;
        let account = accounts
            .iter()
            .find(|account| Some(account.number) == number || named(account))
            .or_else(|| {
                accounts
                    .iter()
                    .find(|account| account.card.name.to_lowercase().contains(&lower))
            });
        if let Some(account) = account {
            return self.go(Route::Account(account.number), cx);
        }
        if self.code_of(&lower).is_some() {
            return self.go(Route::Program(lower), cx);
        }
        self.note = Some(Note::NotFound(query));
        cx.notify();
    }

    fn find_hash(&mut self, hash: [u8; 32], link: Option<Linked>, cx: &mut Context<Self>) {
        if let Some(tx) = self.tx(&hash) {
            let at = Route::Tx {
                height: tx.height,
                position: tx.index,
            };
            return self.go(at, cx);
        }
        if let Some(block) = self.chain.blocks.iter().find(|block| block.id == hash) {
            return self.go(Route::Block(block.height), cx);
        }
        let host = cx.host();
        let found = async move {
            let by_id = BlockAsk {
                by: BlockRef::Id(hash),
                most: 0,
            };
            if let Some(block) = host.ask::<ChainBlock>(by_id).await? {
                return Ok::<_, Error>(Some((block, Found::Block)));
            }
            let by_hash = TxAsk {
                tx: TxRef::Hash(hash),
                most: 0,
            };
            let Some(located) = host.ask::<ChainTx>(by_hash).await? else {
                return Ok(None);
            };
            let carrying = BlockAsk {
                by: BlockRef::Height(located.height),
                most: 0,
            };
            let block = host.ask::<ChainBlock>(carrying).await?;
            let found = Found::Tx {
                position: located.position,
            };
            Ok(block.map(|block| (block, found)))
        };
        let listed_from = self.chain.listed_from;
        cx.land(found, move |view, found, cx| {
            match found {
                Ok(Some((block, found))) => {
                    let (row, txs) = rows(block);
                    let route = match found {
                        Found::Block => Route::Block(row.height),
                        Found::Tx { position } => Route::Tx {
                            height: row.height,
                            position,
                        },
                    };
                    view.opened = Loadable::Ready(Some((row, txs)));
                    view.go(route, cx);
                }
                Ok(None) => view.note = Some(Note::NoSuchHash { listed_from, link }),
                Err(refusal) => view.note = Some(Note::Refused(design::sentence(&refusal))),
            }
            cx.notify();
        })
        .detach();
    }

    pub(crate) fn open_route(&mut self, route: &str, cx: &mut Context<Self>) {
        if let Some(opened) = Route::from_path(route) {
            return self.go(opened, cx);
        }
        let hashed = (route
            .strip_prefix("block/")
            .map(|hash| (hash, Linked::Block)))
        .or_else(|| route.strip_prefix("tx/").map(|hash| (hash, Linked::Tx)))
        .and_then(|(hash, link)| Some((hash_of(hash)?, link)));
        if let Some((hash, link)) = hashed {
            return self.find_hash(hash, Some(link), cx);
        }
        self.note = Some(Note::Unlinked(route.to_owned()));
        cx.notify();
    }

    pub(crate) fn link(&self, route: &Route) -> Option<String> {
        let path = route.path();
        let tail: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        ducklink::mint(&self.session_chain, "explorer", &tail)
    }

    pub(crate) fn dm_link(&self, account: &identity::Account) -> Option<String> {
        let me = self.acting.account?;
        if me == account.number {
            return None;
        }
        let room = chat::dm_channel_id(me, account.number);
        ducklink::mint(&self.session_chain, chat::PROGRAM, &[&room])
    }

    pub(crate) fn copy(&mut self, text: String, what: &'static str, cx: &mut Context<Self>) {
        let ask = cx.host().ask::<ClipboardWrite>(text);
        cx.land(ask, move |view, copied, cx| {
            view.note = Some(match copied {
                Ok(()) => Note::Copied(what.to_owned()),
                Err(refusal) => Note::Refused(design::sentence(&refusal)),
            });
            cx.notify();
        })
        .detach();
    }
}
