//! Writing a scenario back into its fence (ADR 0023).
//!
//! The Morse screen edits a step as a form; what it saves is the fence. This
//! module turns a [`MorseScenario`] into fence text in the shape people write
//! by hand, and splices it into a document without touching a byte of the
//! prose around it.
//!
//! The writer is held to one promise: **what it writes parses back to exactly
//! what it was given.** The YAML subset this crate reads is small on purpose
//! — quoted strings carry no escapes, a quoted key in block form keeps its
//! quotes — so some values have no spelling in it. Rather than write
//! something that would read back differently, the writer re-parses its own
//! output and refuses when the two disagree, naming the field.

use crate::quote::QuoteScan;

use dit_model::{
    ExpectRule, MorseScenario, MorseStep, MorseValue, PartContent, RawContent, RequestBody,
    Selector, StepTarget,
};

use crate::morse::{morse_fences, parse_morse_scenario, scenario_in_fence, MORSE_FENCE};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MorseWriteError {
    #[error("`{0}` cannot be written as a fence value — it holds both kinds of quote")]
    Unquotable(String),
    #[error("`{0}` cannot be a key in a fence — keys may not contain spaces, quotes, `:`, `,`, `#` or brackets")]
    BadKey(String),
    #[error("the scenario would not read back as written: {0}")]
    WouldNotRoundTrip(String),
}

/// The fence body for one scenario — the lines between the markers.
pub fn write_morse_scenario(scenario: &MorseScenario) -> Result<String, MorseWriteError> {
    let text = emit(scenario)?;
    match parse_morse_scenario(&text) {
        Ok(back) if &back == scenario => Ok(text),
        Ok(_) => Err(MorseWriteError::WouldNotRoundTrip(
            "a value changed on the way back through the reader".to_owned(),
        )),
        Err(err) => Err(MorseWriteError::WouldNotRoundTrip(err.to_string())),
    }
}

/// True when a fence carries a `#` comment. A comment is a person's words,
/// and a re-serialised fence would drop it, so such a fence is edited in its
/// document rather than from a form.
pub fn has_comments(fence_body: &str) -> bool {
    fence_body.lines().any(|line| {
        let mut scan = QuoteScan::new();
        let mut chars = line.char_indices().peekable();
        while let Some((i, ch)) = chars.next() {
            let next = chars.peek().map(|&(_, c)| c);
            if scan.step(ch, next) && ch == '#' && (i == 0 || line[..i].ends_with([' ', '\t'])) {
                return true;
            }
        }
        false
    })
}

/// The lines of a document, and the indexes of the opening and closing
/// markers of the fence that names `scenario`.
fn fence_bounds<'a>(document: &'a str, scenario: &str) -> Option<(Vec<&'a str>, usize, usize)> {
    let target = morse_fences(document)
        .into_iter()
        .find(|f| scenario_in_fence(&f.body).as_deref() == Some(scenario))?;
    let lines: Vec<&str> = document.split_inclusive('\n').collect();
    let open = target.line - 1;
    let marker = lines.get(open)?;
    let ticks = marker
        .trim_start()
        .chars()
        .take_while(|c| *c == '`')
        .count();
    let close = lines
        .iter()
        .enumerate()
        .skip(open + 1)
        .find(|(_, l)| {
            let t = l.trim_start();
            t.chars().take_while(|c| *c == '`').count() >= ticks
                && t.trim_start_matches('`').trim().is_empty()
        })
        .map(|(i, _)| i)?;
    Some((lines, open, close))
}

/// Replace the body of the fence that names `scenario`, leaving the markers
/// and every other line of the document exactly as they were. `None` when no
/// fence in the document names it.
pub fn replace_morse_fence(document: &str, scenario: &str, body: &str) -> Option<String> {
    let (lines, open, close) = fence_bounds(document, scenario)?;
    let mut out = String::with_capacity(document.len() + body.len());
    for line in &lines[..=open] {
        out.push_str(line);
    }
    out.push_str(body.trim_end_matches('\n'));
    out.push('\n');
    for line in &lines[close..] {
        out.push_str(line);
    }
    Some(out)
}

