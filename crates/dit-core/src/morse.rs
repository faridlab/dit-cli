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

use std::path::Path;

use dit_index::{StoredMorseScenario, StoredMorseSpec};
use dit_model::{MorseScenario, SpecEntry};
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
enum SpecRepo<'a> {
    Here(&'a Repo),
    Linked(Repo),
}

impl SpecRepo<'_> {
    fn get(&self) -> &Repo {
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
            });
        }

        let mut scenarios = Vec::new();
        for stored in self.index.morse_scenarios()? {
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
        let Some(name) = &entry.repo else {
            return Ok(SpecRepo::Here(&self.repo));
        };
        let Some(link) = self.config.repos.iter().find(|r| &r.name == name) else {
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
                    }
                },
            }
            self.index
                .set_morse_health(&row.scenario, stale_by, &reasons)?;
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
                continue;
            };
            let known = self.index.morse_operations(&op.spec)?;
            if !known.iter().any(|k| k.operation_id == op.operation) {
                reasons.push(format!(
                    "step `{}` calls `{}`, which the spec no longer describes",
                    step.id,
                    op.qualified()
                ));
            }
        }
        Ok(reasons)
    }
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
