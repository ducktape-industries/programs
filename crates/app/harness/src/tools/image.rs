//! `image_view`: show the model an image file of `files` as it is at a snapshot. The result names
//! the file, and the image itself comes beside it, read again from files whenever a request is
//! built (`pictures.rs`), so no row holds its bytes.
use ::program::{Error, ExecCtx, invalid, not_found};
use files::{EntryInfo, Node, Query, Reply};
use serde::Deserialize;
use serde_json::{Value, json};

use super::files::{resolved, snapshot, snapshot_schema, stat};
use super::{Run, Seat, Tool, blob, object};
use crate::pictures::{self, Picture};

#[derive(Deserialize)]
pub(crate) struct ViewArgs {
    path: String,
    snapshot: Option<String>,
}

pub(crate) struct View;

impl Tool for View {
    const NAME: &'static str = "image_view";
    const DESCRIPTION: &'static str = "Look at an image file of the network's filesystem (png, jpeg, gif or webp): the result names it, and the image itself comes with it. It stays in your context until the conversation is summarized, or until the model refuses a request over its images, after which it reads as words. To see an image in a pod, such as a screenshot, copy it to files first (the pods service's POST /v1/pods/NAME/file with path and to), then view it here.";
    type Args = ViewArgs;

    fn schema() -> Value {
        object(
            json!({
                "path": { "type": "string", "description": "an absolute file path, like /shared/qa/shot.png" },
                "snapshot": snapshot_schema(),
            }),
            &["path"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: ViewArgs) -> Result<Run, Error> {
        let Some(at) = resolved(ctx, snapshot(&args.snapshot)?)? else {
            return Err(not_found("files holds nothing yet"));
        };
        let path = args.path;
        let Some(EntryInfo {
            node: Node::File { size, .. },
            ..
        }) = stat(ctx, &path, Some(&at))?
        else {
            return Err(not_found(format!("there is no file at {path}")));
        };
        // its kind and size are in its first bytes: one read, as far as files answers at once
        let read = Query::Read {
            path: path.clone(),
            at: Some(at),
            offset: 0,
            len: u64::MAX,
        };
        let Reply::Contents(contents) = ctx.query::<Query, Reply>(files::PROGRAM, &read)? else {
            return Err(invalid("files answered a read with something else"));
        };
        let Some(format) = pictures::format(&contents.bytes) else {
            return Err(invalid(format!(
                "{path} is not a png, jpeg, gif or webp image"
            )));
        };
        let mut answer = json!({
            "path": path,
            "snapshot": blob(&at),
            "size": size,
            "media_type": format.media(),
        });
        if let Some((width, height)) = pictures::dimensions(&contents.bytes, format) {
            answer["width"] = json!(width);
            answer["height"] = json!(height);
        }
        let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
        let picture = Picture {
            name,
            path,
            snapshot: at,
        };
        pictures::opened(ctx, seat.thread, seat.call, vec![picture]);
        Ok(Run::Done(answer))
    }
}
