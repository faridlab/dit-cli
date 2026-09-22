//! The `dit-morse` fence (§20.3, ADR 0022): an API scenario, written in the
//! YAML subset this crate already speaks.
//!
//! The grammar is deliberately inert. A step names an operation in a
//! registered spec, states what it expects, and binds values for later steps
//! by *selector* — a JSONPath, a header, the status. There is no expression
//! language, no pre-request hook, no transform, and the keys that would
//! become one are refused here rather than left unimplemented: a scenario
//! arriving by pull request must be data (I7). Reading a fence never sends
//! anything (I11); what does lives in `dit-morse`.

use dit_model::{
    Capture, Expect, ExpectRule, InlineRequest, JsonCheck, MorseScenario, MorseStep, MorseValue,
    OperationRef, Selector, SpecPin, StepTarget,
};

use crate::flowshape::{fences, Fence};
use crate::yaml::{self, Yaml, YamlError};

/// The info string that marks a scenario block.
pub const MORSE_FENCE: &str = "dit-morse";

/// The methods an inline request may use. A closed list: an unknown verb is
/// almost always a typo, and passing it through would only surface as a
/// failure much later, in Morse 2, against a real server.
const METHODS: &[&str] = &[
    "GET", "PUT", "POST", "DELETE", "OPTIONS", "HEAD", "PATCH", "TRACE",
];

/// Key names a DIT file may never carry, at any depth — the same list the
/// flow fence refuses, for the same reason.
const FORBIDDEN_KEYS: &[&str] = &[
    "run",
    "cmd",
    "command",
    "exec",
    "executable",
    "script",
    "shell",
    "url",
    "hook",
    "webhook",
    "pre_request",
    "post_request",
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MorseError {
    #[error("line {line}: {source}")]
    Yaml { line: usize, source: YamlError },
    #[error("the fence has no `scenario:` — a scenario must say what it is called")]
    NoScenario,
    #[error("`spec:` must name a registered spec and the commit it was checked against, as `{{ id, commit }}`")]
    BadSpec,
    #[error("`{0}` is not a key a DIT file may carry: a field naming something to run or fetch is remote code execution by pull request")]
    Forbidden(String),
    #[error("`steps:` must be a list of steps")]
    BadSteps,
    #[error("step {index} has no `id:`")]
    StepWithoutId { index: usize },
    #[error("step `{0}` is declared twice — a capture would have no defined order")]
    DuplicateStep(String),
    #[error("step `{step}`: `operation:` must be `<spec>/<operationId>`, found `{found}`")]
    BadOperation { step: String, found: String },
    #[error(
        "step `{step}` names both an `operation:` and a `request:` — one step calls one thing"
    )]
    AmbiguousStep { step: String },
    #[error("step `{step}`: `request: {found}` is not declared under `requests:` in this fence")]
    UnknownRequest { step: String, found: String },
    #[error("`requests:` entries need an `id`, a `method` and a `path`")]
    BadRequests,
    #[error("request `{id}` is declared twice")]
    DuplicateRequest { id: String },
    #[error("request `{id}`: `{found}` is not an HTTP method")]
    BadMethod { id: String, found: String },
    #[error("request `{id}`: `path: {found}` must be a path beginning with `/`, never an address — an inline request is a path on the server the scenario's spec names, and a DIT file that carries a URL is remote code execution by pull request")]
    RequestAddress { id: String, found: String },
    #[error("step `{step}`: `capture:` value `{found}` is not a selector — write a JSONPath (`$.token`), `header:<name>`, or `status`")]
    BadCapture { step: String, found: String },
    #[error("step `{step}`: `expect.status` must be a three-digit status code, found `{found}`")]
    BadStatus { step: String, found: String },
    #[error("step `{step}`: an `expect.jsonpath` entry must be `{{ exists: true }}` or a value to compare against")]
    BadExpect { step: String },
}

impl From<YamlError> for MorseError {
    fn from(source: YamlError) -> Self {
        let line = match &source {
            YamlError::BadLine { line, .. }
            | YamlError::Unterminated { line }
            | YamlError::Indent { line, .. } => *line,
        };
        MorseError::Yaml { line, source }
    }
}

/// The scenario fences in a document, with the line each one starts on.
pub fn morse_fences(body: &str) -> Vec<Fence> {
    fences(body)
        .into_iter()
        .filter(|f| f.info == MORSE_FENCE)
        .collect()
}

