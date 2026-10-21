use super::*;

const ALICE: AccountNumber = 11;
const ALICE_KEY: u64 = 7;

fn apps() -> Vec<Founding> {
    vec![founding(activity::PROGRAM, &program("activity"))]
}

impl Net {
    async fn acted(&self, account: AccountNumber) -> Vec<activity::Frame> {
        let asked = activity::Query::Acted {
            account,
            page: PageRequest::default(),
        };
        let activity::Reply::Frames(page) = self.ask(activity::PROGRAM, &asked).await else {
            panic!()
        };
        page.items
    }

    async fn ran(&self, account: AccountNumber) -> Vec<(u64, u32, bool)> {
        let acted = self.acted(account).await;
        acted
            .into_iter()
            .map(|frame| (frame.height, frame.position, frame.applied))
            .collect()
    }

    async fn rename(&mut self, name: &str) {
        let op = identity::Op::SetName {
            account: ALICE,
            name: name.into(),
        };
        self.apply(&public(ALICE_KEY), identity::PROGRAM, &op).await;
    }

    async fn schedule(&mut self, height: u64, change: programs::Change) {
        let op = programs::Op::Schedule(programs::Scheduled { height, change });
        output_of(&self.govern(&op).await);
    }
}

#[test]
fn a_founding_hook_is_told_of_every_frame_and_keeps_it_by_account() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found_with(
            context,
            dir.path(),
            apps(),
            vec![activity::PROGRAM],
            MEMBER_CAP,
        )
        .await;
        let create = identity::Op::Create {
            name: "Alice".into(),
            scheme: Scheme::Ed25519,
        };
        let created: AccountNumber = abi::decode(
            &net.apply(&public(ALICE_KEY), identity::PROGRAM, &create)
                .await,
        )
        .unwrap();
        assert_eq!(created, ALICE);
        net.rename("Ada").await;
        assert_eq!(
            net.refuse(&public(ALICE_KEY), identity::PROGRAM, &create)
                .await,
            reason::ALREADY_EXISTS
        );
        assert_eq!(
            net.ran(ALICE).await,
            [(3, 0, false), (2, 0, true)],
            "every frame the account acted in, refused or not, newest first: the create ran before the key held an account"
        );
        let acted = net.acted(ALICE).await;
        assert_eq!(acted[1].target, identity::PROGRAM);
        assert_eq!(acted[1].time, TIME + 2000);
        let every = activity::Query::Frames {
            page: PageRequest::default(),
        };
        let refusal = net.refused(activity::PROGRAM, &every).await;
        assert_eq!(refusal.reason, reason::UNSUPPORTED, "the node lists them");
        let asked = activity::Query::Used {
            page: PageRequest::default(),
        };
        let activity::Reply::Used(used) = net.ask(activity::PROGRAM, &asked).await else {
            panic!()
        };
        assert_eq!(
            used.items,
            [activity::Used {
                account: ALICE,
                program: identity::PROGRAM.into(),
                frames: 2,
            }]
        );
    });
}

#[test]
fn a_hook_the_programs_schedules_is_told_from_its_height_until_it_is_removed() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found_with(context, dir.path(), apps(), Vec::new(), MEMBER_CAP).await;
        let create = identity::Op::Create {
            name: "Alice".into(),
            scheme: Scheme::Ed25519,
        };
        net.apply(&public(ALICE_KEY), identity::PROGRAM, &create)
            .await;
        let hooked_at = net.height + 3;
        net.schedule(
            hooked_at,
            programs::Change::SetHook(activity::PROGRAM.into()),
        )
        .await;
        net.rename("Before").await;
        assert_eq!(net.height, hooked_at - 1);
        assert_eq!(net.ran(ALICE).await, []);
        net.rename("Hooked").await;
        assert_eq!(net.ran(ALICE).await, [(hooked_at, 0, true)]);
        let unhooked_at = net.height + 2;
        net.schedule(
            unhooked_at,
            programs::Change::RemoveHook(activity::PROGRAM.into()),
        )
        .await;
        assert_eq!(net.height, unhooked_at - 1);
        net.rename("Unhooked").await;
        assert_eq!(
            net.ran(ALICE).await,
            [(hooked_at, 0, true)],
            "a frame from the height the hook is removed goes unheard"
        );
    });
}
