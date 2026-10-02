//! The runner against a real server.
//!
//! A few dozen lines of `TcpListener` rather than a mock: what is under test
//! is whether a chain actually carries a value from one response into the
//! next request over a socket, and a stubbed client would test the stub.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver};
use std::thread;

use dit_model::{
    Capture, Expect, ExpectRule, JsonCheck, MorseValue, MultipartPart, PartContent, RawContent,
    RequestBody, Selector,
};
use dit_morse::{run, LocalConfig, PlannedStep, Policy, RunPlan};

/// What the server saw.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
    body: String,
}

/// A server that answers each connection with the next canned response and
/// reports what it was asked. Returns its port and the receiver.
fn serve(responses: Vec<(u16, &'static str)>) -> (u16, Receiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = channel();
    thread::spawn(move || {
        for (index, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { break };
            let seen = read_request(&mut stream);
            let (status, body) = responses.get(index).copied().unwrap_or((500, "{}"));
            let reason = if status < 300 { "OK" } else { "NOPE" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n\
                 X-Token: header-token\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            let _ = tx.send(seen);
            if index + 1 >= responses.len() {
                break;
            }
        }
    });
    (port, rx)
}

fn read_request(stream: &mut TcpStream) -> Seen {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut start = String::new();
    reader.read_line(&mut start).unwrap();
    let mut parts = start.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
        }
    }
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    if len > 0 {
        reader.read_exact(&mut body).unwrap();
    }
    Seen {
        method,
        target,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    }
}

fn allowing_localhost() -> Policy {
    let mut allow = LocalConfig::default();
    allow.allow("127.0.0.1");
    Policy {
        allow,
        timeout_secs: 5,
    }
}

fn step(id: &str, method: &str, path: &str) -> PlannedStep {
    PlannedStep {
        id: id.into(),
        base_url: None,
        method: method.into(),
        path: path.into(),
        params: vec![],
        headers: vec![],
        query: vec![],
        body: None,
        expect: Expect::default(),
        capture: vec![],
    }
}