/// The `scenario:` a fence names, even when the rest of it does not parse —
/// so a broken fence can still report itself against the right scenario.
pub fn scenario_in_fence(body: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("scenario:") {
            let name = rest.trim().trim_matches('"').trim_matches('\'');
            if !name.is_empty() {
                return Some(name.to_owned());
            }
        }
    }
    None
}

pub fn parse_morse_scenario(text: &str) -> Result<MorseScenario, MorseError> {
    let root = yaml::parse(text)?;
    refuse_forbidden(&root)?;

    let scenario = root
        .get("scenario")
        .and_then(Yaml::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(MorseError::NoScenario)?
        .to_owned();

    let spec_node = root.get("spec").ok_or(MorseError::BadSpec)?;
    let spec = SpecPin {
        id: entry_str(spec_node, "id").ok_or(MorseError::BadSpec)?,
        commit: entry_str(spec_node, "commit").ok_or(MorseError::BadSpec)?,
    };

    let env = root
        .get("env")
        .and_then(Yaml::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let requires = match root.get("requires") {
        Some(Yaml::Seq(items)) => items
            .iter()
            .filter_map(Yaml::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
        Some(Yaml::Str(one)) if !one.trim().is_empty() => vec![one.trim().to_owned()],
        _ => Vec::new(),
    };

    let mut requests: Vec<InlineRequest> = Vec::new();
    if let Some(node) = root.get("requests") {
        for item in node.as_seq().ok_or(MorseError::BadRequests)? {
            let id = entry_str(item, "id").ok_or(MorseError::BadRequests)?;
            if requests.iter().any(|r| r.id == id) {
                return Err(MorseError::DuplicateRequest { id });
            }
            let raw_method = entry_str(item, "method").ok_or(MorseError::BadRequests)?;
            let method = raw_method.to_ascii_uppercase();
            if !METHODS.contains(&method.as_str()) {
                return Err(MorseError::BadMethod {
                    id,
                    found: raw_method,
                });
            }
            let path = entry_str(item, "path").ok_or(MorseError::BadRequests)?;
            if !is_plain_path(&path) {
                return Err(MorseError::RequestAddress { id, found: path });
            }
            requests.push(InlineRequest {
                id,
                method,
                path,
                summary: entry_str(item, "summary"),
            });
        }
    }

    let mut steps = Vec::new();
    if let Some(node) = root.get("steps") {
        let items = node.as_seq().ok_or(MorseError::BadSteps)?;
        for (index, item) in items.iter().enumerate() {
            let id = entry_str(item, "id").ok_or(MorseError::StepWithoutId { index })?;
            if steps.iter().any(|s: &MorseStep| s.id == id) {
                return Err(MorseError::DuplicateStep(id));
            }
            steps.push(step(item, &id, &requests)?);
        }
    }

    Ok(MorseScenario {
        scenario,
        spec,
        env,
        requires,
        requests,
        steps,
    })
}

/// A path on the spec's own server: it begins with `/` and carries no
/// scheme and no authority. Broad on purpose — a scheme of any name, and
/// the protocol-relative form a scheme test alone would miss.
fn is_plain_path(path: &str) -> bool {
    let trimmed = path.trim();
    if !trimmed.starts_with('/') || trimmed.starts_with("//") {
        return false;
    }
    !trimmed.contains("://")
}

fn step(node: &Yaml, id: &str, requests: &[InlineRequest]) -> Result<MorseStep, MorseError> {
    let named_request = entry_str(node, "request");
    let raw_operation = entry_str(node, "operation");
    let operation = match (raw_operation, named_request) {
        (Some(_), Some(_)) => {
            return Err(MorseError::AmbiguousStep {
                step: id.to_owned(),
            })
        }
        (None, Some(name)) => {
            if !requests.iter().any(|r| r.id == name) {
                return Err(MorseError::UnknownRequest {
                    step: id.to_owned(),
                    found: name,
                });
            }
            StepTarget::Inline(name)
        }
        (raw, None) => {
            let raw = raw.unwrap_or_default();
            StepTarget::Operation(OperationRef::parse(&raw).ok_or_else(|| {
                MorseError::BadOperation {
                    step: id.to_owned(),
                    found: raw.clone(),
                }
            })?)
        }
    };

    let pairs = |key: &str| -> Vec<(String, MorseValue)> {
        match node.get(key) {
            Some(Yaml::Map(entries)) => {
                entries.iter().map(|(k, v)| (k.clone(), value(v))).collect()
            }
            _ => Vec::new(),
        }
    };

    let mut expect = Expect::default();
    if let Some(node) = node.get("expect") {
        if let Some(raw) = entry_str(node, "status") {
            expect.status = raw
                .parse::<u16>()
                .ok()
                .filter(|s| (100..=599).contains(s))
                .ok_or(MorseError::BadStatus {
                    step: id.to_owned(),
                    found: raw.clone(),
                })
                .map(Some)?;
        }
        if let Some(Yaml::Map(entries)) = node.get("jsonpath") {
            for (path, rule) in entries {
                expect.json.push(JsonCheck {
                    path: path.clone(),
                    rule: expect_rule(rule).ok_or(MorseError::BadExpect {
                        step: id.to_owned(),
                    })?,
                });
            }
        }
    }

    let mut capture = Vec::new();
    if let Some(Yaml::Map(entries)) = node.get("capture") {
        for (name, from) in entries {
            let raw = from.as_str().unwrap_or_default().trim();
            capture.push(Capture {
                name: name.clone(),
                from: selector(raw).ok_or_else(|| MorseError::BadCapture {
                    step: id.to_owned(),
                    found: raw.to_owned(),
                })?,
            });
        }
    }

    Ok(MorseStep {
        id: id.to_owned(),
        operation,
        headers: pairs("headers"),
        query: pairs("query"),
        body: node.get("body").map(value),
        expect,
        capture,
    })
}

/// `{ exists: true }`, or a literal to compare the value against.
fn expect_rule(node: &Yaml) -> Option<ExpectRule> {
    match node {
        Yaml::Map(_) => match node.get("exists").and_then(Yaml::as_bool) {
            Some(true) => Some(ExpectRule::Exists),
            _ => None,
        },
        Yaml::Str(s) => Some(ExpectRule::Equals(s.clone())),
        _ => None,
    }
}

/// The three things a capture may read. Reads only — a transform here is
/// where a scripting language starts, and there is no fourth form.
fn selector(raw: &str) -> Option<Selector> {
    if raw == "status" {
        return Some(Selector::Status);
    }
    if let Some(name) = raw.strip_prefix("header:") {
        let name = name.trim();
        return (!name.is_empty()).then(|| Selector::Header(name.to_owned()));
    }
    raw.starts_with('$')
        .then(|| Selector::JsonPath(raw.to_owned()))
}

fn value(node: &Yaml) -> MorseValue {
    match node {
        Yaml::Null => MorseValue::Str(String::new()),
        Yaml::Str(s) => MorseValue::Str(s.clone()),
        Yaml::Seq(items) => MorseValue::Seq(items.iter().map(value).collect()),
        Yaml::Map(entries) => {
            MorseValue::Map(entries.iter().map(|(k, v)| (k.clone(), value(v))).collect())
        }
    }
}

fn entry_str(node: &Yaml, key: &str) -> Option<String> {
    node.get(key)
        .and_then(Yaml::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn refuse_forbidden(node: &Yaml) -> Result<(), MorseError> {
    match node {
        Yaml::Map(entries) => {
            for (key, value) in entries {
                let lower = key.to_ascii_lowercase();
                if FORBIDDEN_KEYS.contains(&lower.as_str()) {
                    return Err(MorseError::Forbidden(key.clone()));
                }
                refuse_forbidden(value)?;
            }
            Ok(())
        }
        Yaml::Seq(items) => items.iter().try_for_each(refuse_forbidden),
        _ => Ok(()),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const SCENARIO: &str = r#"scenario: register
spec: { id: auth, commit: a3f9c2d }
env: local
requires: [email, password]
steps:
  - id: create
    operation: auth/createUser
    body: { email: "{{email}}", password: "{{password}}" }
    expect:
      status: 201
      jsonpath:
        $.data.id: { exists: true }
    capture: { user_id: $.data.id }
  - id: login
    operation: auth/loginUser
    body: { email: "{{email}}" }
    expect:
      status: 200
    capture: { token: $.token, kind: header:X-Token }
  - id: me
    operation: auth/getCurrentUser
    headers: { Authorization: "Bearer {{token}}" }
    expect:
      status: 200
      jsonpath:
        $.id: "{{user_id}}"
"#;

    #[test]
    fn a_scenario_carries_its_chain_in_order() {
        let s = parse_morse_scenario(SCENARIO).unwrap();
        assert_eq!(s.scenario, "register");
        assert_eq!(s.spec.id, "auth");
        assert_eq!(s.spec.commit, "a3f9c2d");
        assert_eq!(s.env.as_deref(), Some("local"));
        assert_eq!(s.requires, vec!["email".to_owned(), "password".to_owned()]);
        assert_eq!(s.steps.len(), 3);
        assert_eq!(s.steps[0].operation.qualified(), "auth/createUser");
        assert_eq!(s.steps[0].expect.status, Some(201));
        assert_eq!(
            s.steps[0].expect.json,
            vec![JsonCheck {
                path: "$.data.id".into(),
                rule: ExpectRule::Exists
            }]
        );
        assert_eq!(
            s.steps[0].capture,
            vec![Capture {
                name: "user_id".into(),
                from: Selector::JsonPath("$.data.id".into())
            }]
        );
        assert_eq!(
            s.steps[1].capture[1].from,
            Selector::Header("X-Token".into())
        );
        assert_eq!(
            s.steps[2].expect.json[0].rule,
            ExpectRule::Equals("{{user_id}}".into())
        );
        assert!(
            s.unbound_variables().is_empty(),
            "every reference is provided or captured earlier"
        );
    }

    const WITH_INLINE: &str = r#"scenario: legacy
spec: { id: auth, commit: a3f9c2d }
requests:
  - id: legacyPing
    method: get
    path: /internal/ping
    summary: The endpoint nobody documented
steps:
  - id: ping
    request: legacyPing
    expect: { status: 200 }
    capture: { up: $.ok }
  - id: me
    operation: auth/getCurrentUser
"#;

    #[test]
    fn an_endpoint_no_document_describes_can_be_written_in_the_fence() {
        let s = parse_morse_scenario(WITH_INLINE).unwrap();
        assert_eq!(
            s.requests,
            vec![InlineRequest {
                id: "legacyPing".into(),
                method: "GET".into(),
                path: "/internal/ping".into(),
                summary: Some("The endpoint nobody documented".into()),
            }],
            "the method is normalised the way the catalogue writes it"
        );
        assert_eq!(
            s.steps[0].operation,
            StepTarget::Inline("legacyPing".into())
        );
        assert_eq!(s.steps[0].operation.qualified(), "legacyPing");
        assert_eq!(
            s.steps[1]
                .operation
                .as_operation()
                .map(OperationRef::qualified),
            Some("auth/getCurrentUser".to_owned()),
            "the two kinds of step live side by side in one chain"
        );
    }

    #[test]
    fn an_inline_request_may_not_carry_an_address() {
        // §20.6 / I7: no committed DIT file introduces a URL Morse could
        // reach. An inline request is a path on the spec's own server.
        for bad in [
            "https://evil.example/ping",
            "//evil.example/ping",
            "ftp://x/y",
        ] {
            let text = format!(
                "scenario: a\nspec: {{ id: x, commit: y }}\nrequests:\n  - {{ id: r, method: get, path: \"{bad}\" }}\nsteps:\n  - id: s\n    request: r\n"
            );
            let err = parse_morse_scenario(&text).unwrap_err();
            assert!(
                matches!(err, MorseError::RequestAddress { .. }),
                "`{bad}` must be refused, got: {err}"
            );
        }
        // A relative path is refused too: without a leading slash there is
        // no saying what it is relative to.
        let err = parse_morse_scenario(
            "scenario: a\nspec: { id: x, commit: y }\nrequests:\n  - { id: r, method: get, path: ping }\nsteps:\n  - id: s\n    request: r\n",
        )
        .unwrap_err();
        assert!(matches!(err, MorseError::RequestAddress { .. }), "{err}");
    }

    #[test]
    fn a_step_pointing_at_a_request_that_is_not_declared_is_refused() {
        let err = parse_morse_scenario(
            "scenario: a\nspec: { id: x, commit: y }\nsteps:\n  - id: s\n    request: nowhere\n",
        )
        .unwrap_err();
        assert!(matches!(err, MorseError::UnknownRequest { .. }), "{err}");
    }

    #[test]
    fn a_step_may_not_be_both_kinds_at_once() {
        let err = parse_morse_scenario(
            "scenario: a\nspec: { id: x, commit: y }\nrequests:\n  - { id: r, method: get, path: /p }\nsteps:\n  - id: s\n    operation: x/y\n    request: r\n",
        )
        .unwrap_err();
        assert!(matches!(err, MorseError::AmbiguousStep { .. }), "{err}");
    }

    #[test]
    fn an_unknown_http_method_is_refused_rather_than_passed_through() {
        let err = parse_morse_scenario(
            "scenario: a\nspec: { id: x, commit: y }\nrequests:\n  - { id: r, method: yeet, path: /p }\nsteps:\n  - id: s\n    request: r\n",
        )
        .unwrap_err();
        assert!(matches!(err, MorseError::BadMethod { .. }), "{err}");
    }

    #[test]
    fn a_fence_that_names_something_to_run_is_refused_by_name() {
        for bad in [
            "scenario: a\nspec: { id: x, commit: y }\nscript: rm -rf /\n",
            "scenario: a\nspec: { id: x, commit: y }\nsteps:\n  - id: s\n    operation: a/b\n    url: \"http://evil\"\n",
            "scenario: a\nspec: { id: x, commit: y }\nsteps:\n  - id: s\n    operation: a/b\n    pre_request: \"doThing()\"\n",
        ] {
            let err = parse_morse_scenario(bad).unwrap_err();
            assert!(
                matches!(err, MorseError::Forbidden(_)),
                "expected a refusal naming the key, got: {err}"
            );
        }
    }

    #[test]
    fn an_unqualified_operation_is_refused_because_it_cannot_resolve() {
        let err = parse_morse_scenario(
            "scenario: a\nspec: { id: x, commit: y }\nsteps:\n  - id: s\n    operation: loginUser\n",
        )
        .unwrap_err();
        assert!(matches!(err, MorseError::BadOperation { .. }), "{err}");
    }

    #[test]
    fn a_capture_that_is_not_a_selector_is_refused() {
        let err = parse_morse_scenario(
            "scenario: a\nspec: { id: x, commit: y }\nsteps:\n  - id: s\n    operation: a/b\n    capture: { t: \"token.substring(7)\" }\n",
        )
        .unwrap_err();
        assert!(matches!(err, MorseError::BadCapture { .. }), "{err}");
    }

    #[test]
    fn a_missing_spec_pin_is_refused_so_staleness_always_has_an_answer() {
        assert!(matches!(
            parse_morse_scenario("scenario: a\n").unwrap_err(),
            MorseError::BadSpec
        ));
        assert!(matches!(
            parse_morse_scenario("scenario: a\nspec: { id: x }\n").unwrap_err(),
            MorseError::BadSpec
        ));
        assert!(matches!(
            parse_morse_scenario("spec: { id: x, commit: y }\n").unwrap_err(),
            MorseError::NoScenario
        ));
    }

    #[test]
    fn duplicate_step_ids_and_impossible_statuses_are_refused() {
        let dup = "scenario: a\nspec: { id: x, commit: y }\nsteps:\n  - id: s\n    operation: a/b\n  - id: s\n    operation: a/c\n";
        assert!(matches!(
            parse_morse_scenario(dup).unwrap_err(),
            MorseError::DuplicateStep(_)
        ));
        let status = "scenario: a\nspec: { id: x, commit: y }\nsteps:\n  - id: s\n    operation: a/b\n    expect:\n      status: 9000\n";
        assert!(matches!(
            parse_morse_scenario(status).unwrap_err(),
            MorseError::BadStatus { .. }
        ));
    }

    #[test]
    fn only_morse_fences_are_picked_out_and_a_broken_one_still_names_itself() {
        let doc = "# Auth\n\n```dit-morse\nscenario: register\nspec: { id: auth, commit: a }\n```\n\n```mermaid\ngraph TD;\n```\n";
        let found = morse_fences(doc);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 3);
        assert_eq!(
            scenario_in_fence(&found[0].body).as_deref(),
            Some("register")
        );
        assert_eq!(
            scenario_in_fence("scenario: \"quoted name\"\nbroken: ["),
            Some("quoted name".to_owned()),
            "a fence too broken to parse can still report against its scenario"
        );
    }
}
