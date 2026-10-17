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
    /// Opens `route`: a tab's own list becomes the root, any other page
    /// goes over the page it was opened from (Expo's `router.push`). The
    /// desktop has no Back to walk the pages under the top: it keeps only
    /// the page shown over the root, so its stack never grows.
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

    /// A block's step to the one before or after: it takes the page's
    /// place, so Back leaves the blocks (Expo's `router.setParams`,
    /// `block/[height].tsx:57`).
    pub(crate) fn step(&mut self, height: u64, cx: &mut Context<Self>) {
        self.pages.replace(Route::Block(height));
        self.arrive(cx);
    }

    /// The line under the title row goes (its Dismiss), and with it a page
    /// a link opened that shows nothing.
    pub(crate) fn dismiss_note(&mut self, cx: &mut Context<Self>) {
        self.note = None;
        cx.notify();
    }

    /// Back over a drawn sheet: Add, a program's sheet or a form closes.
    pub(crate) fn close_sheet(&mut self, cx: &mut Context<Self>) {
        self.sheet = None;
        self.acting.form = None;
        cx.notify();
    }

    /// Back with no sheet drawn: the top page goes and the one it was
    /// opened from shows, read again. A sheet held but not drawn (its
    /// network still reading) goes with the page, so it never opens over
    /// the one under it.
    pub(crate) fn leave_page(&mut self, cx: &mut Context<Self>) {
        self.acting.form = None;
        self.pages.pop();
        self.arrive(cx);
    }

    /// The top page shows: what it reads past the window is asked, and
    /// what belonged to the page before goes.
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
        // the field holds only what is being typed: a search that lands, a
        // tab, prev/next and a row all leave it empty
        self.search.reset("");
        cx.notify();
    }

    /// A height, a block or transaction hash, an account (`#3` or a name)
    /// or a program name.
    pub(crate) fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.text().trim().to_string();
        // the line before goes now, though a hash the node is asked for
        // lands later
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

    /// A hash, as search and a link (`link`, the page it named) read it: a
    /// transaction or a block in the window, else a block or a transaction
    /// the node finds by it.
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

    /// `duck://<chain>/explorer/<route>`, while the session names a chain.
    pub(crate) fn link(&self, route: &Route) -> Option<String> {
        let path = route.path();
        let tail: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        ducklink::mint(&self.session_chain, "explorer", &tail)
    }

    /// The reader's DM room with `account` in chat, as members-view opens
    /// it: for a person or a program account other than the reader, once
    /// the session names the reader and the chain.
    pub(crate) fn dm_link(&self, account: &identity::Account) -> Option<String> {
        let me = self.acting.account?;
        let reachable = matches!(
            account.kind(),
            identity::Kind::User | identity::Kind::Program { .. }
        );
        if !reachable || me == account.number {
            return None;
        }
        let room = chat::dm_channel_id(me, account.number);
        ducklink::mint(&self.session_chain, chat::PROGRAM, &[&room])
    }

    /// `text` onto the clipboard; the note says `what` was copied.
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
