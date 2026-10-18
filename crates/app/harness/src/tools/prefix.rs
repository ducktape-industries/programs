//! A tool call's input as far as it has arrived: the longest valid JSON its text begins with,
//! open strings closed and open objects and arrays closed, and the top-level fields already
//! whole. A tool that shows its call reads it each time more input arrives.
use serde_json::{Map, Value};

#[derive(Debug, PartialEq)]
pub(crate) struct Prefix {
    pub arrived: Value,
    pub whole: Map<String, Value>,
}

impl Prefix {
    pub fn arriving(&self, key: &str) -> Option<&Value> {
        self.arrived.get(key)
    }

    pub fn settled(&self, key: &str) -> Option<&Value> {
        self.whole.get(key)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Expect {
    Key,
    Colon,
    Value,
    Next,
}

#[derive(Clone, Copy, PartialEq)]
enum Open {
    Object(Expect),
    Array(Expect),
}

#[derive(Clone, Copy, PartialEq)]
enum Text {
    Outside,
    Key,
    Value,
}

#[derive(Clone, Copy)]
struct Escape {
    from: usize,
    hex: Option<u8>,
}

struct Cut {
    at: usize,
    closers: String,
}

struct Scan {
    stack: Vec<Open>,
    text: Text,
    escape: Option<Escape>,
    scalar: bool,
    arrived: Option<Cut>,
    whole: Option<Cut>,
}

impl Scan {
    fn closers(&self) -> String {
        self.stack
            .iter()
            .rev()
            .map(|open| match open {
                Open::Object(_) => '}',
                Open::Array(_) => ']',
            })
            .collect()
    }

    fn cut(&mut self, at: usize) {
        let closers = self.closers();
        let in_root = self.stack.len() <= 1;
        if in_root {
            self.whole = Some(Cut {
                at,
                closers: closers.clone(),
            });
        }
        self.arrived = Some(Cut { at, closers });
    }

    fn valued(&mut self, at: usize) {
        if let Some(top) = self.stack.last_mut() {
            *top = match top {
                Open::Object(_) => Open::Object(Expect::Next),
                Open::Array(_) => Open::Array(Expect::Next),
            };
        }
        self.cut(at);
    }

    fn expecting(&self) -> Option<Expect> {
        match self.stack.last() {
            Some(Open::Object(expect)) | Some(Open::Array(expect)) => Some(*expect),
            None => Some(Expect::Value),
        }
    }

    fn end_scalar(&mut self, at: usize) {
        if self.scalar {
            self.scalar = false;
            self.valued(at);
        }
    }

    fn escaping(&mut self, escape: Escape, c: char) {
        self.escape = match (escape.hex, c) {
            (None, 'u') => Some(Escape {
                hex: Some(4),
                ..escape
            }),
            (None, _) | (Some(1), _) => None,
            (Some(left), _) => Some(Escape {
                hex: Some(left - 1),
                ..escape
            }),
        };
    }

    fn in_text(&mut self, at: usize, c: char) {
        if let Some(escape) = self.escape {
            return self.escaping(escape, c);
        }
        match c {
            '\\' => {
                self.escape = Some(Escape {
                    from: at,
                    hex: None,
                })
            }
            '"' => {
                let was = self.text;
                self.text = Text::Outside;
                match was {
                    Text::Key => self.set_top(Open::Object(Expect::Colon)),
                    Text::Value => self.valued(at + 1),
                    Text::Outside => {}
                }
            }
            _ => {}
        }
    }

    fn set_top(&mut self, open: Open) {
        if let Some(top) = self.stack.last_mut() {
            *top = open;
        }
    }

    fn step(&mut self, at: usize, c: char) {
        if self.text != Text::Outside {
            return self.in_text(at, c);
        }
        let delimits = matches!(c, ',' | '}' | ']' | ':') || c.is_whitespace();
        if delimits {
            self.end_scalar(at);
        }
        match c {
            '{' => {
                self.stack.push(Open::Object(Expect::Key));
                self.cut(at + 1);
            }
            '[' => {
                self.stack.push(Open::Array(Expect::Value));
                self.cut(at + 1);
            }
            '}' | ']' => {
                self.stack.pop();
                self.valued(at + 1);
            }
            '"' => {
                self.text = match self.expecting() {
                    Some(Expect::Key) => Text::Key,
                    _ => Text::Value,
                };
                self.escape = None;
            }
            ':' => self.set_top(Open::Object(Expect::Value)),
            ',' => match self.stack.last() {
                Some(Open::Object(_)) => self.set_top(Open::Object(Expect::Key)),
                Some(Open::Array(_)) => self.set_top(Open::Array(Expect::Value)),
                None => {}
            },
            c if c.is_whitespace() => {}
            _ => self.scalar = true,
        }
    }
}

fn closed(text: &str, cut: &Cut, quote: bool) -> Option<Value> {
    let mut whole = text[..cut.at].to_owned();
    if quote {
        whole.push('"');
    }
    whole.push_str(&cut.closers);
    serde_json::from_str(&whole).ok()
}

pub(crate) fn prefix(text: &str) -> Prefix {
    let mut scan = Scan {
        stack: Vec::new(),
        text: Text::Outside,
        escape: None,
        scalar: false,
        arrived: None,
        whole: None,
    };
    for (at, c) in text.char_indices() {
        scan.step(at, c);
    }
    let open_value = scan.text == Text::Value;
    let arrived = match open_value {
        true => {
            let at = scan.escape.map_or(text.len(), |escape| escape.from);
            let cut = Cut {
                at,
                closers: scan.closers(),
            };
            closed(text, &cut, true)
        }
        false => scan
            .arrived
            .as_ref()
            .and_then(|cut| closed(text, cut, false)),
    };
    let whole = scan
        .whole
        .as_ref()
        .and_then(|cut| closed(text, cut, false))
        .and_then(|value| match value {
            Value::Object(fields) => Some(fields),
            _ => None,
        })
        .unwrap_or_default();
    Prefix {
        arrived: arrived.unwrap_or(Value::Null),
        whole,
    }
}

#[cfg(test)]
mod tests;
