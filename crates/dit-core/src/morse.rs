//! Morse (§20, ADR 0022): the read side of API scenarios, and the reindex
//! pass that fills it.
//!
//! Two things are assembled here and neither of them sends a request. The
//! catalogue is derived from each registered OpenAPI document at reindex —
//! never copied into a DIT file (I5) — and every scenario found in a
//! `dit-morse` fence is judged against it: is the spec it was pinned to
//! still where it was, and does every operation it names still exist?
//!
//! Both answers are computed during reindex and stored, so the read path
//! still comes only from the index (I2). Firing a scenario is Morse 2 and
//! lives in `dit-morse`; nothing in this module can reach the network (I11).

use std::collections::BTreeMap;
use std::path::Path;

use dit_index::{StoredMorseProof, StoredMorseScenario, StoredMorseSpec};
use dit_model::{
    Capture, Expect, MorseScenario, MorseStep, MorseValue, OperationRef, SpecEntry, StepTarget,
};
use dit_morse::{LocalConfig, PlannedStep, Policy, RunOutcome, RunPlan};
use dit_vcs::Repo;

use crate::{Dit, DitError};

/// One registered spec, as the catalogue holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorseSpecView {
    pub id: String,
    /// The linked repo holding it (Mode A), or `None` for this workspace.
    pub repo: Option<String>,
    pub path: String,
    pub title: Option<String>,
    pub version: Option<String>,
    /// The commit of the repo holding the spec when the catalogue was built.
    pub head: Option<String>,
    pub operations: Vec<dit_model::SpecOperation>,
    /// The document's `servers:`.
    pub servers: Vec<dit_model::SpecServer>,
    /// Why the document could not be read, when it could not. A spec with a
    /// problem still appears: a service whose document has gone missing is
    /// something to say out loud, not something to hide.
    pub problem: Option<String>,
}

/// What `dit morse check` reports about one scenario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioHealth {
    /// The spec has not moved since the pin and every operation resolves.
    Fresh,
    /// The spec has moved. The scenario may still be correct — that is what
    /// running it answers — but nothing has checked since.
    Stale { commits: usize },
    /// It cannot be run as written: an operation that no longer exists, an
    /// unregistered spec, a value nothing provides.
    Broken { reasons: Vec<String> },
    /// The fence itself did not parse.
    Unreadable { detail: String },
}

impl ScenarioHealth {
    pub fn label(&self) -> &'static str {
        match self {
            ScenarioHealth::Fresh => "fresh",
            ScenarioHealth::Stale { .. } => "stale",
            ScenarioHealth::Broken { .. } => "broken",
            ScenarioHealth::Unreadable { .. } => "unreadable",
        }
    }
}

/// One scenario, and where to find it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorseScenarioView {
    pub scenario: String,
    pub path: String,
    pub line: usize,
    pub spec_id: String,
    pub pin: String,
    pub env: Option<String>,
    /// Step ids in order — enough for a list without re-parsing the fence.
    pub steps: Vec<String>,
    /// The variable names the environment must provide. Names only: a value
    /// in a committed file would be a secret in git history (§20.6).
    pub requires: Vec<String>,
    pub health: ScenarioHealth,
    /// The most recent run, if this workspace has one since its last
    /// reindex. Derived and disposable (§20.7) — it says what one machine
    /// saw at one moment, which is why it never reaches a file.
    pub last_run: Option<LastRun>,
    /// Where it was proven green, one per environment, each judged against
    /// the spec as it stands (ADR 0024).
    pub proofs: Vec<ProofView>,
}

/// One environment's proof, as `check`, the screen and readiness read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofView {
    pub env: String,
    pub commit: String,
    pub on: String,
    pub health: ProofHealth,
}

/// Whether a proof still speaks for the spec as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofHealth {
    /// Proven against the spec at its current state.
    Fresh,
    /// The spec has moved `commits` times since it was proven.
    Stale { commits: usize },
    /// The proof cannot be judged: its commit is not in the spec's history,
    /// or the spec could not be read.
    Broken { reason: String },
}

impl ProofView {
    /// Proven for the spec as it stands — what readiness asks (ADR 0024).
    pub fn holds(&self) -> bool {
        self.health == ProofHealth::Fresh
    }
}

fn proof_view(p: StoredMorseProof) -> ProofView {
    let health = match (&p.broken, p.stale_by) {
        (Some(reason), _) => ProofHealth::Broken {
            reason: reason.clone(),
        },
        (None, Some(0)) => ProofHealth::Fresh,
        (None, Some(commits)) => ProofHealth::Stale { commits },
        (None, None) => ProofHealth::Broken {
            reason: "the spec's repository could not be read to judge it".to_owned(),
        },
    };
    ProofView {
        env: p.env,
        commit: p.commit,
        on: p.on,
        health,
    }
}

/// A run, reduced to what a screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastRun {
    pub ran_at: i64,
    pub passed: bool,
    pub refused: Option<String>,
    pub steps: Vec<RunStepLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStepLine {
    pub id: String,
    pub method: String,
    pub status: Option<u16>,
    pub duration_ms: u64,
    pub passed: bool,
    /// Why it failed, or what it captured — whichever there is to say.
    pub detail: String,
}

/// Everything the Morse screen and `dit morse check` read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MorseReport {
    pub specs: Vec<MorseSpecView>,
    pub scenarios: Vec<MorseScenarioView>,
}

impl MorseReport {
    /// True when nothing is broken or unreadable — what a check command
    /// exits on. Stale is not a failure: it is a fact about the world.
    pub fn is_clean(&self) -> bool {
        !self.scenarios.iter().any(|s| {
            matches!(
                s.health,
                ScenarioHealth::Broken { .. } | ScenarioHealth::Unreadable { .. }
            )
        }) && self.specs.iter().all(|s| s.problem.is_none())
    }
}

/// The repository a spec lives in. Mode C and a vendored document are here;
/// Mode A points at a linked repo, read through a ref exactly as §5 reads
/// code — never merged and never checked out.
pub(crate) enum SpecRepo<'a> {
    Here(&'a Repo),
    Linked(Repo),
}

impl SpecRepo<'_> {
    pub(crate) fn get(&self) -> &Repo {
        match self {
            SpecRepo::Here(repo) => repo,
            SpecRepo::Linked(repo) => repo,
        }
    }
}

impl Dit {
    /// The catalogue and every scenario judged against it. A pure index
    /// read: everything it reports was worked out at reindex.
    pub fn morse_report(&self) -> Result<MorseReport, DitError> {
        let mut specs = Vec::new();
        for spec in self.index.morse_specs()? {
            specs.push(MorseSpecView {
                operations: self.index.morse_operations(&spec.spec_id)?,
                id: spec.spec_id,
                repo: spec.repo,
                path: spec.path,
                title: spec.title,
                version: spec.version,
                head: spec.head,
                problem: spec.problem,
                servers: spec.servers,
            });
        }

        let mut proofs_by: std::collections::HashMap<String, Vec<ProofView>> =
            std::collections::HashMap::new();
        for p in self.index.morse_proofs()? {
            proofs_by
                .entry(p.scenario.clone())
                .or_default()
                .push(proof_view(p));
        }
        let mut scenarios = Vec::new();
        for stored in self.index.morse_scenarios()? {
            let proofs = proofs_by.remove(&stored.scenario).unwrap_or_default();
            let last_run = self.index.morse_run(&stored.scenario)?;
            let parsed = dit_parse::parse_morse_scenario(&stored.body).ok();
            let health = if let Some(detail) = &stored.problem {
                ScenarioHealth::Unreadable {
                    detail: detail.clone(),
                }
            } else if let Some(broken) = &stored.broken {
                ScenarioHealth::Broken {
                    reasons: broken.lines().map(str::to_owned).collect(),
                }
            } else {
                match stored.stale_by {
                    Some(0) | None => ScenarioHealth::Fresh,
                    Some(commits) => ScenarioHealth::Stale { commits },
                }
            };
            scenarios.push(MorseScenarioView {
                scenario: stored.scenario,
                path: stored.path,
                line: stored.line,
                spec_id: stored.spec_id,
                pin: stored.pin,
                env: stored.env,
                steps: parsed
                    .as_ref()
                    .map(|s| s.steps.iter().map(|st| st.id.clone()).collect())
                    .unwrap_or_default(),
                requires: parsed.map(|s| s.requires).unwrap_or_default(),
                health,
                last_run: last_run.map(|row| LastRun {
                    ran_at: row.ran_at,
                    passed: row.passed,
                    refused: row.refused,
                    steps: row.steps.lines().filter_map(parse_run_line).collect(),
                }),
                proofs,
            });
        }
        Ok(MorseReport { specs, scenarios })
    }

