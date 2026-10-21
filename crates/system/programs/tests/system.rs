use abi::reason;
use abi::{BlobId, HostOp, Message, Origin, Outcome, Receipt, Scan, Scheme};
use borsh::{BorshDeserialize, BorshSerialize};
use commonware_cryptography::{Signer as _, ed25519};
use commonware_runtime::{Runner as _, deterministic};
use fixture_wasm::PROBE;
use host::{
    Applied, Block, BlockId, Founding, FoundingView, Genesis, Host, Layer, Limits, PARKED,
    Submission, Tip,
};
use identity::AccountNumber;
use keyscheme::testkit;
use programs::PageRequest;

fn release_dir() -> std::path::PathBuf {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| workspace.join("target"))
        .join("wasm32-unknown-unknown/release")
}

fn build() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let jobs = std::env::var("CARGO_BUILD_JOBS")
            .ok()
            .or_else(|| {
                let cores = std::thread::available_parallelism().ok()?;
                Some(cores.to_string())
            })
            .unwrap_or_else(|| "1".into());
        let status = std::process::Command::new("make")
            .arg("-C")
            .arg(&root)
            .arg("-j")
            .arg(jobs)
            .arg("wasm-programs")
            .status()
            .expect("make is on PATH");
        assert!(status.success(), "`make wasm-programs` failed: {status}");
    });
}

