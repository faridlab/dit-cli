//! Morse: an API scenario as the repository states it (§20, ADR 0022).
//!
//! A scenario names the spec it was written against, the steps it walks, what
//! each step expects, and what it carries forward. Endpoints are not here:
//! they are derived from the OpenAPI document at the pinned commit, because a
//! copy in a DIT file would be derived data in the source of truth (I5).
//!
//! Nothing in this module can express something to execute. Chaining is a
//! selector and a comparison — there is no expression language, no script
//! hook, no escape — so a scenario arriving by pull request is data and stays
//! data (I7). What sends a request lives in `dit-morse`, and nothing here
//! knows it exists.

/// The spec a scenario is written against, and the commit it was last proven
/// against. The pin is a commit in the repo that *holds the spec*, which in
/// Mode A is the linked code repo — the API moves in the code's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecPin {
    pub id: String,
    pub commit: String,
}

/// A value written in a scenario: a header, a query value, a body. Recursive
/// so a real request body can be written as one, and deliberately not the
/// parser's YAML type — `dit-model` sits above `dit-parse` and must stay
/// free of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MorseValue {
    Str(String),
    Seq(Vec<MorseValue>),
    Map(Vec<(String, MorseValue)>),
}

/// The opening and closing marks of a template reference.
pub const VAR_OPEN: &str = "{{";
pub const VAR_CLOSE: &str = "}}";

impl MorseValue {
    /// Every `{{name}}` this value references, in the order written.
    pub fn variables(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_variables(&mut out);
        out
    }

    fn collect_variables(&self, out: &mut Vec<String>) {
        match self {
            MorseValue::Str(s) => out.extend(variables_in(s)),
            MorseValue::Seq(items) => items.iter().for_each(|i| i.collect_variables(out)),
            MorseValue::Map(entries) => entries.iter().for_each(|(_, v)| v.collect_variables(out)),
        }
    }
}

/// Every `{{name}}` in a string, in the order written. Whitespace inside the
/// marks is trimmed, and an unclosed mark references nothing.
pub fn variables_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find(VAR_OPEN) {
        let after = &rest[open + VAR_OPEN.len()..];
        let Some(close) = after.find(VAR_CLOSE) else {
            break;
        };
        let name = after[..close].trim();
        if !name.is_empty() {
            out.push(name.to_owned());
        }
        rest = &after[close + VAR_CLOSE.len()..];
    }
    out
}

/// The `{name}` segments of a path, in OpenAPI's own syntax, in the order
/// written. A `{{reference}}` is a template, not a parameter, and is skipped.
pub fn path_params(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        if let Some(inner) = after.strip_prefix('{') {
            match inner.find(VAR_CLOSE) {
                Some(close) => rest = &inner[close + VAR_CLOSE.len()..],
                None => break,
            }
            continue;
        }
        let Some(close) = after.find('}') else {
            break;
        };
        let name = after[..close].trim();
        if !name.is_empty() {
            out.push(name.to_owned());
        }
        rest = &after[close + 1..];
    }
    out
}

/// Which spec's operation a step calls. `operationId` is unique only inside
/// one OpenAPI document, so a workspace with more than one spec needs the
/// namespace for a step to resolve at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRef {
    pub spec: String,
    pub operation: String,
}

impl OperationRef {
    /// `auth/loginUser`. Both halves must be present and non-empty: an
    /// unqualified name has no single answer once two specs are registered.
    pub fn parse(text: &str) -> Option<Self> {
        let (spec, operation) = text.trim().split_once('/')?;
        let (spec, operation) = (spec.trim(), operation.trim());
        if spec.is_empty() || operation.is_empty() || operation.contains('/') {
            return None;
        }
        Some(OperationRef {
            spec: spec.to_owned(),
            operation: operation.to_owned(),
        })
    }

    pub fn qualified(&self) -> String {
        format!("{}/{}", self.spec, self.operation)
    }
}

