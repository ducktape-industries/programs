//! A room's tag filters, its `filters` meta: a chip per tag over the timeline, each showing only
//! the room's posts carrying that tag, read through chat's `TagSearch` a screenful a page.
use chat::MsgRow;
use chat::view::Written;
use view::Paged;
use view::prelude::*;

use crate::{Chat, queries};

pub(crate) const FILTERS: &str = "filters";

pub(crate) struct Filter {
    pub(crate) tag: String,
    pub(crate) rows: Vec<MsgRow>,
    pub(crate) more: bool,
    pub(crate) more_out: bool,
    pub(crate) loading: bool,
    pub(crate) failed: Option<Error>,
    pages: Entity<Paged<MsgRow>>,
    _landed: Subscription,
}

impl Chat {
    pub(crate) fn room_filters(&self) -> Vec<String> {
        let Some(value) = self.room_info().and_then(|info| info.meta.get(FILTERS)) else {
            return Vec::new();
        };
        let named = value
            .split(',')
            .map(|tag| tag.trim().trim_start_matches('#').trim());
        let mut tags: Vec<String> = Vec::new();
        for tag in named.filter(|tag| !tag.is_empty()) {
            if !tags.iter().any(|held| held == tag) {
                tags.push(tag.to_owned());
            }
        }
        tags
    }

    pub(crate) fn filtered(&self) -> Option<&Filter> {
        self.room.as_ref()?.filter.as_ref()
    }

    pub(crate) fn older_loading(&self) -> bool {
        match (self.room.as_ref(), self.filtered()) {
            (_, Some(filter)) => filter.more_out,
            (Some(room), None) => room.older_loading,
            (None, None) => false,
        }
    }

    pub(crate) fn pick_filter(&mut self, tag: Option<String>, cx: &mut Context<Self>) {
        let held = self.filtered().map(|filter| filter.tag.clone());
        if held == tag {
            return;
        }
        let (host, viewer, screenful) = (cx.host(), self.viewer(), self.screenful());
        let Some(room) = &mut self.room else { return };
        let channel = room.id.clone();
        room.filter = tag.map(|tag| {
            let asked = tag.clone();
            let page = move |after| {
                let (host, asked) = (host.clone(), asked.clone());
                queries::tagged(
                    host,
                    asked,
                    channel.clone(),
                    viewer.clone(),
                    after,
                    screenful,
                )
            };
            let pages = cx.new(|cx| Paged::new(page, cx));
            let landed = cx.observe(&pages, |chat, _, cx| chat.filter_landed(cx));
            Filter {
                tag,
                rows: Vec::new(),
                more: false,
                more_out: false,
                loading: true,
                failed: None,
                pages,
                _landed: landed,
            }
        });
        cx.notify();
    }

    pub(crate) fn settle_filter(&mut self, cx: &mut Context<Self>) {
        let Some(tag) = self.filtered().map(|filter| filter.tag.clone()) else {
            return;
        };
        if self.room_filters().contains(&tag) {
            return;
        }
        if let Some(room) = &mut self.room {
            room.filter = None;
        }
        cx.notify();
    }

    pub(crate) fn filter_more(&mut self, cx: &mut Context<Self>) {
        let Some(filter) = self.room.as_mut().and_then(|room| room.filter.as_mut()) else {
            return;
        };
        let asks = filter.more && !filter.more_out;
        if !asks {
            return;
        }
        filter.more_out = true;
        let held = filter.rows.len();
        filter
            .pages
            .update(cx, |pages, cx| pages.show(held..held + 1, cx));
        cx.notify();
    }

    pub(crate) fn reread_filter(&mut self, change: Option<&Change>, cx: &mut Context<Self>) {
        let Some(room) = &self.room else { return };
        let Some(filter) = &room.filter else { return };
        let wrote = change.is_none_or(|change| {
            chat::view::messages_written(&change.keys, &room.id) != Written::default()
        });
        if wrote {
            filter.pages.update(cx, |pages, cx| pages.reread(cx));
        }
    }

    fn filter_landed(&mut self, cx: &mut Context<Self>) {
        let Some(room) = &mut self.room else { return };
        let Some(filter) = &mut room.filter else {
            return;
        };
        let (rows, more, loading, failed) = filter.pages.read(|pages| {
            let rows: Vec<MsgRow> = pages.rows().iter().rev().cloned().collect();
            let more = pages.count() > pages.rows().len();
            (rows, more, pages.is_loading(), pages.failed().cloned())
        });
        let reacted = rows.iter().filter(|row| !row.reactions.is_empty());
        let seqs: Vec<u64> = reacted.map(|row| row.seq).collect();
        filter.rows = rows;
        filter.more = more;
        filter.loading = loading;
        filter.failed = failed;
        filter.more_out = false;
        room.read_reactors_of(seqs, cx);
        cx.notify();
    }
}
