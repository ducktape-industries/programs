//! Standing questions: a view asks a program once (`program.watch`) and the node answers again
//! on each block that moves the answer, until the view lets go. A host or node that stands none
//! ends the question, and the view reads it where the program's changes say, as before.
use std::cell::Cell;
use std::rc::Rc;

use futures::StreamExt;
use serde::{Deserialize, Serialize};

use crate::host::{Error, Host, Subscription, malformed, unexpected_reply_variant};
use crate::methods::{self, Method, Program, Watch};
use crate::{Context, Loadable, Reread, Task};

/// One item of a standing question.
#[derive(Debug)]
pub enum Pushed<R> {
    /// The answer: the first when it is asked, then each one a block moved.
    Answer(R),
    /// The program refused to answer it this block; the question still stands.
    Refused(Error),
    /// The node could not run it this block: read it yourself.
    Moved,
    /// Nothing stands for it any more: read it where the program's changes say.
    Ended(Error),
}

/// A question standing on the node; dropping it lets go of it.
pub struct Standing {
    asked: Vec<u8>,
    ended: Rc<Cell<bool>>,
    _subscription: Task<()>,
}

impl Standing {
    pub fn stands(&self) -> bool {
        !self.ended.get()
    }

    pub fn asks<A: program::Ask>(&self, ask: A) -> bool {
        self.asked == asked(ask)
    }
}

impl std::fmt::Debug for Standing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Standing")
            .field("stands", &self.stands())
            .finish()
    }
}

fn asked<A: program::Ask>(ask: A) -> Vec<u8> {
    let query: <A::Program as Program>::Query = ask.into();
    Watch::<A::Program>::encode_request(&query)
}

fn stands_no_question(refusal: &Error) -> bool {
    matches!(
        refusal.code.as_str(),
        ::error::code::UNSUPPORTED | methods::refusal::UNKNOWN_REQUEST
    )
}

async fn next_pushed<P: Program, R>(
    subscription: &mut Subscription,
    answer: fn(P::Reply) -> Option<R>,
    host: &Host,
) -> Pushed<R> {
    let item = subscription.next().await;
    let last = subscription.ended();
    match item {
        None => Pushed::Ended(Error::new(
            methods::refusal::REQUEST_CLOSED,
            "the host let go of the standing question",
        )),
        Some(Err(refusal)) if last => {
            if stands_no_question(&refusal) {
                host.stood_no_question(refusal.clone());
            }
            Pushed::Ended(refusal)
        }
        Some(Err(refusal)) => Pushed::Refused(refusal),
        Some(Ok(bytes)) => match Watch::<P>::decode_reply(&bytes) {
            Err(error) => Pushed::Refused(malformed(error)),
            Ok(None) => Pushed::Moved,
            Ok(Some(reply)) => answer(reply).map_or_else(
                || Pushed::Refused(unexpected_reply_variant()),
                Pushed::Answer,
            ),
        },
    }
}

/// A `Reread` slot a standing question keeps current: each answer the node pushes lands in it, and the view's own read runs whenever the node can't answer or stands nothing.
pub struct Live<T> {
    shown: Reread<T>,
    standing: Option<Standing>,
}

impl<T> Live<T> {
    pub fn stands(&self) -> bool {
        self.standing.as_ref().is_some_and(Standing::stands)
    }
}

impl<T> Default for Live<T> {
    fn default() -> Self {
        Self {
            shown: Reread::default(),
            standing: None,
        }
    }
}

impl<T> std::ops::Deref for Live<T> {
    type Target = Reread<T>;
    fn deref(&self) -> &Reread<T> {
        &self.shown
    }
}

impl<T> std::ops::DerefMut for Live<T> {
    fn deref_mut(&mut self) -> &mut Reread<T> {
        &mut self.shown
    }
}

impl<T: Serialize> Serialize for Live<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.shown.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Live<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            shown: Reread::deserialize(deserializer)?,
            standing: None,
        })
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Live<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Live")
            .field("shown", &self.shown)
            .field("stands", &self.stands())
            .finish()
    }
}

fn land_pushed<T: PartialEq, V: 'static>(slot: &mut Live<T>, value: T, cx: &mut Context<V>) {
    let moved = slot.shown.ready() != Some(&value);
    let reading = matches!(&*slot.shown, Loadable::Loading(_) | Loadable::Reloading(..));
    if moved {
        cx.notify();
    }
    let replaced = moved || reading;
    if replaced {
        slot.shown = Reread::from(Loadable::Ready(value));
    }
}

impl<V: 'static> Context<'_, V> {
    pub fn watch<A>(
        &mut self,
        ask: A,
        each: impl FnMut(&mut V, Pushed<A::Reply>, &mut Context<V>) + 'static,
    ) -> Standing
    where
        A: program::Ask + 'static,
        A::Reply: 'static,
    {
        self.stand::<A::Program, A::Reply>(asked(ask), A::answer, each)
    }

    pub fn live<A, T>(
        &mut self,
        view: &mut V,
        at: impl Fn(&mut V) -> &mut Live<T> + 'static,
        ask: A,
        land: impl Fn(A::Reply) -> Option<T> + 'static,
        read: impl Fn(&mut V, &mut Context<V>) + 'static,
    ) where
        A: program::Ask + 'static,
        A::Reply: 'static,
        T: PartialEq + 'static,
    {
        let asked = asked(ask);
        let slot = at(view);
        let standing_already = (slot.standing.as_ref()).is_some_and(|held| held.asked == asked);
        if standing_already {
            return;
        }
        let standing =
            self.stand::<A::Program, A::Reply>(asked, A::answer, move |view, pushed, cx| {
                match pushed {
                    Pushed::Answer(reply) => match land(reply) {
                        Some(value) => land_pushed(at(view), value, cx),
                        None => read(view, cx),
                    },
                    Pushed::Refused(refusal) => {
                        at(view).shown = Reread::from(Loadable::Failed(refusal));
                        cx.notify();
                    }
                    Pushed::Moved | Pushed::Ended(_) => read(view, cx),
                }
            });
        slot.standing = Some(standing);
    }

    fn stand<P: Program + 'static, R: 'static>(
        &mut self,
        asked: Vec<u8>,
        answer: fn(P::Reply) -> Option<R>,
        mut each: impl FnMut(&mut V, Pushed<R>, &mut Context<V>) + 'static,
    ) -> Standing {
        let host = self.host();
        let mut questions = match host.stands_no_question() {
            Some(refusal) => Err(refusal),
            None => Ok(host.raw_subscribe(Watch::<P>::KIND, &asked)),
        };
        let ended = Rc::new(Cell::new(false));
        let ending = ended.clone();
        let task = self.spawn(async move |this, cx| {
            loop {
                let pushed = match &mut questions {
                    Err(refusal) => Pushed::Ended(refusal.clone()),
                    Ok(subscription) => next_pushed::<P, R>(subscription, answer, &host).await,
                };
                let over = matches!(pushed, Pushed::Ended(_));
                ending.set(over);
                let heard = this.update(cx, |view, cx| each(view, pushed, cx)).is_ok();
                let goes_on = heard && !over;
                if !goes_on {
                    break;
                }
            }
        });
        Standing {
            asked,
            ended,
            _subscription: task,
        }
    }
}

#[cfg(test)]
mod tests;