/// An endpoint no registered document describes — a legacy service, a
/// third-party callback, something not yet specified. Authored in the fence
/// because it is a fact only a person knows (§20.2).
///
/// It carries a **path, never a URL**: an inline request is a path on the
/// same server the scenario's spec names, so no committed DIT file ever
/// introduces an address (I7, §20.6). An endpoint on a genuinely different
/// host needs that host's spec registered, or an environment that says so —
/// and an environment lives outside the repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineRequest {
    pub id: String,
    /// Upper-case, as people write it in a request line.
    pub method: String,
    pub path: String,
    pub summary: Option<String>,
}

/// What a step calls: an operation in a registered spec, or a request the
/// fence itself declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepTarget {
    Operation(OperationRef),
    /// The `id` of an entry in the scenario's own `requests:`.
    Inline(String),
}

impl StepTarget {
    /// How the step reads on a screen or in a report.
    pub fn qualified(&self) -> String {
        match self {
            StepTarget::Operation(op) => op.qualified(),
            StepTarget::Inline(id) => id.clone(),
        }
    }

    pub fn as_operation(&self) -> Option<&OperationRef> {
        match self {
            StepTarget::Operation(op) => Some(op),
            StepTarget::Inline(_) => None,
        }
    }
}

/// Where a captured value is read from. Three sources, all of them reads —
/// no transform, because a transform is where a scripting language starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    Status,
    Header(String),
    JsonPath(String),
}

/// A value carried from one step to the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    pub name: String,
    pub from: Selector,
}

/// What a step asserts about one JSON path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectRule {
    Exists,
    Equals(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonCheck {
    pub path: String,
    pub rule: ExpectRule,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Expect {
    pub status: Option<u16>,
    pub json: Vec<JsonCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorseStep {
    pub id: String,
    pub operation: StepTarget,
    /// Values for the `{name}` segments of the operation's path, which the
    /// spec writes in OpenAPI's own syntax. Named, never inferred from a
    /// variable that happens to share the name (ADR 0023).
    pub params: Vec<(String, MorseValue)>,
    pub headers: Vec<(String, MorseValue)>,
    pub query: Vec<(String, MorseValue)>,
    pub body: Option<MorseValue>,
    pub expect: Expect,
    pub capture: Vec<Capture>,
}

/// One scenario: the chain, and the commit it is pinned to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorseScenario {
    pub scenario: String,
    pub spec: SpecPin,
    pub env: Option<String>,
    /// The variable *names* the environment must provide. Names only — a
    /// value here would be a secret in a committed file.
    pub requires: Vec<String>,
    /// Endpoints this scenario declares because no spec describes them.
    pub requests: Vec<InlineRequest>,
    pub steps: Vec<MorseStep>,
}

/// A reference a scenario makes that nothing satisfies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnboundVariable {
    /// The step that reads it.
    pub step: String,
    pub name: String,
    /// True when a later step does capture it: the chain is in the wrong
    /// order rather than missing something, and saying which is the whole
    /// difference between a useful message and "undefined variable".
    pub captured_later: bool,
}

impl MorseScenario {
    /// Every variable the scenario reads, in order, with the step reading it.
    pub fn variables_used(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for step in &self.steps {
            let mut names = Vec::new();
            for (_, v) in &step.params {
                names.extend(v.variables());
            }
            for (_, v) in &step.headers {
                names.extend(v.variables());
            }
            for (_, v) in &step.query {
                names.extend(v.variables());
            }
            if let Some(body) = &step.body {
                names.extend(body.variables());
            }
            for check in &step.expect.json {
                if let ExpectRule::Equals(value) = &check.rule {
                    names.extend(variables_in(value));
                }
            }
            for name in names {
                out.push((step.id.clone(), name));
            }
        }
        out
    }

    /// References that nothing satisfies at the point they are read: not
    /// provided by the environment, and not captured by an *earlier* step.
    /// This is the check that a chain written out of order fails.
    pub fn unbound_variables(&self) -> Vec<UnboundVariable> {
        let captured_anywhere: Vec<&str> = self
            .steps
            .iter()
            .flat_map(|s| s.capture.iter().map(|c| c.name.as_str()))
            .collect();
        let mut bound: Vec<&str> = self.requires.iter().map(String::as_str).collect();
        let mut out = Vec::new();
        for step in &self.steps {
            for (owner, name) in self.variables_used() {
                if owner != step.id {
                    continue;
                }
                if bound.contains(&name.as_str()) {
                    continue;
                }
                if out
                    .iter()
                    .any(|u: &UnboundVariable| u.step == owner && u.name == name)
                {
                    continue;
                }
                out.push(UnboundVariable {
                    captured_later: captured_anywhere.contains(&name.as_str()),
                    step: owner,
                    name,
                });
            }
            // A capture is readable by the steps after it, never by its own:
            // the value does not exist until the step has run.
            bound.extend(step.capture.iter().map(|c| c.name.as_str()));
        }
        out
    }
}

/// A literal in a fence that looks like a real credential (§20.6). A
/// scenario states variable *names*; a value belongs in the gitignored
/// environment file or the keychain, and git history does not forget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuspectedSecret {
    pub step: String,
    /// The field it was written in — a header name, a body key.
    pub field: String,
    pub reason: &'static str,
}

