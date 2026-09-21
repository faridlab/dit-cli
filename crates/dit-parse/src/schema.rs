//! `workflow.yaml` and `config.yaml`: load into the typed model, validate,
//! and emit canonically for `dit init`.

use dit_model::{
    Config, Coordination, DataLayout, DerivedRule, DerivedSignal, Gate, Lane, Numbering,
    ReadinessConfig, RepoLink, StatusCategory, Transition, Workflow, WorkflowStatus,
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

/// The gate a blocker must reach (ADR 0015): `terminal`, or a status id.
/// Whether the id is declared is a cross-field check — see `validate_workflow`.
fn gate_of(v: &str) -> Gate {
    if v == "terminal" {
        Gate::Terminal
    } else {
        Gate::Until(v.to_owned())
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

    let mut lanes = Vec::new();
    if let Some(ln) = root.get("lanes") {
        for node in ln.as_seq().ok_or(SchemaError::NotAList("lanes".into()))? {
            lanes.push(Lane {
                id: str_of(node, "id")?,
                label: str_of(node, "label")?,
                owners: node
                    .get("owners")
                    .and_then(Yaml::as_seq)
                    .map(|os| {
                        os.iter()
                            .filter_map(|o| o.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
    }

    let coordination = match root.get("coordination") {
        None | Some(Yaml::Null) => Coordination::default(),
        Some(cn) => {
            if !matches!(cn, Yaml::Map(_)) {
                return Err(SchemaError::NotAMap("coordination".into()));
            }
            let claim_ttl_minutes = opt_u32_of(cn, "claim_ttl_minutes")?.unwrap_or(15);
            let readiness = match cn.get("readiness") {
                None | Some(Yaml::Null) => ReadinessConfig::default(),
                Some(rn) => {
                    if !matches!(rn, Yaml::Map(_)) {
                        return Err(SchemaError::NotAMap("coordination.readiness".into()));
                    }
                    ReadinessConfig {
                        pick_from: category_of(&str_of(rn, "pick_from")?)?,
                        gate: gate_of(&str_of(rn, "gate")?),
                    }
                }
            };
            Coordination {
                claim_ttl_minutes,
                readiness,
            }
        }
    };

    let workflow = Workflow {
        statuses,
        transitions,
        derived,
        lanes,
        coordination,
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
    // Lanes (ADR 0015): ids share the status charset (they land in queries,
    // paths and SQL as tokens) and must be unique.
    let mut seen_lanes = std::collections::HashSet::new();
    for l in &wf.lanes {
        let plain = !l.id.is_empty()
            && l.id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if !plain {
            return Err(SchemaError::BadValue {
                key: "lanes.id".into(),
                value: l.id.clone(),
                hint: "lane ids are lowercase letters, digits, dashes and underscores".into(),
            });
        }
        if !seen_lanes.insert(&l.id) {
            return Err(SchemaError::BadValue {
                key: "lanes".into(),
                value: l.id.clone(),
                hint: "the same lane id appears twice".into(),
            });
        }
    }
    // The coordination block (ADR 0015). A gate naming a status that does not
    // exist would silently block every dependent forever — refuse it here.
    if let Gate::Until(id) = &wf.coordination.readiness.gate {
        if !wf.contains_status(id) {
            return Err(SchemaError::BadValue {
                key: "coordination.readiness.gate".into(),
                value: id.clone(),
                hint: "must be `terminal` or one of the declared statuses".into(),
            });
        }
    }
    if wf.coordination.claim_ttl_minutes == 0 {
        return Err(SchemaError::BadValue {
            key: "coordination.claim_ttl_minutes".into(),
            value: "0".into(),
            hint: "a zero TTL makes every claim instantly stale — pick a real number".into(),
        });
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
    let layout = enum_value(&root, "layout", DataLayout::parse)?.unwrap_or_default();
    let numbering = enum_value(&root, "numbering", Numbering::parse)?.unwrap_or_default();
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
        layout,
        numbering,
        repos,
    })
}

/// Read a closed-set enum key: absent or null → `None` (caller applies the
/// default), a value outside the set → `BadValue` naming the legal values.
/// Two-value enums only — a free-form path here would be a config surface
/// every consumer has to branch on (ADR 0005).
fn enum_value<T>(
    root: &Yaml,
    key: &str,
    parse: fn(&str) -> Option<T>,
) -> Result<Option<T>, SchemaError> {
    match root.get(key) {
        None | Some(Yaml::Null) => Ok(None),
        Some(v) => {
            let raw = v.as_str().ok_or_else(|| SchemaError::BadValue {
                key: key.into(),
                value: type_name(v).to_owned(),
                hint: "must be one of the documented values".into(),
            })?;
            match parse(raw) {
                Some(parsed) => Ok(Some(parsed)),
                None => Err(SchemaError::BadValue {
                    key: key.into(),
                    value: raw.to_owned(),
                    hint: format!("not one of the two legal values ({})", legal_values(key)),
                }),
            }
        }
    }
}

fn legal_values(key: &str) -> &'static str {
    match key {
        "layout" => "root | dotdir",
        "numbering" => "local | on-merge",
        _ => "see DESIGN.md",
    }
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
    // Lanes and coordination (ADR 0015) are emitted only when configured —
    // the seed workflow and every pre-ADR-0015 file stay byte-identical.
    if !wf.lanes.is_empty() {
        out.push_str("lanes:\n");
        for l in &wf.lanes {
            let mut fields = format!(
                "id: {}, label: {}",
                quote_if_needed(&l.id),
                quote_if_needed(&l.label)
            );
            if !l.owners.is_empty() {
                let os: Vec<String> = l.owners.iter().map(|o| quote_if_needed(o)).collect();
                fields.push_str(&format!(", owners: [{}]", os.join(", ")));
            }
            out.push_str(&format!("  - {{ {fields} }}\n"));
        }
    }
    if wf.coordination != Coordination::default() {
        out.push_str("coordination:\n");
        out.push_str(&format!(
            "  claim_ttl_minutes: {}\n",
            wf.coordination.claim_ttl_minutes
        ));
        out.push_str("  readiness:\n");
        out.push_str(&format!(
            "    pick_from: {}\n",
            wf.coordination.readiness.pick_from.as_str()
        ));
        out.push_str(&format!(
            "    gate: {}\n",
            wf.coordination.readiness.gate.as_str()
        ));
    }
    out
}

/// Emit `.dit/config.yaml`. `layout` and `numbering` are always written —
/// the file doubles as the answer to "where do my files go?" (ADR 0005).
pub fn write_config(cfg: &Config) -> String {
    let mut out = format!("schema_version: {}\n", cfg.schema_version);
    out.push_str(&format!("layout: {}\n", cfg.layout.as_str()));
    out.push_str(&format!("numbering: {}\n", cfg.numbering.as_str()));
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
    fn a_workflow_with_lanes_and_coordination_round_trips() {
        let wf = Workflow {
            lanes: vec![
                Lane {
                    id: "backend".into(),
                    label: "Backend".into(),
                    owners: vec!["be-1".into()],
                },
                Lane {
                    id: "frontend".into(),
                    label: "Front End".into(),
                    owners: vec![],
                },
            ],
            coordination: Coordination {
                claim_ttl_minutes: 30,
                readiness: ReadinessConfig {
                    pick_from: StatusCategory::Todo,
                    gate: Gate::Until("review".into()),
                },
            },
            ..Workflow::default_workflow()
        };
        let text = write_workflow(&wf);
        assert!(text.contains("lanes:\n"));
        assert!(text.contains("id: backend, label: Backend, owners: [be-1]"));
        assert!(text.contains("label: \"Front End\""));
        assert!(text.contains("gate: review"));
        assert_eq!(parse_workflow(&text).unwrap(), wf);
    }

    #[test]
    fn a_legacy_workflow_without_the_blocks_defaults_them() {
        let text = "\
statuses:
  - { id: todo, label: To Do, category: todo }
  - { id: done, label: Done, category: done, terminal: true }
transitions:
  - from: [todo]
    to: done
";
        let wf = parse_workflow(text).unwrap();
        assert!(wf.lanes.is_empty());
        assert_eq!(wf.coordination, Coordination::default());
        // And the canonical emitter adds nothing the legacy file lacked.
        let emitted = write_workflow(&wf);
        assert!(!emitted.contains("lanes:"));
        assert!(!emitted.contains("coordination:"));
        assert_eq!(parse_workflow(&emitted).unwrap(), wf);
    }

    #[test]
    fn a_gate_naming_an_undeclared_status_is_refused() {
        let text = "\
statuses:
  - { id: todo, label: To Do, category: todo }
  - { id: done, label: Done, category: done, terminal: true }
coordination:
  claim_ttl_minutes: 15
  readiness:
    pick_from: todo
    gate: shipping
";
        let err = parse_workflow(text).unwrap_err();
        assert!(err.to_string().contains("shipping"), "{err}");
    }

    #[test]
    fn duplicate_lane_ids_and_bad_charsets_are_refused() {
        let lanes = |id: &str| {
            format!(
                "\
statuses:
  - {{ id: todo, label: To Do, category: todo }}
lanes:
  - {{ id: {id}, label: L1 }}
  - {{ id: {id}, label: L2 }}
"
            )
        };
        assert!(parse_workflow(&lanes("backend")).is_err());
        let bad_charset = "\
statuses:
  - { id: todo, label: To Do, category: todo }
lanes:
  - { id: Backend, label: L1 }
";
        assert!(parse_workflow(bad_charset).is_err());
    }

    #[test]
    fn a_zero_claim_ttl_is_refused() {
        let text = "\
statuses:
  - { id: todo, label: To Do, category: todo }
coordination:
  claim_ttl_minutes: 0
";
        let err = parse_workflow(text).unwrap_err();
        assert!(err.to_string().contains("claim_ttl_minutes"), "{err}");
    }

    #[test]
    fn config_round_trips() {
        let cfg = Config {
            schema_version: 1,
            layout: DataLayout::DotDir,
            numbering: Numbering::OnMerge,
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
        assert_eq!(
            text, "schema_version: 1\nlayout: root\nnumbering: local\n",
            "layout and numbering are always explicit — the config answers \
             'where do my files go?' (ADR 0005)"
        );
        assert_eq!(parse_config(&text).unwrap(), Config::default());
    }

    #[test]
    fn a_config_written_before_the_layout_key_still_parses() {
        let legacy = "schema_version: 1\n";
        let cfg = parse_config(legacy).unwrap();
        assert_eq!(cfg.layout, DataLayout::Root);
        assert_eq!(cfg.numbering, Numbering::Local);
    }

    #[test]
    fn a_free_form_layout_path_is_refused() {
        let err = parse_config("schema_version: 1\nlayout: /home/me/data\n").unwrap_err();
        assert!(err.to_string().contains("root | dotdir"), "{err}");
        let err = parse_config("schema_version: 1\nnumbering: whenever\n").unwrap_err();
        assert!(err.to_string().contains("local | on-merge"), "{err}");
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
            ..Workflow::default_workflow()
        };
        let text = write_workflow(&wf);
        assert!(text.contains("label: \"Waiting for Review\""));
        assert_eq!(parse_workflow(&text).unwrap(), wf);
    }
}
