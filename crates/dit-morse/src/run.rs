//! Sending the chain, and saying what happened.
//!
//! Every request passes three gates before a byte leaves the machine: the
//! scheme is `http` or `https`, the authority is one this machine allows, and
//! nothing substituted into the request could have changed either. A refusal
//! stops the whole run and names the host, because a scenario that arrived in
//! a pull request pointing somewhere unfamiliar is exactly the case the
//! allowlist exists for.
//!
//! Redirects are never followed. A 3xx is returned as the response it is, so
//! the scenario's own `expect` sees it — following one would mean deciding,
//! mid-flight, that a second host is as trusted as the first.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use dit_model::{Capture, Expect, ExpectRule, MorseValue, Selector};

use crate::jsonpath;
use crate::local::LocalConfig;
use crate::template::{self, Vars};

/// One step, already resolved to a method and a path by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedStep {
    pub id: String,
    pub method: String,
    /// The spec's path, `{name}` segments and all.
    pub path: String,
    /// What fills each `{name}` in `path`.
    pub params: Vec<(String, MorseValue)>,
    pub headers: Vec<(String, MorseValue)>,
    pub query: Vec<(String, MorseValue)>,
    pub body: Option<MorseValue>,
    pub expect: Expect,
    pub capture: Vec<Capture>,
}

/// A whole scenario, ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    pub scenario: String,
    /// Where the API says it lives — from the spec's `servers:`, or the local
    /// override. Never from a committed DIT file.
    pub base_url: String,
    pub vars: Vars,
    pub steps: Vec<PlannedStep>,
}

/// What this machine permits.
#[derive(Debug, Clone)]
pub struct Policy {
    pub allow: LocalConfig,
    pub timeout_secs: u64,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            allow: LocalConfig::default(),
            timeout_secs: 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome {
    pub id: String,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub duration_ms: u64,
    /// How large the response body was. Its size, never its content: the
    /// body stays here, where only the terminal that asked may print it.
    pub bytes: Option<u64>,
    /// The response body, for the terminal that asked (§20.7). It is never
    /// stored and never serialised for a page: the server's wire type has no
    /// field for it, and a test there says so.
    pub body: Option<String>,
    /// Assertions that did not hold, in the order they were written.
    pub failures: Vec<String>,
    /// What this step bound for the steps after it.
    pub captured: Vec<(String, String)>,
    /// The request could not be made or the response could not be read.
    pub error: Option<String>,
}

impl StepOutcome {
    pub fn passed(&self) -> bool {
        self.error.is_none() && self.failures.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub scenario: String,
    pub steps: Vec<StepOutcome>,
    /// Set when the run never started, or stopped, because this machine does
    /// not allow the host. Carries what to do about it.
    pub refused: Option<String>,
}

impl RunOutcome {
    pub fn passed(&self) -> bool {
        self.refused.is_none()
            && !self.steps.is_empty()
            && self.steps.iter().all(StepOutcome::passed)
    }
}

/// Run a plan. Blocking, like the rest of `dit-core` (§16.1), and it returns
/// what happened rather than failing: a scenario that goes red is a result,
/// not an error.
pub fn run(plan: &RunPlan, policy: &Policy) -> RunOutcome {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(policy.timeout_secs)))
        // Never follow a redirect, and do not treat one as an error: the
        // scenario's own `expect` should see the 3xx it got.
        .max_redirects(0)
        .max_redirects_will_error(false)
        // Nor is a 4xx or 5xx an error here. A scenario asserting `status:
        // 404` is asserting something true about the API, and a client that
        // raised on it would make that assertion impossible to write.
        .http_status_as_error(false)
        .build()
        .new_agent();

    let mut vars = plan.vars.clone();
    let mut steps = Vec::new();
    for step in &plan.steps {
        let started = Instant::now();
        let outcome = match send(&agent, plan, step, &vars, policy) {
            Ok(mut sent) => {
                sent.duration_ms = started.elapsed().as_millis() as u64;
                for (name, value) in &sent.captured {
                    vars.insert(name.clone(), value.clone());
                }
                sent
            }
            Err(Refusal::Blocked(message)) => {
                return RunOutcome {
                    scenario: plan.scenario.clone(),
                    steps,
                    refused: Some(message),
                }
            }
            Err(Refusal::Failed(outcome)) => *outcome,
        };
        let stop = !outcome.passed();
        steps.push(outcome);
        // A chain does not continue past a failed step: everything after it
        // would be reading values the failed step never bound.
        if stop {
            break;
        }
    }
    RunOutcome {
        scenario: plan.scenario.clone(),
        steps,
        refused: None,
    }
}

enum Refusal {
    /// The host is not allowed — the whole run stops.
    Blocked(String),
    /// This step could not be made or did not hold; later steps are moot.
    Failed(Box<StepOutcome>),
}

