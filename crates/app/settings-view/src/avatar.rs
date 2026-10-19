//! Change photo and Remove photo: the reader picks a picture, cuts its square
//! in the crop sheet and saves it as their account's avatar, or takes the
//! avatar off. Both keep the card's bio as it is.
use std::sync::Arc;

use view::methods::{
    BlobBody, FilePick, FileRead, FileRelease, Method as _, NewBlob, OpPut, Pick, PutBlobs,
    ReadFile,
};
use view::prelude::*;

use crate::Settings;
use crate::photo::Photo;
use crate::queries::{Account, Seat};
use crate::state::Problem;
use identity::Identity;

// The kind `op.put` names the saved picture's blob by.
const KIND: &str = "avatar";

impl Settings {
    // Whether this host reads a picked file: Change photo is offered only
    // where it does.
    pub(crate) fn ask_hosted(&mut self, cx: &mut Context<Self>) {
        let hosted = cx.host().hosted();
        let asked = async move { Ok::<_, view::host::Error>(hosted.await) };
        cx.land(asked, |view, hosted, cx| {
            let kinds = [FilePick::KIND, FileRead::KIND, OpPut::KIND];
            view.reads_files = hosted
                .ok()
                .flatten()
                .is_some_and(|hosted| kinds.iter().all(|kind| hosted.has(kind)));
            cx.notify();
        })
        .detach();
    }

    fn seated(&self) -> Option<&Account> {
        match self.account.ready() {
            Some(Some(Seat::Account(account))) => Some(account),
            _ => None,
        }
    }

    // The seated account's picture, once read.
    pub(crate) fn picture(&self, cx: &mut Context<Self>) -> Option<Arc<view::Image>> {
        let blob = self.seated()?.avatar?;
        let pictures = self.pictures.clone()?;
        pictures.update(cx, |pictures, cx| pictures.get(&blob, cx))
    }

    // Change photo: the picker, then the file read whole and decoded; the
    // crop sheet opens on it.
    pub(crate) fn change_photo(&mut self, cx: &mut Context<Self>) {
        if self.change_photo.busy || self.remove_photo.busy {
            return;
        }
        self.change_photo.busy = true;
        self.change_photo.problem = None;
        self.remove_photo.problem = None;
        cx.notify();
        let host = cx.host();
        let picked = host.ask::<FilePick>(Pick { multiple: false });
        let read = async move {
            let Some(file) = picked.await?.into_iter().next() else {
                return Ok(None);
            };
            let ask = ReadFile {
                file: file.file,
                offset: 0,
                len: file.size,
            };
            let bytes = host.ask::<FileRead>(ask).await;
            host.notify::<FileRelease>(vec![file.file]);
            bytes.map(Some)
        };
        cx.land(read, |view, read, cx| {
            view.change_photo.busy = false;
            match read {
                Ok(None) => {}
                Ok(Some(bytes)) => match Photo::decode(&bytes) {
                    Some(photo) => view.photo = Some(photo),
                    None => view.change_photo.problem = Some(Problem::NotAPicture),
                },
                Err(refusal) => {
                    let said = design::sentence(&refusal);
                    view.change_photo.problem = Some(Problem::Refused(said));
                }
            }
            cx.notify();
        })
        .detach();
    }

    // Save in the crop sheet: the square put as one blob, then the card's
    // avatar set to it.
    pub(crate) fn save_photo(&mut self, cx: &mut Context<Self>) {
        let (Some(photo), Some(account)) = (&self.photo, self.seated()) else {
            return;
        };
        if self.change_photo.busy {
            return;
        }
        let saved = photo.saved();
        let (number, bio) = (account.number, account.bio.clone());
        let host = cx.host();
        let put = host.put(PutBlobs {
            blobs: vec![NewBlob {
                kind: KIND.into(),
                body: BlobBody::Bytes(saved),
            }],
        });
        let set = async move {
            let held = put.await?;
            let Some(digest) = held.first().copied() else {
                return Err(view::host::malformed(
                    "the host held no blob for the picture".into(),
                ));
            };
            let op = identity::Op::SetProfile {
                account: number,
                avatar: Some(abi::BlobId::Sha256(digest)),
                bio,
            };
            host.ask::<Submit<Identity>>(op).await
        };
        self.set_profile(set, |view| &mut view.change_photo, cx);
    }

    // Remove photo: the card's avatar taken off.
    pub(crate) fn remove_photo(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.seated() else {
            return;
        };
        if self.change_photo.busy || self.remove_photo.busy {
            return;
        }
        let op = identity::Op::SetProfile {
            account: account.number,
            avatar: None,
            bio: account.bio.clone(),
        };
        self.change_photo.problem = None;
        let set = cx.host().ask::<Submit<Identity>>(op);
        self.set_profile(set, |view| &mut view.remove_photo, cx);
    }

    fn set_profile(
        &mut self,
        set: impl Future<Output = Result<Submitted, view::host::Error>> + 'static,
        form: fn(&mut Settings) -> &mut crate::state::Form,
        cx: &mut Context<Self>,
    ) {
        let pending = form(self);
        pending.busy = true;
        pending.problem = None;
        cx.notify();
        cx.land(set, move |view, result, cx| {
            let pending = form(view);
            pending.busy = false;
            match design::failure(&result) {
                None => view.photo = None,
                Some(said) => form(view).problem = Some(Problem::Refused(said)),
            }
            view.refresh_account(cx);
            cx.notify();
        })
        .detach();
    }

    // Cancel, or the sheet dismissed: the picture goes, nothing is saved. A
    // save in flight goes on and lands as it lands.
    pub(crate) fn cancel_photo(&mut self, cx: &mut Context<Self>) {
        if !self.change_photo.busy {
            self.photo = None;
            self.change_photo.problem = None;
        }
        cx.notify();
    }

    pub(crate) fn drag_photo(&mut self, (x, y): (Pixels, Pixels), cx: &mut Context<Self>) {
        if let Some(photo) = &mut self.photo {
            photo.dragged((f32::from(x), f32::from(y)));
            cx.notify();
        }
    }

    // The zoom slider's thumb moved `by` points along its `travel`.
    pub(crate) fn slide_zoom(&mut self, by: f32, travel: f32, cx: &mut Context<Self>) {
        if let Some(photo) = &mut self.photo {
            photo.slid(by / travel);
            cx.notify();
        }
    }

    pub(crate) fn pinch_photo(&mut self, delta: f32, cx: &mut Context<Self>) {
        if let Some(photo) = &mut self.photo {
            photo.pinched(delta);
            cx.notify();
        }
    }
}
