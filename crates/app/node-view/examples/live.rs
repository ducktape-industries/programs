//! `cargo run -p node-view --example live`: Nodes in the real app over a
//! fake node whose validators are the dev key and a node key on the dev
//! account's agent, beside a resident; the dev key votes, so the vote is
//! on screen with a promotion and a change of valset's code waiting.
use programs::{Change, Entry, Scheduled};
use valset::{Motion, Op};
use view::rig;

fn main() {
    rig::run(valset::PROGRAM, |net| {
        let me = net.me();
        net.submit(me, valset::PROGRAM, &Op::Vote(Motion::Door { open: true }))?;
        let enroll = |host: &str| Op::Enroll {
            address: Some(format!("{host}:4000")),
            halo: format!("{host}:9100"),
            invite: None,
        };
        let vps = net.agent(me, "validator vps")?;
        net.submit(vps, valset::PROGRAM, &enroll("10.0.0.3"))?;
        let seat = Motion::Promote {
            key: net.key_of(vps),
        };
        net.submit(me, valset::PROGRAM, &Op::Vote(seat))?;
        let ada = net.person("ada");
        net.submit(ada, valset::PROGRAM, &enroll("10.0.0.2"))?;
        let promote = Motion::Promote {
            key: net.key_of(ada),
        };
        net.submit(me, valset::PROGRAM, &Op::Vote(promote))?;
        let change = programs::Op::Schedule(Scheduled {
            height: net.height() + 300,
            change: Change::Set(Entry {
                program: valset::PROGRAM.into(),
                code: abi::BlobId::Sha256([0x7e; 32]),
                params: Vec::new(),
            }),
        });
        let programs = Motion::Role {
            role: abi::role::PROGRAMS.into(),
            op: abi::encode(&change),
        };
        net.submit(me, valset::PROGRAM, &Op::Vote(programs))
            .map(drop)
    })
}
