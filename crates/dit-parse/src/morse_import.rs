//! Importing into Morse (ADR 0027): a `curl` line, a Postman collection, a
//! Postman environment — converted once into what a fence and the local
//! file hold, with a note for everything that could not come along.
//!
//! Three rules shape every conversion:
//!
//! - **No address is kept.** A URL's host must be a registered spec's server
//!   or an environment's (or a `{{variable}}` standing for one); what is kept
//!   is the path, matched to a catalogue operation where one fits and an
//!   inline request where none does.
//! - **No credential is kept.** `Authorization`, cookies, API keys and
//!   fields named like secrets become `{{variables}}`, named in `requires:`.
//! - **No script is kept.** Postman's pre-request and test scripts are what
//!   ADR 0022 refuses; each is listed by request, and a plain status
//!   assertion survives as `expect.status`.
//!
//! Pure, no I/O, wasm-clean: the catalogue comes in as data.

use dit_model::{Expect, InlineRequest, MorseStep, MorseValue, RequestBody, StepTarget};

/// What the importer knows about where requests may go.
#[derive(Debug, Clone, Default)]
pub struct ImportCatalogue {
    /// `(spec id, base URL)` for every spec server and every environment
    /// server, in the order to prefer them.
    pub servers: Vec<(String, String)>,
    /// `(spec id, operationId, method, path template)`.
    pub operations: Vec<(String, String, String, String)>,
    /// The spec a request goes to when its host is a variable
    /// (`{{baseUrl}}/users`) or matches no server.
    pub default_spec: Option<String>,
}

/// One scenario the import would write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedScenario {
    pub name: String,
    pub spec: String,
    pub requires: Vec<String>,
    pub requests: Vec<InlineRequest>,
    pub steps: Vec<MorseStep>,
}

/// What an import would write, and everything it left behind or changed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    pub scenarios: Vec<ImportedScenario>,
    pub notes: Vec<String>,
}

/// An environment from a Postman environment file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedEnv {
    pub name: String,
    pub server: Option<String>,
    pub vars: Vec<(String, String)>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    #[error("this is not a curl command — it should start with `curl`")]
    NotCurl,
    #[error("the curl command names no URL")]
    NoUrl,
    #[error("{0}")]
    Json(String),
    #[error("this is not a Postman collection (v2.1): {0}")]
    NotCollection(String),
    #[error("this is not a Postman environment: {0}")]
    NotEnvironment(String),
    #[error("`{host}` is not a server any registered spec or environment names — register its spec, give an environment that server, or choose a spec to import into")]
    UnknownHost { host: String },
    #[error("`{0}` stands for the host — choose which spec's server it is")]
    HostVariable(String),
    #[error("nothing to import: the collection holds no requests")]
    Empty,
}

