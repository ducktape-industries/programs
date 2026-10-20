//! The open file decoded once, for either design: its lines and their
//! highlight tokens, and its markdown blocks, so a frame redraws a file
//! without decoding and tokenizing it again.
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use markdown::highlight::{self, Token};
use markdown::{Block, parse};

pub(crate) struct Lines {
    pub(crate) rows: Vec<String>,
    pub(crate) tokens: Vec<Vec<(Range<usize>, Token)>>,
}

#[derive(Default)]
pub(crate) struct BlobCache {
    lines: RefCell<Option<(String, String, Rc<Lines>)>>,
    doc: RefCell<Option<(String, Rc<Vec<Block>>)>>,
}

impl BlobCache {
    pub(crate) fn lines(&self, oid: &str, name: &str, bytes: &[u8]) -> Rc<Lines> {
        let mut slot = self.lines.borrow_mut();
        if let Some((o, n, lines)) = slot.as_ref()
            && o == oid
            && n == name
        {
            return lines.clone();
        }
        let text = String::from_utf8_lossy(bytes);
        let rows: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let tokens = highlight::tokens(name, &rows.iter().map(String::as_str).collect::<Vec<_>>());
        let lines = Rc::new(Lines { rows, tokens });
        *slot = Some((oid.to_owned(), name.to_owned(), lines.clone()));
        lines
    }

    pub(crate) fn doc(&self, oid: &str, bytes: &[u8]) -> Rc<Vec<Block>> {
        let mut slot = self.doc.borrow_mut();
        if let Some((o, blocks)) = slot.as_ref()
            && o == oid
        {
            return blocks.clone();
        }
        let blocks = Rc::new(parse(&String::from_utf8_lossy(bytes)));
        *slot = Some((oid.to_owned(), blocks.clone()));
        blocks
    }
}

#[cfg(test)]
mod tests {
    use super::BlobCache;
    use std::rc::Rc;

    #[test]
    fn a_blob_is_decoded_once_per_id_and_name() {
        let cache = BlobCache::default();
        let first = cache.lines("a1", "x.rs", b"fn a() {}\n");
        assert!(Rc::ptr_eq(&first, &cache.lines("a1", "x.rs", b"")));
        assert!(!Rc::ptr_eq(&first, &cache.lines("a1", "x.toml", b"")));
        assert!(!Rc::ptr_eq(&first, &cache.lines("b2", "x.toml", b"")));
        let doc = cache.doc("a1", b"# Hi");
        assert!(Rc::ptr_eq(&doc, &cache.doc("a1", b"")));
        assert!(!Rc::ptr_eq(&doc, &cache.doc("b2", b"# Hi")));
    }
}
