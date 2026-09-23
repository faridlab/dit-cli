//! Putting values into a request, and nothing more.
//!
//! `{{name}}` is replaced by what the environment supplied or an earlier step
//! captured. There is no expression to evaluate, no function to call and no
//! way to reach anything that was not bound: a name nothing provides is an
//! error, never an empty string quietly sent to a server.
//!
//! Substituted values never reach the scheme, host or port — the caller
//! builds those from the spec's own `servers:` entry — and a value landing in
//! a path or a query is percent-encoded, so a captured `../admin` is a
//! segment with those characters in it rather than a different endpoint.

use std::collections::BTreeMap;

use dit_model::{variables_in, MorseValue};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{{{{{name}}}}}` is not bound — the environment does not supply it and no earlier step captured it")]
pub struct Unbound {
    pub name: String,
}

pub type Vars = BTreeMap<String, String>;

/// Replace every `{{name}}` in a string.
pub fn fill(text: &str, vars: &Vars) -> Result<String, Unbound> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            out.push_str(&rest[open..]);
            return Ok(out);
        };
        let name = after[..close].trim();
        if name.is_empty() {
            out.push_str("{{}}");
        } else {
            let value = vars.get(name).ok_or_else(|| Unbound {
                name: name.to_owned(),
            })?;
            out.push_str(value);
        }
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Replace every `{{name}}`, percent-encoding what is put in. For paths and
/// query values, where a raw `/`, `?` or `#` would change which endpoint is
/// reached rather than what is sent to it.
pub fn fill_encoded(text: &str, vars: &Vars) -> Result<String, Unbound> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            out.push_str(&rest[open..]);
            return Ok(out);
        };
        let name = after[..close].trim();
        if name.is_empty() {
            out.push_str("{{}}");
        } else {
            let value = vars.get(name).ok_or_else(|| Unbound {
                name: name.to_owned(),
            })?;
            out.push_str(&percent_encode(value));
        }
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Fill the `{name}` segments of a spec's path — OpenAPI's own syntax — from
/// a step's `params:`. Each value is rendered, then percent-encoded as one
/// segment. A `{{name}}` reference is left for [`fill_encoded`]; a segment no
/// entry fills is an error naming it, because sending the literal `{id}` to
/// a server is a request nobody wrote.
pub fn fill_path_params(
    path: &str,
    params: &[(String, MorseValue)],
    vars: &Vars,
) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        if after.starts_with('{') {
            // A template reference: copy it through whole.
            let Some(close) = after.find("}}") else {
                out.push_str(&rest[open..]);
                return Ok(out);
            };
            out.push_str(&rest[open..open + 1 + close + 2]);
            rest = &after[close + 2..];
            continue;
        }
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return Ok(out);
        };
        let name = after[..close].trim();
        let value = params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
            .ok_or_else(|| {
                format!(
                    "path parameter `{name}` has no value — the path is `{path}`; add \
                     `params: {{ {name}: ... }}` to the step"
                )
            })?;
        let rendered = match value {
            MorseValue::Str(text) => fill(text, vars).map_err(|e| e.to_string())?,
            other => to_json(other, vars).map_err(|e| e.to_string())?,
        };
        out.push_str(&percent_encode(&rendered));
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Everything outside the unreserved set, so a substituted value can only
/// ever be one path segment or one query value.
pub fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The variables a value reads, so a caller can check them before sending.
pub fn variables_of(value: &MorseValue) -> Vec<String> {
    value.variables()
}

