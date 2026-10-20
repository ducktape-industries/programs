//! Lines of a text, as `files_read` and `forge_read` take them: a range counted from 1, or the
//! section a markdown heading opens, up to the next heading of its level or a higher one.
use ::program::{Error, invalid, not_found};

#[derive(Debug)]
pub(crate) struct Picked {
    pub text: String,
    pub first: u64,
    pub last: u64,
    pub total: u64,
}

fn picked(all: &[&str], first: u64, last: u64) -> Picked {
    let text = all
        .iter()
        .skip(first as usize - 1)
        .take((last + 1).saturating_sub(first) as usize)
        .copied()
        .collect();
    Picked {
        text,
        first,
        last,
        total: all.len() as u64,
    }
}

pub(crate) fn range(text: &str, from: u64, to: u64) -> Result<Picked, Error> {
    if from == 0 || to < from {
        return Err(invalid(
            "lines is [from, to], counted from 1, with to at least from",
        ));
    }
    let all: Vec<&str> = text.split_inclusive('\n').collect();
    let total = all.len() as u64;
    Ok(picked(&all, from.min(total + 1), to.min(total)))
}

fn level(line: &str) -> Option<usize> {
    let marks = line.bytes().take_while(|&byte| byte == b'#').count();
    let rest = &line[marks..];
    let spaced = rest.is_empty() || rest.starts_with([' ', '\t']);
    ((1..=6).contains(&marks) && spaced).then_some(marks)
}

fn fence(line: &str) -> Option<(char, usize)> {
    let line = line.trim_start();
    let mark = line
        .chars()
        .next()
        .filter(|mark| matches!(mark, '`' | '~'))?;
    let count = line.chars().take_while(|&c| c == mark).count();
    (count >= 3).then_some((mark, count))
}

fn headings(all: &[&str]) -> Vec<(usize, usize)> {
    let mut open: Option<(char, usize)> = None;
    let mut found = Vec::new();
    for (index, line) in all.iter().enumerate() {
        let line = line.trim_end();
        match (open, fence(line)) {
            (None, Some(opened)) => open = Some(opened),
            (Some((mark, count)), Some((closing, closed)))
                if closing == mark && closed >= count && line.trim().chars().all(|c| c == mark) =>
            {
                open = None
            }
            (None, None) => found.extend(level(line).map(|level| (index, level))),
            _ => {}
        }
    }
    found
}

pub(crate) fn section(text: &str, heading: &str) -> Result<Picked, Error> {
    let wanted = heading.trim_end();
    let Some(depth) = level(wanted) else {
        return Err(invalid(
            "heading is a markdown heading line, one to six # and a space before its words, like \"## Step 2\"",
        ));
    };
    let all: Vec<&str> = text.split_inclusive('\n').collect();
    let found = headings(&all);
    let starts: Vec<usize> = found
        .iter()
        .map(|&(index, _)| index)
        .filter(|&index| all[index].trim_end() == wanted)
        .collect();
    let start = match starts.as_slice() {
        [start] => *start,
        [] => return Err(not_found(format!("no heading of the file is {wanted}"))),
        many => {
            let at: Vec<String> = many.iter().map(|index| (index + 1).to_string()).collect();
            return Err(invalid(format!(
                "{wanted} heads the file {} times, at lines {}; read one by its lines",
                many.len(),
                at.join(", ")
            )));
        }
    };
    let end = found
        .iter()
        .find(|&&(index, level)| index > start && level <= depth)
        .map_or(all.len(), |&(index, _)| index);
    Ok(picked(&all, start as u64 + 1, end as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Plan\nintro\n## Step 1\none\n```sh\n# not a heading\n## Step 2\n```\n### Detail\nmore\n## Step 2\ntwo\n# Next\nlast\n";

    fn shown(picked: Picked) -> (String, u64, u64, u64) {
        (picked.text, picked.first, picked.last, picked.total)
    }

    #[test]
    fn a_heading_reads_to_the_next_heading_of_its_level_or_a_higher_one() {
        let one = "## Step 1\none\n```sh\n# not a heading\n## Step 2\n```\n### Detail\nmore\n";
        assert_eq!(
            shown(section(DOC, "## Step 1").unwrap()),
            (one.to_owned(), 3, 10, 14),
            "a fenced line and a deeper heading stay inside"
        );
        assert_eq!(
            shown(section(DOC, "## Step 2  ").unwrap()),
            ("## Step 2\ntwo\n".to_owned(), 11, 12, 14),
            "the fenced copy is no heading"
        );
        assert_eq!(
            shown(section(DOC, "# Next").unwrap()),
            ("# Next\nlast\n".to_owned(), 13, 14, 14)
        );
        assert_eq!(section(DOC, "# Plan").unwrap().last, 12);
        let crlf = "## A\r\nx\r\n## B\r\ny\r\n";
        assert_eq!(section(crlf, "## A").unwrap().text, "## A\r\nx\r\n");
        let tilde = "~~~~\n## In\n~~~\n## Still in\n~~~~\n## Out\n";
        assert!(section(tilde, "## Still in").is_err());
        assert_eq!(section(tilde, "## Out").unwrap().first, 6);
    }

    #[test]
    fn a_heading_is_matched_whole_and_once() {
        let missing = section(DOC, "## Step").unwrap_err();
        assert!(missing.message.contains("no heading"), "{missing:?}");
        let bare = section(DOC, "Step 1").unwrap_err();
        assert!(bare.message.contains("one to six #"), "{bare:?}");
        let hashtag = section("#tag\n", "#tag").unwrap_err();
        assert!(hashtag.message.contains("one to six #"), "{hashtag:?}");
        let twice = section("## A\nx\n## A\ny\n", "## A").unwrap_err();
        assert!(
            twice.message.contains("2 times, at lines 1, 3"),
            "{twice:?}"
        );
    }

    #[test]
    fn a_range_counts_lines_from_one_and_stops_at_the_end() {
        assert_eq!(
            shown(range("a\nb\nc", 2, 9).unwrap()),
            ("b\nc".to_owned(), 2, 3, 3)
        );
        assert_eq!(
            shown(range("a\nb\n", 5, 6).unwrap()),
            (String::new(), 3, 2, 2)
        );
        assert!(range("a\n", 0, 1).is_err());
        assert!(range("a\n", 3, 2).is_err());
        assert_eq!(
            range("é\n🦆 한\nz\n", 2, 2).unwrap().text,
            "🦆 한\n",
            "a line keeps every byte of its characters"
        );
    }
}
