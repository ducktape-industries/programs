//! Pictures a view draws by blob id, a person's avatar first: each id read
//! once with `blob.get` and kept for as long as the view holds the entity.
use std::collections::BTreeMap;
use std::sync::Arc;

use program::abi::{self, BlobId};

use crate::methods::BlobGet;
use crate::{Context, Image, Task};

// An entity (`cx.new(|_| Pictures::default())`): a view asks it for each
// picture it draws, and redraws when one lands.
#[derive(Default)]
pub struct Pictures {
    held: BTreeMap<BlobId, Option<Arc<Image>>>,
    reading: BTreeMap<BlobId, Task<()>>,
}

impl Pictures {
    // The picture `blob` holds, once read. `None` while it is read, and for
    // a blob the node holds none of, one that is no picture the app draws,
    // or a read the host refused. The first ask of an id reads it, and
    // notifies once it lands.
    pub fn get(&mut self, blob: &BlobId, cx: &mut Context<Self>) -> Option<Arc<Image>> {
        if let Some(picture) = self.held.get(blob) {
            return picture.clone();
        }
        if self.reading.contains_key(blob) {
            return None;
        }
        let ask = cx.host().ask::<BlobGet>(blob_get_id(blob));
        let id = *blob;
        let task = cx.spawn(async move |this, cx| {
            let picture = ask.await.ok().flatten().and_then(|bytes| {
                let format = crate::picture::picture_format(&bytes)?;
                Some(Arc::new(Image::from_bytes(format, bytes)))
            });
            let _ = this.update(cx, |pictures, cx| {
                pictures.reading.remove(&id);
                pictures.held.insert(id, picture);
                cx.notify();
            });
        });
        self.reading.insert(id, task);
        None
    }
}

fn blob_get_id(blob: &BlobId) -> String {
    let hash = match blob {
        BlobId::Sha256(_) => "sha256",
        BlobId::Sha1(_) => "sha1",
    };
    format!("{hash}:{}", abi::hex(blob.digest()))
}
