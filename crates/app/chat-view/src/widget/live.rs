//! A widget's data path: its file read and parsed, its source program's schema read from that
//! program's code, the query asked and the reply read as JSON, again on each block of the source
//! program, and of files for the file itself.
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use abi::BlobId;
use borsh_json::Interface;
use serde_json::Value;
use view::methods::{BlobGet, Call, NamedChanges, NamedQuery};
use view::prelude::*;
use view::{Host, Task, store};

use super::{Slot, Widget, at, parse, put, with_me};
use crate::{Chat, words};

// Every widget the open room names, by file path, and what they read.
#[derive(Default)]
pub(crate) struct Widgets {
    pub(crate) files: BTreeMap<String, Live>,
    following: BTreeMap<String, Task<()>>,
    schemas: Schemas,
    // the rooms whose pinned card the reader folded
    pub(crate) folded: BTreeSet<String>,
    // the desktop's side pane is open
    pub(crate) side: bool,
}

// Each code's interface, or why its schema doesn't read.
type Schema = Result<Rc<Interface>, String>;
type Schemas = Rc<RefCell<BTreeMap<BlobId, Schema>>>;

// Where the device keeps a program's schema section, with the code it came
// from (`store`): read again only once the program runs other code.
const KEPT_SCHEMA: &str = "widget-schema/";

// The file object last read and what it parsed to.
type File = (BlobId, Result<Rc<Widget>, String>);

#[derive(Default)]
pub(crate) struct Live {
    pub(crate) shown: Shown,
    file: Option<File>,
    // the read in flight: a newer one in its place drops it
    reading: Option<Task<()>>,
    // the read in flight asks the source again
    asking: bool,
    // the program whose blocks ask again: the source once its schema
    // reads, else programs, whose block may deploy it
    follows: Option<String>,
}

#[derive(Default, Clone, PartialEq, Debug)]
pub(crate) enum Shown {
    #[default]
    Loading,
    Failed {
        title: Option<String>,
        why: String,
    },
    Ready {
        widget: Rc<Widget>,
        reply: Value,
        rows: Vec<Value>,
    },
}

impl Shown {
    pub(crate) fn title(&self) -> &str {
        match self {
            Shown::Ready { widget, .. } => &widget.title,
            Shown::Failed {
                title: Some(title), ..
            } => title,
            Shown::Loading | Shown::Failed { .. } => words::WIDGET,
        }
    }
}

// What a read brings: the file as it now is, what to show unless it shows
// what it showed, and whose blocks to ask again on.
struct Landed {
    file: Option<File>,
    shown: Option<Shown>,
    follows: Option<String>,
}

impl Chat {
    // The file the open room's meta names for `slot`.
    pub(crate) fn widget_path(&self, slot: Slot) -> Option<&str> {
        let path = self.room_info()?.meta.get(slot.key())?.trim();
        (!path.is_empty()).then_some(path)
    }

    pub(crate) fn widget(&self, slot: Slot) -> Option<&Shown> {
        let path = self.widget_path(slot)?;
        Some(&self.widgets.files.get(path)?.shown)
    }

    pub(crate) fn widget_side_shown(&self) -> bool {
        self.widgets.side && self.widget_path(Slot::Side).is_some()
    }

    pub(crate) fn toggle_widget_side(&mut self) {
        self.widgets.side = !self.widgets.side;
        if self.widgets.side {
            self.details = None;
            self.close_thread();
        }
    }

    pub(crate) fn fold_widget(&mut self) {
        let Some(room) = &self.room else {
            return;
        };
        if !self.widgets.folded.remove(&room.id) {
            self.widgets.folded.insert(room.id.clone());
        }
    }

    // A pressed row's target: a `duck://` or `https://` link opens through
    // the host, anything else is a room here.
    pub(crate) fn open_widget_target(&mut self, target: String, cx: &mut Context<Self>) {
        if target.starts_with("duck://") || target.starts_with("https://") {
            cx.host().open_link(&target);
            return;
        }
        self.choose(target, cx);
        cx.notify();
    }

