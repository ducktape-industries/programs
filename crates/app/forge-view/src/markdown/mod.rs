//! A document's links: a pressed one calls the caller's `OnLink` with its raw destination, and
//! [`target`] resolves forge's, the web through the host or a file of the repository relative to
//! the document's folder, its `%XX` escapes read by ducklink.
mod links;

use std::rc::Rc;

use view::prelude::*;

use crate::Forge;
pub(crate) use links::{Target, target};
use markdown::OnLink;

// A document's links, a relative one resolved against `dir`, its folder
// (the root for a change's body).
pub(crate) fn links(dir: Vec<u8>, cx: &mut Context<Forge>) -> OnLink {
    Rc::new(cx.listener(move |forge, dest: &String, _, cx| forge.follow_link(&dir, dest, cx)))
}
