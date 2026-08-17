//! `workflow.yaml` and `config.yaml`: load into the typed model, validate,
//! and emit canonically for `dit init`.

use dit_model::{
    Config, DerivedRule, DerivedSignal, RepoLink, StatusCategory, Transition, Workflow,
    WorkflowStatus,
};

use crate::yaml::{self, Yaml, YamlError};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SchemaError {
    #[error(transparent)]
    Yaml(#[from] YamlError),
    #[error("`{0}` must be a list — check the indentation under it")]
    NotAList(String),
    #[error("`{0}` must be a mapping — check the indentation under it")]
    NotAMap(String),
    #[error("`{0}` is missing — the file is incomplete")]
    Missing(String),
    #[error("`{key}`: bad value `{value}` — {hint}")]
    BadValue {
        key: String,
        value: String,
        hint: String,
    },
}

fn str_of(node: &Yaml, key: &str) -> Result<String, SchemaError> {
    node.get(key)
        .and_then(Yaml::as_str)
        .map(str::to_owned)
        .ok_or_else(|| SchemaError::Missing(key.to_owned()))
}

fn opt_bool_of(node: &Yaml, key: &str) -> Result<Option<bool>, SchemaError> {
    match node.get(key) {
        None => Ok(None),
        Some(Yaml::Null) => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or(SchemaError::BadValue {
            key: key.to_owned(),
            value: type_name(v).to_owned(),
            hint: "must be true or false".into(),
        }),
    }
}

fn opt_u32_of(node: &Yaml, key: &str) -> Result<Option<u32>, SchemaError> {
    match node.get(key) {
        None => Ok(None),
        Some(Yaml::Null) => Ok(None),
        Some(v) => v.as_u32().map(Some).ok_or(SchemaError::BadValue {
            key: key.to_owned(),
            value: type_name(v).to_owned(),
            hint: "must be a whole number".into(),
        }),
    }
}

fn type_name(v: &Yaml) -> String {
    match v {
        Yaml::Null => "null".into(),
        Yaml::Str(s) => s.clone(),
        Yaml::Seq(_) => "[list]".into(),
        Yaml::Map(_) => "{map}".into(),
    }
}

fn category_of(v: &str) -> Result<StatusCategory, SchemaError> {
    match v {
        "todo" => Ok(StatusCategory::Todo),
        "doing" => Ok(StatusCategory::Doing),
        "done" => Ok(StatusCategory::Done),
        other => Err(SchemaError::BadValue {
            key: "category".into(),
            value: other.to_owned(),
            hint: "must be todo, doing, or done".into(),
        }),
    }
}

fn signal_of(v: &str) -> Result<DerivedSignal, SchemaError> {
    match v {
        "commit_trailer" => Ok(DerivedSignal::CommitTrailer),
        "pr_merged" => Ok(DerivedSignal::PrMerged),
        other => Err(SchemaError::BadValue {
            key: "on".into(),
            value: other.to_owned(),
            hint: "must be commit_trailer or pr_merged".into(),
        }),
    }
}

/// Parse and validate `schema/workflow.yaml`.
pub fn parse_workflow(text: &str) -> Result<Workflow, SchemaError> {
    let root = yaml::parse(text)?;
    let statuses_node = root
        .get("statuses")
        .ok_or_else(|| SchemaError::Missing("statuses".into()))?;
    let statuses_seq = statuses_node
        .as_seq()
        .ok_or(SchemaError::NotAList("statuses".into()))?;
    if statuses_seq.is_empty() {
        return Err(SchemaError::NotAList("statuses".into()));
    }

    let mut statuses = Vec::new();
    for node in statuses_seq {
        let id = str_of(node, "id")?;
        let label = str_of(node, "label")?;
        let category = category_of(&str_of(node, "category")?)?;
        statuses.push(WorkflowStatus {
            id,
            label,
            category,
            wip_limit: opt_u32_of(node, "wip_limit")?,
            terminal: opt_bool_of(node, "terminal")?.unwrap_or(false),
        });
    }

    let mut transitions = Vec::new();
    if let Some(tn) = root.get("transitions") {
        for node in tn
            .as_seq()
            .ok_or(SchemaError::NotAList("transitions".into()))?
        {
            let to = str_of(node, "to")?;
            let from_node = node
                .get("from")
                .ok_or_else(|| SchemaError::Missing("from".into()))?;
            let from = from_node
                .as_seq()
                .ok_or(SchemaError::NotAList("from".into()))?
                .iter()
                .map(|f| {
                    f.as_str().map(str::to_owned).ok_or(SchemaError::BadValue {
                        key: "from".into(),
                        value: type_name(f).to_owned(),
                        hint: "status ids must be plain text".into(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut requires = Vec::new();
            if let Some(req) = node.get("requires") {
                requires = req
                    .as_seq()
                    .ok_or(SchemaError::NotAList("requires".into()))?
                    .iter()
                    .map(|r| {
                        r.as_str().map(str::to_owned).ok_or(SchemaError::BadValue {
                            key: "requires".into(),
                            value: type_name(r).to_owned(),
                            hint: "must be plain text".into(),
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
            }
            transitions.push(Transition { from, to, requires });
        }
    }

    let mut derived = Vec::new();
    if let Some(dn) = root.get("derived") {
        for node in dn.as_seq().ok_or(SchemaError::NotAList("derived".into()))? {
            derived.push(DerivedRule {
                signal: signal_of(&str_of(node, "on")?)?,
                implies: str_of(node, "implies")?,
            });
        }
    }

    let workflow = Workflow {
        statuses,
        transitions,
        derived,
    };
    validate_workflow(&workflow)?;
    Ok(workflow)
}

/// Cross-field checks the type system cannot express.
fn validate_workflow(wf: &Workflow) -> Result<(), SchemaError> {
    let mut seen = std::collections::HashSet::new();
    for s in &wf.statuses {
        if !seen.insert(&s.id) {
            return Err(SchemaError::BadValue {
                key: "statuses".into(),
                value: s.id.clone(),
                hint: "the same status id appears twice".into(),
            });
        }
    }
    for t in &wf.transitions {
        if !wf.contains_status(&t.to) {
            return Err(SchemaError::BadValue {
                key: "transitions.to".into(),
                value: t.to.clone(),
                hint: "not one of the declared statuses".into(),
            });
        }
        for f in &t.from {
            if f != "*" && !wf.contains_status(f) {
                return Err(SchemaError::BadValue {
                    key: "transitions.from".into(),
                    value: f.clone(),
                    hint: "not one of the declared statuses".into(),
                });
            }
        }
    }
    for d in &wf.derived {
        if !wf.contains_status(&d.implies) {
            return Err(SchemaError::BadValue {
                key: "derived.implies".into(),
                value: d.implies.clone(),
                hint: "not one of the declared statuses".into(),
            });
        }
    }
    Ok(())
}

/// Parse `.dit/config.yaml`.
pub fn parse_config(text: &str) -> Result<Config, SchemaError> {
    let root = yaml::parse(text)?;
    let schema_version = match root.get("schema_version") {
        None => 1,
        Some(Yaml::Null) => 1,
        Some(v) => v.as_u32().ok_or(SchemaError::BadValue {
            key: "schema_version".into(),
            value: type_name(v).to_owned(),
            hint: "must be a whole number".into(),
        })?,
    };
    let mut repos = Vec::new();
    if let Some(rn) = root.get("repos") {
        for node in rn.as_seq().ok_or(SchemaError::NotAList("repos".into()))? {
            repos.push(RepoLink {
                name: str_of(node, "name")?,
                remote: str_of(node, "remote")?,
                branches: node
                    .get("branches")
                    .and_then(Yaml::as_seq)
                    .map(|bs| {
                        bs.iter()
                            .filter_map(|b| b.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
    }
    Ok(Config {
        schema_version,
        repos,
    })
}

// ---- canonical emitters (used by `dit init` to seed the files) ----

fn quote_if_needed(s: &str) -> String {
    let safe = !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '@' | '+')
        });
    if safe {
        s.to_owned()
    } else {
        format!("{s:?}")
    }
}

/// Emit `schema/workflow.yaml` in the canonical layout: one flow map per
/// status on a single line, transitions as block maps, derived rules last.
pub fn write_workflow(wf: &Workflow) -> String {
    let mut out = String::from("statuses:\n");
    for s in &wf.statuses {
        let mut fields = format!(
            "id: {}, label: {}, category: {}",
            quote_if_needed(&s.id),
            quote_if_needed(&s.label),
            s.category.as_str()
        );
        if let Some(wip) = s.wip_limit {
            fields.push_str(&format!(", wip_limit: {wip}"));
        }
        if s.terminal {
            fields.push_str(", terminal: true");
        }
        out.push_str(&format!("  - {{ {fields} }}\n"));
    }
    if !wf.transitions.is_empty() {
        out.push_str("transitions:\n");
        for t in &wf.transitions {
            let from: Vec<String> = t.from.iter().map(|f| quote_if_needed(f)).collect();
            out.push_str(&format!("  - from: [{}]\n", from.join(", ")));
            out.push_str(&format!("    to: {}\n", quote_if_needed(&t.to)));
            if !t.requires.is_empty() {
                let req: Vec<String> = t.requires.iter().map(|r| quote_if_needed(r)).collect();
                out.push_str(&format!("    requires: [{}]\n", req.join(", ")));
            }
        }
    }
    if !wf.derived.is_empty() {
        out.push_str("derived:\n");
        for d in &wf.derived {
            let on = match d.signal {
                DerivedSignal::CommitTrailer => "commit_trailer",
                DerivedSignal::PrMerged => "pr_merged",
            };
            out.push_str(&format!("  - on: {on}\n"));
            out.push_str(&format!("    implies: {}\n", quote_if_needed(&d.implies)));
        }
    }
    out
}

/// Emit `.dit/config.yaml`.
pub fn write_config(cfg: &Config) -> String {
    let mut out = format!("schema_version: {}\n", cfg.schema_version);
    if cfg.repos.is_empty() {
        return out;
    }
    out.push_str("repos:\n");
    for r in &cfg.repos {
        out.push_str(&format!(
            "  - name: {}\n    remote: {}\n",
            quote_if_needed(&r.name),
            quote_if_needed(&r.remote)
        ));
        if !r.branches.is_empty() {
            let bs: Vec<String> = r.branches.iter().map(|b| quote_if_needed(b)).collect();
            out.push_str(&format!("    branches: [{}]\n", bs.join(", ")));
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use dit_model::Workflow;

    #[test]
    fn the_default_workflow_round_trips_through_yaml() {
        let wf = Workflow::default_workflow();
        let text = write_workflow(&wf);
        let back = parse_workflow(&text).unwrap();
        assert_eq!(back, wf);
    }

    #[test]
    fn config_round_trips() {
        let cfg = Config {
            schema_version: 1,
            repos: vec![RepoLink {
                name: "backend".into(),
                remote: "git@github.com:acme/backend.git".into(),
                branches: vec!["main".into(), "develop".into()],
            }],
        };
        let text = write_config(&cfg);
        assert_eq!(parse_config(&text).unwrap(), cfg);
    }

    #[test]
    fn empty_config_is_minimal() {
        let text = write_config(&Config::default());
        assert_eq!(text, "schema_version: 1\n");
        assert_eq!(parse_config(&text).unwrap(), Config::default());
    }

    #[test]
    fn unknown_status_in_a_transition_is_rejected() {
        let wf = Workflow::default_workflow();
        let mut broken = wf.clone();
        broken.transitions.push(Transition {
            from: vec!["todo".into()],
            to: "nonexistent".into(),
            requires: vec![],
        });
        let text = write_workflow(&broken);
        let err = parse_workflow(&text).unwrap_err();
        assert!(err.to_string().contains("transitions.to"), "{err}");
    }

    #[test]
    fn duplicate_status_ids_are_rejected() {
        let text = "\
statuses:
  - { id: todo, label: Todo, category: todo }
  - { id: todo, label: Again, category: doing }
";
        assert!(parse_workflow(text).is_err());
    }

    #[test]
    fn bad_category_names_the_problem() {
        let text = "\
statuses:
  - { id: todo, label: Todo, category: someday }
";
        let err = parse_workflow(text).unwrap_err();
        assert!(err.to_string().contains("someday"), "{err}");
    }

    #[test]
    fn labels_with_spaces_are_quoted_and_survive() {
        let wf = Workflow {
            statuses: vec![WorkflowStatus {
                id: "waiting".into(),
                label: "Waiting for Review".into(),
                category: StatusCategory::Doing,
                wip_limit: Some(5),
                terminal: false,
            }],
            transitions: vec![],
            derived: vec![],
        };
        let text = write_workflow(&wf);
        assert!(text.contains("label: \"Waiting for Review\""));
        assert_eq!(parse_workflow(&text).unwrap(), wf);
    }
}