    // The widgets the open room's meta names read, those it no longer
    // names dropped, and the programs they read followed: settled each
    // frame, since the room and its meta arrive by many paths.
    pub(crate) fn settle_widgets(&mut self, cx: &mut Context<Self>) {
        let named: BTreeSet<String> = Slot::ALL
            .into_iter()
            .filter_map(|slot| self.widget_path(slot).map(str::to_owned))
            .collect();
        self.widgets.files.retain(|path, _| named.contains(path));
        for path in &named {
            if !self.widgets.files.contains_key(path) {
                self.widgets.files.insert(path.clone(), Live::default());
                self.refresh_widget(path, true, cx);
            }
        }
        let mut programs: BTreeSet<String> = self
            .widgets
            .files
            .values()
            .filter_map(|live| live.follows.clone())
            .collect();
        if !named.is_empty() {
            programs.insert(files::PROGRAM.to_owned());
        }
        self.widgets
            .following
            .retain(|program, _| programs.contains(program));
        for program in programs {
            if self.widgets.following.contains_key(&program) {
                continue;
            }
            let each = program.clone();
            let follow =
                cx.follow::<NamedChanges>(program.clone(), move |chat, change, cx| match change {
                    Ok(_) => chat.widgets_moved(&each, cx),
                    Err(refusal) => cx.log_refused(&words::widget_heads(&each), &refusal),
                });
            self.widgets.following.insert(program, follow);
        }
    }

    // A block of `program`: the widgets that read it ask again, and on a
    // block of files every widget looks whether its file moved. A widget
    // whose read failed asks again on any block it hears.
    fn widgets_moved(&mut self, program: &str, cx: &mut Context<Self>) {
        let moved: Vec<(String, bool)> = self
            .widgets
            .files
            .iter()
            .map(|(path, live)| {
                let failed = matches!(live.shown, Shown::Failed { .. });
                let reads = live.follows.as_deref() == Some(program);
                (path.clone(), failed || reads)
            })
            .collect();
        for (path, reads) in moved {
            if reads || program == files::PROGRAM {
                self.refresh_widget(&path, reads, cx);
            }
        }
    }

    fn refresh_widget(&mut self, path: &str, ask: bool, cx: &mut Context<Self>) {
        let me = self.my_account();
        let schemas = self.widgets.schemas.clone();
        let Some(live) = self.widgets.files.get_mut(path) else {
            return;
        };
        live.asking |= ask;
        let (ask, known, host, path) = (live.asking, live.file.clone(), cx.host(), path.to_owned());
        let task = cx.spawn(async move |this, cx| {
            let landed = read(&host, &path, known, ask, me, &schemas).await;
            let _ = this.update(cx, |chat, cx| chat.widget_landed(&path, landed, cx));
        });
        live.reading = Some(task);
    }

    fn widget_landed(&mut self, path: &str, landed: Landed, cx: &mut Context<Self>) {
        let Some(live) = self.widgets.files.get_mut(path) else {
            return;
        };
        live.asking = false;
        live.file = landed.file;
        if let Some(shown) = landed.shown
            && live.shown != shown
        {
            live.shown = shown;
            cx.notify();
        }
        // a program to follow anew is followed from the next frame
        if landed.follows.is_some() && live.follows != landed.follows {
            live.follows = landed.follows;
            cx.notify();
        }
    }
}

// The widget at `path` as it is now: its file looked at, read again when
// it moved, and its source asked when `ask` says or the file moved.
async fn read(
    host: &Host,
    path: &str,
    known: Option<File>,
    ask: bool,
    me: Option<u64>,
    schemas: &Schemas,
) -> Landed {
    let failed = |file: Option<File>, title: Option<String>, why: String| Landed {
        file,
        shown: Some(Shown::Failed { title, why }),
        follows: None,
    };
    let stat = files::ask::Stat {
        path: path.to_owned(),
        at: None,
    };
    let (snapshot, object) = match host.query(stat).await {
        Ok((snapshot, Some(entry))) => match entry.node {
            files::Node::File { file, .. } => (snapshot, file),
            _ => return failed(None, None, words::widget_not_a_file(path)),
        },
        Ok((_, None)) => return failed(None, None, words::widget_not_a_file(path)),
        Err(refusal) => return failed(None, None, design::sentence(&refusal)),
    };
    let file = match known {
        Some((id, widget)) if id == object => {
            if !ask {
                return Landed {
                    file: Some((id, widget)),
                    shown: None,
                    follows: None,
                };
            }
            (id, widget)
        }
        _ => {
            let text = contents(host, path, snapshot).await;
            (object, text.and_then(|text| parse(&text)).map(Rc::new))
        }
    };
    let widget = match &file.1 {
        Ok(widget) => widget.clone(),
        Err(why) => {
            let why = words::widget_unreadable(why);
            return failed(Some(file), None, why);
        }
    };
    let program = &widget.source.program;
    let (shown, follows) = match interface(host, program, schemas).await {
        Ok(interface) => {
            let shown = match source(host, &interface, &widget, me).await {
                Ok((reply, rows)) => Shown::Ready {
                    widget: widget.clone(),
                    reply,
                    rows,
                },
                Err(why) => Shown::Failed {
                    title: Some(widget.title.clone()),
                    why,
                },
            };
            (shown, program.clone())
        }
        Err(why) => {
            let title = Some(widget.title.clone());
            (Shown::Failed { title, why }, programs::PROGRAM.to_owned())
        }
    };
    Landed {
        file: Some(file),
        shown: Some(shown),
        follows: Some(follows),
    }
}

