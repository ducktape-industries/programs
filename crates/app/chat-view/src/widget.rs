//! Widgets: a TOML file in files that a channel's meta names (`widget.header`, `widget.pinned`,
//! `widget.side`), drawn from one read-only query of any program (`docs/widgets.md`). This is the
//! file read, its templates and its filters; `widget/live.rs` is the data path.
mod live;

use std::collections::BTreeMap;

use chat::Principal;
use chat::view::Names;
use serde::Deserialize;
use serde_json::Value;
use view::{Hsla, Theme};

#[cfg(test)]
pub(crate) use live::module_with;
pub(crate) use live::{Shown, Text, Widgets, file_bytes};

// Where a channel shows a widget: the meta key that names its file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Slot {
    Header,
    Pinned,
    Side,
}

impl Slot {
    pub(crate) const ALL: [Slot; 3] = [Slot::Header, Slot::Pinned, Slot::Side];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Slot::Header => "widget.header",
            Slot::Pinned => "widget.pinned",
            Slot::Side => "widget.side",
        }
    }

    // the word element ids carry, so one file in two slots draws two
    pub(crate) fn name(self) -> &'static str {
        match self {
            Slot::Header => "header",
            Slot::Pinned => "pinned",
            Slot::Side => "side",
        }
    }
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Widget {
    pub(crate) title: String,
    pub(crate) source: Source,
    pub(crate) view: Shape,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    pub(crate) program: String,
    pub(crate) query: Value,
    #[serde(default)]
    pub(crate) rows: String,
    #[serde(default)]
    pub(crate) cursor: Option<String>,
    // a file read beside the source, its path a template over the first row
    #[serde(default)]
    pub(crate) text: Option<TextSource>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TextSource {
    pub(crate) path: String,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    List,
    Columns,
    Table,
    Stat,
    Checklist,
    Text,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::List => "list",
            Kind::Columns => "columns",
            Kind::Table => "table",
            Kind::Stat => "stat",
            Kind::Checklist => "checklist",
            Kind::Text => "text",
        }
    }
}

// A row's fields and the values each may be: every field must hold one.
pub(crate) type Filter = BTreeMap<String, Value>;

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Shape {
    pub(crate) kind: Kind,
    #[serde(default, rename = "where")]
    pub(crate) only: Filter,
    #[serde(default)]
    pub(crate) except: Filter,
    #[serde(default)]
    pub(crate) item: Option<String>,
    #[serde(default)]
    pub(crate) group_by: Option<String>,
    #[serde(default)]
    pub(crate) order: Vec<String>,
    #[serde(default)]
    pub(crate) columns: Vec<Column>,
    #[serde(default)]
    pub(crate) value: Option<String>,
    #[serde(default)]
    pub(crate) label: Option<String>,
    #[serde(default)]
    pub(crate) checked: Option<Filter>,
    #[serde(default)]
    pub(crate) on_press: Option<Press>,
    #[serde(default)]
    pub(crate) empty: Option<String>,
    // a list in parts (`ui/widget/sections.rs`): the sections that start
    // folded, the row's parts beside `item`, its badge, and each section's
    // icon and tone
    #[serde(default)]
    pub(crate) fold: Vec<String>,
    #[serde(default)]
    pub(crate) lead: Option<String>,
    #[serde(default)]
    pub(crate) chip: Option<String>,
    #[serde(default)]
    pub(crate) trail: Option<String>,
    #[serde(default)]
    pub(crate) badge: Option<String>,
    #[serde(default)]
    pub(crate) badges: Vec<Badge>,
    #[serde(default)]
    pub(crate) icons: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) tones: BTreeMap<String, String>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Column {
    pub(crate) title: String,
    pub(crate) value: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Press {
    pub(crate) open: String,
}

// What a row wears when `badge` fills to `when`: a mark in a tone, and what
// it says to assistive tech.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Badge {
    pub(crate) when: String,
    pub(crate) mark: String,
    #[serde(default)]
    pub(crate) tone: Option<String>,
    #[serde(default)]
    pub(crate) label: Option<String>,
}

// The chat theme's tones a file may name, for a section's bar and a mark.
pub(crate) const TONES: [&str; 9] = [
    "accent",
    "success",
    "warning",
    "danger",
    "agent",
    "foreground",
    "muted",
    "faint",
    "link",
];

pub(crate) fn tone(theme: &Theme, name: Option<&str>) -> Option<Hsla> {
    Some(match name? {
        "accent" => theme.accent,
        "success" => theme.success,
        "warning" => theme.warning,
        "danger" => theme.danger,
        "agent" => theme.agent,
        "foreground" => theme.foreground,
        "muted" => theme.muted,
        "faint" => theme.faint,
        "link" => theme.link,
        _ => return None,
    })
}