    /// Rebuild the catalogue from every registered spec. Called at reindex,
    /// before the fences are read, because a scenario is judged against it.
    pub(crate) fn refresh_morse_specs(&mut self) -> Result<(), DitError> {
        self.index.clear_morse_specs()?;
        let entries = self.config.specs.clone();
        for entry in &entries {
            let (stored, operations) = self.read_spec(entry);
            self.index.replace_morse_spec(&stored, &operations)?;
        }
        Ok(())
    }

    /// Read one registered document into a catalogue row. Every failure is
    /// reported rather than raised: one unreadable spec must not cost a
    /// workspace its reindex.
    fn read_spec(&self, entry: &SpecEntry) -> (StoredMorseSpec, Vec<dit_model::SpecOperation>) {
        let mut stored = StoredMorseSpec {
            spec_id: entry.id.clone(),
            repo: entry.repo.clone(),
            path: entry.path.clone(),
            head: None,
            title: None,
            version: None,
            problem: None,
            servers: Vec::new(),
        };
        let repo = match self.spec_repo(entry) {
            Ok(repo) => repo,
            Err(detail) => {
                stored.problem = Some(detail);
                return (stored, Vec::new());
            }
        };
        stored.head = repo.get().head().ok();
        let Some(text) = repo.get().show_text(&format!("HEAD:{}", entry.path)) else {
            stored.problem = Some(format!(
                "`{}` is not in {} at HEAD",
                entry.path,
                entry
                    .repo
                    .as_deref()
                    .map_or_else(|| "this workspace".to_owned(), |r| format!("repo `{r}`"))
            ));
            return (stored, Vec::new());
        };
        match dit_parse::parse_openapi(&text) {
            Ok(spec) => {
                stored.title = spec.title;
                stored.version = spec.version;
                stored.servers = spec.servers;
                (stored, spec.operations)
            }
            Err(err) => {
                stored.problem = Some(err.to_string());
                (stored, Vec::new())
            }
        }
    }

    /// Which repository holds a spec. A linked repo has to be available as a
    /// local checkout: DIT will not fetch one to answer a read, and Morse 1
    /// sends nothing at all.
    fn spec_repo(&self, entry: &SpecEntry) -> Result<SpecRepo<'_>, String> {
        self.repo_for(entry.repo.as_deref())
    }

    /// The repository a registry entry names: this workspace when `None`, a
    /// `repos:` entry otherwise — opened only when it is a local checkout,
    /// and read through git, never checked out.
    pub(crate) fn repo_for(&self, name: Option<&str>) -> Result<SpecRepo<'_>, String> {
        let Some(name) = name else {
            return Ok(SpecRepo::Here(&self.repo));
        };
        let Some(link) = self.config.repos.iter().find(|r| r.name == name) else {
            return Err(format!(
                "`repo: {name}` is not one of the linked repos — add it under `repos:` first"
            ));
        };
        let path = Path::new(&link.remote);
        if !path.is_dir() {
            return Err(format!(
                "the linked repo `{name}` is not a local checkout ({}), so its spec cannot be read here",
                link.remote
            ));
        }
        Repo::open(path)
            .map(SpecRepo::Linked)
            .map_err(|e| format!("the linked repo `{name}` could not be opened: {e}"))
    }

    /// Read one document's `dit-morse` fences into the index. Returns how
    /// many were passed over: a fence too broken to even name its scenario
    /// has nowhere to report itself, and a second fence for a scenario
    /// another document already holds is a warning, not a merge.
    pub(crate) fn absorb_morse_fences(
        &mut self,
        path: &str,
        text: &str,
    ) -> Result<usize, DitError> {
        let mut skipped = 0;
        for fence in dit_parse::morse_fences(text) {
            let parsed = dit_parse::parse_morse_scenario(&fence.body);
            let stored = match &parsed {
                Ok(scenario) => StoredMorseScenario {
                    scenario: scenario.scenario.clone(),
                    path: path.to_owned(),
                    line: fence.line,
                    spec_id: scenario.spec.id.clone(),
                    pin: scenario.spec.commit.clone(),
                    env: scenario.env.clone(),
                    body: fence.body.clone(),
                    problem: None,
                    stale_by: None,
                    broken: None,
                },
                Err(err) => {
                    let Some(name) = dit_parse::scenario_in_fence(&fence.body) else {
                        skipped += 1;
                        continue;
                    };
                    StoredMorseScenario {
                        scenario: name,
                        path: path.to_owned(),
                        line: fence.line,
                        spec_id: String::new(),
                        pin: String::new(),
                        env: None,
                        body: fence.body.clone(),
                        problem: Some(err.to_string()),
                        stale_by: None,
                        broken: None,
                    }
                }
            };
            if !self.index.upsert_morse_scenario(&stored)? {
                skipped += 1;
            }
        }
        Ok(skipped)
    }

    /// Judge every stored scenario against the catalogue and the history of
    /// the repo holding its spec, and record the verdict. Runs at the end of
    /// reindex, once both halves exist.
    pub(crate) fn judge_morse_scenarios(&mut self) -> Result<(), DitError> {
        let stored = self.index.morse_scenarios()?;
        let entries = self.config.specs.clone();
        for row in stored {
            if row.problem.is_some() {
                continue; // the fence never parsed; there is nothing to judge
            }
            let Ok(scenario) = dit_parse::parse_morse_scenario(&row.body) else {
                continue;
            };
            let entry = entries.iter().find(|e| e.id == scenario.spec.id);
            let mut reasons = self.unresolved_operations(&scenario)?;
            reasons.extend(unbound_reasons(&scenario));

            let mut stale_by = None;
            // Each proof is judged like the pin: how far the spec has moved
            // since the commit it was proven against (ADR 0024).
            let mut proofs: Vec<StoredMorseProof> = scenario
                .proven
                .iter()
                .map(|p| StoredMorseProof {
                    scenario: row.scenario.clone(),
                    env: p.env.clone(),
                    commit: p.commit.clone(),
                    on: p.on.clone(),
                    stale_by: None,
                    broken: None,
                })
                .collect();
            match entry {
                None => reasons.push(format!(
                    "`spec: {}` is not registered — add it under `specs:` in .dit/config.yaml",
                    scenario.spec.id
                )),
                Some(entry) => match self.spec_repo(entry) {
                    Err(detail) => reasons.push(detail),
                    Ok(repo) => {
                        let repo = repo.get();
                        if !repo.has_commit(&scenario.spec.commit) {
                            reasons.push(format!(
                                "the pinned commit `{}` is not in the repo holding `{}` — \
                                 the pin names a history this spec does not have",
                                scenario.spec.commit, scenario.spec.id
                            ));
                        } else {
                            stale_by = repo
                                .commits_touching_since(&scenario.spec.commit, &entry.path)
                                .ok();
                        }
                        for proof in &mut proofs {
                            if repo.has_commit(&proof.commit) {
                                proof.stale_by =
                                    repo.commits_touching_since(&proof.commit, &entry.path).ok();
                            } else {
                                proof.broken = Some(format!(
                                    "proven against `{}`, which is not in the repo holding `{}`",
                                    proof.commit, scenario.spec.id
                                ));
                            }
                        }
                    }
                },
            }
            self.index
                .set_morse_health(&row.scenario, stale_by, &reasons)?;
            self.index.replace_morse_proofs(&row.scenario, &proofs)?;
        }
        Ok(())
    }

    /// Steps naming an operation the catalogue does not have. The catalogue
    /// is the spec at HEAD, so this is "the API no longer offers this".
    fn unresolved_operations(&self, scenario: &MorseScenario) -> Result<Vec<String>, DitError> {
        let mut reasons = Vec::new();
        for step in &scenario.steps {
            // An inline request is the fence's own statement about an
            // endpoint no document describes, so there is no catalogue to
            // check it against — the parser has already refused the shapes
            // that would be wrong.
            let Some(op) = step.operation.as_operation() else {
                if let StepTarget::Inline(id) = &step.operation {
                    if let Some(request) = scenario.requests.iter().find(|r| &r.id == id) {
                        reasons.extend(unfilled_params(&step.id, &request.path, &step.params));
                    }
                }
                continue;
            };
            let known = self.index.morse_operations(&op.spec)?;
            let Some(found) = known.iter().find(|k| k.operation_id == op.operation) else {
                reasons.push(format!(
                    "step `{}` calls `{}`, which the spec no longer describes",
                    step.id,
                    op.qualified()
                ));
                continue;
            };
            reasons.extend(unfilled_params(&step.id, &found.path, &step.params));
        }
        Ok(reasons)
    }
}