// The file's text at `snapshot`, read a chunk at a time.
async fn contents(host: &Host, path: &str, at: Option<BlobId>) -> Result<String, String> {
    let bytes = file_bytes(host, path, at).await?;
    String::from_utf8(bytes).map_err(|_| words::WIDGET_NOT_TEXT.into())
}

// The file's bytes at `snapshot`, read a chunk at a time.
pub(crate) async fn file_bytes(
    host: &Host,
    path: &str,
    at: Option<BlobId>,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    loop {
        let read = files::ask::Read {
            path: path.to_owned(),
            at,
            offset: bytes.len() as u64,
            len: files::CHUNK_SIZE,
        };
        let contents = host
            .query(read)
            .await
            .map_err(|refusal| design::sentence(&refusal))?;
        // a read short of the size it states would never end
        if contents.bytes.is_empty() && !contents.eof {
            return Err(words::NOT_THERE.into());
        }
        bytes.extend(contents.bytes);
        if contents.eof {
            return Ok(bytes);
        }
    }
}

// The source's reply and the rows it lists: every page of them when the
// source names a cursor.
async fn source(
    host: &Host,
    interface: &Rc<Interface>,
    widget: &Widget,
    me: Option<u64>,
) -> Result<(Value, Vec<Value>), String> {
    let source = &widget.source;
    let query = with_me(&source.query, me);
    let Some(cursor) = source.cursor.clone() else {
        let reply = ask(host, interface, &source.program, query)
            .await
            .map_err(|refusal| said(&refusal))?;
        let rows = rows_of(&reply, &source.rows).map_err(|why| words::widget_reply(&why))?;
        return Ok((reply, rows));
    };
    let next = match source.rows.rsplit_once('.') {
        Some((list, _)) => format!("{list}.next"),
        None => "next".to_owned(),
    };
    let first: Rc<RefCell<Option<Value>>> = Rc::default();
    let pages = view::host::all_pages(|after: Option<Vec<u8>>| {
        let (host, interface, first) = (host.clone(), interface.clone(), first.clone());
        let (program, rows, next) = (source.program.clone(), source.rows.clone(), next.clone());
        let mut query = query.clone();
        let cursor = cursor.clone();
        async move {
            let bad = |why: String| view::host::malformed(words::widget_reply(&why));
            if let Some(after) = &after {
                let after: Value = serde_json::from_slice(after).map_err(|e| bad(e.to_string()))?;
                put(&mut query, &cursor, after).map_err(bad)?;
            }
            let reply = ask(&host, &interface, &program, query).await?;
            let more = match at(&reply, &next) {
                None => return Err(bad(words::widget_no_next(&next))),
                Some(Value::Null) => None,
                Some(next) => Some(serde_json::to_vec(next).map_err(|e| bad(e.to_string()))?),
            };
            let listed = rows_of(&reply, &rows).map_err(bad)?;
            if after.is_none() {
                *first.borrow_mut() = Some(reply);
            }
            Ok((listed, more))
        }
    })
    .await
    .map_err(|refusal| said(&refusal))?;
    let reply = first.take().unwrap_or(Value::Null);
    Ok((reply, pages))
}

// The list at `path` in the reply; the empty path names the reply itself,
// which need not be a list (a stat reads it whole).
fn rows_of(reply: &Value, path: &str) -> Result<Vec<Value>, String> {
    match at(reply, path) {
        Some(Value::Array(items)) => Ok(items.clone()),
        Some(_) if path.is_empty() => Ok(Vec::new()),
        Some(_) => Err(words::widget_not_a_list(path)),
        None => Err(words::widget_nothing_at(path)),
    }
}

