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

use dit_model::{OpenApiSpec, SpecField, SpecOperation, SpecParam, SpecServer};

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
        let shared = item.get("parameters");
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
            let mut params = parameters(root, op.get("parameters"));
            for shared in parameters(root, shared) {
                if !params
                    .iter()
                    .any(|p| p.name == shared.name && p.location == shared.location)
                {
                    params.push(shared);
                }
            }
            out.push(SpecOperation {
                operation_id,
                method: lower.to_ascii_uppercase(),
                path: path.clone(),
                summary: op.get("summary").and_then(str_value),
                tag: op
                    .get("tags")
                    .and_then(Yaml::as_seq)
                    .and_then(|t| t.first())
                    .and_then(str_value),
                params,
                body: body_fields(root, op.get("requestBody")),
                responses: match op.get("responses") {
                    // A quoted key (`'201':`, which is how most documents write
                    // them) keeps its quotes in this reader's tree.
                    Some(Yaml::Map(codes)) => codes
                        .iter()
                        .map(|(c, _)| c.trim_matches(['\'', '"']).to_owned())
                        .collect(),
                    _ => Vec::new(),
                },
            });
        }
    }
    Ok(out)
}

/// How deep a chain of `$ref`s is followed. A document may refer to itself
/// in a cycle; this is what stops that from being a hang.
const MAX_REF_DEPTH: usize = 8;

/// Follow a local `$ref` (`#/components/...`) to what it names. A reference
/// to any other document is left unresolved: following it would be DIT
/// fetching a file of its own accord (I7, I11).
fn deref<'a>(root: &'a Yaml, mut node: &'a Yaml) -> Option<&'a Yaml> {
    for _ in 0..MAX_REF_DEPTH {
        let Some(target) = node.get("$ref").and_then(Yaml::as_str) else {
            return Some(node);
        };
        let pointer = target.trim().strip_prefix("#/")?;
        let mut at = root;
        for part in pointer.split('/') {
            let part = part.replace("~1", "/").replace("~0", "~");
            at = at.get(&part)?;
        }
        node = at;
    }
    None
}

fn parameters(root: &Yaml, node: Option<&Yaml>) -> Vec<SpecParam> {
    let Some(items) = node.and_then(Yaml::as_seq) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let p = deref(root, item)?;
            Some(SpecParam {
                name: p.get("name").and_then(str_value)?,
                location: p.get("in").and_then(str_value)?,
                required: p.get("required").and_then(Yaml::as_str) == Some("true"),
            })
        })
        .collect()
}

fn body_fields(root: &Yaml, node: Option<&Yaml>) -> Vec<SpecField> {
    let Some(schema) = node
        .and_then(|b| deref(root, b))
        .and_then(|b| b.get("content"))
        .and_then(|c| c.get("application/json"))
        .and_then(|j| j.get("schema"))
        .and_then(|s| deref(root, s))
    else {
        return Vec::new();
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Yaml::as_seq)
        .map(|r| r.iter().filter_map(Yaml::as_str).collect())
        .unwrap_or_default();
    let Some(Yaml::Map(props)) = schema.get("properties") else {
        return Vec::new();
    };
    props
        .iter()
        .map(|(name, prop)| {
            let resolved = deref(root, prop);
            let kind = resolved
                .and_then(|p| p.get("type"))
                .and_then(str_value)
                .unwrap_or_else(|| {
                    if resolved.and_then(|p| p.get("properties")).is_some() {
                        "object".to_owned()
                    } else {
                        "any".to_owned()
                    }
                });
            SpecField {
                name: name.clone(),
                kind,
                required: required.contains(&name.as_str()),
            }
        })
        .collect()
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

    /// The shape a generated document has: parameters and bodies reached
    /// through `$ref`, which is how serpa's 45 documents are all written.
    const WITH_REFS: &str = r#"openapi: 3.0.3
paths:
  /api/v1/party/parties/{id}:
    parameters:
      - $ref: '#/components/parameters/IdParam'
    put:
      tags:
        - Parties
      operationId: updateParty
      parameters:
        - $ref: '#/components/parameters/PageParam'
        - name: X-Trace
          in: header
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/PartyInput'
      responses:
        '200': { description: ok }
        '404': { description: gone }
    get:
      operationId: getParty
      parameters:
        - $ref: 'https://evil.example/params.yaml#/Id'
components:
  parameters:
    IdParam:
      name: id
      in: path
      required: true
    PageParam:
      name: page
      in: query
  schemas:
    PartyInput:
      type: object
      required: [name, party_kind]
      properties:
        name:
          type: string
        party_kind:
          $ref: '#/components/schemas/Kind'
        active:
          type: boolean
    Kind:
      type: string
"#;

    #[test]
    fn an_operation_carries_what_a_form_needs_with_local_refs_resolved() {
        let spec = parse_openapi(WITH_REFS).unwrap();
        let op = spec.operation("updateParty").unwrap();
        assert_eq!(op.tag.as_deref(), Some("Parties"));
        let params: Vec<(&str, &str, bool)> = op
            .params
            .iter()
            .map(|p| (p.name.as_str(), p.location.as_str(), p.required))
            .collect();
        assert_eq!(
            params,
            vec![
                ("page", "query", false),
                ("X-Trace", "header", false),
                ("id", "path", true)
            ],
            "the operation's own parameters, then the path item's shared ones"
        );
        let body: Vec<(&str, &str, bool)> = op
            .body
            .iter()
            .map(|f| (f.name.as_str(), f.kind.as_str(), f.required))
            .collect();
        assert_eq!(
            body,
            vec![
                ("name", "string", true),
                ("party_kind", "string", true),
                ("active", "boolean", false)
            ]
        );
        assert_eq!(op.responses, vec!["200".to_owned(), "404".to_owned()]);
    }

    #[test]
    fn a_ref_to_another_document_is_not_followed() {
        // Following it would be DIT fetching something of its own accord
        // (I7, I11). The shared path parameter still comes through.
        let spec = parse_openapi(WITH_REFS).unwrap();
        let op = spec.operation("getParty").unwrap();
        let names: Vec<&str> = op.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["id"]);
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