/// Path parameters the step leaves without a value. Sending the literal
/// `{id}` to a server is a request nobody wrote (ADR 0023).
fn unfilled_params(
    step: &str,
    path: &str,
    params: &[(String, dit_model::MorseValue)],
) -> Vec<String> {
    dit_model::path_params(path)
        .into_iter()
        .filter(|name| !params.iter().any(|(key, _)| key == name))
        .map(|name| {
            format!(
                "step `{step}` calls `{path}` and gives path parameter `{name}` no value — add \
                 `params: {{ {name}: ... }}`"
            )
        })
        .collect()
}

/// Variables a scenario reads that nothing provides. Pure — the analysis
/// lives in `dit-model`; this only phrases it.
fn unbound_reasons(scenario: &MorseScenario) -> Vec<String> {
    scenario
        .unbound_variables()
        .into_iter()
        .map(|u| {
            if u.captured_later {
                format!(
                    "step `{}` reads `{{{{{}}}}}`, which a later step captures — the chain is \
                     in the wrong order",
                    u.step, u.name
                )
            } else {
                format!(
                    "step `{}` reads `{{{{{}}}}}`, which no step captures and `requires:` \
                     does not list",
                    u.step, u.name
                )
            }
        })
        .collect()
}

// ---- Morse 2: running a scenario (§20.4, §20.5) ----------------------------

/// What `dit morse sync` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    pub run: RunOutcome,
    /// The commit the pin was moved to, when it moved. `None` means the run
    /// was not green and the pin was left exactly where it was — a pin that
    /// advanced on a red run would be a claim nobody made.
    pub moved_to: Option<String>,
    /// The document the pin lives in, so a caller can say what it changed.
    pub path: String,
    /// The environment the run was proven in — `--env`, else the fence's
    /// `env:`, else `default`. A green sync records a proof under it.
    pub env: String,
}

impl Dit {
    /// Run one scenario against a live environment. Nothing in DIT calls
    /// this: it exists for `dit morse run`, `dit morse sync` and the Run
    /// control, which is the whole of §20.5.
    pub fn morse_run(&mut self, scenario: &str, env: Option<&str>) -> Result<RunOutcome, DitError> {
        let plan = self.plan(scenario, env)?;
        let policy = Policy {
            allow: self.morse_local(env.unwrap_or("default"))?,
            timeout_secs: 30,
        };
        let outcome = dit_morse::run(&plan, &policy);
        self.record_run(&outcome)?;
        Ok(outcome)
    }

