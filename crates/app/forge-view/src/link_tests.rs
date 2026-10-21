//! A link's `blob/<ref>/<path>` and `tree/<ref>/<path>`: a ref with `/` in it, a tag, a commit,
//! a branch that no longer exists, and the Copy link a file on another ref mints.
use super::*;

const MANUAL: &str = "0000000000000000000000000000000000000002";
const TAGGED: &str = "0000000000000000000000000000000000000003";
const COMMIT: &str = "26607f522099476177a45a8058a93108fba5a84d";
const FEATURE: &str = "0000000000000000000000000000000000000004";
const FEATURE_SRC: &str = "0000000000000000000000000000000000000005";
const V1_TAG: &str = "0000000000000000000000000000000000000006";
const V1_BRANCH: &str = "0000000000000000000000000000000000000007";

fn with_refs(cx: &mut TestAppContext) {
    cx.host().handle::<Ask>(|query| {
        let mut reply = answer(&query, "default");
        let first = matches!(
            query,
            Query::Refs {
                page: PageRequest { after: None, .. },
                ..
            }
        );
        if let (true, Reply::Refs { page, .. }) = (first, &mut reply) {
            for (name, target) in [
                ("refs/heads/j-177/manual", MANUAL),
                ("refs/tags/j-177", TAGGED),
                ("refs/heads/feature", FEATURE),
                ("refs/heads/feature/src", FEATURE_SRC),
                ("refs/tags/v1", V1_TAG),
                ("refs/heads/v1", V1_BRANCH),
            ] {
                page.items.push(forge::RefInfo {
                    name: name.as_bytes().to_vec(),
                    target: target.into(),
                });
            }
        }
        Ok(reply)
    });
}

fn route(cx: &mut TestAppContext, view: &Entity<Forge>, to: &str) {
    cx.update(view, |forge, _, cx| forge.open_route(to, cx));
    cx.run_until_parked();
}

fn trees_at(cx: &TestAppContext) -> Vec<(String, Vec<u8>)> {
    cx.host()
        .requests::<Ask>()
        .into_iter()
        .filter_map(|query| match query {
            Query::Tree { at, path, .. } => Some((at, path)),
            _ => None,
        })
        .collect()
}

fn desktop() -> (TestAppContext, Entity<Forge>) {
    let (mut cx, view) = booted("default");
    with_refs(&mut cx);
    (cx, view)
}

fn phone() -> (TestAppContext, Entity<Forge>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx, "default");
    with_refs(&mut cx);
    let props = cx.host().stream::<HostSession>();
    let view = cx.open::<Forge>();
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    props.send(seat(b"owner", Some(1)));
    cx.run_until_parked();
    (cx, view)
}

#[test]
fn a_blob_link_opens_its_path_on_the_longest_ref_it_names() {
    let (mut cx, view) = desktop();
    route(&mut cx, &view, "project/blob/j-177/manual/src");
    view.read(|forge| {
        assert_eq!(
            forge.nav().rev.as_deref(),
            Some(&b"refs/heads/j-177/manual"[..])
        );
        assert!(forge.nav().gone.is_none());
        assert!(forge.nav().expanded.contains(b"src".as_slice()));
    });
    assert!(
        trees_at(&cx).contains(&(MANUAL.into(), b"src".to_vec())),
        "{:?}",
        trees_at(&cx)
    );
    assert!(!cx.has_text("No blob/j-177/manual/src on this ref."));
}

#[test]
fn a_tree_link_opens_a_tag_and_a_commit() {
    let (mut cx, view) = desktop();
    route(&mut cx, &view, "project/tree/j-177/src");
    view.read(|forge| assert_eq!(forge.nav().rev.as_deref(), Some(&b"refs/tags/j-177"[..])));
    assert!(trees_at(&cx).contains(&(TAGGED.into(), b"src".to_vec())));
    route(&mut cx, &view, &format!("project/blob/{COMMIT}/src"));
    view.read(|forge| assert_eq!(forge.nav().rev.as_deref(), Some(COMMIT.as_bytes())));
    assert!(trees_at(&cx).contains(&(COMMIT.into(), b"src".to_vec())));
}

