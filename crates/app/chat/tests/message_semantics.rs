//! Emphasis markers follow CommonMark's flanking rule, so an identifier or a
//! file name in a chat message is never read as italic.

use chat::{Mark, message::inline_spans};

type Row = (&'static str, &'static [(&'static str, bool, bool)]);

const ROWS: &[Row] = &[
    ("my_var_name", &[("my_var_name", false, false)]),
    (
        "see src/my_file_name.rs now",
        &[("see src/my_file_name.rs now", false, false)],
    ),
    ("__init__", &[("init", false, true)]),
    ("a__b__c", &[("a__b__c", false, false)]),
    ("_var_", &[("var", true, false)]),
    (
        "say _this_ now",
        &[
            ("say ", false, false),
            ("this", true, false),
            (" now", false, false),
        ],
    ),
    ("__bold__", &[("bold", false, true)]),
    (
        "un*believ*able",
        &[
            ("un", false, false),
            ("believ", true, false),
            ("able", false, false),
        ],
    ),
    ("a * b * c", &[("a * b * c", false, false)]),
    ("2 ** 3 ** 4", &[("2 ** 3 ** 4", false, false)]),
    (
        "*it* and **bo**",
        &[
            ("it", true, false),
            (" and ", false, false),
            ("bo", false, true),
        ],
    ),
];

#[test]
fn emphasis_follows_the_flanking_rule() {
    for (text, expected) in ROWS {
        let got: Vec<(String, bool, bool)> = inline_spans(text)
            .into_iter()
            .map(|span| {
                let italic = span.marks.contains(&Mark::Italic);
                let bold = span.marks.contains(&Mark::Bold);
                (span.text, italic, bold)
            })
            .collect();
        let want: Vec<(String, bool, bool)> = expected
            .iter()
            .map(|(text, italic, bold)| (text.to_string(), *italic, *bold))
            .collect();
        assert_eq!(got, want, "for `{text}`");
    }
}

type CodeRow = (&'static str, &'static [(&'static str, bool, bool)]);

const CODE_ROWS: &[CodeRow] = &[
    (
        "run `cargo test` now",
        &[
            ("run ", false, false),
            ("cargo test", true, false),
            (" now", false, false),
        ],
    ),
    ("a ` b", &[("a ` b", false, false)]),
    (
        "`**not bold** <@7> https://x`",
        &[("**not bold** <@7> https://x", true, false)],
    ),
    (
        "**a `b` c**",
        &[("a ", false, true), ("b", true, true), (" c", false, true)],
    ),
    ("`` a`b ``", &[("a`b", true, false)]),
    ("``x` y``", &[("x` y", true, false)]),
];

#[test]
fn a_backtick_run_opens_a_code_span_only_the_same_run_closes() {
    for (text, expected) in CODE_ROWS {
        let got: Vec<(String, bool, bool)> = inline_spans(text)
            .into_iter()
            .map(|span| {
                let code = span.marks.contains(&Mark::Code);
                let bold = span.marks.contains(&Mark::Bold);
                assert!(
                    !code
                        || !span
                            .marks
                            .iter()
                            .any(|m| matches!(m, Mark::Link(_) | Mark::Mention(_))),
                    "no link or mention inside code, for `{text}`"
                );
                (span.text, code, bold)
            })
            .collect();
        let want: Vec<(String, bool, bool)> = expected
            .iter()
            .map(|(text, code, bold)| (text.to_string(), *code, *bold))
            .collect();
        assert_eq!(got, want, "for `{text}`");
    }
}

#[test]
fn a_paragraph_opening_with_a_marker_reads_as_a_list_item() {
    use chat::{Block, ListMarker, list_item, parse_message};
    let item = |text: &str| {
        let blocks = parse_message(text);
        let [Block::Paragraph(spans)] = blocks.as_slice() else {
            panic!("`{text}` is one paragraph");
        };
        list_item(spans).map(|(marker, spans)| {
            let text: String = spans.iter().map(|span| span.text.as_str()).collect();
            (marker, text, spans.len())
        })
    };
    assert_eq!(
        item("- apples"),
        Some((ListMarker::Bullet, "apples".into(), 1))
    );
    assert_eq!(
        item("* pears"),
        Some((ListMarker::Bullet, "pears".into(), 1))
    );
    assert_eq!(
        item("12. twelfth"),
        Some((ListMarker::Ordered(12), "twelfth".into(), 1))
    );
    assert_eq!(
        item("- **ship** it"),
        Some((ListMarker::Bullet, "ship it".into(), 2))
    );
    for plain in ["-apples", "1.5 litres", ". x", "a - b", "**- x**", "*it*"] {
        assert_eq!(item(plain), None, "`{plain}` is no list item");
    }
    let blocks = parse_message("- a\n- b\n1. c");
    assert_eq!(blocks.len(), 3);
    assert!(
        blocks
            .iter()
            .all(|block| matches!(block, Block::Paragraph(spans) if list_item(spans).is_some()))
    );
}

// A bare link typed at a sentence's end leaves its full stop to the sentence.
#[test]
fn a_bare_links_trailing_punctuation_stays_prose() {
    let spans = inline_spans(
        "see duck://n-b5b6ea90/files/a/before-whole.png. Then (https://x.example/q?a=1.) too",
    );
    let links: Vec<&str> = spans
        .iter()
        .filter_map(|span| {
            span.marks.iter().find_map(|mark| match mark {
                Mark::Link(url) => Some(url.as_str()),
                _ => None,
            })
        })
        .collect();
    assert_eq!(
        links,
        [
            "duck://n-b5b6ea90/files/a/before-whole.png",
            "https://x.example/q?a=1."
        ]
    );
}