    /// Keep the last run in the index, so the screen shows what the terminal
    /// just did and a reload does not lose it. Nothing of it reaches a file.
    fn record_run(&mut self, outcome: &RunOutcome) -> Result<(), DitError> {
        let steps = outcome
            .steps
            .iter()
            .map(|s| {
                let detail = if s.failures.is_empty() && s.error.is_none() {
                    s.captured
                        .iter()
                        .map(|(n, _)| format!("captured {n}"))
                        .collect::<Vec<_>>()
                        .join("; ")
                } else {
                    // Values are never kept — only that a capture happened,
                    // and why a step failed.
                    s.error
                        .clone()
                        .into_iter()
                        .chain(s.failures.iter().cloned())
                        .collect::<Vec<_>>()
                        .join("; ")
                };
                format!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    s.id,
                    s.method,
                    s.status.map_or_else(|| "-".to_owned(), |v| v.to_string()),
                    s.duration_ms,
                    if s.passed() { "ok" } else { "fail" },
                    detail.replace('\t', " ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        self.index.record_morse_run(&dit_index::StoredMorseRun {
            scenario: outcome.scenario.clone(),
            ran_at: now_seconds(),
            passed: outcome.passed(),
            refused: outcome.refused.clone(),
            steps,
        })?;
        Ok(())
    }

    /// Run a scenario and, only if every step passed, move its `commit:` pin
    /// to where the spec stands now. The pin then means "this was proven to
    /// work at this commit" rather than "these operations still exist"
    /// (§20.4), which is why nothing else may move it.
    pub fn morse_sync(
        &mut self,
        scenario: &str,
        env: Option<&str>,
        author: &str,
    ) -> Result<SyncOutcome, DitError> {
        let run = self.morse_run(scenario, env)?;
        let stored = self.stored_scenario(scenario)?;
        let parsed = dit_parse::parse_morse_scenario(&stored.body)
            .map_err(|e| DitError::Refuse(format!("scenario `{scenario}`: {e}")))?;
        let env_name = env
            .or(parsed.env.as_deref())
            .unwrap_or("default")
            .to_owned();
        if !run.passed() {
            return Ok(SyncOutcome {
                run,
                moved_to: None,
                path: stored.path,
                env: env_name,
            });
        }
        let entry = self.spec_entry(&parsed.spec.id)?;
        let repo = self.spec_repo(&entry).map_err(DitError::Refuse)?;
        let head = repo
            .get()
            .head()
            .map_err(|e| DitError::Refuse(format!("the spec's repo has no HEAD: {e}")))?;

        let body = self.read_doc(&stored.path)?;
        let repinned = repin(&body, scenario, &head).ok_or_else(|| {
            DitError::Refuse(format!(
                "the `commit:` of scenario `{scenario}` could not be found in {}",
                stored.path
            ))
        })?;
        let today = time::OffsetDateTime::now_utc().date().to_string();
        let updated = reprove(&repinned, scenario, &env_name, &head, &today).ok_or_else(|| {
            DitError::Refuse(format!(
                "the fence of scenario `{scenario}` could not be found in {}",
                stored.path
            ))
        })?;
        let mut tx = self.transaction(author)?;
        tx.write_doc(&stored.path, &updated)?;
        tx.commit(&format!(
            "dit morse sync {scenario}: verified green on {env_name} against {}",
            &head[..7.min(head.len())]
        ))?;
        Ok(SyncOutcome {
            run,
            moved_to: Some(head),
            path: stored.path,
            env: env_name,
        })
    }

    /// Trust a host on this machine. Written straight to the gitignored
    /// local file — never through a transaction, because a host becoming
    /// trusted must not be something that travels in a commit to everyone
    /// else's checkout. Returns false when it was already allowed.
    pub fn morse_allow(&self, host: &str) -> Result<bool, DitError> {
        let path = self.repo.root().join(crate::MORSE_LOCAL_PATH);
        let mut local = match std::fs::read_to_string(&path) {
            Ok(text) => LocalConfig::parse(&text)
                .map_err(|e| DitError::Refuse(format!("{}: {e}", crate::MORSE_LOCAL_PATH)))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => LocalConfig::default(),
            Err(e) => return Err(e.into()),
        };
        if !local.allow(host) {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = local
            .write()
            .map_err(|e| DitError::Refuse(format!("{}: {e}", crate::MORSE_LOCAL_PATH)))?;
        dit_store::atomic::write(&path, &text)?;
        Ok(true)
    }

    /// Change one of this machine's environments from the page (ADR 0027).
    /// Values are write-only — nothing here returns one — and the allowlist
    /// is carried over untouched: a page may point an environment at a new
    /// server, but only `dit morse allow` makes that server reachable. The
    /// file is local configuration, never committed, so it is written like
    /// `morse_allow` writes it: atomically, with no transaction and no commit.
    pub fn morse_edit_env(&self, name: &str, edit: EnvEdit) -> Result<(), DitError> {
        check_env_name(name)?;
        let path = self.repo.root().join(crate::MORSE_LOCAL_PATH);
        // The file itself, never the CI overlay: a save must not write the
        // pipeline's values into it.
        let mut local = match std::fs::read_to_string(&path) {
            Ok(text) => LocalConfig::parse(&text)
                .map_err(|e| DitError::Refuse(format!("{}: {e}", crate::MORSE_LOCAL_PATH)))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => LocalConfig::default(),
            Err(e) => return Err(e.into()),
        };
        let allowed_before = local.allow_hosts.clone();
        match edit {
            EnvEdit::Upsert { server, set } => {
                let env = local.envs.entry(name.to_owned()).or_default();
                if let Some(server) = server {
                    env.server = match server
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                    {
                        Some(url) => {
                            check_env_server(&url)?;
                            Some(url)
                        }
                        None => None,
                    };
                }
                for (var, value) in set {
                    check_var_name(&var)?;
                    match value {
                        Some(value) => {
                            if value.contains(['\n', '\r']) {
                                return Err(DitError::Refuse(format!(
                                    "the value of `{var}` holds a line break, which this file cannot keep"
                                )));
                            }
                            env.vars.insert(var, value);
                        }
                        None => {
                            env.vars.remove(&var);
                        }
                    }
                }
            }
            EnvEdit::Rename { to } => {
                check_env_name(&to)?;
                if to != name && local.envs.contains_key(&to) {
                    return Err(DitError::Refuse(format!(
                        "an environment called `{to}` already exists"
                    )));
                }
                let env = local
                    .envs
                    .remove(name)
                    .ok_or_else(|| DitError::NotFound(format!("environment `{name}`")))?;
                local.envs.insert(to, env);
            }
            EnvEdit::Delete => {
                local
                    .envs
                    .remove(name)
                    .ok_or_else(|| DitError::NotFound(format!("environment `{name}`")))?;
            }
        }
        // Belt and braces: whatever happened above, trust did not change.
        local.allow_hosts = allowed_before;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = local
            .write()
            .map_err(|e| DitError::Refuse(format!("{}: {e}", crate::MORSE_LOCAL_PATH)))?;
        dit_store::atomic::write(&path, &text)?;
        Ok(())
    }

    /// The hosts this machine allows, including anything the two CI
    /// environment variables add.
    pub fn morse_allowed_hosts(&self) -> Result<Vec<String>, DitError> {
        Ok(self.morse_local("default")?.allow_hosts)
    }

    fn stored_scenario(&self, scenario: &str) -> Result<StoredMorseScenario, DitError> {
        self.index
            .morse_scenarios()?
            .into_iter()
            .find(|s| s.scenario == scenario)
            .ok_or_else(|| DitError::NotFound(format!("scenario `{scenario}`")))
    }

    fn spec_entry(&self, id: &str) -> Result<SpecEntry, DitError> {
        self.config
            .specs
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .ok_or_else(|| {
                DitError::Refuse(format!(
                    "`spec: {id}` is not registered — add it under `specs:` in .dit/config.yaml"
                ))
            })
    }

    /// This machine's environments and allowed hosts, with the two CI
    /// environment variables folded in. Read from the gitignored local file,
    /// never from anything that travels with the repository.
    fn morse_local(&self, env_name: &str) -> Result<LocalConfig, DitError> {
        let path = self.repo.root().join(crate::MORSE_LOCAL_PATH);
        let mut local = match std::fs::read_to_string(&path) {
            Ok(text) => LocalConfig::parse(&text)
                .map_err(|e| DitError::Refuse(format!("{}: {e}", crate::MORSE_LOCAL_PATH)))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => LocalConfig::default(),
            Err(e) => return Err(e.into()),
        };
        local.overlay(
            env_name,
            std::env::var(dit_morse::ALLOW_HOSTS_VAR).ok().as_deref(),
            std::env::var(dit_morse::VARS_VAR).ok().as_deref(),
        );
        Ok(local)
    }

    /// Turn a stored scenario into something that can be sent: every step
    /// resolved against the spec at HEAD, the base URL taken from the API's
    /// own `servers:` entry, and the values from this machine.
    fn plan(&self, scenario: &str, env: Option<&str>) -> Result<RunPlan, DitError> {
        let stored = self.stored_scenario(scenario)?;
        if let Some(problem) = &stored.problem {
            return Err(DitError::Refuse(format!(
                "scenario `{scenario}` does not parse ({}:{}): {problem}",
                stored.path, stored.line
            )));
        }
        let parsed = dit_parse::parse_morse_scenario(&stored.body)
            .map_err(|e| DitError::Refuse(format!("scenario `{scenario}`: {e}")))?;
        let env_name = env.or(parsed.env.as_deref());
        let (spec, base_url, vars) = self.target(&parsed.spec.id, env_name)?;
        // Specs other than the scenario's own, each read once at HEAD.
        let mut others: std::collections::BTreeMap<String, (dit_model::OpenApiSpec, String)> =
            std::collections::BTreeMap::new();

        let mut steps = Vec::new();
        for step in &parsed.steps {
            let mut step_base = None;
            let (method, path) = match &step.operation {
                StepTarget::Inline(id) => {
                    let found = parsed
                        .requests
                        .iter()
                        .find(|r| &r.id == id)
                        .ok_or_else(|| {
                            DitError::Refuse(format!("step `{}` names no request", step.id))
                        })?;
                    (found.method.clone(), found.path.clone())
                }
                StepTarget::Operation(op) => {
                    let step_spec = if op.spec == parsed.spec.id {
                        &spec
                    } else {
                        if !others.contains_key(&op.spec) {
                            let (other, other_base, _) = self.target(&op.spec, env_name)?;
                            others.insert(op.spec.clone(), (other, other_base));
                        }
                        let (other, other_base) = &others[&op.spec];
                        step_base = Some(other_base.clone());
                        other
                    };
                    let found = step_spec.operation(&op.operation).ok_or_else(|| {
                        DitError::Refuse(format!(
                            "step `{}` calls `{}`, which the spec no longer describes",
                            step.id,
                            op.qualified()
                        ))
                    })?;
                    (found.method.clone(), found.path.clone())
                }
            };
            steps.push(PlannedStep {
                id: step.id.clone(),
                base_url: step_base,
                method,
                path,
                params: step.params.clone(),
                headers: step.headers.clone(),
                query: step.query.clone(),
                body: step.body.clone(),
                expect: step.expect.clone(),
                capture: step.capture.clone(),
            });
        }

        Ok(RunPlan {
            scenario: parsed.scenario.clone(),
            base_url,
            files: self.files_sent_by(&steps)?,
            vars,
            steps,
        })
    }

    /// Where a spec's requests go and what this machine supplies for them:
    /// the document at HEAD, the base URL from the environment's override or
    /// the spec's own `servers:`, and the environment's values. Nothing here
    /// comes from a committed DIT file (§20.6), which is why a run and a
    /// single Send share it.
    fn target(
        &self,
        spec_id: &str,
        env_name: Option<&str>,
    ) -> Result<(dit_model::OpenApiSpec, String, dit_morse::template::Vars), DitError> {
        let entry = self.spec_entry(spec_id)?;
        let repo = self.spec_repo(&entry).map_err(DitError::Refuse)?;
        let text = repo
            .get()
            .show_text(&format!("HEAD:{}", entry.path))
            .ok_or_else(|| {
                DitError::Refuse(format!("`{}` is not in its repo at HEAD", entry.path))
            })?;
        let spec = dit_parse::parse_openapi(&text)
            .map_err(|e| DitError::Refuse(format!("{}: {e}", entry.path)))?;

        let local = self.morse_local(env_name.unwrap_or("default"))?;
        let local_env = env_name.and_then(|name| local.envs.get(name));
        let base_url = local_env
            .and_then(|e| e.server.clone())
            .or_else(|| spec.server_for(env_name).map(|s| s.url.clone()))
            .ok_or_else(|| {
                DitError::Refuse(format!(
                    "nothing says where `{spec_id}` lives — the spec has no `servers:` entry and \
                     no environment overrides it"
                ))
            })?;
        // A generated spec very often declares `servers: - url: /`, which says
        // "wherever this is deployed" and names no host at all. That is a
        // perfectly good document and an impossible instruction, so it is
        // worth its own sentence rather than failing later as a malformed URL.
        if !base_url.contains("://") {
            return Err(DitError::Refuse(format!(
                "the spec for `{}` says its server is `{base_url}`, which is relative — it names \
                 no host, so nothing can be sent. Give the `{}` environment a `server:` in \
                 {}, for example:\n\
                 \n  envs:\n    {}:\n      server: \"http://localhost:8080\"\n",
                spec_id,
                env_name.unwrap_or("default"),
                crate::MORSE_LOCAL_PATH,
                env_name.unwrap_or("default"),
            )));
        }
        let vars = local_env.map(|e| e.vars.clone()).unwrap_or_default();
        Ok((spec, base_url, vars))
    }
}

// ---- The workbench (ADR 0023) ----------------------------------------------

/// One scenario in full, for a form to edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorseScenarioDetail {
    pub scenario: MorseScenario,
    pub path: String,
    pub line: usize,
    /// The fence's text as the document holds it.
    pub fence: String,
    /// False when the fence carries a `#` comment: re-serialising it would
    /// drop a person's words, so it is edited in its document instead.
    pub editable: bool,
}

/// One operation as a tab drafted it. It has no field that could name a
/// host: the method and path come from the spec, the base URL from the spec
/// or this machine (§20.6, ADR 0023).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendDraft {
    pub operation: OperationRef,
    pub params: Vec<(String, MorseValue)>,
    pub query: Vec<(String, MorseValue)>,
    pub headers: Vec<(String, MorseValue)>,
    pub body: Option<dit_model::RequestBody>,
    pub expect: Expect,
    pub capture: Vec<Capture>,
}

