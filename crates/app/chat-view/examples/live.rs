//! `cargo run -p chat-view --example live`: Chat in the real app over a
//! fake node seeded with a channel, a dm and a few messages.
use chat::{Block, Op, PostPolicy};
use view::rig::{self, Network, now_ms};

fn main() {
    rig::run("chat", |net| {
        net.seat::<chat::Chat>(chat::PROGRAM);
        let me = net.me();
        let ada = net.person("ada");
        let bob = net.person("bob");
        let hour = 60 * 60 * 1000;
        net.at(now_ms() - 26 * hour);
        net.submit(
            ada,
            chat::PROGRAM,
            &Op::CreateChannel {
                channel_id: "general".into(),
                name: "General".into(),
                post_policy: PostPolicy::Open,
            },
        )?;
        let posts = [
            (
                ada,
                25 * hour,
                "Welcome to the live window: the real Chat view, a fake node.",
            ),
            (
                bob,
                24 * hour,
                "Everything here is **real program state**: a submit runs chat's rules.",
            ),
            (
                me,
                3 * hour,
                "Post something below and watch it come back through `program.changes`.",
            ),
            (
                ada,
                hour,
                "Threads, reactions and search read the same chain.",
            ),
        ];
        for (seq, (who, ago, text)) in posts.into_iter().enumerate() {
            net.at(now_ms() - ago);
            post(net, who, "general", &format!("m{seq}"), text)?;
        }
        for who in [ada, bob, me] {
            net.submit(
                who,
                chat::PROGRAM,
                &Op::AddReaction {
                    channel_id: "general".into(),
                    seq: 1,
                    emoji: "👍".into(),
                },
            )?;
        }
        net.at(now_ms() - 30 * 60 * 1000);
        net.submit(
            bob,
            chat::PROGRAM,
            &Op::CreateDmChannel {
                counterpart: me,
                name: "dm".into(),
            },
        )?;
        post(
            net,
            bob,
            &chat::dm_channel_id(bob, me),
            "dm0",
            "ping: are you on the live node?",
        )
    })
}

fn post(net: &Network, who: u64, channel: &str, id: &str, text: &str) -> Result<(), rig::Error> {
    net.submit(
        who,
        chat::PROGRAM,
        &Op::PostMessage {
            channel_id: channel.into(),
            message_id: id.into(),
            blocks: vec![Block::paragraph(text)],
            thread: None,
        },
    )
    .map(drop)
}