pub fn import_curl(
    command: &str,
    cat: &ImportCatalogue,
    scenario: &str,
) -> Result<ImportReport, ImportError> {
    let words = shell_words(command);
    if words.first().map(String::as_str) != Some("curl") {
        return Err(ImportError::NotCurl);
    }
    let mut req = RawRequest {
        name: scenario.to_owned(),
        method: None,
        url: None,
        headers: Vec::new(),
        body: RawBody::None,
        status: None,
    };
    let mut data: Vec<String> = Vec::new();
    let mut binary: Option<String> = None;
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut json = false;
    let mut get = false;
    let mut notes = Vec::new();
    let mut it = words.iter().skip(1).peekable();
    while let Some(word) = it.next() {
        let mut value = || it.next().cloned().unwrap_or_default();
        match word.as_str() {
            "-X" | "--request" => req.method = Some(value().to_ascii_uppercase()),
            "-H" | "--header" => {
                let raw = value();
                if let Some((k, v)) = raw.split_once(':') {
                    req.headers.push((k.trim().to_owned(), v.trim().to_owned()));
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-ascii" => data.push(value()),
            "--data-urlencode" => {
                let raw = value();
                data.push(match raw.split_once('=') {
                    Some((k, v)) => format!("{k}={}", form_encode(v)),
                    None => form_encode(&raw),
                });
            }
            "--data-binary" => binary = Some(value()),
            "--json" => {
                json = true;
                data.push(value());
            }
            "-F" | "--form" | "--form-string" => {
                let raw = value();
                if let Some((k, v)) = raw.split_once('=') {
                    parts.push((k.to_owned(), v.to_owned()));
                }
            }
            "-u" | "--user" => {
                let _ = value();
                req.headers
                    .push(("Authorization".into(), "Basic {{basic_auth}}".into()));
                notes.push("`-u` became `Authorization: Basic {{basic_auth}}` — the credentials were not kept".into());
            }
            "-b" | "--cookie" => {
                let _ = value();
                req.headers.push(("Cookie".into(), "{{cookie}}".into()));
            }
            "-A" | "--user-agent" => req.headers.push(("User-Agent".into(), value())),
            "-e" | "--referer" => req.headers.push(("Referer".into(), value())),
            "--url" => req.url = Some(value()),
            "-G" | "--get" => get = true,
            "-L" | "--location" => {
                notes.push("`-L` was dropped: Morse never follows a redirect".into())
            }
            "-o" | "--output" | "-w" | "--write-out" | "-m" | "--max-time"
            | "--connect-timeout" | "-x" | "--proxy" | "--cacert" | "--cert" | "--key" => {
                let _ = value();
            }
            other if other.starts_with('-') => {}
            other => {
                if req.url.is_none() {
                    req.url = Some(other.to_owned());
                }
            }
        }
    }
    let Some(url) = req.url.clone() else {
        return Err(ImportError::NoUrl);
    };
    if get && !data.is_empty() {
        let joiner = if url.contains('?') { '&' } else { '?' };
        req.url = Some(format!("{url}{joiner}{}", data.join("&")));
        data.clear();
    }
    let content_type = header(&req.headers, "content-type").map(|t| t.to_ascii_lowercase());
    req.body = if !parts.is_empty() {
        RawBody::Multipart(
            parts
                .into_iter()
                .map(|(name, value)| match value.strip_prefix('@') {
                    Some(file) => {
                        let (path, kind) = match file.split_once(";type=") {
                            Some((p, t)) => (p.to_owned(), Some(t.to_owned())),
                            None => (file.to_owned(), None),
                        };
                        (name, RawPart::File { path, kind })
                    }
                    None => (name, RawPart::Text(value)),
                })
                .collect(),
        )
    } else if let Some(bin) = binary {
        let kind = content_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".into());
        match bin.strip_prefix('@') {
            Some(path) => RawBody::File {
                kind,
                path: path.to_owned(),
            },
            None => RawBody::Text { kind, text: bin },
        }
    } else if !data.is_empty() {
        let text = data.join("&");
        let is_json = json || content_type.as_deref().is_some_and(|t| t.contains("json"));
        if is_json {
            RawBody::Json(text)
        } else if content_type
            .as_deref()
            .is_none_or(|t| t.contains("x-www-form-urlencoded"))
        {
            RawBody::Form(
                text.split('&')
                    .filter(|p| !p.is_empty())
                    .map(|p| match p.split_once('=') {
                        Some((k, v)) => (form_decode(k), form_decode(v)),
                        None => (form_decode(p), String::new()),
                    })
                    .collect(),
            )
        } else {
            RawBody::Text {
                kind: content_type.clone().unwrap_or_else(|| "text/plain".into()),
                text,
            }
        }
    } else {
        RawBody::None
    };
    if req.method.is_none() {
        req.method = Some(if matches!(req.body, RawBody::None) {
            "GET".into()
        } else {
            "POST".into()
        });
    }
    let mut out = Converter::new(cat);
    out.notes.extend(notes);
    let step_id = slug(scenario, "request");
    out.add(&slug(scenario, "imported"), &step_id, req)?;
    Ok(out.finish())
}

pub fn import_postman(json: &str, cat: &ImportCatalogue) -> Result<ImportReport, ImportError> {
    let root = crate::json::parse(json).map_err(|e| ImportError::Json(e.to_string()))?;
    let items = root
        .get("item")
        .and_then(Yaml::as_seq)
        .ok_or_else(|| ImportError::NotCollection("it has no `item` list".into()))?;
    let collection = root
        .get("info")
        .and_then(|i| i.get("name"))
        .and_then(Yaml::as_str)
        .unwrap_or("imported")
        .to_owned();
    let mut out = Converter::new(cat);
    if let Some(vars) = root.get("variable").and_then(Yaml::as_seq) {
        let names: Vec<&str> = vars
            .iter()
            .filter_map(|v| v.get("key").and_then(Yaml::as_str))
            .collect();
        if !names.is_empty() {
            out.notes.push(format!(
                "the collection's own variables ({}) were not imported as values — import the environment, or set them in one",
                names.join(", ")
            ));
        }
    }
    // Folders first, each its own scenario; loose requests under the
    // collection's name.
    let mut loose = Vec::new();
    for item in items {
        if let Some(children) = item.get("item").and_then(Yaml::as_seq) {
            let folder = item.get("name").and_then(Yaml::as_str).unwrap_or("folder");
            let scenario = slug(folder, "folder");
            walk_folder(&mut out, &scenario, folder, children)?;
        } else {
            loose.push(item);
        }
    }
    let scenario = slug(&collection, "imported");
    for item in loose {
        add_postman_request(&mut out, &scenario, item)?;
    }
    let report = out.finish();
    if report.scenarios.is_empty() {
        return Err(ImportError::Empty);
    }
    Ok(report)
}

/// A folder's requests, nested folders flattened into it in order.
fn walk_folder(
    out: &mut Converter,
    scenario: &str,
    path: &str,
    items: &[Yaml],
) -> Result<(), ImportError> {
    for item in items {
        match item.get("item").and_then(Yaml::as_seq) {
            Some(children) => {
                let name = item.get("name").and_then(Yaml::as_str).unwrap_or("folder");
                walk_folder(out, scenario, &format!("{path} / {name}"), children)?;
            }
            None => add_postman_request(out, scenario, item)?,
        }
    }
    Ok(())
}

fn add_postman_request(
    out: &mut Converter,
    scenario: &str,
    item: &Yaml,
) -> Result<(), ImportError> {
    let name = item
        .get("name")
        .and_then(Yaml::as_str)
        .unwrap_or("request")
        .to_owned();
    let Some(request) = item.get("request") else {
        return Ok(());
    };
    let (url, path_vars) = match request.get("url") {
        Some(Yaml::Str(raw)) => (raw.clone(), Vec::new()),
        Some(node) => (
            node.get("raw")
                .and_then(Yaml::as_str)
                .unwrap_or_default()
                .to_owned(),
            node.get("variable")
                .and_then(Yaml::as_seq)
                .map(|vars| {
                    vars.iter()
                        .filter_map(|v| {
                            let key = v.get("key").and_then(Yaml::as_str)?;
                            let value = v.get("value").and_then(Yaml::as_str).unwrap_or_default();
                            Some((key.to_owned(), value.to_owned()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        None => (String::new(), Vec::new()),
    };
    // Postman writes path parameters as `:name`; a fence writes `{name}`.
    let mut url = url;
    let mut params = Vec::new();
    for (key, value) in &path_vars {
        url = url.replace(&format!(":{key}"), &format!("{{{key}}}"));
        params.push((
            key.clone(),
            if value.is_empty() {
                format!("{{{{{key}}}}}")
            } else {
                value.clone()
            },
        ));
    }
    let headers: Vec<(String, String)> = request
        .get("header")
        .and_then(Yaml::as_seq)
        .map(|hs| {
            hs.iter()
                .filter(|h| h.get("disabled").and_then(Yaml::as_bool) != Some(true))
                .filter_map(|h| {
                    Some((
                        h.get("key").and_then(Yaml::as_str)?.to_owned(),
                        h.get("value")
                            .and_then(Yaml::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut headers = headers;
    if let Some(auth) = request.get("auth") {
        match auth.get("type").and_then(Yaml::as_str) {
            Some("bearer") => headers.push(("Authorization".into(), "Bearer {{token}}".into())),
            Some("basic") => headers.push(("Authorization".into(), "Basic {{basic_auth}}".into())),
            Some("apikey") => headers.push(("X-API-Key".into(), "{{api_key}}".into())),
            Some("noauth") | None => {}
            Some(other) => out.notes.push(format!(
                "`{name}`: `{other}` auth was dropped — add the header it sends by hand"
            )),
        }
    }
    let content_type = header(&headers, "content-type").map(|t| t.to_ascii_lowercase());
    let body = match request.get("body") {
        None => RawBody::None,
        Some(body) => match body.get("mode").and_then(Yaml::as_str) {
            Some("raw") => {
                let text = body
                    .get("raw")
                    .and_then(Yaml::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let language = body
                    .get("options")
                    .and_then(|o| o.get("raw"))
                    .and_then(|r| r.get("language"))
                    .and_then(Yaml::as_str)
                    .unwrap_or("text");
                let kind = content_type.clone().unwrap_or_else(|| match language {
                    "json" => "application/json".into(),
                    "xml" => "application/xml".into(),
                    "html" => "text/html".into(),
                    "javascript" => "application/javascript".into(),
                    _ => "text/plain".into(),
                });
                if text.trim().is_empty() {
                    RawBody::None
                } else if kind.contains("json") {
                    RawBody::Json(text)
                } else {
                    RawBody::Text { kind, text }
                }
            }
            Some("urlencoded") => RawBody::Form(pairs(body.get("urlencoded"))),
            Some("formdata") => RawBody::Multipart(
                body.get("formdata")
                    .and_then(Yaml::as_seq)
                    .map(|items| {
                        items
                            .iter()
                            .filter(|p| p.get("disabled").and_then(Yaml::as_bool) != Some(true))
                            .filter_map(|p| {
                                let key = p.get("key").and_then(Yaml::as_str)?.to_owned();
                                if p.get("type").and_then(Yaml::as_str) == Some("file") {
                                    let path = p
                                        .get("src")
                                        .and_then(Yaml::as_str)
                                        .unwrap_or_default()
                                        .to_owned();
                                    Some((
                                        key,
                                        RawPart::File {
                                            path,
                                            kind: p
                                                .get("contentType")
                                                .and_then(Yaml::as_str)
                                                .map(str::to_owned),
                                        },
                                    ))
                                } else {
                                    Some((
                                        key,
                                        RawPart::Text(
                                            p.get("value")
                                                .and_then(Yaml::as_str)
                                                .unwrap_or_default()
                                                .to_owned(),
                                        ),
                                    ))
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            Some("file") => RawBody::File {
                kind: content_type
                    .clone()
                    .unwrap_or_else(|| "application/octet-stream".into()),
                path: body
                    .get("file")
                    .and_then(|f| f.get("src"))
                    .and_then(Yaml::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            },
            Some("graphql") => {
                let query = body
                    .get("graphql")
                    .and_then(|g| g.get("query"))
                    .and_then(Yaml::as_str)
                    .unwrap_or_default();
                RawBody::Json(format!("{{\"query\":{}}}", json_string(query)))
            }
            _ => RawBody::None,
        },
    };
    let mut status = None;
    if let Some(events) = item.get("event").and_then(Yaml::as_seq) {
        for event in events {
            let listen = event
                .get("listen")
                .and_then(Yaml::as_str)
                .unwrap_or("script");
            let lines: Vec<&str> = event
                .get("script")
                .and_then(|s| s.get("exec"))
                .and_then(Yaml::as_seq)
                .map(|ls| ls.iter().filter_map(Yaml::as_str).collect())
                .unwrap_or_default();
            if lines.iter().all(|l| l.trim().is_empty()) {
                continue;
            }
            if listen == "test" {
                status = status.or_else(|| lines.iter().find_map(|l| status_in(l)));
            }
            out.notes.push(format!(
                "`{name}`: its {} script was dropped — Morse runs no scripts{}",
                if listen == "prerequest" {
                    "pre-request"
                } else {
                    "test"
                },
                if status.is_some() && listen == "test" {
                    " (its status check was kept)"
                } else {
                    ""
                }
            ));
        }
    }
    let req = RawRequest {
        name: name.clone(),
        method: request
            .get("method")
            .and_then(Yaml::as_str)
            .map(|m| m.to_ascii_uppercase()),
        url: Some(url),
        headers,
        body,
        status,
    };
    let step_id = slug(&name, "request");
    out.add_with_params(scenario, &step_id, req, params)
}

pub fn import_postman_env(json: &str) -> Result<ImportedEnv, ImportError> {
    let root = crate::json::parse(json).map_err(|e| ImportError::Json(e.to_string()))?;
    let values = root
        .get("values")
        .and_then(Yaml::as_seq)
        .ok_or_else(|| ImportError::NotEnvironment("it has no `values` list".into()))?;
    let name = slug(
        root.get("name")
            .and_then(Yaml::as_str)
            .unwrap_or("imported"),
        "imported",
    );
    let mut env = ImportedEnv {
        name,
        server: None,
        vars: Vec::new(),
        notes: Vec::new(),
    };
    for value in values {
        let Some(key) = value.get("key").and_then(Yaml::as_str) else {
            continue;
        };
        let text = value
            .get("value")
            .and_then(Yaml::as_str)
            .unwrap_or_default();
        if value.get("enabled").and_then(Yaml::as_bool) == Some(false) {
            env.notes
                .push(format!("`{key}` is disabled in Postman and was skipped"));
            continue;
        }
        let looks_like_host = matches!(
            key.to_ascii_lowercase().as_str(),
            "baseurl" | "base_url" | "url" | "host" | "server" | "apiurl" | "api_url"
        ) && (text.starts_with("http://") || text.starts_with("https://"));
        if looks_like_host && env.server.is_none() {
            env.server = Some(text.trim_end_matches('/').to_owned());
        } else if text.contains(['\n', '\r']) {
            env.notes
                .push(format!("`{key}` holds a line break and was skipped"));
        } else {
            env.vars.push((key.to_owned(), text.to_owned()));
        }
    }
    Ok(env)
}

// ---- one request, converted ------------------------------------------------

use crate::yaml::Yaml;

struct RawRequest {
    name: String,
    method: Option<String>,
    url: Option<String>,
    headers: Vec<(String, String)>,
    body: RawBody,
    status: Option<u16>,
}

enum RawBody {
    None,
    Json(String),
    Form(Vec<(String, String)>),
    Text { kind: String, text: String },
    File { kind: String, path: String },
    Multipart(Vec<(String, RawPart)>),
}

enum RawPart {
    Text(String),
    File { path: String, kind: Option<String> },
}

/// Header names whose values are credentials, and the variable each becomes.
const CREDENTIAL_HEADERS: &[(&str, &str)] = &[
    ("cookie", "cookie"),
    ("x-api-key", "api_key"),
    ("api-key", "api_key"),
    ("apikey", "api_key"),
    ("x-auth-token", "auth_token"),
];

/// Field names whose literal values are credentials.
const SECRET_FIELDS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "access_token",
    "refresh_token",
    "client_secret",
    "api_key",
    "apikey",
    "private_key",
];

/// Where a URL goes: the spec, and its path and query below that spec's base.
struct Resolved {
    spec: String,
    path: String,
    query: Vec<(String, MorseValue)>,
}

struct Converter<'a> {
    cat: &'a ImportCatalogue,
    scenarios: Vec<ImportedScenario>,
    notes: Vec<String>,
}

impl<'a> Converter<'a> {
    fn new(cat: &'a ImportCatalogue) -> Self {
        Converter {
            cat,
            scenarios: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn add(&mut self, scenario: &str, step_id: &str, req: RawRequest) -> Result<(), ImportError> {
        self.add_with_params(scenario, step_id, req, Vec::new())
    }

    fn add_with_params(
        &mut self,
        scenario: &str,
        step_id: &str,
        req: RawRequest,
        named: Vec<(String, String)>,
    ) -> Result<(), ImportError> {
        let url = req.url.clone().unwrap_or_default();
        let Resolved { spec, path, query } = self.resolve(&url, &req.name)?;
        let method = req.method.clone().unwrap_or_else(|| "GET".into());
        let mut requires: Vec<String> = Vec::new();
        let secret = |name: &str, requires: &mut Vec<String>| -> MorseValue {
            if !requires.iter().any(|r| r == name) {
                requires.push(name.to_owned());
            }
            MorseValue::Str(format!("{{{{{name}}}}}"))
        };

        let mut headers = Vec::new();
        for (key, value) in &req.headers {
            let lower = key.to_ascii_lowercase();
            if lower == "content-type" && !matches!(req.body, RawBody::None | RawBody::Json(_)) {
                // The body shape sets its own type.
                continue;
            }
            if lower == "content-length" || lower == "host" {
                continue;
            }
            let converted = if lower == "authorization" && !is_only_variables(value) {
                let (scheme, var) = match value.split_once(' ') {
                    Some((scheme, _)) if scheme.eq_ignore_ascii_case("bearer") => {
                        ("Bearer ", "token")
                    }
                    Some((scheme, _)) if scheme.eq_ignore_ascii_case("basic") => {
                        ("Basic ", "basic_auth")
                    }
                    _ => ("", "authorization"),
                };
                if !requires.iter().any(|r| r == var) {
                    requires.push(var.to_owned());
                }
                self.notes.push(format!(
                    "`{}`: the `{key}` value became `{scheme}{{{{{var}}}}}`",
                    req.name
                ));
                MorseValue::Str(format!("{scheme}{{{{{var}}}}}"))
            } else if let Some((_, var)) = CREDENTIAL_HEADERS.iter().find(|(h, _)| *h == lower) {
                if !is_only_variables(value) {
                    self.notes.push(format!(
                        "`{}`: the `{key}` value became `{{{{{var}}}}}`",
                        req.name
                    ));
                }
                secret(var, &mut requires)
            } else {
                MorseValue::Str(value.clone())
            };
            headers.push((key.clone(), converted));
        }

        let mut notes = Vec::new();
        let who = req.name.clone();
        let body = match req.body {
            RawBody::None => None,
            RawBody::Json(text) => match crate::json::parse(&quote_bare_variables(&text)) {
                Ok(tree) => Some(RequestBody::Json(scrub_json(&tree, &mut |k, v| {
                    scrub_field(&who, k, v, &mut requires, &mut notes)
                }))),
                Err(_) => Some(RequestBody::Raw { media_type: "application/json".into(), content: dit_model::RawContent::Text(text) }),
            },
            RawBody::Form(fields) => Some(RequestBody::Form(
                fields
                    .into_iter()
                    .map(|(k, v)| {
                        let v = scrub_field(&who, &k, v, &mut requires, &mut notes);
                        (k, v)
                    })
                    .collect(),
            )),
            RawBody::Text { kind, text } => Some(RequestBody::Raw { media_type: kind, content: dit_model::RawContent::Text(text) }),
            RawBody::File { kind, path } => {
                notes.push(format!("`{}`: `{path}` is sent from the repository's last commit — commit it if it is not", req.name));
                Some(RequestBody::Raw { media_type: kind, content: dit_model::RawContent::File(path) })
            }
            RawBody::Multipart(parts) => Some(RequestBody::Multipart(
                parts
                    .into_iter()
                    .map(|(name, part)| match part {
                        RawPart::Text(value) => dit_model::MultipartPart {
                            content: dit_model::PartContent::Value(scrub_field(&who, &name, value, &mut requires, &mut notes)),
                            name,
                            media_type: None,
                        },
                        RawPart::File { path, kind } => {
                            notes.push(format!("`{}`: `{path}` is sent from the repository's last commit — commit it if it is not", req.name));
                            dit_model::MultipartPart { name, content: dit_model::PartContent::File(path), media_type: kind }
                        }
                    })
                    .collect(),
            )),
        };
        self.notes.extend(notes);

        // The operation the path is, or an inline request when it is none.
        let mut params: Vec<(String, MorseValue)> = Vec::new();
        let operation = self
            .cat
            .operations
            .iter()
            .filter(|(s, _, m, _)| *s == spec && m.eq_ignore_ascii_case(&method))
            .find_map(|(s, op, _, template)| {
                match_template(template, &path).map(|found| (s, op, found))
            });
        let mut requests = Vec::new();
        let target = match operation {
            Some((s, op, found)) => {
                for (name, value) in found {
                    let value = named
                        .iter()
                        .find(|(k, _)| *k == name)
                        .map(|(_, v)| v.clone())
                        .unwrap_or(value);
                    params.push((name, MorseValue::Str(value)));
                }
                StepTarget::Operation(dit_model::OperationRef {
                    spec: s.clone(),
                    operation: op.clone(),
                })
            }
            None => {
                for (name, value) in named {
                    params.push((name, MorseValue::Str(value)));
                }
                requests.push(InlineRequest {
                    id: step_id.to_owned(),
                    method: method.clone(),
                    path: path.clone(),
                    summary: Some(req.name.clone()).filter(|n| n != step_id),
                });
                StepTarget::Inline(step_id.to_owned())
            }
        };
        for (_, value) in params.iter().chain(query.iter()).chain(headers.iter()) {
            for var in value.variables() {
                if !requires.contains(&var) {
                    requires.push(var);
                }
            }
        }
        if let Some(body) = &body {
            for var in body.scannable().variables() {
                if !requires.contains(&var) {
                    requires.push(var);
                }
            }
        }
        let step = MorseStep {
            id: step_id.to_owned(),
            operation: target,
            params,
            headers,
            query,
            body,
            expect: Expect {
                status: req.status,
                json: Vec::new(),
            },
            capture: Vec::new(),
        };

        let entry = match self.scenarios.iter_mut().find(|s| s.name == scenario) {
            Some(existing) => {
                if existing.spec != spec {
                    self.notes.push(format!(
                        "`{}` calls the `{spec}` spec while `{scenario}` is pinned to `{}`",
                        req.name, existing.spec
                    ));
                }
                existing
            }
            None => {
                self.scenarios.push(ImportedScenario {
                    name: scenario.to_owned(),
                    spec: spec.clone(),
                    requires: Vec::new(),
                    requests: Vec::new(),
                    steps: Vec::new(),
                });
                self.scenarios.last_mut().ok_or(ImportError::Empty)?
            }
        };
        let mut step = step;
        let mut n = 2;
        let base = step.id.clone();
        while entry.steps.iter().any(|s| s.id == step.id) {
            step.id = format!("{base}-{n}");
            n += 1;
        }
        if let StepTarget::Inline(_) = step.operation {
            for mut r in requests {
                r.id = step.id.clone();
                entry.requests.push(r);
            }
            step.operation = StepTarget::Inline(step.id.clone());
        }
        for name in requires {
            if !entry.requires.contains(&name) {
                entry.requires.push(name);
            }
        }
        entry.steps.push(step);
        Ok(())
    }

    /// The spec a URL goes to, and its path and query below that spec's
    /// base. A host nobody names is refused unless a spec was chosen.
    fn resolve(&mut self, url: &str, name: &str) -> Result<Resolved, ImportError> {
        let (address, query) = match url.split_once('?') {
            Some((a, q)) => (a, q),
            None => (url, ""),
        };
        let query: Vec<(String, MorseValue)> = query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|p| match p.split_once('=') {
                Some((k, v)) => (form_decode(k), MorseValue::Str(form_decode(v))),
                None => (form_decode(p), MorseValue::Str(String::new())),
            })
            .collect();
        // `{{baseUrl}}/users` — the host is a variable.
        if let Some(rest) = address.strip_prefix("{{") {
            let (var, after) = rest.split_once("}}").unwrap_or((rest, ""));
            let spec = self
                .cat
                .default_spec
                .clone()
                .ok_or_else(|| ImportError::HostVariable(format!("{{{{{var}}}}}")))?;
            return Ok(Resolved {
                spec,
                path: normal_path(after),
                query,
            });
        }
        let lower = address.to_ascii_lowercase();
        let found = self.cat.servers.iter().find(|(_, base)| {
            let base = base.trim_end_matches('/').to_ascii_lowercase();
            lower == base || lower.starts_with(&format!("{base}/"))
        });
        if let Some((spec, base)) = found {
            let rest = &address[base.trim_end_matches('/').len()..];
            return Ok(Resolved {
                spec: spec.clone(),
                path: normal_path(rest),
                query,
            });
        }
        let (host, path) = split_address(address);
        match &self.cat.default_spec {
            Some(spec) => {
                self.notes.push(format!(
                    "`{name}`: `{host}` is no server a spec or an environment names — it will go to `{spec}`'s server instead"
                ));
                Ok(Resolved {
                    spec: spec.clone(),
                    path: normal_path(path),
                    query,
                })
            }
            None => Err(ImportError::UnknownHost {
                host: host.to_owned(),
            }),
        }
    }

    fn finish(self) -> ImportReport {
        ImportReport {
            scenarios: self.scenarios,
            notes: self.notes,
        }
    }
}

/// A field's value, or — when the field is named like a secret and holds a
/// literal — a variable of that name, required and reported.
fn scrub_field(
    who: &str,
    field: &str,
    value: String,
    requires: &mut Vec<String>,
    notes: &mut Vec<String>,
) -> MorseValue {
    let lower = field.to_ascii_lowercase();
    if SECRET_FIELDS.contains(&lower.as_str()) && !value.is_empty() && !is_only_variables(&value) {
        notes.push(format!(
            "`{who}`: the literal `{field}` became `{{{{{lower}}}}}`"
        ));
        if !requires.contains(&lower) {
            requires.push(lower.clone());
        }
        return MorseValue::Str(format!("{{{{{lower}}}}}"));
    }
    MorseValue::Str(value)
}

/// A JSON tree as a fence value, with literal secrets swapped for variables.
fn scrub_json(tree: &Yaml, scrub: &mut dyn FnMut(&str, String) -> MorseValue) -> MorseValue {
    match tree {
        Yaml::Null => MorseValue::Str(String::new()),
        Yaml::Str(s) => MorseValue::Str(s.clone()),
        Yaml::Seq(items) => MorseValue::Seq(items.iter().map(|i| scrub_json(i, scrub)).collect()),
        Yaml::Map(entries) => MorseValue::Map(
            entries
                .iter()
                .map(|(k, v)| match v {
                    Yaml::Str(text) => (k.clone(), scrub(k, text.clone())),
                    other => (k.clone(), scrub_json(other, scrub)),
                })
                .collect(),
        ),
    }
}

/// `{"id": {{id}}}` is how people write a variable in a JSON body, and it is
/// not JSON. Quoted, it is — and a value written as one reference takes the
/// shape of what it holds when the step runs.
fn quote_bare_variables(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if !in_string && rest.starts_with("{{") {
            if let Some(end) = rest.find("}}") {
                out.push('"');
                out.push_str(&rest[..end + 2]);
                out.push('"');
                rest = &rest[end + 2..];
                continue;
            }
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// `{x}` segments match any one segment; their values come back by name.
fn match_template(template: &str, path: &str) -> Option<Vec<(String, String)>> {
    let t: Vec<&str> = template.trim_matches('/').split('/').collect();
    let p: Vec<&str> = path.trim_matches('/').split('/').collect();
    if t.len() != p.len() {
        return None;
    }
    let mut found = Vec::new();
    for (ts, ps) in t.iter().zip(p.iter()) {
        if let Some(name) = ts.strip_prefix('{').and_then(|n| n.strip_suffix('}')) {
            if ps.is_empty() {
                return None;
            }
            // A path already written with `{name}` passes its variable on.
            let value = match ps.strip_prefix('{').and_then(|n| n.strip_suffix('}')) {
                Some(inner) if !ps.starts_with("{{") => format!("{{{{{inner}}}}}"),
                _ => (*ps).to_owned(),
            };
            found.push((name.to_owned(), value));
        } else if !ts.eq_ignore_ascii_case(ps) {
            return None;
        }
    }
    Some(found)
}

fn split_address(address: &str) -> (&str, &str) {
    let after_scheme = address.split_once("://").map_or(address, |(_, rest)| rest);
    match after_scheme.find('/') {
        Some(i) => (&after_scheme[..i], &after_scheme[i..]),
        None => (after_scheme, "/"),
    }
}

fn normal_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        "/".into()
    } else if trimmed.starts_with('/') {
        trimmed.to_owned()
    } else {
        format!("/{trimmed}")
    }
}

fn header<'h>(headers: &'h [(String, String)], name: &str) -> Option<&'h str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn pairs(node: Option<&Yaml>) -> Vec<(String, String)> {
    node.and_then(Yaml::as_seq)
        .map(|items| {
            items
                .iter()
                .filter(|p| p.get("disabled").and_then(Yaml::as_bool) != Some(true))
                .filter_map(|p| {
                    Some((
                        p.get("key").and_then(Yaml::as_str)?.to_owned(),
                        p.get("value")
                            .and_then(Yaml::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `pm.response.to.have.status(201)` or `pm.expect(pm.response.code).to.eql(201)`.
fn status_in(line: &str) -> Option<u16> {
    let at = line
        .find("to.have.status(")
        .map(|i| i + "to.have.status(".len())
        .or_else(|| {
            line.contains("pm.response.code")
                .then(|| line.rfind('(').map(|i| i + 1))
                .flatten()
        })?;
    let digits: String = line[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok().filter(|s| (100..=599).contains(s))
}

fn is_only_variables(text: &str) -> bool {
    let residue = dit_model::variables_in(text)
        .iter()
        .fold(text.to_owned(), |acc, v| {
            acc.replace(&format!("{{{{{v}}}}}"), "")
        });
    !text.trim().is_empty() && residue.trim().is_empty()
}

/// A scenario or step name from words: lowercase, dashes, starting with a letter.
fn slug(words: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in words.chars() {
        if c.is_ascii_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(c.to_ascii_lowercase());
        } else {
            dash = true;
        }
        if out.len() >= 48 {
            break;
        }
    }
    if out.is_empty() {
        return fallback.to_owned();
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert_str(0, "s-");
    }
    out
}

/// Split a command line the way a POSIX shell would for quotes and
/// backslashes — enough for a `curl` copied from a browser or a README.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('\n') => {}
                Some(next) => {
                    cur.push(next);
                    has = true;
                }
                None => {}
            },
            '\'' => {
                has = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    cur.push(q);
                }
            }
            '"' => {
                has = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' => {
                            if let Some(&next) = chars.peek() {
                                if matches!(next, '"' | '\\' | '$' | '`') {
                                    cur.push(next);
                                    chars.next();
                                    continue;
                                }
                            }
                            cur.push('\\');
                        }
                        other => cur.push(other),
                    }
                }
            }
            c if c.is_whitespace() => {
                if has {
                    words.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            other => {
                cur.push(other);
                has = true;
            }
        }
    }
    if has {
        words.push(cur);
    }
    // `$'…'` and similar are read as plain text; a leading `$` prompt goes.
    if words.first().map(String::as_str) == Some("$") {
        words.remove(0);
    }
    words
}

fn form_decode(text: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi * 16 + lo);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn form_encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'*' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use dit_model::{PartContent, RawContent};

    fn catalogue() -> ImportCatalogue {
        ImportCatalogue {
            servers: vec![
                ("auth".into(), "https://api.acme.test/v1".into()),
                ("auth".into(), "http://localhost:3000/v1".into()),
                ("files".into(), "https://files.acme.test".into()),
            ],
            operations: vec![
                (
                    "auth".into(),
                    "loginUser".into(),
                    "POST".into(),
                    "/sessions".into(),
                ),
                (
                    "auth".into(),
                    "getUser".into(),
                    "GET".into(),
                    "/users/{id}".into(),
                ),
                (
                    "files".into(),
                    "upload".into(),
                    "POST".into(),
                    "/uploads".into(),
                ),
            ],
            default_spec: None,
        }
    }

    fn only_step(report: &ImportReport) -> &MorseStep {
        assert_eq!(report.scenarios.len(), 1, "{report:?}");
        assert_eq!(report.scenarios[0].steps.len(), 1, "{report:?}");
        &report.scenarios[0].steps[0]
    }

    // ---- curl ----------------------------------------------------------------

    #[test]
    fn a_curl_line_becomes_the_operation_it_calls_with_its_path_parameter() {
        let r = import_curl(
            "curl https://api.acme.test/v1/users/42?include=roles",
            &catalogue(),
            "user",
        )
        .unwrap();
        let step = only_step(&r);
        assert_eq!(r.scenarios[0].spec, "auth");
        assert_eq!(r.scenarios[0].name, "user");
        assert_eq!(
            step.operation,
            StepTarget::Operation(dit_model::OperationRef {
                spec: "auth".into(),
                operation: "getUser".into()
            })
        );
        assert_eq!(
            step.params,
            [("id".to_owned(), MorseValue::Str("42".into()))]
        );
        assert_eq!(
            step.query,
            [("include".to_owned(), MorseValue::Str("roles".into()))]
        );
    }

    #[test]
    fn a_curl_credential_becomes_a_variable_and_is_never_kept() {
        let r = import_curl(
            "curl -X POST 'https://api.acme.test/v1/sessions' \\\n  -H 'Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.secret' \\\n  -H 'Content-Type: application/json' \\\n  --cookie 'sid=abc123' \\\n  -d '{\"email\":\"a@b.test\",\"password\":\"hunter2\"}'",
            &catalogue(),
            "login",
        )
        .unwrap();
        let step = only_step(&r);
        let header = |name: &str| {
            step.headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            header("authorization"),
            Some(MorseValue::Str("Bearer {{token}}".into()))
        );
        assert_eq!(header("cookie"), Some(MorseValue::Str("{{cookie}}".into())));
        match &step.body {
            Some(RequestBody::Json(MorseValue::Map(fields))) => {
                assert!(fields.contains(&("email".into(), MorseValue::Str("a@b.test".into()))));
                assert!(
                    fields.contains(&("password".into(), MorseValue::Str("{{password}}".into()))),
                    "{fields:?}"
                );
            }
            other => panic!("{other:?}"),
        }
        let text = format!("{r:?}");
        for secret in ["eyJhbGciOiJIUzI1NiJ9.secret", "abc123", "hunter2"] {
            assert!(!text.contains(secret), "{secret} survived: {text}");
        }
        assert_eq!(r.scenarios[0].requires, ["token", "cookie", "password"]);
        assert!(!r.notes.is_empty(), "the replacements are reported");
    }

    #[test]
    fn a_curl_form_and_upload_become_form_and_multipart_bodies() {
        let r = import_curl(
            "curl https://api.acme.test/v1/sessions -d grant_type=password -d 'user=a%40b.test'",
            &catalogue(),
            "f",
        )
        .unwrap();
        assert_eq!(
            only_step(&r).body,
            Some(RequestBody::Form(vec![
                ("grant_type".into(), MorseValue::Str("password".into())),
                ("user".into(), MorseValue::Str("a@b.test".into())),
            ]))
        );
        let r = import_curl("curl https://files.acme.test/uploads -F title=Hi -F 'file=@fixtures/a.png;type=image/png'", &catalogue(), "u").unwrap();
        match &only_step(&r).body {
            Some(RequestBody::Multipart(parts)) => {
                assert_eq!(parts[0].name, "title");
                assert_eq!(parts[1].content, PartContent::File("fixtures/a.png".into()));
                assert_eq!(parts[1].media_type.as_deref(), Some("image/png"));
            }
            other => panic!("{other:?}"),
        }
        let r = import_curl("curl -X PUT https://files.acme.test/raw -H 'Content-Type: application/xml' --data-binary @fixtures/a.xml", &catalogue(), "x").unwrap();
        assert_eq!(
            only_step(&r).body,
            Some(RequestBody::Raw {
                media_type: "application/xml".into(),
                content: RawContent::File("fixtures/a.xml".into())
            })
        );
    }

    #[test]
    fn a_curl_path_no_spec_describes_becomes_an_inline_request() {
        let r = import_curl(
            "curl -X DELETE http://localhost:3000/v1/internal/cache",
            &catalogue(),
            "purge",
        )
        .unwrap();
        let scenario = &r.scenarios[0];
        assert_eq!(scenario.requests.len(), 1);
        assert_eq!(scenario.requests[0].method, "DELETE");
        assert_eq!(scenario.requests[0].path, "/internal/cache");
        assert_eq!(
            scenario.steps[0].operation,
            StepTarget::Inline(scenario.requests[0].id.clone())
        );
    }

    #[test]
    fn a_curl_host_nothing_names_is_refused_unless_a_spec_is_chosen() {
        let err = import_curl("curl https://evil.test/x", &catalogue(), "x").unwrap_err();
        assert!(matches!(err, ImportError::UnknownHost { .. }), "{err}");
        let cat = ImportCatalogue {
            default_spec: Some("auth".into()),
            ..catalogue()
        };
        let r = import_curl("curl https://somewhere.else/sessions -X POST", &cat, "x").unwrap();
        assert!(
            r.notes.iter().any(|n| n.contains("somewhere.else")),
            "the change of host is said: {:?}",
            r.notes
        );
        assert_eq!(
            r.scenarios[0].steps[0].operation.qualified(),
            "auth/loginUser"
        );
        assert!(matches!(
            import_curl("wget x", &cat, "x"),
            Err(ImportError::NotCurl)
        ));
    }

    #[test]
    fn a_bare_variable_in_a_json_body_is_read_as_one_value() {
        let r = import_curl(
            "curl https://api.acme.test/v1/sessions -H 'Content-Type: application/json' -d '{\"id\": {{user_id}}, \"note\": \"a {{b}} c\"}'",
            &catalogue(),
            "j",
        )
        .unwrap();
        match &only_step(&r).body {
            Some(RequestBody::Json(MorseValue::Map(fields))) => {
                assert_eq!(
                    fields[0],
                    ("id".into(), MorseValue::Str("{{user_id}}".into()))
                );
                assert_eq!(
                    fields[1],
                    ("note".into(), MorseValue::Str("a {{b}} c".into()))
                );
            }
            other => panic!("{other:?}"),
        }
    }

    // ---- Postman ---------------------------------------------------------------

    const COLLECTION: &str = r#"{
      "info": { "name": "Acme API", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
      "variable": [{ "key": "baseUrl", "value": "https://api.acme.test/v1" }],
      "item": [
        { "name": "Auth", "item": [
          { "name": "Log in", "request": {
              "method": "POST",
              "header": [{ "key": "Content-Type", "value": "application/json" }],
              "url": { "raw": "{{baseUrl}}/sessions" },
              "body": { "mode": "raw", "raw": "{\"email\":\"{{email}}\",\"password\":\"{{password}}\"}", "options": { "raw": { "language": "json" } } } },
            "event": [{ "listen": "test", "script": { "exec": ["pm.test('ok', function () {", "  pm.response.to.have.status(201);", "});", "pm.environment.set('token', pm.response.json().token);"] } }]
          },
          { "name": "Get user", "request": {
              "method": "GET",
              "header": [{ "key": "Authorization", "value": "Bearer abc.def.ghi" }],
              "url": { "raw": "{{baseUrl}}/users/:id", "variable": [{ "key": "id", "value": "{{user_id}}" }] } },
            "event": [{ "listen": "prerequest", "script": { "exec": ["console.log('hi')"] } }]
          }
        ]},
        { "name": "Health", "request": { "method": "GET", "url": "{{baseUrl}}/health" } }
      ]
    }"#;

    #[test]
    fn a_collection_becomes_a_scenario_per_folder_with_its_scripts_listed_not_kept() {
        let cat = ImportCatalogue {
            default_spec: Some("auth".into()),
            ..catalogue()
        };
        let r = import_postman(COLLECTION, &cat).unwrap();
        let names: Vec<&str> = r.scenarios.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            ["auth", "acme-api"],
            "folders first, loose requests under the collection's name"
        );
        let auth = &r.scenarios[0];
        assert_eq!(
            auth.steps.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["log-in", "get-user"]
        );
        assert_eq!(auth.steps[0].operation.qualified(), "auth/loginUser");
        assert_eq!(
            auth.steps[0].expect.status,
            Some(201),
            "a plain status test survives"
        );
        assert_eq!(auth.steps[1].operation.qualified(), "auth/getUser");
        assert_eq!(
            auth.steps[1].params,
            [("id".to_owned(), MorseValue::Str("{{user_id}}".into()))]
        );
        let auth_header = &auth.steps[1].headers[0].1;
        assert_eq!(auth_header, &MorseValue::Str("Bearer {{token}}".into()));
        assert!(!format!("{r:?}").contains("abc.def.ghi"));
        for dropped in ["Log in", "Get user"] {
            assert!(
                r.notes
                    .iter()
                    .any(|n| n.contains(dropped) && n.contains("script")),
                "{dropped}: {:?}",
                r.notes
            );
        }
        let health = &r.scenarios[1];
        assert_eq!(
            health.requests[0].path, "/health",
            "a path the spec lacks is an inline request"
        );
    }

    #[test]
    fn a_collection_without_a_chosen_spec_for_its_host_variable_says_so() {
        let err = import_postman(COLLECTION, &catalogue()).unwrap_err();
        assert!(
            matches!(err, ImportError::HostVariable(ref v) if v == "{{baseUrl}}"),
            "{err}"
        );
        assert!(matches!(
            import_postman("{\"not\": 1}", &catalogue()),
            Err(ImportError::NotCollection(_))
        ));
        assert!(matches!(
            import_postman("not json", &catalogue()),
            Err(ImportError::Json(_))
        ));
    }

    #[test]
    fn a_postman_environment_gives_its_server_and_values() {
        let env = import_postman_env(
            r#"{
          "name": "Acme Staging",
          "values": [
            { "key": "baseUrl", "value": "https://api.staging.acme.test/v1", "enabled": true },
            { "key": "token", "value": "s3cret", "enabled": true, "type": "secret" },
            { "key": "old", "value": "x", "enabled": false }
          ]
        }"#,
        )
        .unwrap();
        assert_eq!(env.name, "acme-staging");
        assert_eq!(
            env.server.as_deref(),
            Some("https://api.staging.acme.test/v1")
        );
        assert_eq!(
            env.vars,
            [("token".to_owned(), "s3cret".to_owned())],
            "local values do come along — that is where they belong"
        );
        assert!(
            env.notes.iter().any(|n| n.contains("old")),
            "a disabled value is reported as skipped"
        );
        assert!(import_postman_env("{\"info\": {}}").is_err());
    }
}