/// One environment on this machine, by name. The variables are *names*:
/// their values never leave the local file through this (§20.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorseEnvView {
    pub name: String,
    pub server: Option<String>,
    pub vars: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorseEnvsView {
    pub envs: Vec<MorseEnvView>,
    pub allow_hosts: Vec<String>,
}

/// A run or a send, as History lists it. `key` is the scenario name, or
/// `send:<spec>/<operationId>` for a single operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorseRunRecord {
    pub key: String,
    pub run: LastRun,
}

/// The prefix a single operation's run is kept under, so it can never be
/// mistaken for a scenario's.
pub const SEND_KEY_PREFIX: &str = "send:";

/// One change the Morse screen makes to a scenario (ADR 0027).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioEdit {
    Rename {
        to: String,
    },
    /// `None` clears `env:`.
    SetEnv(Option<String>),
    SetRequires(Vec<String>),
    DeleteStep {
        step: String,
    },
    /// Move a step to position `to` (clamped to the end).
    MoveStep {
        step: String,
        to: usize,
    },
    DuplicateStep {
        step: String,
        as_id: String,
    },
    RenameStep {
        step: String,
        to: String,
    },
}

/// One change to an environment from the page (ADR 0027).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvEdit {
    /// Create the environment if needed. `server`: `None` keeps it,
    /// `Some(None)` clears it. `set`: each value replaces, `None` removes.
    Upsert {
        server: Option<Option<String>>,
        set: Vec<(String, Option<String>)>,
    },
    Rename {
        to: String,
    },
    Delete,
}

fn check_env_name(name: &str) -> Result<(), DitError> {
    let mut chars = name.chars();
    let ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok {
        Ok(())
    } else {
        Err(DitError::Refuse(format!(
            "`{name}` is not an environment name — a letter, then letters, digits, `-` or `_`"
        )))
    }
}

fn check_var_name(name: &str) -> Result<(), DitError> {
    let mut chars = name.chars();
    let ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if ok {
        Ok(())
    } else {
        Err(DitError::Refuse(format!(
            "`{name}` is not a variable name — like `token` or `api_key`"
        )))
    }
}

/// A server is an `http` or `https` URL with a host and no credentials in
/// it — a password belongs in a variable, where it is never read back.
fn check_env_server(url: &str) -> Result<(), DitError> {
    let refuse = |why: &str| Err(DitError::Refuse(format!("`{url}` {why}")));
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return refuse("is not an http or https address");
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() {
        return refuse("names no host");
    }
    if authority.contains('@') {
        return refuse("carries credentials — put them in a variable instead");
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return refuse("holds a space or a control character");
    }
    Ok(())
}

/// A scenario name: a lowercase letter, then letters, digits, `_` or `-`.
fn check_scenario_name(name: &str) -> Result<(), DitError> {
    let mut chars = name.chars();
    let ok = chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok {
        Ok(())
    } else {
        Err(DitError::Refuse(format!(
            "`{name}` is not a scenario name — start with a lowercase letter, then letters, digits, `-` or `_`"
        )))
    }
}

/// A step id is one word.
fn check_step_id(id: &str) -> Result<(), DitError> {
    if id.is_empty()
        || id
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, ',' | '{' | '}' | '[' | ']' | ':' | '#'))
    {
        return Err(DitError::Refuse(format!(
            "`{id}` is not a step id — one word, like `login`"
        )));
    }
    Ok(())
}

/// The largest file a body may send (ADR 0027).
pub const MORSE_FILE_MAX_BYTES: usize = 10 * 1024 * 1024;

impl Dit {
    /// One scenario, parsed, from the index — the body the indexer stored.
    pub fn morse_scenario(&self, scenario: &str) -> Result<MorseScenarioDetail, DitError> {
        let stored = self.stored_scenario(scenario)?;
        if let Some(problem) = &stored.problem {
            return Err(DitError::Refuse(format!(
                "scenario `{scenario}` does not parse ({}:{}): {problem}",
                stored.path, stored.line
            )));
        }
        let parsed = dit_parse::parse_morse_scenario(&stored.body)
            .map_err(|e| DitError::Refuse(format!("scenario `{scenario}`: {e}")))?;
        Ok(MorseScenarioDetail {
            scenario: parsed,
            editable: !dit_parse::has_comments(&stored.body),
            fence: stored.body,
            path: stored.path,
            line: stored.line,
        })
    }

    /// Save one step: replace the step with the same id, or append it. Any
    /// name the chain now reads that nothing provides is added to
    /// `requires:` — a name, never a value.
    pub fn morse_save_step(
        &mut self,
        scenario: &str,
        step: MorseStep,
        author: &str,
    ) -> Result<(), DitError> {
        let id = step.id.clone();
        self.rewrite_scenario(
            scenario,
            author,
            &format!("dit morse: save step {id} of {scenario}"),
            |s| {
                match s.steps.iter_mut().find(|x| x.id == id) {
                    Some(existing) => *existing = step,
                    None => s.steps.push(step),
                }
                require_unbound(s);
                Ok(())
            },
        )?;
        Ok(())
    }