/// Take out the fence that names `scenario`, markers and all, with one of
/// the blank lines around it so the prose closes up. Every other line stays
/// exactly as it was. `None` when no fence in the document names it.
pub fn remove_morse_fence(document: &str, scenario: &str) -> Option<String> {
    let (lines, open, close) = fence_bounds(document, scenario)?;
    let blank = |i: usize| lines.get(i).is_some_and(|l| l.trim().is_empty());
    // Drop the blank line after the fence; if the fence ended the document,
    // drop the one before it instead.
    let (from, to) = if blank(close + 1) {
        (open, close + 1)
    } else if open > 0 && blank(open - 1) {
        (open - 1, close)
    } else {
        (open, close)
    };
    let mut out = String::with_capacity(document.len());
    for (i, line) in lines.iter().enumerate() {
        if i < from || i > to {
            out.push_str(line);
        }
    }
    Some(out)
}

/// Add a new fence at the end of a document, separated from what came before
/// by one blank line.
pub fn append_morse_fence(document: &str, body: &str) -> String {
    let mut out = document.trim_end_matches('\n').to_owned();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str("```");
    out.push_str(MORSE_FENCE);
    out.push('\n');
    out.push_str(body.trim_end_matches('\n'));
    out.push_str("\n```\n");
    out
}

// ---- emitting --------------------------------------------------------------

fn emit(s: &MorseScenario) -> Result<String, MorseWriteError> {
    let mut out = String::new();
    out.push_str(&format!("scenario: {}\n", scalar(&s.scenario)?));
    out.push_str(&format!(
        "spec: {{ id: {}, commit: {} }}\n",
        scalar(&s.spec.id)?,
        scalar(&s.spec.commit)?
    ));
    if let Some(env) = &s.env {
        out.push_str(&format!("env: {}\n", scalar(env)?));
    }
    if !s.requires.is_empty() {
        let names: Result<Vec<_>, _> = s.requires.iter().map(|n| scalar(n)).collect();
        out.push_str(&format!("requires: [{}]\n", names?.join(", ")));
    }
    if !s.requests.is_empty() {
        out.push_str("requests:\n");
        for r in &s.requests {
            let mut fields = vec![
                format!("id: {}", scalar(&r.id)?),
                format!("method: {}", scalar(&r.method)?),
                format!("path: {}", scalar(&r.path)?),
            ];
            if let Some(summary) = &r.summary {
                fields.push(format!("summary: {}", scalar(summary)?));
            }
            out.push_str(&format!("  - {{ {} }}\n", fields.join(", ")));
        }
    }
    if !s.steps.is_empty() {
        out.push_str("steps:\n");
        for step in &s.steps {
            emit_step(&mut out, step)?;
        }
    }
    if !s.proven.is_empty() {
        out.push_str("proven:\n");
        for p in &s.proven {
            out.push_str(&format!(
                "  {}: {{ commit: {}, on: {} }}\n",
                key(&p.env)?,
                scalar(&p.commit)?,
                scalar(&p.on)?
            ));
        }
    }
    Ok(out)
}