/// Prefixes that are credentials wherever they appear, whatever the field is
/// called. Short and well-known on purpose: a wider net would cry wolf, and
/// a check nobody believes is worse than no check.
const SECRET_PREFIXES: &[(&str, &str)] = &[
    ("Bearer ", "a bearer token written out in full"),
    ("Basic ", "an HTTP basic credential"),
    ("sk-", "an API secret key"),
    ("ghp_", "a GitHub personal access token"),
    ("github_pat_", "a GitHub personal access token"),
    ("xoxb-", "a Slack bot token"),
    ("AKIA", "an AWS access key id"),
    ("eyJ", "a JSON Web Token"),
];

/// Auth scheme words that legitimately sit in front of a reference. They
/// are not the secret; what follows them is.
const SCHEME_WORDS: &[&str] = &["Bearer", "Basic", "Token", "Digest"];

/// Field names whose value is a secret by definition, so a literal there is
/// one however it is spelled.
const SECRET_FIELDS: &[&str] = &[
    "authorization",
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "x-api-key",
    "access_token",
    "refresh_token",
    "client_secret",
    "private_key",
];

impl MorseScenario {
    /// Literals in this scenario that look like credentials. Empty is the
    /// normal case: a scenario written properly carries `{{token}}`, and the
    /// value lives outside the repository.
    pub fn suspected_secrets(&self) -> Vec<SuspectedSecret> {
        let mut out = Vec::new();
        for step in &self.steps {
            let mut scan = |field: &str, value: &MorseValue| {
                collect_secrets(&step.id, field, value, &mut out);
            };
            for (name, value) in &step.params {
                scan(name, value);
            }
            for (name, value) in &step.headers {
                scan(name, value);
            }
            for (name, value) in &step.query {
                scan(name, value);
            }
            if let Some(body) = &step.body {
                collect_secrets(&step.id, "body", body, &mut out);
            }
        }
        out
    }
}

fn collect_secrets(step: &str, field: &str, value: &MorseValue, out: &mut Vec<SuspectedSecret>) {
    match value {
        MorseValue::Str(text) => {
            if let Some(reason) = secret_reason(field, text) {
                out.push(SuspectedSecret {
                    step: step.to_owned(),
                    field: field.to_owned(),
                    reason,
                });
            }
        }
        MorseValue::Seq(items) => {
            for item in items {
                collect_secrets(step, field, item, out);
            }
        }
        MorseValue::Map(entries) => {
            for (key, item) in entries {
                collect_secrets(step, key, item, out);
            }
        }
    }
}

/// Why one literal looks like a credential, if it does. A value made only of
/// template references is never one: that is the shape the design asks for.
fn secret_reason(field: &str, text: &str) -> Option<&'static str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    // What the author actually typed, with the references taken out.
    let residue = without_variables(trimmed);
    let residue = residue.trim();
    // `{{password}}` leaves nothing behind — the value is not in the file.
    if residue.is_empty() {
        return None;
    }
    // `Bearer {{token}}` leaves the scheme word and nothing else, which is
    // exactly the shape §20.6 asks for.
    if trimmed.contains(VAR_OPEN)
        && SCHEME_WORDS
            .iter()
            .any(|word| residue.eq_ignore_ascii_case(word))
    {
        return None;
    }
    for (prefix, reason) in SECRET_PREFIXES {
        if trimmed.starts_with(prefix) {
            return Some(reason);
        }
    }
    let plain = field.trim().to_ascii_lowercase();
    if SECRET_FIELDS.contains(&plain.as_str()) {
        return Some("a field that holds a credential, written as a literal value");
    }
    None
}