impl From<Box<StepOutcome>> for Refusal {
    fn from(outcome: Box<StepOutcome>) -> Self {
        Refusal::Failed(outcome)
    }
}

fn send(
    agent: &ureq::Agent,
    plan: &RunPlan,
    step: &PlannedStep,
    vars: &Vars,
    policy: &Policy,
) -> Result<StepOutcome, Refusal> {
    let mut outcome = StepOutcome {
        id: step.id.clone(),
        method: step.method.clone(),
        url: String::new(),
        status: None,
        duration_ms: 0,
        bytes: None,
        body: None,
        failures: Vec::new(),
        captured: Vec::new(),
        error: None,
    };

    let url = match build_url(&plan.base_url, step, vars) {
        Ok(url) => url,
        Err(message) => {
            outcome.error = Some(message);
            return Err(Box::new(outcome).into());
        }
    };
    outcome.url = url.clone();

    // The three gates, before anything is sent.
    let (scheme, host, port) = match authority(&url) {
        Some(parts) => parts,
        None => {
            outcome.error = Some(format!("`{url}` is not a URL Morse can send to"));
            return Err(Box::new(outcome).into());
        }
    };
    if scheme != "http" && scheme != "https" {
        outcome.error = Some(format!(
            "`{scheme}` is not a scheme Morse sends over — http and https only"
        ));
        return Err(Box::new(outcome).into());
    }
    if !policy.allow.allows(&host, port) {
        return Err(Refusal::Blocked(format!(
            "{host} is not allowed on this machine, so nothing was sent. \
             If you trust it, run:  dit morse allow {host}"
        )));
    }

    let mut request = ureq::http::Request::builder()
        .method(step.method.as_str())
        .uri(&url);
    for (name, value) in &step.headers {
        let rendered = match render(value, vars) {
            Ok(text) => text,
            Err(message) => {
                outcome.error = Some(message);
                return Err(Box::new(outcome).into());
            }
        };
        request = request.header(name.as_str(), rendered);
    }

    let body = match &step.body {
        None => None,
        Some(value) => match template::to_json(value, vars) {
            Ok(json) => Some(json),
            Err(err) => {
                outcome.error = Some(err.to_string());
                return Err(Box::new(outcome).into());
            }
        },
    };
    if body.is_some()
        && !step
            .headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case("content-type"))
    {
        request = request.header("content-type", "application/json");
    }

    let built = match body {
        Some(json) => request.body(json),
        None => request.body(String::new()),
    };
    let built = match built {
        Ok(built) => built,
        Err(err) => {
            outcome.error = Some(format!("the request could not be built: {err}"));
            return Err(Box::new(outcome).into());
        }
    };

    let response = match agent.run(built) {
        Ok(response) => response,
        Err(err) => {
            outcome.error = Some(format!("{err}"));
            return Err(Box::new(outcome).into());
        }
    };
    let status = response.status().as_u16();
    outcome.status = Some(status);
    let headers: BTreeMap<String, String> = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_ascii_lowercase(),
                v.to_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let text = match response.into_body().read_to_string() {
        Ok(text) => text,
        Err(err) => {
            outcome.error = Some(format!("the response could not be read: {err}"));
            return Err(Box::new(outcome).into());
        }
    };
    outcome.bytes = Some(text.len() as u64);
    let json = dit_parse::parse_json(&text).ok();
    outcome.body = Some(text);

    check(&mut outcome, step, vars, status, json.as_ref());
    capture(&mut outcome, step, status, &headers, json.as_ref());
    if outcome.passed() {
        Ok(outcome)
    } else {
        Err(Box::new(outcome).into())
    }
}

fn check(
    outcome: &mut StepOutcome,
    step: &PlannedStep,
    vars: &Vars,
    status: u16,
    json: Option<&dit_parse::Yaml>,
) {
    let Expect {
        status: want,
        json: checks,
    } = &step.expect;
    if let Some(want) = want {
        if *want != status {
            outcome
                .failures
                .push(format!("expected status {want}, got {status}"));
        }
    }
    for check in checks {
        let Some(body) = json else {
            outcome.failures.push(format!(
                "`{}` could not be checked — the response is not JSON",
                check.path
            ));
            continue;
        };
        let found = jsonpath::select(body, &check.path);
        match &check.rule {
            ExpectRule::Exists => {
                if found.is_none() {
                    outcome
                        .failures
                        .push(format!("`{}` is not in the response", check.path));
                }
            }
            ExpectRule::Equals(expected) => {
                let expected = match template::fill(expected, vars) {
                    Ok(text) => text,
                    Err(err) => {
                        outcome.failures.push(err.to_string());
                        continue;
                    }
                };
                match found.and_then(jsonpath::as_text) {
                    Some(actual) if actual == expected => {}
                    Some(actual) => outcome.failures.push(format!(
                        "`{}` is `{actual}`, expected `{expected}`",
                        check.path
                    )),
                    None => outcome.failures.push(format!(
                        "`{}` is not a single value in the response",
                        check.path
                    )),
                }
            }
        }
    }
}

