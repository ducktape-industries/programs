//! The node-less screen dumps: one tree per screen of the plan, light and
//! dark, for the app's renderer (`ducktape-app --render-tree <json>`).
use super::{accounts, booted, change_screen, opened, refusal};
use crate::Forge;
use crate::api::Ask;
use crate::state::ChangeTab;
use view::Theme;
use view::methods::Query;
use view::testing::TestAppContext;

/// `FORGE_SCREEN_EXPORT=1` writes each screen's frame for the app's
/// node-less renderer (`ducktape-app --render-tree <json>`), light and dark.
#[test]
fn export_forge_screens() {
    if std::env::var_os("FORGE_SCREEN_EXPORT").is_none() {
        return;
    }
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/forge-screens");
    std::fs::create_dir_all(&out).unwrap();
    let mut manifest = Vec::new();
    for (index, state) in SCREENS.iter().enumerate() {
        for dark in [false, true] {
            let mut cx = screen(state);
            if dark {
                cx.set_global(Theme::dark());
            }
            let theme = if dark { "dark" } else { "light" };
            let name = format!("{:02}-{state}-{theme}", index + 1);
            std::fs::write(
                out.join(format!("{name}.json")),
                serde_json::to_vec(&cx.whole_frame()).unwrap(),
            )
            .unwrap();
            manifest.push(serde_json::json!({
                "name": name,
                "theme": theme,
                "width": 1180,
                "height": 760,
                "how": "TestAppContext + FakeHost over forge fixtures",
            }));
        }
    }
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

/// The `blob` screen is README.md opened from the Code tab's tree: the
/// repository opens on its README page, which has no tree to press.
#[test]
fn the_blob_screen_opens_readme_from_the_code_tab() {
    let cx = screen("blob");
    assert_eq!(cx.interactivity("forge-tab-code").aria.selected, Some(true));
    assert!(cx.find("forge-blob-markdown").is_some(), "{:?}", cx.texts());
}

const SCREENS: [&str; 12] = [
    "repos",
    "repos-empty",
    "repos-refused",
    "code",
    "blob",
    "commits",
    "commit",
    "refs",
    "settings",
    "changes",
    "change-conversation",
    "change-files",
];

/// One named screen, in the state the plan names it.
fn screen(state: &str) -> TestAppContext {
    match state {
        "repos" => booted("default").0,
        "repos-empty" => booted("empty").0,
        "repos-refused" => {
            let mut cx = TestAppContext::new();
            cx.host()
                .handle::<Ask>(|_| Err(refusal("refused-not-found")));
            cx.host()
                .handle::<Query<::chat::Chat>>(|_| Ok(chat::Reply::Accounts(accounts())));
            cx.host()
                .handle::<Query<identity::Identity>>(|_| Ok(super::no_accounts()));
            cx.open::<Forge>();
            cx.run_until_parked();
            cx
        }
        "code" => {
            let (mut cx, _) = opened("default");
            cx.simulate_click("forge-ref-picker");
            cx.simulate_click("forge-ref-refs/heads/clean");
            cx.run_until_parked();
            cx
        }
        "blob" => {
            let (mut cx, _) = opened("default");
            cx.simulate_click("forge-ref-picker");
            cx.simulate_click("forge-ref-refs/heads/clean");
            cx.run_until_parked();
            cx.simulate_click("forge-tab-code");
            cx.run_until_parked();
            cx.simulate_click("forge-tree-README.md");
            cx.run_until_parked();
            cx
        }
        "commits" => {
            let (mut cx, _) = opened("default");
            cx.simulate_click("forge-tab-commits");
            cx.run_until_parked();
            cx
        }
        "commit" => {
            let (mut cx, _) = opened("default");
            cx.simulate_click("forge-tab-commits");
            cx.run_until_parked();
            cx.simulate_click("forge-commit-26607f522099476177a45a8058a93108fba5a84d");
            cx.run_until_parked();
            cx
        }
        "refs" => {
            let (mut cx, _) = opened("default");
            cx.simulate_click("forge-tab-refs");
            cx.run_until_parked();
            cx
        }
        "settings" => {
            let (mut cx, _) = opened("default");
            cx.simulate_click("forge-tab-settings");
            cx.run_until_parked();
            cx
        }
        "changes" => {
            let (mut cx, _) = opened("default");
            cx.simulate_click("forge-tab-changes");
            cx.run_until_parked();
            cx
        }
        "change-conversation" => change_screen("reviewed", ChangeTab::Conversation).0,
        "change-files" => change_screen("reviewed", ChangeTab::Files).0,
        other => panic!("no screen named {other}"),
    }
}