    /// One change to a scenario from the screen (ADR 0027), written back
    /// through the fence writer as one commit. Returns the scenario's name
    /// afterwards, which a rename changes.
    pub fn morse_edit_scenario(
        &mut self,
        scenario: &str,
        edit: ScenarioEdit,
        author: &str,
    ) -> Result<String, DitError> {
        let (message, renamed) = match &edit {
            ScenarioEdit::Rename { to } => {
                check_scenario_name(to)?;
                if to != scenario && self.stored_scenario(to).is_ok() {
                    return Err(DitError::Refuse(format!(
                        "a scenario called `{to}` already exists"
                    )));
                }
                self.refuse_if_named(scenario, "renamed")?;
                (
                    format!("dit morse: rename scenario {scenario} to {to}"),
                    Some(to.clone()),
                )
            }
            ScenarioEdit::SetEnv(_) | ScenarioEdit::SetRequires(_) => {
                (format!("dit morse: edit scenario {scenario}"), None)
            }
            ScenarioEdit::DeleteStep { step } => {
                (format!("dit morse: delete step {step} of {scenario}"), None)
            }
            ScenarioEdit::MoveStep { step, .. } => {
                (format!("dit morse: move step {step} of {scenario}"), None)
            }
            ScenarioEdit::DuplicateStep { step, as_id } => (
                format!("dit morse: copy step {step} of {scenario} as {as_id}"),
                None,
            ),
            ScenarioEdit::RenameStep { step, to } => (
                format!("dit morse: rename step {step} of {scenario} to {to}"),
                None,
            ),
        };
        self.rewrite_scenario(scenario, author, &message, move |s| {
            let index_of = |s: &MorseScenario, id: &str| {
                s.steps.iter().position(|x| x.id == id).ok_or_else(|| {
                    DitError::NotFound(format!("step `{id}` of scenario `{}`", s.scenario))
                })
            };
            let free = |s: &MorseScenario, id: &str| -> Result<(), DitError> {
                check_step_id(id)?;
                if s.steps.iter().any(|x| x.id == id) {
                    return Err(DitError::Refuse(format!(
                        "a step called `{id}` is already in this scenario"
                    )));
                }
                Ok(())
            };
            match edit {
                ScenarioEdit::Rename { to } => s.scenario = to,
                ScenarioEdit::SetEnv(env) => s.env = env.filter(|e| !e.trim().is_empty()),
                ScenarioEdit::SetRequires(names) => {
                    let mut kept: Vec<String> = Vec::new();
                    for name in names
                        .into_iter()
                        .map(|n| n.trim().to_owned())
                        .filter(|n| !n.is_empty())
                    {
                        if !kept.contains(&name) {
                            kept.push(name);
                        }
                    }
                    s.requires = kept;
                }
                ScenarioEdit::DeleteStep { step } => {
                    let at = index_of(s, &step)?;
                    s.steps.remove(at);
                }
                ScenarioEdit::MoveStep { step, to } => {
                    let at = index_of(s, &step)?;
                    let moved = s.steps.remove(at);
                    let to = to.min(s.steps.len());
                    s.steps.insert(to, moved);
                }
                ScenarioEdit::DuplicateStep { step, as_id } => {
                    let at = index_of(s, &step)?;
                    free(s, &as_id)?;
                    let mut copy = s.steps[at].clone();
                    copy.id = as_id;
                    s.steps.insert(at + 1, copy);
                }
                ScenarioEdit::RenameStep { step, to } => {
                    let at = index_of(s, &step)?;
                    if to != step {
                        free(s, &to)?;
                    }
                    s.steps[at].id = to;
                }
            }
            Ok(())
        })?;
        Ok(renamed.unwrap_or_else(|| scenario.to_owned()))
    }

    /// Take a scenario's fence out of its document, prose left as it was —
    /// unless an issue still names it, whose gate would silently go.
    pub fn morse_delete_scenario(&mut self, scenario: &str, author: &str) -> Result<(), DitError> {
        let detail = self.morse_scenario(scenario)?;
        self.refuse_if_named(scenario, "deleted")?;
        let document = self.read_doc(&detail.path)?;
        let written = dit_parse::remove_morse_fence(&document, scenario).ok_or_else(|| {
            DitError::Refuse(format!(
                "the fence for `{scenario}` is no longer in {}",
                detail.path
            ))
        })?;
        let mut tx = self.transaction(author)?;
        tx.write_doc(&detail.path, &written)?;
        tx.commit(&format!("dit morse: delete scenario {scenario}"))?;
        Ok(())
    }

    /// Issues whose `needs_scenarios` or `proves` names this scenario. A
    /// rename or a delete would leave them pointing at nothing, so both are
    /// refused with the list, not quietly applied.
    fn refuse_if_named(&self, scenario: &str, verb: &str) -> Result<(), DitError> {
        let naming: Vec<String> = self
            .query("", None)?
            .into_iter()
            .filter(|hit| {
                hit.issue.needs_scenarios.iter().any(|n| n == scenario)
                    || hit.issue.proves.iter().any(|n| n == scenario)
            })
            .map(|hit| {
                format!(
                    "{} ({})",
                    hit.issue.title,
                    hit.issue.id.short_ref().as_str()
                )
            })
            .collect();
        if naming.is_empty() {
            return Ok(());
        }
        Err(DitError::Refuse(format!(
            "`{scenario}` cannot be {verb}: {} name{} it in `needs_scenarios` or `proves` — {}",
            naming.len(),
            if naming.len() == 1 { "s" } else { "" },
            naming.join(", ")
        )))
    }

    /// Read a scenario, change it, write its fence back as one commit. The
    /// fence must be editable (no `#` comments) and the result must still be
    /// free of literal secrets.
    fn rewrite_scenario(
        &mut self,
        scenario: &str,
        author: &str,
        message: &str,
        change: impl FnOnce(&mut MorseScenario) -> Result<(), DitError>,
    ) -> Result<(), DitError> {
        let detail = self.morse_scenario(scenario)?;
        if !detail.editable {
            return Err(DitError::Refuse(format!(
                "scenario `{scenario}` carries a comment in its fence ({}:{}); a form would drop \
                 it, so edit this one in the document",
                detail.path, detail.line
            )));
        }
        let mut updated = detail.scenario;
        change(&mut updated)?;
        refuse_secrets(&updated)?;
        let fence = dit_parse::write_morse_scenario(&updated)
            .map_err(|e| DitError::Refuse(format!("scenario `{scenario}`: {e}")))?;
        let document = self.read_doc(&detail.path)?;
        let written =
            dit_parse::replace_morse_fence(&document, scenario, &fence).ok_or_else(|| {
                DitError::Refuse(format!(
                    "the fence for `{scenario}` is no longer in {}",
                    detail.path
                ))
            })?;
        let mut tx = self.transaction(author)?;
        tx.write_doc(&detail.path, &written)?;
        tx.commit(message)?;
        Ok(())
    }