#[test]
fn a_link_to_a_deleted_branch_says_so_and_opens_the_path_on_the_default_branch() {
    let (mut cx, view) = desktop();
    let before = trees_at(&cx).len();
    route(&mut cx, &view, "project/blob/j-9/gone/src/lib.rs");
    assert!(
        cx.has_text("Branch j-9/gone doesn’t exist; it may have merged."),
        "{:?}",
        cx.texts()
    );
    view.read(|forge| {
        assert_eq!(
            forge.nav().gone,
            Some(("j-9/gone".into(), "src/lib.rs".into()))
        );
        assert_eq!(forge.nav().rev, None);
    });
    let default = view
        .read(|forge| forge.head_oid())
        .expect("the default head");
    assert!(
        trees_at(&cx)[before..].iter().all(|(at, _)| *at == default),
        "only the default branch is read: {:?}",
        trees_at(&cx)
    );
    cx.simulate_click("forge-gone-open");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(forge.nav().gone, None);
        assert_eq!(forge.nav().rev, None);
        assert!(forge.nav().expanded.contains(b"src".as_slice()));
    });
    assert!(!cx.has_text("Branch j-9/gone doesn’t exist; it may have merged."));
}

#[test]
fn new_navigation_after_a_dead_link_drops_its_gone_branch() {
    let (mut cx, view) = desktop();
    let gone = "Branch j-9/gone doesn’t exist; it may have merged.";
    route(&mut cx, &view, "project/blob/j-9/gone/src/lib.rs");
    assert!(cx.has_text(gone), "{:?}", cx.texts());
    cx.update(&view, |forge, _, cx| {
        forge.open_file(
            b"empty.txt".to_vec(),
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391".into(),
            cx,
        )
    });
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.nav().gone, None));
    assert!(!cx.has_text(gone), "{:?}", cx.texts());
    assert!(cx.find("forge-blob-header").is_some(), "{:?}", cx.texts());
    route(&mut cx, &view, "project/blob/j-9/gone/src/lib.rs");
    assert!(cx.has_text(gone));
    cx.update(&view, |forge, _, cx| {
        forge.pick_ref(b"refs/heads/feature".to_vec(), cx)
    });
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.nav().gone, None));
    assert!(!cx.has_text(gone));
}

#[test]
fn a_copied_link_that_would_name_another_ref_names_the_commit() {
    let (mut cx, view) = desktop();
    route(&mut cx, &view, "project/blob/feature/empty.txt");
    view.read(|forge| {
        assert_eq!(forge.nav().rev.as_deref(), Some(&b"refs/heads/feature"[..]));
        assert_eq!(
            forge.blob_route("empty.txt"),
            ["blob", "feature", "empty.txt"]
        );
        assert_eq!(
            forge.blob_route("src/lib.rs"),
            ["blob", FEATURE, "src", "lib.rs"],
            "feature/src is a branch of its own"
        );
    });
    route(&mut cx, &view, "project/blob/v1/empty.txt");
    view.read(|forge| {
        assert_eq!(forge.nav().rev.as_deref(), Some(&b"refs/heads/v1"[..]));
        assert_eq!(forge.blob_route("empty.txt"), ["blob", "v1", "empty.txt"]);
    });
    cx.update(&view, |forge, _, cx| {
        forge.pick_ref(b"refs/tags/v1".to_vec(), cx)
    });
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(
            forge.blob_route("empty.txt"),
            ["blob", V1_TAG, "empty.txt"],
            "branch v1 would take the tag's name"
        );
    });
    for (copied, rev) in [
        (format!("project/blob/{FEATURE}/src/lib.rs"), FEATURE),
        (format!("project/blob/{V1_TAG}/empty.txt"), V1_TAG),
    ] {
        route(&mut cx, &view, &copied);
        view.read(|forge| assert_eq!(forge.nav().rev.as_deref(), Some(rev.as_bytes())));
    }
}

#[test]
fn a_phone_says_a_branch_is_gone_on_its_own_page() {
    let (mut cx, view) = phone();
    route(&mut cx, &view, "project/blob/j-9/gone/src/lib.rs");
    assert!(cx.find("forge-page-missing").is_some(), "{:?}", cx.texts());
    assert!(cx.has_text("Branch j-9/gone doesn’t exist; it may have merged."));
    assert!(cx.find("forge-gone-open").is_some());
}

#[test]
fn copy_link_names_the_ref_only_off_the_default_branch() {
    let (mut cx, view) = phone();
    let copied = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let seen = copied.clone();
    cx.host()
        .handle::<view::methods::ClipboardWrite>(move |text| {
            *seen.borrow_mut() = text;
            Ok(())
        });
    route(&mut cx, &view, "project/blob/j-177/manual/empty.txt");
    cx.simulate_click("forge-blob-copy-link");
    cx.run_until_parked();
    assert_eq!(
        *copied.borrow(),
        "duck://testnet-0a1b2c3d/forge/project/blob/j-177/manual/empty.txt"
    );
    route(&mut cx, &view, "project/empty.txt");
    cx.simulate_click("forge-blob-copy-link");
    cx.run_until_parked();
    assert_eq!(
        *copied.borrow(),
        "duck://testnet-0a1b2c3d/forge/project/empty.txt"
    );
}
