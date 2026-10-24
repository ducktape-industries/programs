//! Exact-commit report identity, replacement, pagination, validation and absence of scheduling.
use crate::*;
use ::program::{MockChain, Origin};

const LANE: &[u8] = b"lane-key";
const REVIEWER: &[u8] = b"reviewer-key";
const REPO: &str = "project";

fn chain() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<forge::Forge>(forge::PROGRAM);
    chain.register(forge::PROGRAM, 900);
    chain.seat::<access::Access>("access");
    chain.register("access", 902);
    chain.seat::<ForgeReach>(PROGRAM);
    chain.register(PROGRAM, 903);
    chain.hold(LANE, 23);
    chain.hold(REVIEWER, 56);
    chain.init(PROGRAM, &()).unwrap();
    chain
}

fn report(commit: &str, context: &str) -> Op {
    Op::Report {
        repo: REPO.into(),
        commit: commit.into(),
        context: context.into(),
        state: CheckState::Passed,
        url: Some("duck://artifacts/exact-run".into()),
        reach: vec!["src".into()],
        change: Some(1),
    }
}

fn patch(commit: &str, context: &str) -> Op {
    Op::ReportPatch {
        repo: REPO.into(),
        n: 1,
        commit: commit.into(),
        context: context.into(),
        state: CheckState::Passed,
        url: Some("duck://reviews/exact-head".into()),
    }
}

fn submit(chain: &MockChain, key: &[u8], op: &Op) -> Result<Vec<u8>, ::program::Error> {
    chain
        .host(PROGRAM)
        .attempt(|| chain.submit(Origin::Signed(key.to_vec()), PROGRAM, op))
}

fn page(chain: &MockChain, repo: &str, commit: &str, request: PageRequest) -> PageResponse<Report> {
    let Reply::Reports(page) = chain
        .query(
            PROGRAM,
            &Query::Reports {
                repo: repo.into(),
                commit: commit.into(),
                page: request,
            },
        )
        .unwrap()
    else {
        panic!("Reports answers Reports")
    };
    page
}

fn reports(chain: &MockChain, commit: &str) -> Vec<Report> {
    page(chain, REPO, commit, PageRequest::first(10)).items
}

fn no_outbound_work(chain: &MockChain) {
    let host = chain.host(PROGRAM);
    let state = host.borrow();
    assert!(state.emissions.is_empty());
    assert!(state.calls.is_empty());
    assert!(state.resumes.is_empty());
    assert!(!state.deferred);
}

#[test]
fn a_report_keeps_its_exact_repo_commit_reporter_context_and_artifact() {
    let chain = chain();
    let head = "a".repeat(40);
    chain.at(7, 0);
    submit(&chain, LANE, &report(&head, "lane")).unwrap();
    let rows = reports(&chain, &head);
    assert_eq!(
        rows,
        vec![Report {
            reporter: 23,
            context: "lane".into(),
            state: CheckState::Passed,
            url: Some("duck://artifacts/exact-run".into()),
            height: 7,
            reach: vec!["src".into()]
        }]
    );
    assert!(reports(&chain, &"b".repeat(40)).is_empty());
    assert!(
        page(&chain, "another", &head, PageRequest::first(1))
            .items
            .is_empty()
    );
    no_outbound_work(&chain);
}

#[test]
fn reports_do_not_carry_to_another_head_or_reask_when_blocks_advance() {
    let chain = chain();
    let source = "a".repeat(40);
    let target = "b".repeat(40);
    submit(&chain, LANE, &report(&source, "lane")).unwrap();
    submit(&chain, REVIEWER, &patch(&source, "review")).unwrap();
    let before = reports(&chain, &source);
    chain.at(20, 0);
    assert_eq!(reports(&chain, &source), before);
    assert!(reports(&chain, &target).is_empty());
    no_outbound_work(&chain);
}