    /// Start a scenario with one step, as a new fence at the end of a
    /// document (created if it does not exist). It is pinned where the spec
    /// stands now, which is what the pin of something just written means.
    #[allow(clippy::too_many_arguments)]
    pub fn morse_create_scenario(
        &mut self,
        doc: &str,
        name: &str,
        spec_id: &str,
        env: Option<&str>,
        step: MorseStep,
        author: &str,
    ) -> Result<MorseScenario, DitError> {
        if self
            .index
            .morse_scenarios()?
            .iter()
            .any(|s| s.scenario == name)
        {
            return Err(DitError::Refuse(format!(
                "a scenario called `{name}` already exists — scenario names are unique in a workspace"
            )));
        }
        let entry = self.spec_entry(spec_id)?;
        let head = self
            .spec_repo(&entry)
            .map_err(DitError::Refuse)?
            .get()
            .head()
            .map_err(|e| DitError::Refuse(format!("the spec's repo has no HEAD: {e}")))?;
        let mut scenario = MorseScenario {
            scenario: name.to_owned(),
            spec: dit_model::SpecPin {
                id: spec_id.to_owned(),
                commit: head,
            },
            env: env.map(str::to_owned),
            requires: Vec::new(),
            requests: Vec::new(),
            steps: vec![step],
            proven: Vec::new(),
        };
        require_unbound(&mut scenario);
        refuse_secrets(&scenario)?;
        let fence = dit_parse::write_morse_scenario(&scenario)
            .map_err(|e| DitError::Refuse(format!("scenario `{name}`: {e}")))?;
        let document = match self.read_doc(doc) {
            Ok(text) => text,
            Err(DitError::NotFound(_)) => format!("# {name}\n"),
            Err(other) => return Err(other),
        };
        let mut tx = self.transaction(author)?;
        tx.write_doc(doc, &dit_parse::append_morse_fence(&document, &fence))?;
        tx.commit(&format!("dit morse: new scenario {name}"))?;
        Ok(scenario)
    }

    /// Send one operation as a tab drafted it (ADR 0023). A one-step plan
    /// through the same gates as a run: the operation must resolve at HEAD,
    /// the host must be allowed here, and nothing is written but the index
    /// row that says what happened.
    pub fn morse_send(
        &mut self,
        draft: &SendDraft,
        env: Option<&str>,
    ) -> Result<RunOutcome, DitError> {
        let (spec, base_url, vars) = self.target(&draft.operation.spec, env)?;
        let op = spec.operation(&draft.operation.operation).ok_or_else(|| {
            DitError::Refuse(format!(
                "`{}` is not an operation the spec describes at HEAD",
                draft.operation.qualified()
            ))
        })?;
        let key = format!("{SEND_KEY_PREFIX}{}", draft.operation.qualified());
        let mut plan = RunPlan {
            scenario: key,
            base_url,
            files: BTreeMap::new(),
            vars,
            steps: vec![PlannedStep {
                id: draft.operation.operation.clone(),
                base_url: None,
                method: op.method.clone(),
                path: op.path.clone(),
                params: draft.params.clone(),
                headers: draft.headers.clone(),
                query: draft.query.clone(),
                body: draft.body.clone(),
                expect: draft.expect.clone(),
                capture: draft.capture.clone(),
            }],
        };
        plan.files = self.files_sent_by(&plan.steps)?;
        let policy = Policy {
            allow: self.morse_local(env.unwrap_or("default"))?,
            timeout_secs: 30,
        };
        let outcome = dit_morse::run(&plan, &policy);
        self.record_run(&outcome)?;
        Ok(outcome)
    }

    /// The bytes of every file the steps' bodies send, read from the
    /// workspace's HEAD (ADR 0027). Never the working tree: an edit nobody
    /// committed is not what the scenario says, and a git-ignored file — the
    /// local secrets, a `.env` — is not in HEAD at all, so a fence arriving
    /// by pull request cannot name one and have it sent.
    fn files_sent_by(&self, steps: &[PlannedStep]) -> Result<BTreeMap<String, Vec<u8>>, DitError> {
        let mut files = BTreeMap::new();
        for step in steps {
            let Some(body) = &step.body else { continue };
            for path in body.files() {
                if files.contains_key(path) {
                    continue;
                }
                if let Some(problem) = dit_model::morse_file_path_problem(path) {
                    return Err(DitError::Refuse(format!(
                        "step `{}`, `{path}`: {problem}",
                        step.id
                    )));
                }
                let bytes = self.repo.blob_bytes(&format!("HEAD:{path}")).ok_or_else(|| {
                    DitError::Refuse(format!(
                        "step `{}` sends `{path}`, which is not in the repository at HEAD — a file part \
                         is read from the last commit, never from the working tree, so commit it first",
                        step.id
                    ))
                })?;
                if bytes.len() > MORSE_FILE_MAX_BYTES {
                    return Err(DitError::Refuse(format!(
                        "step `{}` sends `{path}`, which is {} bytes — a file part is at most 10 MB",
                        step.id,
                        bytes.len()
                    )));
                }
                files.insert(path.to_owned(), bytes);
            }
        }
        Ok(files)
    }

    /// This machine's environments, by name, and the hosts it allows.
    pub fn morse_envs(&self) -> Result<MorseEnvsView, DitError> {
        let local = self.morse_local("default")?;
        Ok(MorseEnvsView {
            envs: local
                .envs
                .iter()
                .map(|(name, env)| MorseEnvView {
                    name: name.clone(),
                    server: env.server.clone(),
                    vars: env.vars.keys().cloned().collect(),
                })
                .collect(),
            allow_hosts: local.allow_hosts,
        })
    }

    /// Every run and send this workspace has kept since its last reindex,
    /// newest first.
    pub fn morse_runs(&self) -> Result<Vec<MorseRunRecord>, DitError> {
        Ok(self
            .index
            .morse_runs_all()?
            .into_iter()
            .map(|row| MorseRunRecord {
                key: row.scenario,
                run: LastRun {
                    ran_at: row.ran_at,
                    passed: row.passed,
                    refused: row.refused,
                    steps: row.steps.lines().filter_map(parse_run_line).collect(),
                },
            })
            .collect())
    }
}

/// A JSON body, as the Body tab holds it, in the shape a fence stores. Every
/// scalar becomes text, exactly as the fence reader does; which ones go out
/// as numbers is decided when the request is built (`template::to_json`).
pub fn morse_value_from_json(text: &str) -> Result<MorseValue, DitError> {
    dit_parse::parse_json(text)
        .map(|tree| dit_parse::morse_value(&tree))
        .map_err(|e| DitError::Refuse(format!("the body is not JSON: {e}")))
}

/// A fence value as indented JSON for the Body tab. Leaves go through the
/// same rule the sender uses, so what the tab shows as a number is what
/// would be sent as one; a `{{name}}` is shown as written, never filled.
pub fn morse_value_to_json(value: &MorseValue) -> String {
    let mut out = String::new();
    pretty(value, 0, &mut out);
    out
}

fn pretty(value: &MorseValue, depth: usize, out: &mut String) {
    let pad = |n: usize| "  ".repeat(n);
    match value {
        MorseValue::Str(text) => {
            let as_written: dit_morse::template::Vars = dit_model::variables_in(text)
                .into_iter()
                .map(|name| (name.clone(), format!("{{{{{name}}}}}")))
                .collect();
            let leaf = dit_morse::template::to_json(value, &as_written)
                .unwrap_or_else(|_| dit_morse::template::json_string(text));
            out.push_str(&leaf);
        }
        MorseValue::Seq(items) if items.is_empty() => out.push_str("[]"),
        MorseValue::Map(entries) if entries.is_empty() => out.push_str("{}"),
        MorseValue::Seq(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                pretty(item, depth + 1, out);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(depth));
            out.push(']');
        }
        MorseValue::Map(entries) => {
            out.push_str("{\n");
            for (i, (key, item)) in entries.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                out.push_str(&dit_morse::template::json_string(key));
                out.push_str(": ");
                pretty(item, depth + 1, out);
                out.push_str(if i + 1 < entries.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(depth));
            out.push('}');
        }
    }
}

/// A capture's source as the Capture tab writes it: `$.path`,
/// `header:Name`, or `status`.
pub fn morse_selector(text: &str) -> Option<dit_model::Selector> {
    dit_parse::parse_selector(text)
}