#[test]
fn a_chain_carries_a_value_from_one_response_into_the_next_request() {
    let (port, seen) = serve(vec![
        (201, r#"{"data":{"id":"u_7"}}"#),
        (200, r#"{"token":"t0k"}"#),
        (200, r#"{"id":"u_7"}"#),
    ]);
    let plan = RunPlan {
        scenario: "register".into(),
        base_url: format!("http://127.0.0.1:{port}"),
        files: BTreeMap::new(),
        vars: [("email".to_owned(), "dev@acme.test".to_owned())]
            .into_iter()
            .collect(),
        steps: vec![
            PlannedStep {
                body: Some(RequestBody::Json(MorseValue::Map(vec![(
                    "email".into(),
                    MorseValue::Str("{{email}}".into()),
                )]))),
                expect: Expect {
                    status: Some(201),
                    json: vec![],
                },
                capture: vec![Capture {
                    name: "user_id".into(),
                    from: Selector::JsonPath("$.data.id".into()),
                }],
                ..step("create", "POST", "/users")
            },
            PlannedStep {
                expect: Expect {
                    status: Some(200),
                    json: vec![],
                },
                capture: vec![
                    Capture {
                        name: "token".into(),
                        from: Selector::JsonPath("$.token".into()),
                    },
                    Capture {
                        name: "kind".into(),
                        from: Selector::Header("X-Token".into()),
                    },
                ],
                ..step("login", "POST", "/sessions")
            },
            PlannedStep {
                headers: vec![(
                    "Authorization".into(),
                    MorseValue::Str("Bearer {{token}}".into()),
                )],
                expect: Expect {
                    status: Some(200),
                    json: vec![JsonCheck {
                        path: "$.id".into(),
                        rule: ExpectRule::Equals("{{user_id}}".into()),
                    }],
                },
                ..step("me", "GET", "/users/{{user_id}}")
            },
        ],
    };

    let outcome = run(&plan, &allowing_localhost());
    assert!(outcome.passed(), "{outcome:#?}");
    assert_eq!(outcome.steps.len(), 3);

    let create = seen.recv().unwrap();
    assert_eq!(create.method, "POST");
    assert_eq!(create.body, r#"{"email":"dev@acme.test"}"#);
    assert_eq!(
        create.headers.get("content-type").map(String::as_str),
        Some("application/json"),
        "a body implies its type unless the scenario said otherwise"
    );

    let _login = seen.recv().unwrap();
    let me = seen.recv().unwrap();
    assert_eq!(
        me.target, "/users/u_7",
        "the id captured from the first response reached the third request's path"
    );
    assert_eq!(
        me.headers.get("authorization").map(String::as_str),
        Some("Bearer t0k"),
        "and the token captured from the second reached its header"
    );
    assert_eq!(
        outcome.steps[1].captured,
        vec![
            ("token".to_owned(), "t0k".to_owned()),
            ("kind".to_owned(), "header-token".to_owned())
        ]
    );
}

#[test]
fn a_failed_expectation_stops_the_chain_and_says_what_it_wanted() {
    let (port, _seen) = serve(vec![(500, r#"{"error":"boom"}"#)]);
    let plan = RunPlan {
        scenario: "register".into(),
        base_url: format!("http://127.0.0.1:{port}"),
        files: BTreeMap::new(),
        vars: BTreeMap::new(),
        steps: vec![
            PlannedStep {
                expect: Expect {
                    status: Some(201),
                    json: vec![],
                },
                ..step("create", "POST", "/users")
            },
            step("never", "GET", "/me"),
        ],
    };
    let outcome = run(&plan, &allowing_localhost());
    assert!(!outcome.passed());
    assert_eq!(
        outcome.steps.len(),
        1,
        "the rest of the chain would only be reading values that step never bound"
    );
    assert_eq!(
        outcome.steps[0].failures,
        vec!["expected status 201, got 500"]
    );
}

#[test]
fn an_unallowed_host_stops_the_run_before_anything_is_sent() {
    let (port, _seen) = serve(vec![(200, "{}")]);
    let plan = RunPlan {
        scenario: "register".into(),
        base_url: format!("http://127.0.0.1:{port}"),
        files: BTreeMap::new(),
        vars: BTreeMap::new(),
        steps: vec![step("one", "GET", "/a")],
    };
    // A policy that allows nothing — a fresh clone, before anyone decided.
    let outcome = run(&plan, &Policy::default());
    assert!(!outcome.passed());
    assert!(outcome.steps.is_empty(), "nothing was attempted");
    let refused = outcome.refused.unwrap();
    assert!(refused.contains("127.0.0.1"), "{refused}");
    assert!(
        refused.contains("dit morse allow 127.0.0.1"),
        "the message has to carry the way out: {refused}"
    );
}

#[test]
fn a_redirect_is_reported_rather_than_followed() {
    // Following one would mean deciding mid-flight that a second host is as
    // trusted as the first.
    let (port, _seen) = serve(vec![(302, "{}")]);
    let plan = RunPlan {
        scenario: "redirected".into(),
        base_url: format!("http://127.0.0.1:{port}"),
        files: BTreeMap::new(),
        vars: BTreeMap::new(),
        steps: vec![PlannedStep {
            expect: Expect {
                status: Some(200),
                json: vec![],
            },
            ..step("one", "GET", "/a")
        }],
    };
    let outcome = run(&plan, &allowing_localhost());
    assert_eq!(outcome.steps[0].status, Some(302));
    assert_eq!(
        outcome.steps[0].failures,
        vec!["expected status 200, got 302"]
    );
}

#[test]
fn a_value_nothing_bound_is_reported_before_the_request_is_made() {
    let (port, _seen) = serve(vec![(200, "{}")]);
    let plan = RunPlan {
        scenario: "unbound".into(),
        base_url: format!("http://127.0.0.1:{port}"),
        files: BTreeMap::new(),
        vars: BTreeMap::new(),
        steps: vec![step("one", "GET", "/users/{{missing}}")],
    };
    let outcome = run(&plan, &allowing_localhost());
    assert!(!outcome.passed());
    let error = outcome.steps[0].error.clone().unwrap();
    assert!(error.contains("missing"), "{error}");
    assert_eq!(outcome.steps[0].status, None, "nothing was sent");
}

#[test]
fn a_capture_that_reaches_nothing_fails_the_step_rather_than_binding_empty() {
    let (port, _seen) = serve(vec![(200, r#"{"other":"x"}"#)]);
    let plan = RunPlan {
        scenario: "nocapture".into(),
        base_url: format!("http://127.0.0.1:{port}"),
        files: BTreeMap::new(),
        vars: BTreeMap::new(),
        steps: vec![PlannedStep {
            capture: vec![Capture {
                name: "token".into(),
                from: Selector::JsonPath("$.token".into()),
            }],
            ..step("one", "GET", "/a")
        }],
    };
    let outcome = run(&plan, &allowing_localhost());
    assert!(!outcome.passed());
    assert!(
        outcome.steps[0].failures[0].contains("$.token"),
        "{:?}",
        outcome.steps[0].failures
    );
}

#[test]
fn a_path_parameter_is_filled_from_params_and_stays_one_segment() {
    let (port, seen) = serve(vec![(200, r#"{"data":{}}"#)]);
    let plan = RunPlan {
        scenario: "fetch".into(),
        base_url: format!("http://127.0.0.1:{port}"),
        files: BTreeMap::new(),
        vars: [("party_id".to_owned(), "p/1?x".to_owned())]
            .into_iter()
            .collect(),
        steps: vec![PlannedStep {
            params: vec![("id".into(), MorseValue::Str("{{party_id}}".into()))],
            ..step("fetch", "GET", "/api/v1/party/parties/{id}")
        }],
    };
    let outcome = run(&plan, &allowing_localhost());
    assert!(outcome.passed(), "{outcome:?}");
    let got = seen.recv().unwrap();
    assert_eq!(
        got.target, "/api/v1/party/parties/p%2F1%3Fx",
        "the spec's `{{id}}` is filled, and a captured value cannot add a segment or a query"
    );
}

#[test]
fn a_path_parameter_with_no_value_is_refused_before_anything_is_sent() {
    let (port, _seen) = serve(vec![(200, "{}")]);
    let plan = RunPlan {
        scenario: "fetch".into(),
        base_url: format!("http://127.0.0.1:{port}"),
        files: BTreeMap::new(),
        vars: BTreeMap::new(),
        steps: vec![step("fetch", "GET", "/parties/{id}/contacts/{contact_id}")],
    };
    let outcome = run(&plan, &allowing_localhost());
    assert!(!outcome.passed());
    let error = outcome.steps[0].error.clone().unwrap();
    assert!(
        error.contains("`id`") && error.contains("params:"),
        "the message names the parameter and where it goes: {error}"
    );
    assert_eq!(outcome.steps[0].status, None, "nothing was sent");
}

// ---- Body shapes (ADR 0027) -------------------------------------------------

fn one_step_plan(port: u16, body: RequestBody, files: BTreeMap<String, Vec<u8>>) -> RunPlan {
    let mut vars = BTreeMap::new();
    vars.insert("email".to_owned(), "dev@acme.test".to_owned());
    RunPlan {
        scenario: "shapes".into(),
        base_url: format!("http://127.0.0.1:{port}"),
        files,
        vars: vars.into_iter().collect(),
        steps: vec![PlannedStep {
            body: Some(body),
            ..step("one", "POST", "/in")
        }],
    }
}

fn str_value(s: &str) -> MorseValue {
    MorseValue::Str(s.to_owned())
}

#[test]
fn a_form_body_is_url_encoded_with_its_content_type() {
    let (port, seen) = serve(vec![(200, "{}")]);
    let body = RequestBody::Form(vec![
        ("grant_type".into(), str_value("password")),
        ("user".into(), str_value("{{email}}")),
        ("note".into(), str_value("a b&c=d")),
    ]);
    let outcome = run(
        &one_step_plan(port, body, BTreeMap::new()),
        &allowing_localhost(),
    );
    assert!(outcome.passed(), "{outcome:#?}");
    let seen = seen.recv().unwrap();
    assert_eq!(
        seen.headers["content-type"],
        "application/x-www-form-urlencoded"
    );
    assert_eq!(
        seen.body,
        "grant_type=password&user=dev%40acme.test&note=a+b%26c%3Dd"
    );
}

#[test]
fn raw_text_goes_out_as_written_under_its_own_type() {
    let (port, seen) = serve(vec![(200, "{}")]);
    let body = RequestBody::Raw {
        media_type: "application/xml".into(),
        content: RawContent::Text("<u>{{email}}</u>".into()),
    };
    let outcome = run(
        &one_step_plan(port, body, BTreeMap::new()),
        &allowing_localhost(),
    );
    assert!(outcome.passed(), "{outcome:#?}");
    let seen = seen.recv().unwrap();
    assert_eq!(seen.headers["content-type"], "application/xml");
    assert_eq!(seen.body, "<u>dev@acme.test</u>");
}

#[test]
fn a_raw_file_sends_the_bytes_the_plan_carries() {
    let (port, seen) = serve(vec![(200, "{}")]);
    let body = RequestBody::Raw {
        media_type: "application/octet-stream".into(),
        content: RawContent::File("fixtures/blob.bin".into()),
    };
    let mut files = BTreeMap::new();
    files.insert("fixtures/blob.bin".to_owned(), b"committed bytes".to_vec());
    let outcome = run(&one_step_plan(port, body, files), &allowing_localhost());
    assert!(outcome.passed(), "{outcome:#?}");
    let seen = seen.recv().unwrap();
    assert_eq!(seen.headers["content-type"], "application/octet-stream");
    assert_eq!(seen.body, "committed bytes");
}

#[test]
fn a_multipart_body_carries_fields_and_files_between_boundaries() {
    let (port, seen) = serve(vec![(200, "{}")]);
    let body = RequestBody::Multipart(vec![
        MultipartPart {
            name: "title".into(),
            content: PartContent::Value(str_value("Hi {{email}}")),
            media_type: None,
        },
        MultipartPart {
            name: "avatar".into(),
            content: PartContent::File("fixtures/avatar.png".into()),
            media_type: Some("image/png".into()),
        },
    ]);
    let mut files = BTreeMap::new();
    files.insert("fixtures/avatar.png".to_owned(), b"PNGDATA".to_vec());
    let outcome = run(&one_step_plan(port, body, files), &allowing_localhost());
    assert!(outcome.passed(), "{outcome:#?}");
    let seen = seen.recv().unwrap();
    let kind = &seen.headers["content-type"];
    let boundary = kind
        .strip_prefix("multipart/form-data; boundary=")
        .unwrap_or_else(|| panic!("{kind}"));
    assert_eq!(
        seen.body,
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nHi dev@acme.test\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"avatar.png\"\r\n\
             Content-Type: image/png\r\n\r\nPNGDATA\r\n--{boundary}--\r\n"
        )
    );
    assert!(
        !"Hi dev@acme.testPNGDATA".contains(boundary),
        "the boundary occurs in no part"
    );
}

#[test]
fn a_header_content_type_wins_except_for_multipart_whose_boundary_it_would_lose() {
    let (port, seen) = serve(vec![(200, "{}"), (200, "{}")]);
    let mut plan = one_step_plan(
        port,
        RequestBody::Raw {
            media_type: "text/plain".into(),
            content: RawContent::Text("x".into()),
        },
        BTreeMap::new(),
    );
    plan.steps[0].headers = vec![("Content-Type".into(), str_value("text/csv"))];
    assert!(run(&plan, &allowing_localhost()).passed());
    assert_eq!(seen.recv().unwrap().headers["content-type"], "text/csv");
    plan.steps[0].body = Some(RequestBody::Multipart(vec![MultipartPart {
        name: "a".into(),
        content: PartContent::Value(str_value("b")),
        media_type: None,
    }]));
    assert!(run(&plan, &allowing_localhost()).passed());
    assert!(
        seen.recv().unwrap().headers["content-type"].starts_with("multipart/form-data; boundary=")
    );
}

#[test]
fn a_file_the_plan_does_not_carry_refuses_the_step_before_sending() {
    let body = RequestBody::Raw {
        media_type: "application/octet-stream".into(),
        content: RawContent::File("fixtures/missing.bin".into()),
    };
    // Nothing listens on this port: a request sent would fail differently.
    let outcome = run(
        &one_step_plan(9, body, BTreeMap::new()),
        &allowing_localhost(),
    );
    let error = outcome.steps[0].error.clone().unwrap_or_default();
    assert!(error.contains("fixtures/missing.bin"), "{error}");
    assert_eq!(outcome.steps[0].status, None);
}
