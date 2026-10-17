//! `cargo run -p <view> --example live`: the view built for wasm32, every
//! seated program's describe module built, each packed into its code
//! blob, the fake node served, and the app opened on it. Both build with
//! the workspace's `fast` profile, as `make dev` does;
//! `WASM_PROFILE=release` builds them as a release does (without wasm-opt).

use std::path::{Path, PathBuf};
use std::process::Command;

use super::node::{self, Msg, say};
use super::{DEV_SEED, Network, pack};
use crate::host::Error;

/// The whole thing. `entry` is the roster entry the view rides: a seated
/// program's name, or, for a view with no program behind it, its own.
/// `seed` seats the programs and submits the ops the view opens on.
pub fn run(entry: &str, seed: impl FnOnce(&Network) -> Result<(), Error>) {
    let net = Network::new();
    if let Err(refusal) = seed(&net) {
        eprintln!(
            "the seed was refused: {}: {}",
            refusal.code, refusal.message
        );
        std::process::exit(1);
    }
    let Some(workspace) = Workspace::here() else {
        eprintln!("run this through cargo: `cargo run -p <view> --example live`");
        std::process::exit(1);
    };
    let view = workspace.build_view();
    if net.seats().contains(&entry.to_owned()) {
        for program in net.seats() {
            let mut sections: Vec<(&str, &[u8])> = Vec::new();
            if program == entry {
                sections.push(("ducktape.view", &view));
            }
            let describe = workspace.build_describe(&program);
            if let Some(describe) = &describe {
                sections.push(("ducktape.describe", describe));
            }
            net.code(&program, &pack::wrapper(&sections));
        }
    } else {
        for program in net.seats() {
            let describe = workspace.build_describe(&program);
            let sections: Vec<(&str, &[u8])> = describe
                .iter()
                .map(|bytes| ("ducktape.describe", bytes.as_slice()))
                .collect();
            net.code(&program, &pack::wrapper(&sections));
        }
        net.code_bare(entry, &view);
    }
    net.found_roster();

    let (jobs, msgs) = std::sync::mpsc::channel();
    let addr = match node::bind(jobs.clone()) {
        Ok(addr) => addr,
        Err(error) => {
            eprintln!("the fake node could not bind a loopback port: {error}");
            std::process::exit(1);
        }
    };
    let endpoint = format!("http://{addr}");
    say(format!("node at {endpoint}; the view is `{entry}`"));
    let mut app = workspace.app_command(&endpoint, entry);
    let waiter = std::thread::spawn(move || {
        let status = app.status();
        let _ = jobs.send(Msg::Stop);
        status
    });
    node::serve(&net, msgs);
    match waiter.join() {
        Ok(Ok(status)) if status.success() => {}
        Ok(Ok(status)) => {
            eprintln!("the app exited with {status}");
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(Err(error)) => {
            eprintln!("the app did not start: {error}");
            std::process::exit(1);
        }
        Err(_) => std::process::exit(1),
    }
}

/// Where cargo put us: the view crate, its workspace, its target dir, and
/// the profile the wasm builds with.
struct Workspace {
    cargo: String,
    crate_name: String,
    crate_dir: PathBuf,
    root: PathBuf,
    target: PathBuf,
    profile: String,
}

impl Workspace {
    fn here() -> Option<Self> {
        let crate_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
        let crate_name = std::env::var("CARGO_PKG_NAME").ok()?;
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let metadata = Command::new(&cargo)
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .current_dir(&crate_dir)
            .output()
            .ok()?;
        let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).ok()?;
        Some(Workspace {
            cargo,
            crate_name,
            crate_dir,
            root: PathBuf::from(metadata.get("workspace_root")?.as_str()?),
            target: PathBuf::from(metadata.get("target_directory")?.as_str()?),
            profile: std::env::var("WASM_PROFILE").unwrap_or_else(|_| "fast".into()),
        })
    }

    fn cargo(&self) -> Command {
        let mut cargo = Command::new(&self.cargo);
        cargo.current_dir(&self.crate_dir);
        cargo
    }

    /// The view for wasm32, as `make wasm-views` builds it, before wasm-opt.
    fn build_view(&self) -> Vec<u8> {
        say(format!(
            "building {} for wasm32 ({})",
            self.crate_name, self.profile
        ));
        let built = self
            .cargo()
            .args(["build", "--profile", &self.profile])
            .args(["--target", "wasm32-unknown-unknown", "-p"])
            .arg(&self.crate_name)
            .arg("--target-dir")
            .arg(&self.target)
            .status();
        if !built.is_ok_and(|status| status.success()) {
            eprintln!("the view did not build");
            std::process::exit(1);
        }
        let artifact = self
            .target
            .join("wasm32-unknown-unknown")
            .join(&self.profile)
            .join(format!("{}.wasm", self.crate_name.replace('-', "_")));
        match std::fs::read(&artifact) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("{}: {error}", artifact.display());
                std::process::exit(1);
            }
        }
    }

    /// `program`'s describe module, as `make wasm-describes` builds it (in
    /// a target dir of its own: the crate with `describe` on is another
    /// unit); `None`, said, for a program this workspace does not build.
    fn build_describe(&self, program: &str) -> Option<Vec<u8>> {
        let target = self.target.join("describe").join(program);
        say(format!("building {program}'s describe module"));
        let built = self
            .cargo()
            .args(["build", "--profile", &self.profile])
            .args(["--target", "wasm32-unknown-unknown", "-p"])
            .arg(program)
            .args(["--features", "describe", "--target-dir"])
            .arg(&target)
            .status();
        if !built.is_ok_and(|status| status.success()) {
            say(format!(
                "{program}'s describe module did not build: `program.describe` answers None for it"
            ));
            return None;
        }
        let artifact = target
            .join("wasm32-unknown-unknown")
            .join(&self.profile)
            .join(format!("{}.wasm", program.replace('-', "_")));
        std::fs::read(artifact).ok()
    }

    /// The app on the fake node: `$DUCKTAPE_APP`, a built binary, or
    /// `cargo run` in `$DUCKTAPE_APP_DIR` (`../app` beside this checkout),
    /// its own state under `<target>/live/<entry>/`.
    fn app_command(&self, endpoint: &str, entry: &str) -> Command {
        let home = self.target.join("live").join(entry);
        let mut command = match std::env::var_os("DUCKTAPE_APP") {
            Some(binary) => Command::new(binary),
            None => {
                let dir = std::env::var_os("DUCKTAPE_APP_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.root.join("../app"));
                let mut command = Command::new(&self.cargo);
                command
                    .arg("run")
                    .arg("--manifest-path")
                    .arg(dir.join("Cargo.toml"))
                    .arg("--");
                command
            }
        };
        command
            .arg("--live")
            .arg(endpoint)
            .arg(entry)
            .arg(DEV_SEED.to_string());
        for (var, dir) in [
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_CACHE_HOME", "cache"),
            ("DUCKTAPE_HOME", "home"),
        ] {
            let dir = home.join(dir);
            let _ = std::fs::create_dir_all(&dir);
            command.env(var, dir);
        }
        say(format!(
            "app log: {}",
            Path::new(&home).join("state/ducktape/app.log").display()
        ));
        command
    }
}