#[test]
fn a_report_said_again_replaces_its_reach_and_state_only_for_its_identity() {
    let chain = chain();
    let head = "a".repeat(40);
    submit(&chain, LANE, &report(&head, "lane")).unwrap();
    submit(&chain, REVIEWER, &report(&head, "lane")).unwrap();
    submit(&chain, LANE, &report(&head, "other")).unwrap();
    let mut replaced = report(&head, "lane");
    let Op::Report {
        reach, state, url, ..
    } = &mut replaced
    else {
        unreachable!()
    };
    *reach = vec!["Cargo.lock".into(), "".into()];
    *state = CheckState::Failed;
    *url = None;
    submit(&chain, LANE, &replaced).unwrap();
    let rows = reports(&chain, &head);
    assert_eq!(rows.len(), 3);
    let row = rows
        .iter()
        .find(|row| row.reporter == 23 && row.context == "lane")
        .unwrap();
    assert_eq!(row.state, CheckState::Failed);
    assert_eq!(row.url, None);
    assert_eq!(row.reach, ["", "Cargo.lock"]);
    assert!(
        rows.iter()
            .filter(|row| row.state == CheckState::Passed)
            .count()
            == 2
    );
    no_outbound_work(&chain);
}

#[test]
fn patch_and_tree_reports_replace_each_other_at_the_same_exact_identity() {
    let chain = chain();
    let head = "a".repeat(40);
    submit(&chain, LANE, &report(&head, "review")).unwrap();
    submit(&chain, LANE, &patch(&head, "review")).unwrap();
    let rows = reports(&chain, &head);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].reach.is_empty());
    submit(&chain, LANE, &report(&head, "review")).unwrap();
    assert_eq!(reports(&chain, &head)[0].reach, ["src"]);
}

#[test]
fn reports_page_across_tree_and_patch_rows_without_duplicates_or_cross_commit_cursors() {
    let chain = chain();
    let head = "a".repeat(40);
    submit(&chain, LANE, &report(&head, "lane")).unwrap();
    submit(&chain, REVIEWER, &patch(&head, "review")).unwrap();
    let first = page(&chain, REPO, &head, PageRequest::first(1));
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].context, "lane");
    let next = first.next.expect("the patch is on another page");
    let second = page(
        &chain,
        REPO,
        &head,
        PageRequest::resume(Some(next.clone()), 1),
    );
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].context, "review");
    assert!(second.items[0].reach.is_empty());
    assert!(second.next.is_none());
    let refused = chain
        .query::<_, Reply>(
            PROGRAM,
            &Query::Reports {
                repo: REPO.into(),
                commit: "b".repeat(40),
                page: PageRequest::resume(Some(next), 1),
            },
        )
        .unwrap_err();
    assert_eq!(refused.code, ::program::code::STALE);
}

#[test]
fn a_report_names_a_full_commit_its_context_and_a_valid_nonempty_reach() {
    let chain = chain();
    let head = "a".repeat(40);
    for commit in ["abc".into(), "A".repeat(40), "z".repeat(64)] {
        assert!(submit(&chain, LANE, &report(&commit, "lane")).is_err());
        assert!(submit(&chain, LANE, &patch(&commit, "review")).is_err());
    }
    assert!(submit(&chain, LANE, &report(&head, " ")).is_err());
    assert!(submit(&chain, LANE, &patch(&head, "")).is_err());
    for bad in [
        vec![],
        vec!["/absolute".into()],
        vec!["src/../x".into()],
        vec!["src/".into()],
        vec!["./src".into()],
        vec!["a\0b".into()],
    ] {
        let mut op = report(&head, "lane");
        let Op::Report { reach, .. } = &mut op else {
            unreachable!()
        };
        *reach = bad;
        assert!(submit(&chain, LANE, &op).is_err());
    }
    assert!(reports(&chain, &head).is_empty());
    submit(&chain, LANE, &report(&"a".repeat(64), "lane")).unwrap();
}

#[test]
fn retired_carry_and_reask_tags_cannot_be_reused() {
    use borsh::BorshDeserialize;
    let mut watched = vec![0];
    watched.extend(abi::encode(&forge::Watched::Closed {
        repo: REPO.into(),
        n: 1,
        by: forge::Principal::Account(17),
    }));
    assert!(Op::try_from_slice(&watched).is_err());
    let mut verdict = vec![1];
    verdict.extend(abi::encode(&(REPO.to_owned(), 1u64)));
    assert!(Query::try_from_slice(&verdict).is_err());
    assert_eq!(abi::encode(&report(&"a".repeat(40), "check"))[0], 1);
    assert_eq!(abi::encode(&patch(&"a".repeat(40), "review"))[0], 2);
    let chain = chain();
    let retell = Op::Retell {
        repo: REPO.into(),
        n: 1,
    };
    assert_eq!(abi::encode(&retell)[0], 3);
    assert!(submit(&chain, LANE, &retell).is_err());
    let wanted = Query::Wanted {
        reporter: 23,
        page: PageRequest::first(1),
    };
    assert_eq!(abi::encode(&wanted)[0], 2);
    assert!(chain.query::<_, Reply>(PROGRAM, &wanted).is_err());
    no_outbound_work(&chain);
}