/// Render a body as JSON, substituting as it goes.
///
/// **A scalar that looks like a number, a boolean or null is emitted as one.**
/// The YAML subset this fence is written in does not record whether the author
/// quoted a scalar, so `age: 30` and `age: "30"` arrive here identical — and
/// an API that wants a number is the far commoner case. A value that must stay
/// text and would otherwise be read as a number (`"0123"`, `"1e5"`) keeps a
/// leading zero or a space, which no JSON number may have.
pub fn to_json(value: &MorseValue, vars: &Vars) -> Result<String, Unbound> {
    Ok(match value {
        MorseValue::Str(text) => {
            let filled = fill(text, vars)?;
            // A value the author wrote entirely as one reference takes the
            // shape of what it references, so `{{age}}` with 30 is a number.
            if looks_scalar(&filled) && (!text.contains("{{") || variables_in(text).len() == 1) {
                filled
            } else {
                json_string(&filled)
            }
        }
        MorseValue::Seq(items) => {
            let parts: Result<Vec<_>, _> = items.iter().map(|i| to_json(i, vars)).collect();
            format!("[{}]", parts?.join(","))
        }
        MorseValue::Map(entries) => {
            let mut parts = Vec::new();
            for (key, item) in entries {
                parts.push(format!("{}:{}", json_string(key), to_json(item, vars)?));
            }
            format!("{{{}}}", parts.join(","))
        }
    })
}

fn looks_scalar(text: &str) -> bool {
    if matches!(text, "true" | "false" | "null") {
        return true;
    }
    // A JSON number, and only a JSON number: no leading zero, no leading
    // plus, no surrounding space.
    let mut chars = text.chars().peekable();
    if chars.peek() == Some(&'-') {
        chars.next();
    }
    let digits: String = chars.collect();
    if digits.is_empty() || !digits.starts_with(|c: char| c.is_ascii_digit()) {
        return false;
    }
    if digits.len() > 1 && digits.starts_with('0') && !digits.starts_with("0.") {
        return false;
    }
    digits.parse::<f64>().is_ok() && !digits.contains(|c: char| c.is_whitespace())
}

pub fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
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

    fn vars(pairs: &[(&str, &str)]) -> Vars {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn a_name_nothing_bound_is_an_error_rather_than_an_empty_string() {
        let v = vars(&[("token", "abc")]);
        assert_eq!(fill("Bearer {{token}}", &v).unwrap(), "Bearer abc");
        let err = fill("Bearer {{missing}}", &v).unwrap_err();
        assert_eq!(err.name, "missing");
        assert!(
            err.to_string().contains("no earlier step captured it"),
            "the message has to say where a value could have come from: {err}"
        );
    }

    #[test]
    fn a_value_put_into_a_path_cannot_become_a_different_endpoint() {
        let v = vars(&[("id", "../admin"), ("q", "a b&c=d")]);
        assert_eq!(
            fill_encoded("/users/{{id}}", &v).unwrap(),
            "/users/..%2Fadmin",
            "a captured value is one segment, whatever it contains"
        );
        assert_eq!(fill_encoded("{{q}}", &v).unwrap(), "a%20b%26c%3Dd");
    }

    #[test]
    fn a_body_keeps_the_shape_the_api_expects() {
        let v = vars(&[("email", "a@b.c"), ("age", "30")]);
        let body = MorseValue::Map(vec![
            ("email".into(), MorseValue::Str("{{email}}".into())),
            ("age".into(), MorseValue::Str("{{age}}".into())),
            ("count".into(), MorseValue::Str("3".into())),
            ("active".into(), MorseValue::Str("true".into())),
            ("zip".into(), MorseValue::Str("01234".into())),
            (
                "tags".into(),
                MorseValue::Seq(vec![MorseValue::Str("x".into())]),
            ),
        ]);
        assert_eq!(
            to_json(&body, &v).unwrap(),
            r#"{"email":"a@b.c","age":30,"count":3,"active":true,"zip":"01234","tags":["x"]}"#,
            "a leading zero is what keeps a postcode a string"
        );
    }

    #[test]
    fn a_reference_inside_a_sentence_stays_text() {
        let v = vars(&[("n", "5")]);
        let body = MorseValue::Map(vec![(
            "note".into(),
            MorseValue::Str("saw {{n}} of them".into()),
        )]);
        assert_eq!(to_json(&body, &v).unwrap(), r#"{"note":"saw 5 of them"}"#);
    }

    #[test]
    fn strings_that_would_break_the_json_are_escaped() {
        let v = vars(&[]);
        let body = MorseValue::Map(vec![("s".into(), MorseValue::Str("a\"b\\c\nd".into()))]);
        assert_eq!(to_json(&body, &v).unwrap(), r#"{"s":"a\"b\\c\nd"}"#);
    }
}