// The widget a file's text describes, or why it describes none.
pub(crate) fn parse(text: &str) -> Result<Widget, String> {
    let widget: Widget = toml::from_str(text).map_err(|error| error.to_string())?;
    let shape = &widget.view;
    let needs = |field: &str| format!("a {} widget needs `{field}`", shape.kind.name());
    let unknown = shape.tone_names().find(|name| !TONES.contains(name));
    let unknown = unknown.map(str::to_owned);
    match shape.kind {
        Kind::List | Kind::Checklist if shape.item.is_none() => Err(needs("item")),
        Kind::Columns if shape.group_by.is_none() => Err(needs("group_by")),
        Kind::Columns if shape.item.is_none() => Err(needs("item")),
        Kind::Table if shape.columns.is_empty() => Err(needs("columns")),
        Kind::Stat if shape.value.is_none() => Err(needs("value")),
        Kind::Text if widget.source.text.is_none() => Err(needs("[source.text]")),
        _ if widget.source.cursor.is_some() && widget.source.rows.is_empty() => {
            Err("`cursor` needs `rows` to name the list beside the reply's `next`".into())
        }
        _ => match unknown {
            Some(name) => Err(format!(
                "`{name}` is no tone; a tone is one of {}",
                TONES.join(", ")
            )),
            None => Ok(widget),
        },
    }
}

// The value at a dotted path: a name per object, a number per list; the
// empty path is the value itself.
pub(crate) fn at<'v>(value: &'v Value, path: &str) -> Option<&'v Value> {
    if path.is_empty() {
        return Some(value);
    }
    path.split('.').try_fold(value, |value, step| match value {
        Value::Object(fields) => borsh_json::entry(fields, step),
        Value::Array(items) => items.get(step.parse::<usize>().ok()?),
        _ => None,
    })
}

// A value as a template writes it: a string as it is, null as nothing,
// anything else as JSON.
pub(crate) fn text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

// What a template is filled from besides its value: the reader, the
// names chat knows, for a stat how many rows there are, the chain's
// newest block time (ms), which `|ago` counts from, and the open room.
pub(crate) struct Scope<'a> {
    pub(crate) me: Option<u64>,
    pub(crate) names: Option<&'a Names>,
    pub(crate) count: Option<usize>,
    pub(crate) now: Option<u64>,
    pub(crate) room: Option<&'a str>,
}

// `{path}` the value there, `{path|name}` an account number's name,
// `{path|ago}` how long before the chain's newest block a time (ms) was,
// `{me}` the reader's number, `{room}` the open room's id, `{count}` a
// stat's rows; a `{` with no `}` after it is written as it is.
pub(crate) fn fill(template: &str, value: &Value, scope: &Scope) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        out.push_str(&placeholder(&after[..close], value, scope));
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

fn placeholder(inside: &str, value: &Value, scope: &Scope) -> String {
    let (path, filter) = match inside.split_once('|') {
        Some((path, filter)) => (path.trim(), Some(filter.trim())),
        None => (inside.trim(), None),
    };
    let me = scope.me.map(Value::from);
    let count = scope.count.map(Value::from);
    let room = scope.room.map(Value::from);
    let found = match path {
        "me" => me.as_ref(),
        "room" => room.as_ref(),
        "count" if count.is_some() => count.as_ref(),
        path => at(value, path),
    };
    match (filter, found.and_then(Value::as_u64), scope.names) {
        (Some("ago"), Some(then), _) => scope
            .now
            .map(|now| view::design::ago(now, then))
            .unwrap_or_default(),
        (Some("name"), Some(number), Some(names)) => names.member(&Principal::Account(number)),
        (Some("name"), Some(number), None) => Names::empty().member(&Principal::Account(number)),
        _ => text(found),
    }
}

// The source's query as the reader asks it: a string that is `{me}` alone
// becomes their number, and `{me}` inside a string their number's digits.
pub(crate) fn with_me(query: &Value, me: Option<u64>) -> Value {
    match query {
        Value::String(text) if text == "{me}" => me.map_or(Value::Null, Value::from),
        Value::String(text) => {
            let digits = me.map(|me| me.to_string()).unwrap_or_default();
            Value::String(text.replace("{me}", &digits))
        }
        Value::Array(items) => Value::Array(items.iter().map(|item| with_me(item, me)).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(name, field)| (name.clone(), with_me(field, me)))
                .collect(),
        ),
        other => other.clone(),
    }
}