#[derive(Debug, borsh::BorshDeserialize, borsh::BorshSerialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
enum OtherEvent {
    Heard(forge::Watched) = 255,
}

struct OtherWatcher;

impl ::program::Program for OtherWatcher {
    const NAME: &'static str = "other-watcher";
    type Op = OtherEvent;
    type Query = ();
    type Reply = u64;

    fn execute(ctx: &::program::ExecCtx, _: Self::Op) -> Result<(), ::program::Error> {
        ctx.env().sent_by(forge::PROGRAM)?;
        let heard = store::Item::<u64>::new("heard");
        heard.put(ctx, &(heard.get(ctx)?.unwrap_or_default() + 1));
        Ok(())
    }

    fn query(ctx: &::program::QueryCtx, _: ()) -> Result<u64, ::program::Error> {
        Ok(store::Item::<u64>::new("heard")
            .get(ctx)?
            .unwrap_or_default())
    }
}

#[test]
fn both_receipt_paths_remove_only_their_old_forge_subscription_before_the_next_event() {
    for receipt in [
        report(&"a".repeat(40), "lane"),
        patch(&"a".repeat(40), "review"),
    ] {
        let mut chain = chain();
        chain.seat::<OtherWatcher>("other-watcher");
        chain.register("other-watcher", 904);
        let forge_host = chain.host(forge::PROGRAM);
        for watcher in [PROGRAM, "other-watcher"] {
            chain
                .submit(
                    Origin::Program(watcher.into()),
                    forge::PROGRAM,
                    &forge::Op::Watch {
                        deliver: if watcher == PROGRAM {
                            vec![0]
                        } else {
                            vec![255]
                        },
                    },
                )
                .unwrap();
        }
        let reads = forge_host.query(::program::MockHost::env(forge::PROGRAM));
        let watching = || forge::tables::WATCHERS.all(&reads).unwrap();
        let before = watching();
        assert_eq!(before.len(), 2);
        assert!(
            chain
                .submit(
                    Origin::Signed(LANE.to_vec()),
                    forge::PROGRAM,
                    &forge::Op::Watch {
                        deliver: Vec::new(),
                    }
                )
                .is_err()
        );
        assert_eq!(watching(), before);
        let other = before
            .iter()
            .find(|(name, _)| name == "other-watcher")
            .unwrap()
            .clone();
        submit(&chain, LANE, &receipt).unwrap();
        assert_eq!(watching(), vec![other.clone()]);
        assert_eq!(reports(&chain, &"a".repeat(40)).len(), 1);
        submit(&chain, LANE, &receipt).unwrap();
        assert_eq!(watching(), vec![other]);
        chain
            .submit(
                Origin::Signed(LANE.to_vec()),
                forge::PROGRAM,
                &forge::Op::Create {
                    repo: REPO.into(),
                    hash: ::program::HashKind::Sha1,
                },
            )
            .unwrap();
        let ctx = forge_host.exec(::program::MockHost::env(forge::PROGRAM));
        let branch = b"refs/heads/temporary".to_vec();
        forge::tables::REFS.put(&ctx, &(REPO.to_owned(), branch.clone()), &vec![0xaa; 20]);
        let mut repo = forge::tables::REPOS
            .get(&reads, &REPO.to_owned())
            .unwrap()
            .unwrap();
        repo.refs_count = 1;
        forge::tables::REPOS.put(&ctx, &REPO.to_owned(), &repo);
        let before_reach = chain.host(PROGRAM).borrow().state.clone();
        chain
            .submit(
                Origin::Signed(LANE.to_vec()),
                forge::PROGRAM,
                &forge::Op::DeleteBranch {
                    repo: REPO.into(),
                    branch,
                    expected: "aa".repeat(20),
                },
            )
            .unwrap();
        assert_eq!(chain.query::<_, u64>("other-watcher", &()).unwrap(), 1);
        assert_eq!(chain.host(PROGRAM).borrow().state, before_reach);
        assert_eq!(watching().len(), 1);
    }
}
