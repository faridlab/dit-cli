//! Reading an OpenAPI document into the catalogue Morse browses (§20.2).
//!
//! Both published formats are accepted, because server frameworks are split
//! roughly evenly between them: YAML through this crate's own subset parser,
//! JSON through [`crate::json`]. Both produce the same tree, so everything
//! below this line is format-blind. Swagger 2.0 is read too and upgraded on
//! the way in — a workspace should not have to migrate its document to get a
//! catalogue.
//!
//! Nothing here fetches anything. It is handed bytes that were already read
//! from a git ref, and returns data (I11).

use dit_model::{OpenApiSpec, SpecOperation, SpecServer};

use crate::yaml::{Yaml, YamlError};

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum OpenApiError {
    #[error("this is not an OpenAPI document — it has no `openapi:` or `swagger:` version")]
    NotASpec,
    #[error("the document did not parse as YAML: {0}")]
    Yaml(YamlError),
    #[error("the document did not parse as JSON: {0}")]
    Json(crate::json::JsonError),
    #[error("`paths:` must be a mapping of path to operations")]
    BadPaths,
}

/// The HTTP methods an OpenAPI path item may carry. Anything else under a
/// path is a sibling key like `parameters:`, not an operation.
const METHODS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

pub fn parse_openapi(text: &str) -> Result<OpenApiSpec, OpenApiError> {
    let root = parse_document(text)?;
    let three = root.get("openapi").and_then(Yaml::as_str).is_some();
    let two = root.get("swagger").and_then(Yaml::as_str).is_some();
    if !three && !two {
        return Err(OpenApiError::NotASpec);
    }

    let info = root.get("info");
    let spec = OpenApiSpec {
        title: info.and_then(|i| i.get("title")).and_then(str_value),
        version: info.and_then(|i| i.get("version")).and_then(str_value),
        servers: if two {
            swagger_servers(&root)
        } else {
            servers(&root)
        },
        operations: operations(&root)?,
    };
    Ok(spec)
}

/// JSON or YAML, decided by the first thing that is not whitespace. Both
/// land in the same tree, so this is the only place the difference exists.
fn parse_document(text: &str) -> Result<Yaml, OpenApiError> {
    if text.trim_start().starts_with('{') {
        crate::json::parse(text).map_err(OpenApiError::Json)
    } else {
        crate::yaml::parse(text).map_err(OpenApiError::Yaml)
    }
}

fn str_value(node: &Yaml) -> Option<String> {
    node.as_str().map(str::trim).map(str::to_owned)
}

