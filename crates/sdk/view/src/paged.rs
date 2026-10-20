//! A cursored listing read as far as its list is scrolled.
use std::future::Future;
use std::ops::Range;
use std::rc::Rc;

use futures::FutureExt;
use futures::future::LocalBoxFuture;

use crate::host::{Error, Landed, Page, walk};
use crate::{Context, Task};

type Ask<T> = Rc<dyn Fn(Option<Vec<u8>>) -> LocalBoxFuture<'static, Result<Page<T>, Error>>>;

/// A cursored listing, held a page at a time: the rows read so far and the
/// cursor of the page after them. The list that draws it says which rows it
/// shows ([`show`](Self::show)), and a page is asked for when they reach
/// past the rows held, so a long history costs the pages someone scrolled
/// to. [`host::all_pages`](crate::host::all_pages) is the other shape:
/// every page at once, for a list that is whole by nature.
///
/// It is an entity (`cx.new(|cx| Paged::new(ask, cx))`): it reads into
/// itself, notifies when its rows change, and dropping its last handle
/// cancels the read on its way. One read is out at a time; a newer one
/// supersedes it.
///
/// A program that rewrites its listing refuses a cursor it handed out
/// before the write as `stale` (`error::code::STALE`). The listing is then
/// read from its first page again, through the page the list reached, and
/// the rows on screen stay until it lands. Any other refusal lands in place
/// of the rows ([`failed`](Self::failed)).
///
/// ```
/// # use serde::{Deserialize, Serialize};
/// # use std::ops::Range;
/// use view::{Paged, View, prelude::*, testing::TestAppContext, uniform_list};
/// # use view::methods::{Capability, Program, Query};
/// # struct Numbers;
/// # impl Program for Numbers {
/// #     const NAME: &'static str = "numbers";
/// #     type Op = ();
/// #     type Query = Option<Vec<u8>>;
/// #     type Reply = (Vec<u64>, Option<Vec<u8>>);
/// #     fn execute(_: &program::ExecCtx, _: ()) -> Result<(), program::Error> {
/// #         unreachable!()
/// #     }
/// #     fn query(_: &program::QueryCtx, _: Self::Query) -> Result<Self::Reply, program::Error> {
/// #         unreachable!()
/// #     }
/// # }
///
/// #[derive(Default, Serialize, Deserialize)]
/// struct History {
///     #[serde(skip)]
///     rows: Option<Entity<Paged<u64>>>,
/// }
/// impl View for History {
///     const NAME: &'static str = "History";
/// #   const CAPABILITIES: &'static [Capability] = &[Capability::Program];
///     fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
///         let host = cx.host();
///         // one page of the listing, from a cursor: its rows and the next cursor
///         let page = move |after| host.ask::<Query<Numbers>>(after);
///         self.rows = Some(cx.new(|cx| Paged::new(page, cx)));
///     }
/// }
/// impl Render for History {
///     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
///         let rows = self.rows.clone().expect("attach built it");
///         if let Some(refusal) = rows.read(|rows| rows.failed().cloned()) {
///             return div().child(refusal.message).into_any_element();
///         }
///         if rows.read(Paged::is_loading) {
///             return div().child("Reading…").into_any_element();
///         }
///         // the rows held, and a row more while the listing goes on
///         let count = rows.read(Paged::count);
///         uniform_list("history", count, move |range: Range<usize>, _, cx| {
///             rows.update(cx, |rows, cx| rows.show(range.clone(), cx));
///             rows.read(|rows| {
///                 range
///                     .map(|index| match rows.rows().get(index) {
///                         Some(row) => div().child(format!("row {row}")),
///                         None => div().child("Reading…"),
///                     })
///                     .collect::<Vec<_>>()
///             })
///         })
///         .size_full()
///         .into_any_element()
///     }
/// }
///
/// let mut cx = TestAppContext::new();
/// // a listing of 0..200, fifty rows a page
/// cx.host().handle::<Query<Numbers>>(|after| {
///     let start = after.map_or(0, |cursor| u64::from(cursor[0]));
///     let next = (start + 50 < 200).then(|| vec![start as u8 + 50]);
///     Ok(((start..start + 50).collect(), next))
/// });
/// cx.open::<History>();
/// assert_eq!(cx.host().requests::<Query<Numbers>>(), [None], "the first page alone");
/// cx.simulate_range("history", 40..50);
/// assert!(cx.has_text("row 50"), "the list reached the end of the page: the next is read");
/// assert_eq!(cx.host().requests::<Query<Numbers>>().len(), 2);
///
/// // a listing the program refuses draws its refusal
/// let mut cx = TestAppContext::new();
/// cx.host().refuse::<Query<Numbers>>("not_found", "no such listing");
/// cx.open::<History>();
/// assert!(cx.has_text("no such listing"));
/// ```
pub struct Paged<T> {
    ask: Ask<T>,
    rows: Vec<T>,
    next: Option<Vec<u8>>,
    pages: usize,
    reading: Option<(usize, Task<()>)>,
    failed: Option<Error>,
}