fn capture(
    outcome: &mut StepOutcome,
    step: &PlannedStep,
    status: u16,
    headers: &BTreeMap<String, String>,
    json: Option<&dit_parse::Yaml>,
) {
    for Capture { name, from } in &step.capture {
        let value = match from {
            Selector::Status => Some(status.to_string()),
            Selector::Header(header) => headers.get(&header.to_ascii_lowercase()).cloned(),
            Selector::JsonPath(path) => json
                .and_then(|body| jsonpath::select(body, path))
                .and_then(jsonpath::as_text),
        };
        match value {
            Some(value) => outcome.captured.push((name.clone(), value)),
            None => outcome.failures.push(format!(
                "`{name}` could not be captured — {} reached nothing in the response",
                describe(from)
            )),
        }
    }
}

fn describe(selector: &Selector) -> String {
    match selector {
        Selector::Status => "the status".to_owned(),
        Selector::Header(name) => format!("header `{name}`"),
        Selector::JsonPath(path) => format!("`{path}`"),
    }
}

fn render(value: &MorseValue, vars: &Vars) -> Result<String, String> {
    match value {
        MorseValue::Str(text) => template::fill(text, vars).map_err(|e| e.to_string()),
        other => template::to_json(other, vars).map_err(|e| e.to_string()),
    }
}

/// `<base><path>?<query>`, with every substituted value percent-encoded.
fn build_url(base: &str, step: &PlannedStep, vars: &Vars) -> Result<String, String> {
    let base = base.trim_end_matches('/');
    let path = template::fill_path_params(&step.path, &step.params, vars)?;
    let path = template::fill_encoded(&path, vars).map_err(|e| e.to_string())?;
    let mut url = format!("{base}{path}");
    if !step.query.is_empty() {
        let mut parts = Vec::new();
        for (name, value) in &step.query {
            let rendered = match value {
                MorseValue::Str(text) => {
                    template::fill_encoded(text, vars).map_err(|e| e.to_string())?
                }
                other => template::percent_encode(&render(other, vars)?),
            };
            parts.push(format!("{}={rendered}", template::percent_encode(name)));
        }
        url.push('?');
        url.push_str(&parts.join("&"));
    }
    Ok(url)
}

/// The scheme, host and port of a URL, without pulling in a parser. Anything
/// this cannot read confidently is refused rather than guessed at.
fn authority(url: &str) -> Option<(String, String, Option<u16>)> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    if authority.is_empty() || authority.contains('@') {
        // Credentials in a URL are refused outright: they would be a secret
        // travelling in a place nothing here checks.
        return None;
    }
    let scheme = scheme.to_ascii_lowercase();
    let default_port = if scheme == "https" { 443 } else { 80 };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => {
            (host, port.parse::<u16>().ok()?)
        }
        _ => (authority, default_port),
    };
    if host.is_empty() {
        return None;
    }
    Some((scheme, host.to_ascii_lowercase(), Some(port)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_urls_authority_is_read_or_refused_never_guessed() {
        assert_eq!(
            authority("https://api.acme.com/users"),
            Some(("https".into(), "api.acme.com".into(), Some(443)))
        );
        assert_eq!(
            authority("http://localhost:3000/a?b=c"),
            Some(("http".into(), "localhost".into(), Some(3000)))
        );
        assert_eq!(
            authority("http://user:pw@acme.com/"),
            None,
            "credentials in a URL are a secret in a place nothing checks"
        );
        assert_eq!(authority("/users"), None);
        assert_eq!(authority("https:///users"), None);
    }

    #[test]
    fn a_captured_value_cannot_move_the_request_to_another_host() {
        let step = PlannedStep {
            id: "s".into(),
            method: "GET".into(),
            path: "/users/{{id}}".into(),
            params: vec![],
            headers: vec![],
            query: vec![("q".into(), MorseValue::Str("{{id}}".into()))],
            body: None,
            expect: Expect::default(),
            capture: vec![],
        };
        let vars: Vars = [("id".to_owned(), "evil.com/x?".to_owned())]
            .into_iter()
            .collect();
        let url = build_url("http://localhost:3000/", &step, &vars).unwrap();
        assert_eq!(
            url,
            "http://localhost:3000/users/evil.com%2Fx%3F?q=evil.com%2Fx%3F"
        );
        assert_eq!(
            authority(&url).unwrap().1,
            "localhost",
            "the host is still the one the spec named"
        );
    }
}
