//! Paths as files spells them (NFC, absolute, `/`-separated), and the namespace roots the chain
//! alone writes: `/home`, `/shared`, each `/home/<name>`, and `/chat` with each channel and message
//! folder in it, which chat owns. Who else writes a path is its write list in the access program.
use ::program::{Error, invalid, unauthorized};
use unicode_normalization::UnicodeNormalization;

pub const CHAT: &str = "chat";

pub fn segments(path: &str) -> Result<Vec<String>, Error> {
    if !path.starts_with('/') {
        return Err(invalid(format!(
            "{path:?} is not absolute: a path starts with /"
        )));
    }
    if path.contains('\0') {
        return Err(invalid("a path holds no NUL byte"));
    }
    let composed = path.nfc().eq(path.chars());
    if !composed {
        return Err(invalid(format!("{path:?} is not in NFC form")));
    }
    if path == "/" {
        return Ok(Vec::new());
    }
    let mut segments = Vec::new();
    for segment in path[1..].split('/') {
        let navigates = segment.is_empty() || segment == "." || segment == "..";
        if navigates {
            return Err(invalid(format!("{path:?} has an empty, . or .. segment")));
        }
        segments.push(segment.to_owned());
    }
    Ok(segments)
}

pub fn join(segments: &[String]) -> String {
    format!("/{}", segments.join("/"))
}

pub fn is_namespace_root(segments: &[String]) -> bool {
    match segments {
        [root] => root == "home" || root == "shared" || root == CHAT,
        [folder @ .., _] => holds_namespace_roots(folder),
        [] => false,
    }
}

pub fn holds_namespace_roots(folder: &[String]) -> bool {
    match folder {
        [root] => root == "home" || root == CHAT,
        [root, _] => root == CHAT,
        _ => false,
    }
}

pub fn is_chat_folder(segments: &[String]) -> bool {
    segments.first().is_some_and(|root| root == CHAT) && is_namespace_root(segments)
}

pub(crate) fn beneath_the_roots(segments: &[String]) -> Result<(), Error> {
    if !is_namespace_root(segments) {
        return Ok(());
    }
    Err(unauthorized(format!(
        "{} is a namespace root: /home, /shared, each /home/<name>, and /chat with its channel \
         and message folders are written by the chain alone, and anyone writes beneath them \
         where no write list says otherwise",
        join(segments)
    )))
}

pub fn is_home(segments: &[String]) -> Option<&str> {
    match segments {
        [root, name] if root == "home" => Some(name),
        _ => None,
    }
}

pub fn change_path(path: &str) -> Result<Vec<String>, Error> {
    let segments = segments(path)?;
    if segments.is_empty() {
        return Err(invalid("the root is not a path a change names"));
    }
    Ok(segments)
}

#[cfg(test)]
mod tests;