fn program(name: &str) -> Vec<u8> {
    build();
    let path = release_dir().join(format!("{name}.wasm"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[derive(BorshSerialize, BorshDeserialize)]
enum Step {
    Op(HostOp),
    Env,
    Spin,
    Grow(u32),
    Fail(String),
}

const NETWORK: &[u8] = b"net";
const EPOCH_LENGTH: u64 = 4;
const MEMBER_CAP: u32 = 8;
const TIME: u64 = 1_700_000_000_000;
const PROBE_SIGNER: u64 = 9;
const FOUNDERS: [u64; 2] = [1, 2];

type Ctx = deterministic::Context;

fn key(seed: u64) -> ed25519::PrivateKey {
    ed25519::PrivateKey::from_seed(seed)
}

fn public(seed: u64) -> Vec<u8> {
    key(seed).public_key().as_ref().to_vec()
}

fn halo(seed: u64) -> String {
    format!("v{seed}.example.org:9100")
}

fn member(seed: u64) -> valset::Member {
    valset::Member {
        key: public(seed),
        address: Some(format!("v{seed}:1")),
        halo: halo(seed),
    }
}

fn membership(seed: u64, role: valset::Role) -> valset::Membership {
    valset::Membership {
        key: public(seed),
        address: Some(format!("v{seed}:1")),
        halo: halo(seed),
        role,
    }
}

fn invite(issuer: u64, nonce: u8, expires: u64) -> valset::Invite {
    let grant = valset::Grant {
        network: NETWORK.to_vec(),
        nonce: [nonce; 16],
        expires,
    };
    let signature = key(issuer).sign(valset::INVITE_NAMESPACE, &grant.preimage());
    valset::Invite {
        issuer: public(issuer),
        grant,
        signature: signature.as_ref().to_vec(),
    }
}

fn enroll(seed: u64, invite: Option<valset::Invite>) -> valset::Op {
    valset::Op::Enroll {
        address: Some(format!("v{seed}:1")),
        halo: halo(seed),
        invite,
    }
}

fn founding(program: &str, code: &[u8]) -> Founding {
    Founding {
        program: program.to_owned(),
        code: code.to_vec(),
        params: Vec::new(),
        roles: Vec::new(),
    }
}

fn filling(role: &str, founding: Founding) -> Founding {
    Founding {
        roles: vec![role.to_owned()],
        ..founding
    }
}

fn probe(program: &str) -> Founding {
    Founding {
        program: program.to_owned(),
        code: PROBE.to_vec(),
        params: abi::encode(&Vec::<Step>::new()),
        roles: Vec::new(),
    }
}

fn block_id(height: u64) -> BlockId {
    let mut id = [0u8; 32];
    id[..8].copy_from_slice(&height.to_be_bytes());
    id
}

struct Net {
    host: Host<Ctx>,
    height: u64,
}

impl Net {
    async fn found(context: Ctx, dir: &std::path::Path) -> Net {
        Net::found_with(context, dir, Vec::new(), Vec::new(), MEMBER_CAP).await
    }

    async fn found_with(
        context: Ctx,
        dir: &std::path::Path,
        apps: Vec<Founding>,
        hooks: Vec<&str>,
        member_cap: u32,
    ) -> Net {
        let apps = apps.into_iter().map(|app| match app.program.as_str() {
            egress::PROGRAM => filling(abi::role::EGRESS, app),
            ownership::PROGRAM => filling(abi::role::OWNERSHIP, app),
            _ => app,
        });
        let genesis = Genesis {
            network: NETWORK.to_vec(),
            validators: FOUNDERS.iter().map(|seed| member(*seed)).collect(),
            programs: vec![
                filling(
                    abi::role::PROGRAMS,
                    founding(programs::PROGRAM, &program("programs")),
                ),
                filling(
                    abi::role::VALIDATORS,
                    founding(valset::PROGRAM, &program("valset")),
                ),
                filling(
                    abi::role::IDENTITY,
                    founding(identity::PROGRAM, &program("identity")),
                ),
                filling(
                    abi::role::BLOBS,
                    founding(blobs::PROGRAM, &program("blobs")),
                ),
                probe("probe"),
            ]
            .into_iter()
            .chain(apps)
            .chain([
                filling(
                    abi::role::AWAITS,
                    founding(awaits::PROGRAM, &program("awaits")),
                ),
                filling(
                    abi::role::HALO,
                    Founding {
                        params: abi::encode(&halo::Genesis { lanes: Vec::new() }),
                        ..founding(halo::PROGRAM, &program("halo"))
                    },
                ),
                filling(
                    abi::role::SCHEDULER,
                    founding(scheduler::PROGRAM, &program("scheduler")),
                ),
                filling(abi::role::ACCESS, founding("access", &program("access"))),
            ])
            .collect(),
            views: vec![FoundingView {
                name: "lens".into(),
                view: b"a view".to_vec(),
            }],
            hooks: hooks.into_iter().map(str::to_owned).collect(),
            limits: Limits::default(),
            epoch_length: EPOCH_LENGTH,
            time: TIME,
            member_cap,
            queries: abi::role::validators::QueryPolicy::Signed,
        };
        let (host, applied) = Host::found(context, "net", dir, block_id(0), genesis)
            .await
            .unwrap();
        for receipt in &applied.admissions {
            assert!(
                matches!(receipt.outcome, Outcome::Applied { .. }),
                "{} did not admit: {:?}",
                receipt.program,
                receipt.outcome
            );
        }
        Net { host, height: 0 }
    }

    fn time(&self) -> u64 {
        TIME + self.height * 1000
    }

    async fn block(&mut self, submissions: Vec<Submission>) -> Applied {
        self.height += 1;
        self.host
            .apply(Block {
                height: self.height,
                id: block_id(self.height),
                time: self.time(),
                submissions,
            })
            .await
            .unwrap()
    }

    async fn tick(&mut self) -> Applied {
        self.block(Vec::new()).await
    }

    async fn ticks(&mut self, blocks: u64) {
        for _ in 0..blocks {
            self.tick().await;
        }
    }

    fn submission(&self, signer: &[u8], target: &str, payload: Vec<u8>) -> Submission {
        Submission {
            signer: signer.to_vec(),
            anchor: Tip {
                height: self.height,
                id: block_id(self.height),
            },
            salt: None,
            target: target.to_owned(),
            payload,
            client: None,
        }
    }

    async fn submit<T: BorshSerialize>(&mut self, signer: &[u8], target: &str, op: &T) -> Receipt {
        let submission = self.submission(signer, target, abi::encode(op));
        let applied = self.block(vec![submission]).await;
        applied.submissions.into_iter().next().unwrap()
    }

    async fn apply<T: BorshSerialize>(&mut self, signer: &[u8], target: &str, op: &T) -> Vec<u8> {
        let receipt = self.submit(signer, target, op).await;
        match receipt.outcome {
            Outcome::Applied { output } => output,
            Outcome::Rejected(refusal) => panic!("{target} rejected the op: {refusal}"),
        }
    }

    fn stored(&self, kind: &str, body: &[u8]) -> blobs::Entry {
        let framed = blob::frame(kind, body).unwrap();
        let entry = blobs::Entry::of(&framed).unwrap();
        self.host.blobs().write(&entry.id, &framed).unwrap();
        entry
    }

    async fn put(&mut self, entries: Vec<blobs::Entry>) -> u64 {
        let put = blobs::Op::Put { blobs: entries };
        let receipt = self.as_anyone(blobs::PROGRAM, &put).await;
        abi::decode(output_of(&receipt)).unwrap()
    }

    async fn receipted(&mut self, bundle: u64, by: &[u64]) {
        let receipt = abi::encode(&blobs::Op::Receipt { bundle });
        let submissions = by
            .iter()
            .map(|seed| self.submission(&public(*seed), blobs::PROGRAM, receipt.clone()))
            .collect();
        for receipt in self.block(submissions).await.submissions {
            output_of(&receipt);
        }
    }

    async fn hold(&mut self, kind: &str, body: &[u8]) -> BlobId {
        let entry = self.stored(kind, body);
        let id = entry.id;
        let bundle = self.put(vec![entry]).await;
        self.receipted(bundle, &FOUNDERS).await;
        id
    }

    async fn chunks(&mut self, bytes: &[u8]) -> files::Content {
        let mut chunks = Vec::new();
        for chunk in bytes.chunks(files::CHUNK_SIZE as usize) {
            chunks.push(self.hold(files::CHUNK, chunk).await);
        }
        files::Content::Chunks {
            size: bytes.len() as u64,
            chunks,
        }
    }

    async fn blob(&self, id: &BlobId) -> Option<Vec<u8>> {
        self.host.blob(id, self.time()).await.unwrap()
    }

    fn parked(&self) -> usize {
        let view = self.host.view(Layer::Confirmed);
        view.scan(PARKED, &Scan::prefix(b"frame/")).unwrap().len()
    }

    async fn refuse<T: BorshSerialize>(&mut self, signer: &[u8], target: &str, op: &T) -> String {
        let receipt = self.submit(signer, target, op).await;
        match receipt.outcome {
            Outcome::Applied { .. } => panic!("{target} applied the op"),
            Outcome::Rejected(refusal) => refusal.reason,
        }
    }

    async fn sent_by<T: BorshSerialize>(&mut self, program: &str, target: &str, op: &T) -> Receipt {
        let script = vec![Step::Op(HostOp::Emit(Message {
            target: target.to_owned(),
            payload: abi::encode(op),
            reply: false,
            acting: None,
        }))];
        let signer = public(PROBE_SIGNER);
        let submission = self.submission(&signer, program, abi::encode(&script));
        let applied = self.block(vec![submission]).await;
        applied.submissions[0]
            .nested
            .iter()
            .find(|receipt| receipt.program == target)
            .cloned()
            .unwrap()
    }

    async fn as_anyone<T: BorshSerialize>(&mut self, target: &str, op: &T) -> Receipt {
        self.sent_by("probe", target, op).await
    }

    async fn pass(&mut self, motion: valset::Motion) -> Receipt {
        let vote = abi::encode(&valset::Op::Vote(motion));
        let submissions = FOUNDERS
            .iter()
            .map(|seed| self.submission(&public(*seed), valset::PROGRAM, vote.clone()))
            .collect();
        let mut applied = self.block(submissions).await;
        let enacting = applied.submissions.pop().unwrap();
        for counted in &applied.submissions {
            let voted: valset::Voted = abi::decode(output_of(counted)).unwrap();
            assert!(matches!(voted, valset::Voted::Counted { .. }), "{voted:?}");
        }
        enacting
    }

    async fn govern(&mut self, op: &programs::Op) -> Receipt {
        let motion = valset::Motion::Role {
            role: abi::role::PROGRAMS.into(),
            op: abi::encode(op),
        };
        let enacting = self.pass(motion).await;
        let voted: valset::Voted = abi::decode(output_of(&enacting)).unwrap();
        assert_eq!(voted, valset::Voted::Enacted);
        enacting
            .nested
            .into_iter()
            .find(|receipt| receipt.program == programs::PROGRAM)
            .expect("the vote ran its op on the programs")
    }

    async fn refused<Q: BorshSerialize>(&self, program: &str, query: &Q) -> abi::Refusal {
        self.host
            .query(
                Layer::Confirmed,
                self.time(),
                Origin::External(public(1)),
                program,
                abi::encode(query),
            )
            .await
            .unwrap()
            .unwrap_err()
    }

    async fn ask<Q: BorshSerialize, R: BorshDeserialize>(&self, program: &str, query: &Q) -> R {
        let answer = self
            .host
            .query(
                Layer::Confirmed,
                self.time(),
                Origin::External(public(1)),
                program,
                abi::encode(query),
            )
            .await
            .unwrap()
            .unwrap();
        abi::decode(&answer).unwrap()
    }

    async fn memberships(&self) -> Vec<valset::Membership> {
        match self
            .ask(
                valset::PROGRAM,
                &valset::Query::Memberships {
                    page: PageRequest::default(),
                },
            )
            .await
        {
            valset::Reply::Memberships(memberships) => memberships.items,
            other => panic!("{other:?}"),
        }
    }

    async fn validators(&self) -> Vec<Vec<u8>> {
        match self.ask(valset::PROGRAM, &valset::Query::Validators).await {
            valset::Reply::Validators(validators) => validators,
            other => panic!("{other:?}"),
        }
    }
}

fn output_of(receipt: &Receipt) -> &[u8] {
    match &receipt.outcome {
        Outcome::Applied { output } => output,
        Outcome::Rejected(refusal) => panic!("{} rejected: {refusal}", receipt.program),
    }
}

fn refusal_of(receipt: &Receipt) -> &str {
    match &receipt.outcome {
        Outcome::Applied { .. } => panic!("{} applied the op", receipt.program),
        Outcome::Rejected(refusal) => &refusal.reason,
    }
}

#[path = "system/activity.rs"]
mod activity_tests;
#[path = "system/attribution.rs"]
mod attribution_tests;
#[path = "system/chat.rs"]
mod chat_tests;
#[path = "system/discussion.rs"]
mod discussion_tests;
#[path = "system/files.rs"]
mod files_tests;
#[path = "system/founding.rs"]
mod founding_tests;
#[path = "system/harness.rs"]
mod harness_tests;
#[path = "system/identity.rs"]
mod identity_tests;
#[path = "system/ownership.rs"]
mod ownership_tests;
#[path = "system/programs.rs"]
mod program_programs_tests;
#[path = "system/scheduler.rs"]
mod scheduler_tests;
#[path = "system/valset.rs"]
mod valset_tests;