impl<T> Paged<T> {
    /// The rows read so far. A list of them has [`count`](Self::count)
    /// rows: the one past these is the page on its way.
    pub fn rows(&self) -> &[T] {
        &self.rows
    }

    /// How many rows a list of this shows: the rows held, and one more
    /// while the listing goes on, which the list draws as loading.
    pub fn count(&self) -> usize {
        self.rows.len() + usize::from(self.next.is_some())
    }

    /// Nothing is read yet: the first page is on its way.
    pub fn is_loading(&self) -> bool {
        self.pages == 0 && self.failed.is_none()
    }

    /// The refusal in place of the rows: the listing was refused, and is
    /// read again by [`reread`](Self::reread).
    pub fn failed(&self) -> Option<&Error> {
        self.failed.as_ref()
    }
}

impl<T: PartialEq + 'static> Paged<T> {
    /// A listing `ask` reads a page of: the page after a cursor (`None`,
    /// the first), as [`host::all_pages`](crate::host::all_pages) asks it.
    /// The first page is asked for now.
    pub fn new<F>(ask: impl Fn(Option<Vec<u8>>) -> F + 'static, cx: &mut Context<Self>) -> Self
    where
        F: Future<Output = Result<Page<T>, Error>> + 'static,
    {
        let mut paged = Paged {
            ask: Rc::new(move |after| ask(after).boxed_local()),
            rows: Vec::new(),
            next: None,
            pages: 0,
            reading: None,
            failed: None,
        };
        paged.read(1, None, cx);
        paged
    }

    /// The list shows `rows` (the range a `uniform_list` hands its
    /// processor): when they reach past the rows held and the listing goes
    /// on, its next page is read, once. The processor is also handed the
    /// row the list measures its width from, on screen or not: a list
    /// measured from its last row (`with_width_from_item`) reads every
    /// page as it opens.
    pub fn show(&mut self, rows: Range<usize>, cx: &mut Context<Self>) {
        if rows.end <= self.rows.len() {
            return;
        }
        let Some(after) = self.next.clone() else {
            return;
        };
        let wanted = self.pages + 1;
        match &self.reading {
            Some((pages, _)) if *pages >= wanted => {}
            Some(_) => self.read(wanted, None, cx),
            None => self.read(wanted, Some(after), cx),
        }
    }

    /// Reads the listing again from its start, as far as it was read (the
    /// page on its way included): what a view does when a
    /// [`Change`](crate::methods::Change) touches it. The rows held stay
    /// until the answer lands, and rows that land the same draw nothing; a
    /// refusal lands in their place ([`failed`](Self::failed)).
    pub fn reread(&mut self, cx: &mut Context<Self>) {
        let reading = self.reading.as_ref().map_or(0, |(pages, _)| *pages);
        self.read(self.pages.max(reading).max(1), None, cx);
    }

    fn read(&mut self, pages: usize, after: Option<Vec<u8>>, cx: &mut Context<Self>) {
        let ask = self.ask.clone();
        let task = cx.spawn(async move |this, cx| {
            let landed = walk(|after| ask(after), pages, after).await;
            let _ = this.update(cx, |paged, cx| paged.land(landed, cx));
        });
        self.reading = Some((pages, task));
    }

    fn land(&mut self, landed: Result<Landed<T>, Error>, cx: &mut Context<Self>) {
        self.reading = None;
        match landed {
            Ok(Landed::Next((rows, next))) => {
                self.rows.extend(rows);
                self.next = next;
                self.pages += 1;
                cx.notify();
            }
            Ok(Landed::Again((rows, next), pages)) => {
                let same =
                    self.pages > 0 && self.rows == rows && self.next.is_some() == next.is_some();
                (self.rows, self.next, self.pages) = (rows, next, pages);
                if self.failed.take().is_some() || !same {
                    cx.notify();
                }
            }
            Err(refusal) => {
                if self.failed.as_ref() != Some(&refusal) {
                    cx.notify();
                }
                (self.rows, self.next, self.pages) = (Vec::new(), None, 0);
                self.failed = Some(refusal);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Paged;
    use crate::host::Page;
    use crate::methods::{self, Capability, Program, Query};
    use crate::testing::TestAppContext;
    use crate::wire::Event;
    use crate::{Context, Entity, IntoElement, ParentElement, Render, Styled, View, Window};
    use serde::{Deserialize, Serialize};
    use std::cell::Cell;
    use std::ops::Range;
    use std::rc::Rc;

    struct Numbers;
    impl Program for Numbers {
        const NAME: &'static str = "numbers";
        type Op = ();
        type Query = Option<Vec<u8>>;
        type Reply = Page<u64>;

        fn execute(_: &program::ExecCtx, _: Self::Op) -> Result<(), program::Error> {
            unreachable!()
        }

        fn query(_: &program::QueryCtx, _: Self::Query) -> Result<Self::Reply, program::Error> {
            unreachable!()
        }
    }

    #[derive(Default, Serialize, Deserialize)]
    struct History {
        #[serde(skip)]
        rows: Option<Entity<Paged<u64>>>,
    }
    impl History {
        fn read(&mut self, cx: &mut Context<Self>) {
            let host = cx.host();
            let page = move |after| host.ask::<Query<Numbers>>(after);
            self.rows = Some(cx.new(|cx| Paged::new(page, cx)));
        }
    }
    impl View for History {
        const NAME: &'static str = "History";
        const CAPABILITIES: &'static [Capability] = &[Capability::Program];
        fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
            self.read(cx);
        }
    }
    impl Render for History {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let rows = self.rows.clone().expect("attach built it");
            if let Some(refusal) = rows.read(|rows| rows.failed().cloned()) {
                return crate::div().child(refusal.message).into_any_element();
            }
            let count = rows.read(Paged::count);
            crate::uniform_list("history", count, move |range: Range<usize>, _, cx| {
                rows.update(cx, |rows, cx| rows.show(range.clone(), cx));
                rows.read(|rows| {
                    range
                        .map(|index| match rows.rows().get(index) {
                            Some(row) => crate::div().child(format!("row {row}")),
                            None => crate::div().child("Reading…"),
                        })
                        .collect::<Vec<_>>()
                })
            })
            .size_full()
            .into_any_element()
        }
    }

    #[derive(Clone)]
    struct Listing {
        len: Rc<Cell<u64>>,
        version: Rc<Cell<u8>>,
        write_after_first_page: Rc<Cell<bool>>,
    }
    const PER_PAGE: u64 = 64;

    fn serve(cx: &TestAppContext, len: u64) -> Listing {
        let listing = Listing {
            len: Rc::new(Cell::new(len)),
            version: Rc::default(),
            write_after_first_page: Rc::default(),
        };
        let served = listing.clone();
        cx.host()
            .handle::<Query<Numbers>>(move |after| served.page(after));
        listing
    }

    impl Listing {
        fn page(&self, after: Option<Vec<u8>>) -> Result<Page<u64>, crate::host::Error> {
            let start = match after.as_deref() {
                None => 0,
                Some([version, offset @ ..]) if *version == self.version.get() => {
                    u64::from_be_bytes(offset.try_into().unwrap())
                }
                Some(_) => {
                    return Err(crate::host::Error::new(
                        ::error::code::STALE,
                        "the listing changed; restart it",
                    ));
                }
            };
            let end = (start + PER_PAGE).min(self.len.get());
            let next = (end < self.len.get()).then(|| {
                let mut cursor = vec![self.version.get()];
                cursor.extend(end.to_be_bytes());
                cursor
            });
            if after.is_none() && self.write_after_first_page.take() {
                self.version.set(self.version.get() + 1);
            }
            Ok(((start..end).collect(), next))
        }
    }

    fn asked(cx: &TestAppContext) -> Vec<Option<u64>> {
        cx.host()
            .requests::<Query<Numbers>>()
            .into_iter()
            .map(|after| after.map(|cursor| u64::from_be_bytes(cursor[1..].try_into().unwrap())))
            .collect()
    }

    fn rows(view: &Entity<History>) -> Entity<Paged<u64>> {
        view.read(|history| history.rows.clone().expect("attach built it"))
    }

    #[test]
    fn a_listing_reads_the_pages_its_list_shows() {
        let mut cx = TestAppContext::new();
        serve(&cx, 500);
        let view = cx.open::<History>();
        assert_eq!(asked(&cx), [None], "the first page alone");
        rows(&view).read(|rows| assert_eq!((rows.rows().len(), rows.count()), (64, 65)));
        assert!(cx.has_text("row 0"), "{:?}", cx.texts());
        cx.simulate_range("history", 20..34);
        assert_eq!(asked(&cx), [None], "rows held ask nothing");
        cx.host().never::<Query<Numbers>>();
        cx.simulate_range("history", 50..64);
        assert_eq!(asked(&cx), [None, Some(64)], "the next page");
        assert!(cx.has_text("Reading…"), "{:?}", cx.texts());
        cx.update(&view, |_, _, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(asked(&cx), [None, Some(64)], "one read out");
    }

    #[test]
    fn a_reread_reads_the_pages_held_and_draws_what_moved() {
        let mut cx = TestAppContext::new();
        let listing = serve(&cx, 500);
        let view = cx.open::<History>();
        cx.simulate_range("history", 50..64);
        assert_eq!(asked(&cx), [None, Some(64)]);
        let renders = cx.renders();
        cx.update(&rows(&view), |rows, _, cx| rows.reread(cx));
        cx.run_until_parked();
        assert_eq!(
            asked(&cx),
            [None, Some(64), None, Some(64)],
            "the two pages held"
        );
        assert_eq!(cx.renders(), renders, "the same rows drew nothing");
        listing.len.set(100);
        cx.update(&rows(&view), |rows, _, cx| rows.reread(cx));
        cx.run_until_parked();
        rows(&view).read(|rows| assert_eq!((rows.rows().len(), rows.count()), (100, 100)));
        assert_eq!(cx.renders(), renders + 1, "the rows that moved, once");
    }

    #[test]
    fn a_stale_cursor_starts_the_listing_over() {
        let mut cx = TestAppContext::new();
        let listing = serve(&cx, 500);
        let view = cx.open::<History>();
        listing.version.set(1);
        cx.simulate_range("history", 50..64);
        assert_eq!(asked(&cx), [None, Some(64), None, Some(64)]);
        rows(&view).read(|rows| {
            assert_eq!(rows.rows().len(), 128);
            assert!(rows.failed().is_none());
        });
        assert!(cx.has_text("row 64"), "{:?}", cx.texts());
    }

    #[test]
    fn a_write_between_two_pages_of_a_walk_starts_the_walk_over() {
        let mut cx = TestAppContext::new();
        let listing = serve(&cx, 500);
        let view = cx.open::<History>();
        cx.simulate_range("history", 50..64);
        assert_eq!(asked(&cx), [None, Some(64)]);
        let renders = cx.renders();
        listing.write_after_first_page.set(true);
        cx.update(&rows(&view), |rows, _, cx| rows.reread(cx));
        cx.run_until_parked();
        rows(&view).read(|rows| {
            assert_eq!(rows.failed(), None);
            assert_eq!((rows.rows().len(), rows.count()), (128, 129));
        });
        assert_eq!(cx.renders(), renders, "the rows on screen never left");
        let reread = [None, Some(64), None, Some(64), None, Some(64)];
        assert_eq!(asked(&cx), reread, "the re-read, started over once");
        listing.version.set(listing.version.get() + 1);
        listing.write_after_first_page.set(true);
        cx.simulate_range("history", 114..128);
        let start_over = [Some(128), None, Some(64), None, Some(64), Some(128)];
        assert_eq!(asked(&cx)[reread.len()..], start_over);
        rows(&view).read(|rows| {
            assert_eq!((rows.rows().len(), rows.count()), (192, 193));
            assert_eq!(rows.failed(), None);
        });
        assert!(cx.has_text("row 128"), "{:?}", cx.texts());
    }

    #[test]
    fn a_refused_listing_shows_its_refusal_until_a_reread_answers() {
        let mut cx = TestAppContext::new();
        let listing = serve(&cx, 500);
        let view = cx.open::<History>();
        cx.host()
            .refuse::<Query<Numbers>>("not_found", "no such listing");
        cx.simulate_range("history", 50..64);
        assert!(cx.has_text("no such listing"), "{:?}", cx.texts());
        rows(&view).read(|rows| assert_eq!(rows.count(), 0));
        cx.host()
            .handle::<Query<Numbers>>(move |after| listing.page(after));
        cx.update(&rows(&view), |rows, _, cx| rows.reread(cx));
        cx.run_until_parked();
        assert!(cx.has_text("row 0"), "{:?}", cx.texts());
    }

    #[test]
    fn a_listing_dropped_mid_read_cancels_the_read() {
        let mut cx = TestAppContext::new();
        let listing = serve(&cx, 500);
        let view = cx.open::<History>();
        cx.host().never::<Query<Numbers>>();
        cx.simulate_range("history", 50..64);
        let out = cx
            .last_frame()
            .requests
            .iter()
            .find(|request| request.kind == "program.query")
            .expect("the next page is out")
            .id;
        cx.update(&view, |history, _, cx| {
            history.read(cx);
            cx.notify();
        });
        cx.tick(vec![]);
        assert!(
            cx.last_frame().cancels.contains(&out),
            "the page on its way is cancelled: {:?}",
            cx.last_frame().cancels
        );
        let page = listing.page(asked(&cx)[1].map(|offset| {
            let mut cursor = vec![0];
            cursor.extend(offset.to_be_bytes());
            cursor
        }));
        cx.tick(vec![Event::Response {
            id: out,
            result: Ok(methods::encode(&page.unwrap())),
            done: true,
        }]);
        rows(&view).read(|rows| assert!(rows.is_loading() && rows.rows().is_empty()));
    }
}