fn emit_step(out: &mut String, step: &MorseStep) -> Result<(), MorseWriteError> {
    out.push_str(&format!("  - id: {}\n", scalar(&step.id)?));
    match &step.operation {
        StepTarget::Operation(op) => {
            out.push_str(&format!("    operation: {}\n", scalar(&op.qualified())?));
        }
        StepTarget::Inline(id) => out.push_str(&format!("    request: {}\n", scalar(id)?)),
    }
    for (key, pairs) in [
        ("params", &step.params),
        ("query", &step.query),
        ("headers", &step.headers),
    ] {
        if !pairs.is_empty() {
            out.push_str(&format!("    {key}: {}\n", flow_map(pairs)?));
        }
    }
    if let Some(body) = &step.body {
        emit_body(out, body)?;
    }
    if step.expect.status.is_some() || !step.expect.json.is_empty() {
        out.push_str("    expect:\n");
        if let Some(status) = step.expect.status {
            out.push_str(&format!("      status: {status}\n"));
        }
        if !step.expect.json.is_empty() {
            out.push_str("      jsonpath:\n");
            for check in &step.expect.json {
                let rule = match &check.rule {
                    ExpectRule::Exists => "{ exists: true }".to_owned(),
                    ExpectRule::Equals(value) => scalar(value)?,
                };
                out.push_str(&format!("        {}: {rule}\n", key(&check.path)?));
            }
        }
    }
    if !step.capture.is_empty() {
        let mut parts = Vec::new();
        for c in &step.capture {
            let from = match &c.from {
                Selector::Status => "status".to_owned(),
                Selector::Header(name) => format!("header:{name}"),
                Selector::JsonPath(path) => path.clone(),
            };
            parts.push(format!("{}: {}", key(&c.name)?, scalar(&from)?));
        }
        out.push_str(&format!("    capture: {{ {} }}\n", parts.join(", ")));
    }
    Ok(())
}

/// One of the four body shapes (ADR 0027). Multi-line raw text is a literal
/// block, the only YAML form that holds lines as written; the round-trip
/// check refuses text a block would change (trailing spaces, an indented
/// first line) rather than writing something else.
fn emit_body(out: &mut String, body: &RequestBody) -> Result<(), MorseWriteError> {
    match body {
        RequestBody::Json(value) => out.push_str(&format!("    body: {}\n", flow(value)?)),
        RequestBody::Form(pairs) => out.push_str(&format!("    form: {}\n", flow_map(pairs)?)),
        RequestBody::Raw {
            media_type,
            content: RawContent::File(path),
        } => out.push_str(&format!(
            "    raw: {{ type: {}, file: {} }}\n",
            scalar(media_type)?,
            scalar(path)?
        )),
        RequestBody::Raw {
            media_type,
            content: RawContent::Text(text),
        } if !text.contains('\n') => out.push_str(&format!(
            "    raw: {{ type: {}, text: {} }}\n",
            scalar(media_type)?,
            scalar(text)?
        )),
        RequestBody::Raw {
            media_type,
            content: RawContent::Text(text),
        } => {
            out.push_str(&format!(
                "    raw:\n      type: {}\n      text: |\n",
                scalar(media_type)?
            ));
            for line in text.split('\n') {
                if line.is_empty() {
                    out.push('\n');
                } else {
                    out.push_str(&format!("        {line}\n"));
                }
            }
        }
        RequestBody::Multipart(parts) => {
            out.push_str("    multipart:\n");
            for part in parts {
                let mut fields = vec![format!("name: {}", scalar(&part.name)?)];
                match &part.content {
                    PartContent::Value(value) => fields.push(format!("value: {}", flow(value)?)),
                    PartContent::File(path) => fields.push(format!("file: {}", scalar(path)?)),
                }
                if let Some(kind) = &part.media_type {
                    fields.push(format!("type: {}", scalar(kind)?));
                }
                out.push_str(&format!("      - {{ {} }}\n", fields.join(", ")));
            }
        }
    }
    Ok(())
}

fn flow_map(pairs: &[(String, MorseValue)]) -> Result<String, MorseWriteError> {
    if pairs.is_empty() {
        return Ok("{}".to_owned());
    }
    let mut parts = Vec::new();
    for (k, v) in pairs {
        parts.push(format!("{}: {}", key(k)?, flow(v)?));
    }
    Ok(format!("{{ {} }}", parts.join(", ")))
}

