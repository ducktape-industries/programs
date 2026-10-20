//! `cargo run -p explorer-view --example live`: Explorer in the real app
//! over a fake node whose blocks carry a few people and chat posts; the dev
//! account, as the one validator, votes; ada has a change of chat waiting,
//! beside the vote's adding trade. `READER=none` shows the same network to
//! a reader whose key votes for no one.
use chat::{Block, Op, PostPolicy};
use programs::{Change, Scheduled};
use valset::{Motion, Valset};
use view::methods::Query;
use view::rig;

fn main() {
    let unprivileged = std::env::var_os("READER").is_some_and(|reader| reader == "none");
    rig::run("explorer", |net| {
        net.hook::<activity::Activity>(activity::PROGRAM);
        net.seat::<chat::Chat>(chat::PROGRAM);
        let ada = net.person("ada");
        let bob = net.person("bob");
        net.submit(
            ada,
            chat::PROGRAM,
            &Op::CreateChannel {
                channel_id: "general".into(),
                name: "General".into(),
                post_policy: PostPolicy::Open,
            },
        )?;
        for (seq, (who, text)) in [(ada, "first"), (bob, "second"), (ada, "third")]
            .into_iter()
            .enumerate()
        {
            net.submit(
                who,
                chat::PROGRAM,
                &Op::PostMessage {
                    channel_id: "general".into(),
                    message_id: format!("m{seq}"),
                    blocks: vec![Block::paragraph(text)],
                    thread: None,
                },
            )?;
        }
        let me = net.me();
        let vote = |op: &programs::Op| {
            let motion = Motion::Role {
                role: abi::role::PROGRAMS.into(),
                op: abi::encode(op),
            };
            net.submit(me, valset::PROGRAM, &valset::Op::Vote(motion))
        };
        let kind = programs::CODE_KIND;
        let body = b"chat, the next release".to_vec();
        let code = net.hold(kind, &body)?;
        let at = |height: u64, change: Change| programs::Op::Schedule(Scheduled { height, change });
        let set = |program: &str| {
            Change::Set(programs::Entry {
                program: program.into(),
                code,
                params: Vec::new(),
            })
        };
        net.submit(ada, programs::PROGRAM, &at(700, set(chat::PROGRAM)))?;
        vote(&at(900, set("trade")))?;
        if unprivileged {
            let validators: valset::Reply =
                net.query(valset::PROGRAM, &valset::Query::Validators)?;
            net.handle::<Query<Valset>>(move |query| {
                Ok(match query {
                    valset::Query::Voices { .. } => valset::Reply::Voices(Vec::new()),
                    _ => validators.clone(),
                })
            });
        }
        Ok(())
    })
}
