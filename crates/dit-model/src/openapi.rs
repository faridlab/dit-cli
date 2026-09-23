//! The part of an OpenAPI document Morse derives its catalogue from.
//!
//! Only what a scenario needs to resolve and what a person needs to browse:
//! the operations, and where the API says it lives. Schemas, examples and
//! prose are left in the document — it stays the source of truth, and DIT
//! never copies it (I5, §20.2).

/// One operation a spec describes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpecOperation {
    /// `operationId`. Unique inside this document only, which is why a step
    /// writes `<spec id>/<operationId>`.
    pub operation_id: String,
    /// Upper-case, as people write it in a request line.
    pub method: String,
    pub path: String,
    pub summary: Option<String>,
    /// The first of the operation's `tags:` — how a document groups itself,
    /// and how the Morse explorer groups 6,000 operations into something a
    /// person can scan.
    pub tag: Option<String>,
    /// Path, query and header parameters, `$ref`s resolved.
    pub params: Vec<SpecParam>,
    /// The top-level fields of a JSON request body, `$ref`s resolved. A
    /// shape to pre-fill a form from, not a validator.
    pub body: Vec<SpecField>,
    /// The response codes the document lists, as written (`201`, `4XX`).
    pub responses: Vec<String>,
}

/// One parameter an operation takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecParam {
    pub name: String,
    /// `path`, `query`, `header` or `cookie`, as OpenAPI's `in:` says.
    pub location: String,
    pub required: bool,
}

/// One field of a request body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecField {
    pub name: String,
    /// The schema's `type:`, or `object` / `any` when it gives none.
    pub kind: String,
    pub required: bool,
}

/// One entry of `servers:` — the API's own statement about where it lives.
/// Morse reads base URLs from here and never from a DIT file (§20.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecServer {
    pub url: String,
    pub description: Option<String>,
}

/// A parsed spec, reduced to what Morse uses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpenApiSpec {
    pub title: Option<String>,
    pub version: Option<String>,
    pub servers: Vec<SpecServer>,
    pub operations: Vec<SpecOperation>,
}

impl OpenApiSpec {
    pub fn operation(&self, id: &str) -> Option<&SpecOperation> {
        self.operations.iter().find(|o| o.operation_id == id)
    }

    /// The server an environment name selects, matched on `description`.
    /// Falls back to the first entry, which is what a document with one
    /// server means and what most documents have.
    pub fn server_for(&self, env: Option<&str>) -> Option<&SpecServer> {
        match env {
            Some(name) => self
                .servers
                .iter()
                .find(|s| s.description.as_deref() == Some(name))
                .or_else(|| self.servers.first()),
            None => self.servers.first(),
        }
    }
}
