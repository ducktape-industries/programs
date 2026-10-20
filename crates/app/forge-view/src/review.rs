//! Writing a review: what is staged, at which anchor, and the one operation
//! it all becomes. A refusal keeps every draft.
use view::prelude::*;

use crate::state::{Forge, PendingComment, ReviewRefusal, ReviewSession, change_key};
use forge::{Op, ReviewDraft, Verdict};

impl Forge {
    pub(crate) fn review_key(&self) -> Option<String> {
        Some(change_key(self.nav().repo.as_deref()?, self.nav().change?))
    }

    pub(crate) fn start_review(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.review_key() else { return };
        let Some((_, source, _, _)) = self.change() else {
            return;
        };
        let Some(commit) = source.clone() else { return };
        let base = self.compare().and_then(|c| c.base.clone());
        self.reviews.insert(
            key,
            ReviewSession {
                commit,
                base,
                ..ReviewSession::default()
            },
        );
        cx.notify();
    }

    pub(crate) fn cancel_review(&mut self, cx: &mut Context<Self>) {
        if let Some(key) = self.review_key() {
            self.reviews.remove(&key);
        }
        cx.notify();
    }

    pub(crate) fn open_comment(
        &mut self,
        path: Vec<u8>,
        new_side: bool,
        line: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.review_key() else { return };
        if !self.reviews.contains_key(&key) {
            self.start_review(cx);
        }
        let staged = self
            .reviews
            .get(&key)
            .and_then(|review| review.staged(&path, new_side, line).cloned());
        if let Some(review) = self.reviews.get_mut(&key) {
            review.refused = None;
            let body = TextField::new(staged.map(|staged| staged.body.text()).unwrap_or_default());
            review.open = Some(PendingComment {
                path,
                new_side,
                line,
                body,
            });
        }
        crate::ui::diff::reveal_composer(self);
        window.focus(crate::id("forge-comment-body"));
        cx.notify();
    }

    pub(crate) fn stage_comment(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.review_key() else { return };
        let Some(review) = self.reviews.get_mut(&key) else {
            return;
        };
        let Some(open) = review.open.take() else {
            return;
        };
        if open.body.is_blank() {
            review.comments.retain(|staged| !staged.anchors(&open));
        } else {
            review.stage(open);
        }
        cx.notify();
    }

    pub(crate) fn discard_comment(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.review_key() else { return };
        if let Some(review) = self.reviews.get_mut(&key) {
            review.open = None;
        }
        cx.notify();
    }

    pub(crate) fn pick_verdict(&mut self, verdict: Verdict, cx: &mut Context<Self>) {
        if let Some(review) = self.review_mut() {
            review.verdict = Some(verdict);
        }
        cx.notify();
    }

    pub(crate) fn finishing(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(key) = self.review_key() else { return };
        if let Some(review) = self.reviews.get_mut(&key) {
            review.finishing = on;
        }
        cx.notify();
    }

    pub(crate) fn finish_review(&mut self, verdict: Verdict, cx: &mut Context<Self>) {
        let (Some(repo), Some(n)) = (self.nav().repo.clone(), self.nav().change) else {
            return;
        };
        let key = change_key(&repo, n);
        let Some(review) = self.reviews.get(&key).cloned() else {
            return;
        };
        let comments = review.line_comments();
        if matches!(verdict, Verdict::Comment) && review.body.is_blank() && comments.is_empty() {
            if let Some(review) = self.reviews.get_mut(&key) {
                review.refused = Some(ReviewRefusal::EmptyComment);
            }
            cx.notify();
            return;
        }
        let draft = ReviewDraft {
            commit_oid: review.commit.clone(),
            base_oid: review.base.clone(),
            verdict,
            body: review.body.text(),
            comments,
        };
        if let Some(review) = self.reviews.get_mut(&key) {
            review.finishing = false;
            review.refused = None;
        }
        self.submit(
            Op::ReviewSubmit {
                repo,
                n,
                review: draft,
            },
            key,
            "Submitting this review",
            cx,
        );
    }

    pub(crate) fn outdated(&self, commit_oid: &str) -> bool {
        self.change()
            .and_then(|(_, source, _, _)| source.clone())
            .is_some_and(|head| head != commit_oid)
    }
}