// One question to `program`, written from JSON and answered as JSON by its
// schema.
async fn ask(
    host: &Host,
    interface: &Interface,
    program: &str,
    query: Value,
) -> Result<Value, view::host::Error> {
    let fault = |fault: borsh_json::Fault| view::host::malformed(fault.to_string());
    let body = interface.encode_query(&query).map_err(fault)?;
    let call = Call {
        target: program.to_owned(),
        body,
    };
    let asked = |refusal| view::host::malformed(words::widget_asked(program, &said(&refusal)));
    let bytes = host.ask::<NamedQuery>(call).await.map_err(asked)?;
    interface.decode_reply(&bytes).map_err(fault)
}

// A refusal as the card says it.
fn said(refusal: &view::host::Error) -> String {
    design::sentence(refusal)
}

// The program's query and reply types, from the schema its code carries:
// read once per code, kept on the device with that code, so a program
// swapped to new code is read again and its code is not fetched per run.
async fn interface(host: &Host, program: &str, schemas: &Schemas) -> Schema {
    let reading = |refusal| words::widget_code_unread(program, &design::sentence(&refusal));
    let asked = programs::ask::Program(program.to_owned());
    let (_, entry) = host.query(asked).await.map_err(reading)?;
    let entry = entry.ok_or_else(|| words::widget_no_program(program))?;
    if let Some(known) = schemas.borrow().get(&entry.code) {
        return known.clone();
    }
    let key = format!("{KEPT_SCHEMA}{program}");
    // a store the host refuses is a store with nothing kept
    if let Ok(Some((code, section))) = store::get::<(BlobId, Vec<u8>)>(host, &key).await
        && code == entry.code
        && let Ok(interface) = interface_of(program, &section)
    {
        schemas.borrow_mut().insert(code, Ok(interface.clone()));
        return Ok(interface);
    }
    let id = match entry.code {
        BlobId::Sha256(digest) => format!("sha256:{}", abi::hex(&digest)),
        BlobId::Sha1(digest) => format!("sha1:{}", abi::hex(&digest)),
    };
    let code = host
        .ask::<BlobGet>(id)
        .await
        .map_err(reading)?
        .ok_or_else(|| words::widget_no_code(program))?;
    let section = custom_section(&code, ::program::schema::SECTION)
        .ok_or_else(|| words::widget_no_schema(program));
    let interface = section.and_then(|section| {
        let interface = interface_of(program, section)?;
        store::set(host, &key, Some(&(entry.code, section.to_vec())));
        Ok(interface)
    });
    schemas.borrow_mut().insert(entry.code, interface.clone());
    interface
}

// The interface a schema section names.
fn interface_of(program: &str, section: &[u8]) -> Schema {
    let read = Interface::read(section).map_err(|_| words::widget_bad_schema(program))?;
    Ok(Rc::new(read))
}

// The payload of a wasm module's custom section `name`, by its section
// headers alone.
fn custom_section<'m>(module: &'m [u8], name: &str) -> Option<&'m [u8]> {
    let mut rest = module.strip_prefix(b"\0asm\x01\0\0\0")?;
    while let Some((&id, after)) = rest.split_first() {
        let (size, after) = leb(after)?;
        let body = after.get(..size)?;
        rest = &after[size..];
        if id == 0 {
            let (length, named) = leb(body)?;
            if named.get(..length)? == name.as_bytes() {
                return Some(&named[length..]);
            }
        }
    }
    None
}

// An unsigned LEB128 u32 and what follows it.
fn leb(bytes: &[u8]) -> Option<(usize, &[u8])> {
    let mut value = 0u32;
    for (index, byte) in bytes.iter().take(5).enumerate() {
        value |= u32::from(byte & 0x7f).checked_shl(7 * index as u32)?;
        if byte & 0x80 == 0 {
            return Some((value as usize, &bytes[index + 1..]));
        }
    }
    None
}

#[cfg(test)]
pub(crate) fn module_with(name: &str, payload: &[u8]) -> Vec<u8> {
    fn leb(mut value: usize, out: &mut Vec<u8>) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            match value {
                0 => return out.push(byte),
                _ => out.push(byte | 0x80),
            }
        }
    }
    let mut body = Vec::new();
    leb(name.len(), &mut body);
    body.extend(name.as_bytes());
    body.extend(payload);
    let mut module = b"\0asm\x01\0\0\0".to_vec();
    module.push(0);
    leb(body.len(), &mut module);
    module.extend(body);
    module
}
