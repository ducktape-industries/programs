//! A grep for several patterns: each hit names the patterns its line holds, ASCII letters fold on
//! ask, patterns spanning different line counts give a hit each in line order up to the last line,
//! and pages resume inside a file without losing or repeating a hit.
use super::*;

fn grep_any(
    host: &MockHost,
    patterns: &[&str],
    fold: Fold,
    page: PageRequest,
) -> PageResponse<PatternHit> {
    let query = Query::GrepAny {
        patterns: patterns.iter().map(|pattern| pattern.to_string()).collect(),
        fold,
        prefix: "/shared/".into(),
        at: None,
        page,
    };
    let Reply::Found { hits, .. } = ask(host, query).unwrap() else {
        panic!("a grep for patterns answers Found")
    };
    hits
}

type Seen = (String, u64, String, Vec<u32>);

fn seen(hits: Vec<PatternHit>) -> Vec<Seen> {
    hits.into_iter()
        .map(|hit| (hit.path, hit.line, hit.text, hit.patterns))
        .collect()
}

fn hit(path: &str, line: u64, text: &str, patterns: &[u32]) -> Seen {
    (path.to_owned(), line, text.to_owned(), patterns.to_vec())
}

#[test]
fn each_hit_names_the_patterns_its_line_holds_and_ascii_letters_fold_on_ask() {
    let host = host();
    let text = "Alpha beta\nGAMMA\nalpha and gamma\nÄRGER\n";
    commit(&host, ada(), None, vec![put(&host, "/shared/a.md", text)]).unwrap();
    let patterns = ["alpha", "gamma", "ärger"];
    let exact = grep_any(&host, &patterns, Fold::Exact, PageRequest::first(10));
    let both = hit("/shared/a.md", 3, "alpha and gamma", &[0, 1]);
    assert_eq!(seen(exact.items), std::slice::from_ref(&both));
    let folded = grep_any(&host, &patterns, Fold::Ascii, PageRequest::first(10));
    assert_eq!(
        seen(folded.items),
        [
            hit("/shared/a.md", 1, "Alpha beta", &[0]),
            hit("/shared/a.md", 2, "GAMMA", &[1]),
            both,
        ]
    );
}

#[test]
fn patterns_spanning_different_line_counts_give_a_hit_each_in_line_order_up_to_the_last_line() {
    let host = host();
    let call = "foo(\n  Bar)\nend bar";
    commit(
        &host,
        ada(),
        None,
        vec![put(&host, "/shared/call.rs", call)],
    )
    .unwrap();
    let patterns = ["bar", "FOO(\n  bar)", "foo"];
    let found = grep_any(&host, &patterns, Fold::Ascii, PageRequest::first(10));
    assert_eq!(
        seen(found.items),
        [
            hit("/shared/call.rs", 1, "foo(", &[2]),
            hit("/shared/call.rs", 1, "foo(\n  Bar)", &[1]),
            hit("/shared/call.rs", 2, "  Bar)", &[0]),
            hit("/shared/call.rs", 3, "end bar", &[0]),
        ]
    );
}

#[test]
fn a_grep_for_no_pattern_or_an_empty_one_is_refused() {
    let host = host();
    for patterns in [Vec::new(), vec!["x".to_owned(), String::new()]] {
        let query = Query::GrepAny {
            patterns,
            fold: Fold::Exact,
            prefix: "/".into(),
            at: None,
            page: PageRequest::first(1),
        };
        assert_eq!(ask(&host, query).unwrap_err().code, code::INVALID_INPUT);
    }
}

#[test]
fn pages_of_several_patterns_resume_inside_a_file_without_losing_or_repeating_a_hit() {
    let host = host();
    let hay = "hay hay hay hay\n";
    let lines_a_chunk = CHUNK_SIZE as usize / hay.len();
    let mut big = hay.repeat(lines_a_chunk - 2);
    big.push_str(&format!("{:>31}\n", "Alpha"));
    big.push_str("BETA\n");
    big.push_str(&hay.repeat(1000));
    big.push_str("a Needle past the first chunk\n");
    big.push_str(&hay.repeat(lines_a_chunk));
    big.push_str("needle\n");
    let mut last = hay.repeat(lines_a_chunk + 7);
    last.push_str("the NEEDLE at the end");
    let files = [
        ("/shared/big/a", big),
        ("/shared/big/b", "one needle\nalpha\nbeta\n".to_owned()),
        ("/shared/big/c", last),
    ];
    let changes = files
        .iter()
        .map(|(path, text)| put(&host, path, text))
        .collect();
    commit(&host, ada(), None, changes).unwrap();
    let patterns = ["needle", "alpha\nbeta"];
    let mut expected = Vec::new();
    for (path, text) in &files {
        let lines: Vec<&str> = text.split('\n').collect();
        for start in 0..lines.len() {
            for span in [1, 2] {
                let Some(window) = lines.get(start..start + span) else {
                    continue;
                };
                let joined = window.join("\n");
                let lower = joined.to_ascii_lowercase();
                let held: Vec<u32> = (0..patterns.len() as u32)
                    .filter(|index| {
                        let pattern = patterns[*index as usize];
                        pattern.split('\n').count() == span && lower.contains(pattern)
                    })
                    .collect();
                if !held.is_empty() {
                    expected.push(hit(path, start as u64 + 1, &joined, &held));
                }
            }
        }
    }
    let mut found = Vec::new();
    let mut pages = Vec::new();
    let mut page = PageRequest::first(50);
    loop {
        let hits = grep_any(&host, &patterns, Fold::Ascii, page);
        pages.push(hits.items.len());
        found.extend(seen(hits.items));
        let Some(next) = hits.next else {
            break;
        };
        page = PageRequest::resume(Some(next), 50);
    }
    assert!(
        pages.len() > 2 && pages[0] < expected.len(),
        "pages of {pages:?} hits"
    );
    assert_eq!(found, expected);
}