// The query with `{room}` in its strings as the open room's id.
pub(crate) fn with_room(query: &Value, room: Option<&str>) -> Value {
    match query {
        Value::String(text) => Value::String(text.replace("{room}", room.unwrap_or_default())),
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| with_room(item, room)).collect())
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(name, field)| (name.clone(), with_room(field, room)))
                .collect(),
        ),
        other => other.clone(),
    }
}

// `value` put at `path` in `query`, making each object on the way.
pub(crate) fn put(query: &mut Value, path: &str, value: Value) -> Result<(), String> {
    let mut here = query;
    let mut steps = path.split('.').peekable();
    while let Some(step) = steps.next() {
        let Value::Object(fields) = here else {
            return Err(format!("the query has no object at `{step}` of `{path}`"));
        };
        if steps.peek().is_none() {
            fields.insert(step.to_owned(), value);
            return Ok(());
        }
        here = fields
            .entry(step.to_owned())
            .or_insert_with(|| Value::Object(Default::default()));
    }
    Err("the cursor names no field".into())
}

// Whether every field the filter names holds one of its values, each
// compared as a template writes it.
pub(crate) fn holds(filter: &Filter, row: &Value) -> bool {
    filter.iter().all(|(path, wanted)| {
        let found = text(at(row, path));
        match wanted {
            Value::Array(values) => values.iter().any(|value| text(Some(value)) == found),
            value => text(Some(value)) == found,
        }
    })
}

impl Shape {
    // What the view says when it has no rows: the file's `empty`, else the
    // default.
    pub(crate) fn nothing(&self) -> String {
        let empty = self.empty.clone();
        empty.unwrap_or_else(|| crate::words::WIDGET_EMPTY.to_owned())
    }

    // The rows the view draws: those `where` keeps and `except` leaves.
    pub(crate) fn rows<'v>(&self, rows: &'v [Value]) -> Vec<&'v Value> {
        rows.iter()
            .filter(|row| self.only.is_empty() || holds(&self.only, row))
            .filter(|row| self.except.is_empty() || !holds(&self.except, row))
            .collect()
    }

    // A column per `group_by` value: those `order` names first, in its
    // order and drawn even when empty, then the others as they come.
    pub(crate) fn columns<'v>(&self, rows: &[&'v Value]) -> Vec<(String, Vec<&'v Value>)> {
        let by = self.group_by.as_deref().unwrap_or_default();
        let mut columns: Vec<(String, Vec<&Value>)> = self
            .order
            .iter()
            .map(|value| (value.clone(), Vec::new()))
            .collect();
        for row in rows {
            let value = text(at(row, by));
            match columns.iter_mut().find(|(name, _)| *name == value) {
                Some((_, column)) => column.push(row),
                None => columns.push((value, vec![row])),
            }
        }
        columns
    }

    // A list's sections: each `group_by` value that holds rows, those
    // `order` names first; ungrouped, one section of every row.
    pub(crate) fn sections<'v>(&self, rows: &[&'v Value]) -> Vec<(String, Vec<&'v Value>)> {
        if self.group_by.is_none() {
            return vec![(String::new(), rows.to_vec())];
        }
        let mut sections = self.columns(rows);
        sections.retain(|(_, items)| !items.is_empty());
        sections
    }

    // Whether a list draws its rows in parts: grouped, or naming a part
    // beside `item`.
    pub(crate) fn in_parts(&self) -> bool {
        let parts = [&self.lead, &self.chip, &self.trail, &self.badge];
        self.group_by.is_some() || parts.iter().any(|part| part.is_some())
    }

    // The badge a row wears: the one whose `when` its `badge` fills to.
    pub(crate) fn badge_of(&self, row: &Value, scope: &Scope) -> Option<&Badge> {
        let said = fill(self.badge.as_deref()?, row, scope);
        self.badges.iter().find(|badge| badge.when == said)
    }

    // Every tone the file names.
    fn tone_names(&self) -> impl Iterator<Item = &str> {
        let badges = self.badges.iter().filter_map(|badge| badge.tone.as_deref());
        self.tones.values().map(String::as_str).chain(badges)
    }

    // Where pressing a row goes: `on_press.open` filled, if it fills.
    pub(crate) fn target(&self, row: &Value, scope: &Scope) -> Option<String> {
        let open = &self.on_press.as_ref()?.open;
        Some(fill(open, row, scope)).filter(|target| !target.trim().is_empty())
    }
}