fn servers(root: &Yaml) -> Vec<SpecServer> {
    root.get("servers")
        .and_then(Yaml::as_seq)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| {
                    Some(SpecServer {
                        url: e.get("url").and_then(str_value)?,
                        description: e.get("description").and_then(str_value),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Swagger 2.0 states its address as `schemes` + `host` + `basePath`.
/// Morse needs one URL, so the three are put back together here rather than
/// in every caller.
fn swagger_servers(root: &Yaml) -> Vec<SpecServer> {
    let Some(host) = root.get("host").and_then(str_value) else {
        return Vec::new();
    };
    let base = root.get("basePath").and_then(str_value).unwrap_or_default();
    let schemes: Vec<String> = root
        .get("schemes")
        .and_then(Yaml::as_seq)
        .map(|s| s.iter().filter_map(str_value).collect())
        .unwrap_or_default();
    let schemes = if schemes.is_empty() {
        vec!["https".to_owned()]
    } else {
        schemes
    };
    schemes
        .into_iter()
        .map(|scheme| SpecServer {
            url: format!("{scheme}://{host}{base}"),
            description: None,
        })
        .collect()
}

fn operations(root: &Yaml) -> Result<Vec<SpecOperation>, OpenApiError> {
    let Some(paths) = root.get("paths") else {
        return Ok(Vec::new());
    };
    let Yaml::Map(entries) = paths else {
        return Err(OpenApiError::BadPaths);
    };
    let mut out = Vec::new();
    for (path, item) in entries {
        let Yaml::Map(methods) = item else {
            continue;
        };
        for (method, op) in methods {
            let lower = method.to_ascii_lowercase();
            if !METHODS.contains(&lower.as_str()) {
                continue;
            }
            // An operation with no `operationId` cannot be named by a step.
            // Skipping it is the honest outcome: inventing an id would break
            // every scenario using it the day the document adds a real one.
            let Some(operation_id) = op.get("operationId").and_then(str_value) else {
                continue;
            };
            if operation_id.is_empty() {
                continue;
            }
            out.push(SpecOperation {
                operation_id,
                method: lower.to_ascii_uppercase(),
                path: path.clone(),
                summary: op.get("summary").and_then(str_value),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const YAML_SPEC: &str = r#"openapi: 3.0.3
info:
  title: Acme Auth
  version: "1.4.0"
  description: |
    A description that wraps,
    as real documents do.
servers:
  - url: "https://api.acme.com"
    description: production
  - url: "http://localhost:3000"
    description: local
paths:
  /users:
    parameters: []
    post:
      operationId: createUser
      summary: Register a new user
      responses:
        '201': { description: created }
  /sessions:
    post:
      operationId: loginUser
      summary: Log in
  /me:
    get:
      operationId: getCurrentUser
"#;

    const JSON_SPEC: &str = r#"{
  "openapi": "3.0.3",
  "info": { "title": "Acme Auth", "version": "1.4.0" },
  "servers": [ { "url": "http://localhost:3000", "description": "local" } ],
  "paths": { "/users": { "post": { "operationId": "createUser" } } }
}"#;

    #[test]
    fn a_yaml_document_yields_its_operations_and_servers() {
        let spec = parse_openapi(YAML_SPEC).unwrap();
        assert_eq!(spec.title.as_deref(), Some("Acme Auth"));
        assert_eq!(spec.version.as_deref(), Some("1.4.0"));
        assert_eq!(spec.operations.len(), 3, "and `parameters:` is not one");
        let create = spec.operation("createUser").unwrap();
        assert_eq!(create.method, "POST");
        assert_eq!(create.path, "/users");
        assert_eq!(create.summary.as_deref(), Some("Register a new user"));
        assert_eq!(
            spec.server_for(Some("local")).map(|s| s.url.as_str()),
            Some("http://localhost:3000")
        );
        assert_eq!(
            spec.server_for(Some("nothing-named-this"))
                .map(|s| s.url.as_str()),
            Some("https://api.acme.com"),
            "an unmatched environment falls back to the first server"
        );
    }

    #[test]
    fn the_same_document_as_json_reads_identically() {
        let spec = parse_openapi(JSON_SPEC).unwrap();
        assert_eq!(spec.title.as_deref(), Some("Acme Auth"));
        assert_eq!(spec.operations.len(), 1);
        assert_eq!(spec.operation("createUser").unwrap().path, "/users");
        assert_eq!(spec.servers.len(), 1);
    }

    #[test]
    fn swagger_two_is_read_and_its_server_reassembled() {
        let spec = parse_openapi(
            "swagger: \"2.0\"\ninfo:\n  title: Legacy\n  version: \"1.0\"\nhost: api.acme.com\nbasePath: /v2\nschemes: [https]\npaths:\n  /ping:\n    get:\n      operationId: ping\n",
        )
        .unwrap();
        assert_eq!(spec.operations.len(), 1);
        assert_eq!(
            spec.servers.first().map(|s| s.url.as_str()),
            Some("https://api.acme.com/v2"),
            "2.0 states the address in three keys; Morse needs one"
        );
    }

    #[test]
    fn an_operation_without_an_id_is_skipped_rather_than_invented() {
        // A step names an operationId. An operation without one cannot be
        // referenced, and inventing a name would make scenarios break the
        // day the document adds a real one.
        let spec = parse_openapi(
            "openapi: 3.0.0\npaths:\n  /a:\n    get:\n      summary: no id here\n  /b:\n    get:\n      operationId: hasOne\n",
        )
        .unwrap();
        assert_eq!(spec.operations.len(), 1);
        assert_eq!(spec.operations[0].operation_id, "hasOne");
    }

    #[test]
    fn something_that_is_not_a_spec_says_so() {
        assert_eq!(
            parse_openapi("title: just notes\n"),
            Err(OpenApiError::NotASpec)
        );
        assert!(matches!(
            parse_openapi("{ broken"),
            Err(OpenApiError::Json(_))
        ));
    }
}