/// Refuse a scenario carrying a literal that looks like a real credential —
/// the same judgement `dit doctor`'s `morse-secrets` makes, applied before
/// the commit rather than after it, because git history does not forget.
fn refuse_secrets(scenario: &MorseScenario) -> Result<(), DitError> {
    match scenario.suspected_secrets().first() {
        None => Ok(()),
        Some(found) => Err(DitError::Refuse(format!(
            "step `{}`, `{}`: this looks like {} — put the value in {} and write \
             `{{{{name}}}}` here instead",
            found.step,
            found.field,
            found.reason,
            crate::MORSE_LOCAL_PATH
        ))),
    }
}

/// Add to `requires:` every name the chain reads that nothing provides and
/// no step captures anywhere. A name captured by a *later* step is left
/// alone: that is a chain in the wrong order, which `check` reports, and
/// papering over it with an environment variable would hide the mistake.
fn require_unbound(scenario: &mut MorseScenario) {
    for unbound in scenario.unbound_variables() {
        if !unbound.captured_later && !scenario.requires.contains(&unbound.name) {
            scenario.requires.push(unbound.name);
        }
    }
}

/// Rewrite the `commit:` of one scenario's fence, leaving every other byte
/// alone. Surgical rather than re-serialised, for the same reason issue files
/// are: a document holds a person's prose, and a rewrite would take it.
fn repin(document: &str, scenario: &str, commit: &str) -> Option<String> {
    let mut out = String::new();
    let mut in_fence = false;
    let mut is_ours = false;
    let mut done = false;
    for line in document.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if in_fence {
                in_fence = false;
                is_ours = false;
            } else {
                in_fence = true;
                is_ours = trimmed.trim_start_matches('`').trim() == dit_parse::MORSE_FENCE;
            }
            out.push_str(line);
            continue;
        }
        if in_fence && is_ours && trimmed.starts_with("scenario:") {
            let named = trimmed["scenario:".len()..]
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            is_ours = named == scenario;
        }
        if in_fence && is_ours && !done && trimmed.starts_with("spec:") {
            if let Some(replaced) = replace_commit(line, commit) {
                out.push_str(&replaced);
                done = true;
                continue;
            }
        }
        out.push_str(line);
    }
    done.then_some(out)
}

/// Record that one scenario was proven green in `env` (ADR 0024): set that
/// environment's line under the fence's `proven:` block, adding the block at
/// the end of the fence when there is none. Every other line — other
/// environments, comments, prose outside the fence — is left byte for byte,
/// for the reason `repin` gives. `None` when no fence names `scenario`.
fn reprove(document: &str, scenario: &str, env: &str, commit: &str, on: &str) -> Option<String> {
    let lines: Vec<&str> = document.split_inclusive('\n').collect();
    // Find the opening and closing lines of the fence that names `scenario`.
    let mut ours: Option<(usize, usize)> = None;
    let mut open: Option<usize> = None;
    let mut named = false;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            match open {
                Some(start) => {
                    if named {
                        ours = Some((start, i));
                        break;
                    }
                    open = None;
                }
                None if trimmed.trim_start_matches('`').trim() == dit_parse::MORSE_FENCE => {
                    open = Some(i);
                    named = false;
                }
                None => {}
            }
            continue;
        }
        if open.is_some() && trimmed.starts_with("scenario:") {
            let name = trimmed["scenario:".len()..]
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            named = name == scenario;
        }
    }
    let (start, close) = ours?;
    let entry = format!("  {env}: {{ commit: {commit}, on: {on} }}\n");

    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    let block = (start + 1..close).find(|&i| lines[i].trim_end() == "proven:");
    match block {
        Some(at) => {
            let mut end = at + 1;
            while end < close && lines[end].starts_with([' ', '\t']) {
                end += 1;
            }
            let prefix = format!("{env}:");
            match (at + 1..end).find(|&i| lines[i].trim_start().starts_with(&prefix)) {
                Some(i) => out[i] = entry,
                None => out.insert(end, entry),
            }
        }
        None => {
            if let Some(last) = out.get_mut(close - 1) {
                if !last.ends_with('\n') {
                    last.push('\n');
                }
            }
            out.insert(close, entry);
            out.insert(close, "proven:\n".to_owned());
        }
    }
    Some(out.concat())
}

/// Swap the value of `commit:` inside a `spec: { id: .., commit: .. }` line.
fn replace_commit(line: &str, commit: &str) -> Option<String> {
    let at = line.find("commit:")?;
    let after = &line[at + "commit:".len()..];
    let start = after.len() - after.trim_start().len();
    let value = after[start..].trim_start();
    let end = value.find([',', '}', ' ', '\n']).unwrap_or(value.len());
    let mut out = String::from(&line[..at + "commit:".len()]);
    out.push_str(&after[..start]);
    out.push_str(commit);
    out.push_str(&value[end..]);
    Some(out)
}

fn parse_run_line(line: &str) -> Option<RunStepLine> {
    let mut parts = line.split('\t');
    let id = parts.next()?.to_owned();
    let method = parts.next()?.to_owned();
    let status = parts.next()?.parse::<u16>().ok();
    let duration_ms = parts.next()?.parse::<u64>().unwrap_or(0);
    let passed = parts.next()? == "ok";
    Some(RunStepLine {
        id,
        method,
        status,
        duration_ms,
        passed,
        detail: parts.next().unwrap_or_default().to_owned(),
    })
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod body_json_tests {
    use super::*;

    #[test]
    fn a_body_survives_the_tab_and_keeps_its_references() {
        let text = "{\n  \"name\": \"Acme, QA\",\n  \"age\": 30,\n  \"id\": \"{{party_id}}\",\n  \"tags\": [\n    \"a\"\n  ],\n  \"meta\": {}\n}";
        let value = morse_value_from_json(text).unwrap();
        assert_eq!(morse_value_to_json(&value), text);
        assert!(morse_value_from_json("{ nope").is_err());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod reprove_tests {
    use super::*;

    const DOC: &str = "# Pay\n\n```dit-morse\nscenario: slip-pdf\nspec: { id: payroll, commit: a3f9c2d }\nsteps:\n  - id: pdf\n    operation: payroll/getSalarySlip\n```\n\nProse after.\n";

    #[test]
    fn a_first_proof_adds_the_block_inside_the_fence() {
        let out = reprove(DOC, "slip-pdf", "local-hrperf", "b7e0d11", "2026-09-27").unwrap();
        assert!(
            out.contains("    operation: payroll/getSalarySlip\nproven:\n  local-hrperf: { commit: b7e0d11, on: 2026-09-27 }\n```\n"),
            "{out}"
        );
        assert!(out.ends_with("Prose after.\n"));
    }

    #[test]
    fn proving_the_same_environment_again_replaces_its_line() {
        let once = reprove(DOC, "slip-pdf", "local", "a3f9c2d", "2026-09-26").unwrap();
        let twice = reprove(&once, "slip-pdf", "local", "c1c1c1c", "2026-09-28").unwrap();
        assert!(
            twice.contains("  local: { commit: c1c1c1c, on: 2026-09-28 }\n"),
            "{twice}"
        );
        assert_eq!(twice.matches("  local:").count(), 1, "{twice}");
    }

    #[test]
    fn another_environment_joins_the_block_and_leaves_the_first() {
        let once = reprove(DOC, "slip-pdf", "local", "a3f9c2d", "2026-09-26").unwrap();
        let both = reprove(&once, "slip-pdf", "local-hrperf", "b7e0d11", "2026-09-27").unwrap();
        assert!(
            both.contains("proven:\n  local: { commit: a3f9c2d, on: 2026-09-26 }\n  local-hrperf: { commit: b7e0d11, on: 2026-09-27 }\n```"),
            "{both}"
        );
        let fence = dit_parse::morse_fences(&both).remove(0);
        let parsed = dit_parse::parse_morse_scenario(&fence.body).unwrap();
        assert_eq!(parsed.proven.len(), 2);
    }

    #[test]
    fn another_scenario_s_fence_is_left_alone() {
        assert!(reprove(DOC, "someone-else", "local", "a3f9c2d", "2026-09-26").is_none());
    }
}
