//! A workflow file, `.forge/workflows/<name>.toml`: the events that run it (`on.push` branches,
//! `on.change` "opened"/"updated": each Preview), its jobs, each one check (`needs`, `image`, `cpu`,
//! `memory` and `scratch` for a runner that makes a pod, `runners`), and `notify.channel`.
use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::{Asks, Event, Input, Job, bytes, millicores};

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
    // Kubernetes quantities ("16", "40Gi"), and the paths a pod holds in memory
    cpu: Option<String>,
    memory: Option<String>,
    #[serde(default)]
    scratch: Vec<String>,
    // the ids of the jobs of this file that must pass before this one runs
    #[serde(default)]
    needs: Vec<String>,
    // the accounts whose claim and report count for this job; any runner's when it names none
    runners: Option<Vec<u64>>,
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
    pub(crate) jobs: Vec<Planned>,
}

// A job as its workflow states it: the check's job, what its pod asks, the jobs it needs and the
// accounts that may run it (empty: any).
pub(crate) struct Planned {
    pub(crate) job: Job,
    pub(crate) asks: Asks,
    pub(crate) needs: Vec<String>,
    pub(crate) runners: Vec<u64>,
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
        let quantities = [
            (
                "cpu",
                &job.cpu,
                millicores(job.cpu.as_deref().unwrap_or("0")),
            ),
            (
                "memory",
                &job.memory,
                bytes(job.memory.as_deref().unwrap_or("0")),
            ),
        ];
        if let Some((key, Some(text), _)) = quantities.iter().find(|(_, _, read)| read.is_none()) {
            return Err(format!(
                "job {:?}: {key} takes a Kubernetes quantity such as \"16\" or \"40Gi\", not {text:?}",
                job.id
            ));
        }
        if job.runners.as_ref().is_some_and(Vec::is_empty) {
            return Err(format!(
                "job {:?}: runners names at least one account; leave it out for any runner",
                job.id
            ));
        }
        let asks = Asks {
            cpu: job.cpu,
            memory: job.memory,
            scratch: job.scratch,
        };
        let inputs = job.inputs.into_iter().map(|input| Input {
            path: input.path,
            files: input.files,
        });
        let planned = Job {
            id: job.id,
            runs_on: job.runs_on,
            image: job.image,
            steps: job.steps,
            env: job.env.into_iter().collect(),
            inputs: inputs.collect(),
        };
        jobs.push(Planned {
            job: planned,
            asks,
            needs: job.needs,
            runners: job.runners.unwrap_or_default(),
        });
    }
    for planned in &jobs {
        let id = &planned.job.id;
        if planned.needs.contains(id) {
            return Err(format!("job {id:?} needs itself"));
        }
        if let Some(unknown) = planned.needs.iter().find(|need| !ids.contains(*need)) {
            return Err(format!(
                "job {id:?} needs {unknown:?}, which is no job of this file"
            ));
        }
    }
    let looped = looped(&jobs);
    if !looped.is_empty() {
        return Err(format!(
            "jobs {} need each other in a loop",
            looped.join(", ")
        ));
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

// The jobs left once every job whose needs are all taken is taken, again and again: those that
// need each other, directly or through others.
fn looped(jobs: &[Planned]) -> Vec<String> {
    let mut taken: BTreeSet<&str> = BTreeSet::new();
    loop {
        let ready: Vec<&str> = jobs
            .iter()
            .filter(|planned| !taken.contains(planned.job.id.as_str()))
            .filter(|planned| {
                planned
                    .needs
                    .iter()
                    .all(|need| taken.contains(need.as_str()))
            })
            .map(|planned| planned.job.id.as_str())
            .collect();
        if ready.is_empty() {
            break;
        }
        taken.extend(ready);
    }
    jobs.iter()
        .map(|planned| planned.job.id.clone())
        .filter(|id| !taken.contains(id.as_str()))
        .collect()
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
