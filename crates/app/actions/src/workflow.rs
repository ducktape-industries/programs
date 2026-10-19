//! A workflow file, `.forge/workflows/<name>.toml`: the events that run it (`on.push` branches,
//! `on.change` "opened"/"updated": each Preview of a change), its jobs, each run as one check
//! (`image` only for a runner that makes a container), and `notify.channel`, told of a red push.
use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::{Event, Input, Job};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    name: Option<String>,
    on: On,
    jobs: Vec<JobFile>,
    notify: Option<NotifyFile>,
}

// The chat channel told when a check of a push run ends failed or errored.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NotifyFile {
    channel: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct On {
    #[serde(default)]
    push: Vec<String>,
    #[serde(default)]
    change: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct JobFile {
    id: String,
    runs_on: Vec<String>,
    // empty for a runner that is its own environment (an account runner such as `claude`)
    #[serde(default)]
    image: String,
    steps: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    inputs: Vec<InputFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputFile {
    path: String,
    files: String,
}

pub(crate) struct Workflow {
    pub(crate) name: Option<String>,
    pub(crate) notify: Option<String>,
    push: Vec<String>,
    opened: bool,
    updated: bool,
    pub(crate) jobs: Vec<Job>,
}

const CHANGE_EVENTS: [&str; 2] = ["opened", "updated"];

pub(crate) fn parse(text: &str) -> Result<Workflow, String> {
    let file: File = toml::from_str(text).map_err(|error| error.to_string())?;
    let unknown = file
        .on
        .change
        .iter()
        .find(|event| !CHANGE_EVENTS.contains(&event.as_str()));
    if let Some(unknown) = unknown {
        return Err(format!(
            "on.change takes \"opened\" and \"updated\", not {unknown:?}"
        ));
    }
    let mut ids = BTreeSet::new();
    let mut jobs = Vec::new();
    for job in file.jobs {
        if !ids.insert(job.id.clone()) {
            return Err(format!("two jobs are named {:?}", job.id));
        }
        let inputs = job.inputs.into_iter().map(|input| Input {
            path: input.path,
            files: input.files,
        });
        jobs.push(Job {
            id: job.id,
            runs_on: job.runs_on,
            image: job.image,
            steps: job.steps,
            env: job.env.into_iter().collect(),
            inputs: inputs.collect(),
        });
    }
    let names = |event: &str| file.on.change.iter().any(|named| named == event);
    Ok(Workflow {
        name: file.name,
        notify: file.notify.map(|notify| notify.channel),
        opened: names("opened"),
        updated: names("updated"),
        push: file.on.push,
        jobs,
    })
}

impl Workflow {
    pub(crate) fn starts_on(&self, event: &Event) -> bool {
        match event {
            Event::Push { reference } => {
                let branch = reference.strip_prefix(b"refs/heads/".as_slice());
                self.push
                    .iter()
                    .any(|named| branch == Some(named.as_bytes()))
            }
            Event::Opened { .. } => self.opened,
            Event::Updated { .. } => self.updated,
            Event::Preview { .. } => self.opened || self.updated,
            Event::Manual => true,
        }
    }
}