fn flow(value: &MorseValue) -> Result<String, MorseWriteError> {
    match value {
        MorseValue::Str(s) => scalar(s),
        MorseValue::Seq(items) => {
            let parts: Result<Vec<_>, _> = items.iter().map(flow).collect();
            Ok(format!("[{}]", parts?.join(", ")))
        }
        MorseValue::Map(entries) => flow_map(entries),
    }
}

/// A key is written bare: the reader keeps the quotes of a quoted key in
/// block form, so a key that needs quoting has no faithful spelling.
fn key(k: &str) -> Result<String, MorseWriteError> {
    let bad = k.is_empty()
        || k.chars().any(|c| {
            c.is_whitespace() || matches!(c, ':' | ',' | '#' | '"' | '\'' | '{' | '}' | '[' | ']')
        });
    if bad {
        Err(MorseWriteError::BadKey(k.to_owned()))
    } else {
        Ok(k.to_owned())
    }
}

/// A value, bare when the reader would give it back unchanged and quoted
/// otherwise. The reader has no escapes, so the quote that is not inside the
/// value is the one used.
fn scalar(s: &str) -> Result<String, MorseWriteError> {
    let needs = s.is_empty()
        || s != s.trim()
        || s == "null"
        || s == "~"
        || s.starts_with(['-', '&', '*', '!', '|', '>', '%', '@', '`'])
        || s.contains(": ")
        || s.ends_with(':')
        || s.chars().any(|c| {
            matches!(
                c,
                ',' | '#' | '"' | '\'' | '{' | '}' | '[' | ']' | '\n' | '\t'
            )
        });
    if !needs {
        return Ok(s.to_owned());
    }
    if s.contains('\n') {
        return Err(MorseWriteError::Unquotable(s.to_owned()));
    }
    if !s.contains('"') {
        Ok(format!("\"{s}\""))
    } else if !s.contains('\'') {
        Ok(format!("'{s}'"))
    } else {
        Err(MorseWriteError::Unquotable(s.to_owned()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn removing_a_fence_leaves_the_prose_and_the_other_fences_alone() {
        let doc = "# Auth\n\nWhy these exist.\n\n```dit-morse\nscenario: a\nspec: { id: s, commit: c }\n```\n\nBetween.\n\n```dit-morse\nscenario: b\nspec: { id: s, commit: c }\n```\n";
        let without_a = remove_morse_fence(doc, "a").unwrap();
        assert_eq!(
            without_a,
            "# Auth\n\nWhy these exist.\n\nBetween.\n\n```dit-morse\nscenario: b\nspec: { id: s, commit: c }\n```\n"
        );
        let without_b = remove_morse_fence(doc, "b").unwrap();
        assert_eq!(
            without_b,
            "# Auth\n\nWhy these exist.\n\n```dit-morse\nscenario: a\nspec: { id: s, commit: c }\n```\n\nBetween.\n"
        );
        assert_eq!(remove_morse_fence(doc, "missing"), None);
    }

    // ---- body shapes (ADR 0027) -------------------------------------------

    const BODIES: &str = r#"scenario: shapes
spec: { id: auth, commit: a3f9c2d }
steps:
  - id: token
    operation: auth/token
    form: { grant_type: password, username: "{{email}}", password: "{{password}}" }
  - id: soap
    operation: auth/legacy
    raw:
      type: application/xml
      text: |
        <login>
          <user>{{email}}</user>
        </login>
  - id: plain
    operation: auth/note
    raw: { type: "text/plain; charset=utf-8", text: "hello {{email}}" }
  - id: blob
    operation: files/put
    raw: { type: application/octet-stream, file: fixtures/blob.bin }
  - id: upload
    operation: files/upload
    multipart:
      - { name: title, value: "{{title}}" }
      - { name: avatar, file: fixtures/avatar.png, type: image/png }
"#;

    #[test]
    fn every_body_shape_parses_and_is_written_back_the_same() {
        use dit_model::{PartContent, RawContent, RequestBody};
        let parsed = crate::morse::parse_morse_scenario(BODIES).unwrap();
        let bodies: Vec<_> = parsed
            .steps
            .iter()
            .map(|s| s.body.clone().unwrap())
            .collect();
        assert!(matches!(&bodies[0], RequestBody::Form(pairs) if pairs.len() == 3));
        match &bodies[1] {
            RequestBody::Raw {
                media_type,
                content: RawContent::Text(text),
            } => {
                assert_eq!(media_type, "application/xml");
                assert_eq!(text, "<login>\n  <user>{{email}}</user>\n</login>");
            }
            other => panic!("{other:?}"),
        }
        assert!(
            matches!(&bodies[3], RequestBody::Raw { content: RawContent::File(p), .. } if p == "fixtures/blob.bin")
        );
        match &bodies[4] {
            RequestBody::Multipart(parts) => {
                assert_eq!(parts.len(), 2);
                assert!(
                    matches!(&parts[1].content, PartContent::File(p) if p == "fixtures/avatar.png")
                );
                assert_eq!(parts[1].media_type.as_deref(), Some("image/png"));
            }
            other => panic!("{other:?}"),
        }
        let written = write_morse_scenario(&parsed).unwrap();
        assert_eq!(
            crate::morse::parse_morse_scenario(&written).unwrap(),
            parsed,
            "{written}"
        );
        assert!(
            written.contains("      text: |\n        <login>\n"),
            "multi-line text is a literal block:\n{written}"
        );
    }

    #[test]
    fn raw_text_the_reader_would_change_is_refused_rather_than_altered() {
        use dit_model::{RawContent, RequestBody};
        let mut parsed = crate::morse::parse_morse_scenario(BODIES).unwrap();
        // A literal block drops trailing spaces; writing one would change the body.
        parsed.steps[1].body = Some(RequestBody::Raw {
            media_type: "text/plain".into(),
            content: RawContent::Text("line one   \nline two".into()),
        });
        assert!(write_morse_scenario(&parsed).is_err());
    }

    #[test]
    fn proofs_survive_a_write_from_the_form() {
        let text = "scenario: ping\nspec: { id: auth, commit: a3f9c2d }\nsteps:\n  - id: one\n    operation: auth/ping\nproven:\n  local: { commit: a3f9c2d, on: 2026-09-26 }\n  local-hrperf: { commit: b7e0d11, on: 2026-09-27 }\n";
        let parsed = crate::morse::parse_morse_scenario(text).unwrap();
        let written = write_morse_scenario(&parsed).unwrap();
        assert!(
            written.contains("proven:\n  local: { commit: a3f9c2d, on: 2026-09-26 }\n"),
            "{written}"
        );
        assert_eq!(
            crate::morse::parse_morse_scenario(&written).unwrap(),
            parsed
        );
    }

    #[test]
    fn an_apostrophe_in_a_value_does_not_hide_a_comment() {
        assert!(has_comments(
            "summary: it's the ping # nobody documented it\n"
        ));
        assert!(!has_comments("summary: it's the ping\n"));
    }

    const HAND_WRITTEN: &str = r#"scenario: register
spec: { id: auth, commit: a3f9c2d }
env: local
requires: [email, password]
requests:
  - { id: legacyPing, method: GET, path: /internal/ping, summary: "Nobody documented this, ever" }
steps:
  - id: create
    operation: auth/createUser
    body: { email: "{{email}}", profile: { tags: [a, "b, c"], note: 'say "hi"' } }
    expect:
      status: 201
      jsonpath:
        $.data.id: { exists: true }
    capture: { user_id: $.data.id, loc: "header:Location", code: status }
  - id: fetch
    operation: party/getParty
    params: { id: "{{user_id}}" }
    query: { include: contacts }
    headers: { Authorization: "Bearer {{token}}", X-Empty: "" }
    expect:
      jsonpath:
        $.data.name: "Acme, QA"
  - id: ping
    request: legacyPing
"#;

    #[test]
    fn what_is_written_reads_back_as_the_same_scenario() {
        let scenario = parse_morse_scenario(HAND_WRITTEN).unwrap();
        let written = write_morse_scenario(&scenario).unwrap();
        assert_eq!(
            parse_morse_scenario(&written).unwrap(),
            scenario,
            "{written}"
        );
        assert_eq!(
            write_morse_scenario(&parse_morse_scenario(&written).unwrap()).unwrap(),
            written,
            "writing is stable: a second save changes nothing"
        );
    }

    #[test]
    fn the_writer_emits_the_shape_people_write_by_hand() {
        let scenario = parse_morse_scenario(HAND_WRITTEN).unwrap();
        let written = write_morse_scenario(&scenario).unwrap();
        assert!(written.starts_with("scenario: register\nspec: { id: auth, commit: a3f9c2d }\n"));
        assert!(
            written.contains("    params: { id: \"{{user_id}}\" }\n"),
            "{written}"
        );
        assert!(
            written.contains("        $.data.id: { exists: true }\n"),
            "{written}"
        );
    }

    #[test]
    fn a_value_with_no_spelling_is_refused_rather_than_written_differently() {
        let mut scenario = parse_morse_scenario(HAND_WRITTEN).unwrap();
        scenario.steps[1].headers[0].1 = MorseValue::Str("it's \"both\"".into());
        assert!(matches!(
            write_morse_scenario(&scenario),
            Err(MorseWriteError::Unquotable(_))
        ));
        let mut scenario = parse_morse_scenario(HAND_WRITTEN).unwrap();
        scenario.steps[1].headers[0].0 = "Has Space".into();
        assert!(matches!(
            write_morse_scenario(&scenario),
            Err(MorseWriteError::BadKey(_))
        ));
    }

    #[test]
    fn a_forbidden_key_cannot_be_smuggled_in_through_the_writer() {
        // A header called `url` is refused by the reader (I7); the writer
        // must refuse it too, rather than produce a fence nobody can read.
        let mut scenario = parse_morse_scenario(HAND_WRITTEN).unwrap();
        scenario.steps[1].headers[0].0 = "url".into();
        assert!(matches!(
            write_morse_scenario(&scenario),
            Err(MorseWriteError::WouldNotRoundTrip(_))
        ));
    }

    const DOC: &str = "# Party\n\nProse a person wrote.\n\n```dit-morse\nscenario: other\nspec: { id: x, commit: y }\n```\n\nMore prose — keep me.\n\n```dit-morse\nscenario: register\nspec: { id: auth, commit: old }\n```\n\nTrailing words.\n";

    #[test]
    fn replacing_a_fence_touches_only_its_own_lines() {
        let out = replace_morse_fence(
            DOC,
            "register",
            "scenario: register\nspec: { id: auth, commit: new }\n",
        )
        .unwrap();
        assert_eq!(out, DOC.replace("commit: old", "commit: new"));
        assert!(replace_morse_fence(DOC, "nowhere", "x").is_none());
    }

    #[test]
    fn a_new_fence_goes_after_the_prose() {
        assert_eq!(
            append_morse_fence("# Party\n\nWords.\n", "scenario: a\n"),
            "# Party\n\nWords.\n\n```dit-morse\nscenario: a\n```\n"
        );
        assert_eq!(
            append_morse_fence("", "scenario: a"),
            "```dit-morse\nscenario: a\n```\n"
        );
    }

    #[test]
    fn a_comment_is_noticed_but_a_hash_inside_a_value_is_not() {
        assert!(has_comments("scenario: a\n# why this exists\n"));
        assert!(has_comments("env: local   # the dev box\n"));
        assert!(!has_comments("headers: { X-Tag: \"#1\" }\nq: a#b\n"));
    }
}
