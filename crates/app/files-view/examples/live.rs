//! `cargo run -p files-view --example live`: Files in the real app over a
//! fake node with the real `files` program seated. The dev account (named
//! `dev`) keeps notes and a plan in its home; ada shares a readme, a link
//! to it and a picture, and keeps a diary in hers, which she lists for
//! herself alone; the first snapshot is pinned `v1`. Dev writes anywhere
//! but ada's home, and reads it.
use files::{Change, Content, Op};
use view::rig::{self, Network};

fn put(net: &Network, path: &str, bytes: &[u8]) -> Change {
    let chunks = bytes
        .chunks(files::CHUNK_SIZE as usize)
        .map(|chunk| {
            net.put_blob(files::CHUNK, chunk.to_vec())
                .expect("a chunk blob")
        })
        .collect();
    Change::Put {
        path: path.into(),
        exec: false,
        meta: Default::default(),
        content: Content::Chunks {
            size: bytes.len() as u64,
            chunks,
        },
    }
}

const TAPE: &[u8] = include_bytes!("tape.png");

fn main() {
    rig::run("files", |net| {
        net.seat::<files::Files>(files::PROGRAM);
        net.seat::<access::Access>(access::PROGRAM);
        let dev = net.me();
        let ada = net.person("ada");
        let commit = |who, message: &str, changes| {
            let files::Reply::Head(head) = net.query(files::PROGRAM, &files::Query::Head)? else {
                unreachable!("files answers Head with its head");
            };
            let op = Op::Commit {
                base: head.map(|row| row.id),
                message: message.into(),
                changes,
            };
            net.submit(who, files::PROGRAM, &op)
        };
        let first = commit(
            dev,
            "Start my notes",
            vec![
                put(
                    net,
                    "/home/dev/notes.txt",
                    b"Ship files-view.\nAsk ada about the readme.",
                ),
                put(
                    net,
                    "/home/dev/docs/plan.md",
                    b"# Plan\n\n1. Browse\n2. Write\n3. History",
                ),
            ],
        )?;
        commit(
            ada,
            "Share the readme",
            vec![
                put(
                    net,
                    "/shared/readme.txt",
                    b"Everything under /shared is anyone's to write.",
                ),
                put(net, "/shared/tape.png", TAPE),
                Change::Symlink {
                    path: "/shared/latest".into(),
                    target: "/shared/readme.txt".into(),
                },
                put(net, "/home/ada/diary.txt", b"Dear diary,"),
            ],
        )?;
        let only_ada = access::Op::Set {
            program: files::PROGRAM.into(),
            key: "/home/ada".into(),
            list: Some(files::Ownership::Owner),
        };
        net.submit(ada, access::PROGRAM, &only_ada)?;
        commit(
            dev,
            "Say more in my notes",
            vec![put(
                net,
                "/home/dev/notes.txt",
                b"Ship files-view.\nAsk ada about the readme.\nPin the release.",
            )],
        )?;
        let pin = Op::Pin {
            name: "v1".into(),
            snapshot: abi::decode(&first).expect("a commit answers its snapshot"),
        };
        net.submit(dev, files::PROGRAM, &pin)?;
        Ok(())
    })
}
