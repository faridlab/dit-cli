//! The selector half of a scenario: reading one value out of a response.
//!
//! A deliberately small subset — `$`, `.name`, `['name']`, `[0]` — because
//! this is where a chaining language would start if it were allowed to. There
//! is no filter, no wildcard, no expression and no function: a selector reads
//! a value or it does not, and anything it cannot reach becomes a built-in in
//! a later release of DIT rather than a line of code in someone's repository.

use dit_parse::Yaml;

/// The value at `path`, if the response has one.
pub fn select<'a>(root: &'a Yaml, path: &str) -> Option<&'a Yaml> {
    let mut node = root;
    for step in steps(path)? {
        node = match step {
            Step::Key(name) => node.get(&name)?,
            Step::Index(i) => node.as_seq()?.get(i)?,
        };
    }
    Some(node)
}

/// How a value reads when compared against a literal or carried to the next
/// step. A map or a list has no single text, and saying so is better than
/// inventing one.
pub fn as_text(node: &Yaml) -> Option<String> {
    match node {
        Yaml::Str(s) => Some(s.clone()),
        Yaml::Null => Some("null".to_owned()),
        Yaml::Seq(_) | Yaml::Map(_) => None,
    }
}

enum Step {
    Key(String),
    Index(usize),
}

/// Split a path into steps, or `None` when it is not a path this understands.
fn steps(path: &str) -> Option<Vec<Step>> {
    let trimmed = path.trim();
    let mut rest = trimmed.strip_prefix('$')?;
    let mut out = Vec::new();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('.') {
            let end = after.find(['.', '[']).unwrap_or(after.len());
            let name = &after[..end];
            if name.is_empty() {
                return None;
            }
            out.push(Step::Key(name.to_owned()));
            rest = &after[end..];
            continue;
        }
        if let Some(after) = rest.strip_prefix('[') {
            let end = after.find(']')?;
            let inner = after[..end].trim();
            let step = match inner.strip_prefix('\'').and_then(|i| i.strip_suffix('\'')) {
                Some(name) if !name.is_empty() => Step::Key(name.to_owned()),
                Some(_) => return None,
                None => match inner.strip_prefix('"').and_then(|i| i.strip_suffix('"')) {
                    Some(name) if !name.is_empty() => Step::Key(name.to_owned()),
                    Some(_) => return None,
                    None => Step::Index(inner.parse().ok()?),
                },
            };
            out.push(step);
            rest = &after[end + 1..];
            continue;
        }
        return None;
    }
    Some(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const BODY: &str = r#"{
      "data": { "id": "u_1", "tags": ["a", "b"], "meta": { "count": 2 } },
      "token": "t0k",
      "nothing": null,
      "odd key": "yes"
    }"#;

    fn body() -> Yaml {
        dit_parse::parse_json(BODY).unwrap()
    }

    #[test]
    fn a_selector_reaches_what_a_scenario_actually_captures() {
        let b = body();
        assert_eq!(
            as_text(select(&b, "$.token").unwrap()).as_deref(),
            Some("t0k")
        );
        assert_eq!(
            as_text(select(&b, "$.data.id").unwrap()).as_deref(),
            Some("u_1")
        );
        assert_eq!(
            as_text(select(&b, "$.data.tags[1]").unwrap()).as_deref(),
            Some("b")
        );
        assert_eq!(
            as_text(select(&b, "$.data.meta.count").unwrap()).as_deref(),
            Some("2")
        );
        assert_eq!(
            as_text(select(&b, "$['odd key']").unwrap()).as_deref(),
            Some("yes")
        );
        assert_eq!(
            as_text(select(&b, "$.nothing").unwrap()).as_deref(),
            Some("null")
        );
    }

    #[test]
    fn a_path_that_reaches_nothing_says_nothing_rather_than_guessing() {
        let b = body();
        assert!(select(&b, "$.missing").is_none());
        assert!(select(&b, "$.data.tags[9]").is_none());
        assert!(select(&b, "$.token.deeper").is_none());
    }

    #[test]
    fn a_whole_object_has_no_single_text_and_says_so() {
        let b = body();
        assert!(select(&b, "$.data").is_some());
        assert_eq!(
            as_text(select(&b, "$.data").unwrap()),
            None,
            "capturing a whole object would have no defined value to carry"
        );
    }

    #[test]
    fn something_that_is_not_a_selector_is_refused_rather_than_half_read() {
        let b = body();
        // This is where a chaining language would start.
        for bad in ["token", "$..token", "$.data[*]", "$.data[?(@.id)]", "$."] {
            assert!(select(&b, bad).is_none(), "`{bad}` must not resolve");
        }
    }

    #[test]
    fn the_document_root_itself_is_reachable() {
        let b = body();
        assert!(select(&b, "$").is_some());
    }
}