/// The text with every `{{name}}` removed, so what is left is whatever the
/// author actually wrote down.
fn without_variables(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find(VAR_OPEN) {
        out.push_str(&rest[..open]);
        let after = &rest[open + VAR_OPEN.len()..];
        match after.find(VAR_CLOSE) {
            Some(close) => rest = &after[close + VAR_CLOSE.len()..],
            None => {
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn step(id: &str, body: Option<MorseValue>, capture: &[(&str, &str)]) -> MorseStep {
        MorseStep {
            id: id.into(),
            operation: StepTarget::Operation(OperationRef {
                spec: "auth".into(),
                operation: id.into(),
            }),
            params: vec![],
            headers: vec![],
            query: vec![],
            body,
            expect: Expect::default(),
            capture: capture
                .iter()
                .map(|(n, p)| Capture {
                    name: (*n).into(),
                    from: Selector::JsonPath((*p).into()),
                })
                .collect(),
        }
    }

    fn with_headers(headers: &[(&str, &str)], body: Option<MorseValue>) -> MorseScenario {
        MorseScenario {
            scenario: "s".into(),
            spec: SpecPin {
                id: "auth".into(),
                commit: "a".into(),
            },
            env: None,
            requires: vec![],
            requests: vec![],
            steps: vec![MorseStep {
                id: "one".into(),
                operation: StepTarget::Inline("r".into()),
                params: vec![],
                headers: headers
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), MorseValue::Str((*v).to_owned())))
                    .collect(),
                query: vec![],
                body,
                expect: Expect::default(),
                capture: vec![],
            }],
        }
    }

    #[test]
    fn a_scenario_written_properly_trips_no_secret_check() {
        let ok = with_headers(
            &[("Authorization", "Bearer {{token}}"), ("X-Trace", "abc")],
            Some(MorseValue::Map(vec![(
                "password".into(),
                MorseValue::Str("{{password}}".into()),
            )])),
        );
        assert!(
            ok.suspected_secrets().is_empty(),
            "`Bearer {{{{token}}}}` and `{{{{password}}}}` are the shape the design asks for"
        );
    }

    #[test]
    fn a_credential_written_out_in_full_is_named_with_its_step_and_field() {
        let leaked = with_headers(
            &[("Authorization", "Bearer eyJhbGciOiJIUzI1NiJ9.abc.def")],
            None,
        );
        let found = leaked.suspected_secrets();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].step, "one");
        assert_eq!(found[0].field, "Authorization");
    }

    #[test]
    fn a_secret_field_holding_a_literal_is_caught_however_it_is_spelled() {
        let leaked = with_headers(
            &[],
            Some(MorseValue::Map(vec![
                ("client_secret".into(), MorseValue::Str("hunter2".into())),
                ("email".into(), MorseValue::Str("dev@acme.test".into())),
            ])),
        );
        let found = leaked.suspected_secrets();
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].field, "client_secret");
    }

    #[test]
    fn a_token_prefix_anywhere_is_caught_even_in_an_innocent_field() {
        let leaked = with_headers(&[("X-Thing", "ghp_0123456789abcdefghij")], None);
        assert_eq!(leaked.suspected_secrets().len(), 1);
    }

    #[test]
    fn path_parameters_are_the_single_brace_segments_only() {
        assert_eq!(
            path_params("/parties/{id}/contacts/{contact_id}"),
            vec!["id".to_owned(), "contact_id".to_owned()]
        );
        assert!(
            path_params("/users/{{id}}").is_empty(),
            "a template reference is not a parameter"
        );
        assert!(path_params("/health").is_empty());
    }

    #[test]
    fn a_reference_is_found_wherever_it_is_written() {
        assert_eq!(variables_in("Bearer {{token}}"), vec!["token".to_owned()]);
        assert_eq!(
            variables_in("{{ a }}/{{b}}"),
            vec!["a".to_owned(), "b".to_owned()],
            "whitespace inside the marks is not part of the name"
        );
        assert!(variables_in("{{unclosed").is_empty());
        assert!(variables_in("nothing here").is_empty());
        assert!(
            variables_in("{{}}").is_empty(),
            "an empty name references nothing"
        );
    }

    #[test]
    fn nested_bodies_are_searched_to_the_bottom() {
        let body = MorseValue::Map(vec![(
            "user".into(),
            MorseValue::Map(vec![(
                "tags".into(),
                MorseValue::Seq(vec![MorseValue::Str("{{tag}}".into())]),
            )]),
        )]);
        assert_eq!(body.variables(), vec!["tag".to_owned()]);
    }

    #[test]
    fn an_operation_must_name_its_spec() {
        assert_eq!(
            OperationRef::parse("auth/loginUser"),
            Some(OperationRef {
                spec: "auth".into(),
                operation: "loginUser".into()
            })
        );
        assert_eq!(
            OperationRef::parse("loginUser"),
            None,
            "unqualified has no single answer once two specs are registered"
        );
        assert_eq!(OperationRef::parse("auth/"), None);
        assert_eq!(OperationRef::parse("/loginUser"), None);
        assert_eq!(
            OperationRef::parse("a/b/c"),
            None,
            "one separator, so a spec id can never be ambiguous"
        );
    }

    #[test]
    fn a_value_captured_by_an_earlier_step_is_bound() {
        let scenario = MorseScenario {
            scenario: "register".into(),
            spec: SpecPin {
                id: "auth".into(),
                commit: "a3f9c2d".into(),
            },
            env: Some("local".into()),
            requires: vec!["email".into()],
            requests: vec![],
            steps: vec![
                step(
                    "create",
                    Some(MorseValue::Str("{{email}}".into())),
                    &[("token", "$.token")],
                ),
                step("me", Some(MorseValue::Str("{{token}}".into())), &[]),
            ],
        };
        assert!(
            scenario.unbound_variables().is_empty(),
            "email comes from the environment, token from step one"
        );
    }

    #[test]
    fn a_chain_written_out_of_order_says_so_rather_than_just_failing() {
        let scenario = MorseScenario {
            scenario: "register".into(),
            spec: SpecPin {
                id: "auth".into(),
                commit: "a3f9c2d".into(),
            },
            env: None,
            requires: vec![],
            requests: vec![],
            steps: vec![
                step("me", Some(MorseValue::Str("{{token}}".into())), &[]),
                step("login", None, &[("token", "$.token")]),
            ],
        };
        let unbound = scenario.unbound_variables();
        assert_eq!(unbound.len(), 1);
        assert_eq!(unbound[0].step, "me");
        assert_eq!(unbound[0].name, "token");
        assert!(
            unbound[0].captured_later,
            "the chain is in the wrong order, which is a different fix from a missing variable"
        );
    }

    #[test]
    fn a_path_parameter_reading_a_capture_is_part_of_the_chain() {
        let mut fetch = step("fetch", None, &[]);
        fetch.params = vec![("id".into(), MorseValue::Str("{{party_id}}".into()))];
        let scenario = MorseScenario {
            scenario: "party".into(),
            spec: SpecPin {
                id: "party".into(),
                commit: "a".into(),
            },
            env: None,
            requires: vec![],
            requests: vec![],
            steps: vec![fetch, step("create", None, &[("party_id", "$.data.id")])],
        };
        let unbound = scenario.unbound_variables();
        assert_eq!(unbound.len(), 1, "{unbound:?}");
        assert_eq!(unbound[0].name, "party_id");
        assert!(
            unbound[0].captured_later,
            "a path parameter is read like any other field, so order matters for it too"
        );
    }

    #[test]
    fn a_variable_nothing_provides_is_reported_as_missing() {
        let scenario = MorseScenario {
            scenario: "register".into(),
            spec: SpecPin {
                id: "auth".into(),
                commit: "a3f9c2d".into(),
            },
            env: None,
            requires: vec![],
            requests: vec![],
            steps: vec![step(
                "create",
                Some(MorseValue::Str("{{nowhere}}".into())),
                &[],
            )],
        };
        let unbound = scenario.unbound_variables();
        assert_eq!(unbound.len(), 1);
        assert_eq!(unbound[0].name, "nowhere");
        assert!(!unbound[0].captured_later);
    }
}
