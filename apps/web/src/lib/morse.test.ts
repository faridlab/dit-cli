import { describe, expect, it } from "vitest";
import {
  bodyError,
  draftForOperation,
  emptyBody,
  filePathProblem,
  isMediaType,
  stepPreview,
  tabVerb,
  hostOf,
  isSelector,
  pathParams,
  pathSegments,
  relativeOnly,
  runKey,
  sameStep,
  secretHeaders,
  secretReason,
  usedVars,
  variablesIn,
} from "./morse";
import type { MorseOperationDto, MorseSpecDto } from "./types";

// serpa's `updateParty`, as the index now carries it.
const UPDATE_PARTY: MorseOperationDto = {
  operation_id: "updateParty",
  method: "PUT",
  path: "/api/v1/party/parties/{id}",
  summary: "Update Party",
  tag: "Parties",
  params: [{ name: "id", location: "path", required: true }],
  body: [
    { name: "party_code", kind: "string", required: true },
    { name: "level", kind: "integer", required: true },
    { name: "notes", kind: "string", required: false },
  ],
  responses: ["200", "404"],
};

describe("a draft from an operation", () => {
  it("is pre-filled with what the spec states and nothing else", () => {
    const d = draftForOperation("party", UPDATE_PARTY);
    expect(d.operation).toBe("party/updateParty");
    expect(d.id).toBe("update");
    expect(d.params).toEqual([{ key: "id", value: "" }]);
    expect(d.body?.kind).toBe("json");
    expect(JSON.parse(d.body?.kind === "json" ? d.body.text : "")).toEqual({ party_code: "", level: 0 });
    expect(d.status).toBe(200);
    expect(d.headers).toEqual([]);
  });

  it("finds a path parameter the document forgot to declare", () => {
    const d = draftForOperation("party", { ...UPDATE_PARTY, params: [], path: "/a/{x}/b/{y}" });
    expect(d.params.map((p) => p.key)).toEqual(["x", "y"]);
  });
});

describe("references", () => {
  it("reads single-brace parameters and double-brace names apart", () => {
    expect(pathParams("/users/{id}/x/{{token}}")).toEqual(["id"]);
    expect(variablesIn("Bearer {{ token }} and {{id}}")).toEqual(["token", "id"]);
  });

  it("collects every name a draft reads, once", () => {
    const d = draftForOperation("party", UPDATE_PARTY);
    d.params = [{ key: "id", value: "{{party_id}}" }];
    d.headers.push({ key: "Authorization", value: "Bearer {{token}}" });
    d.body = { kind: "json", text: '{ "party_code": "{{code}}", "again": "{{token}}" }' };
    expect(usedVars(d)).toEqual(["party_id", "token", "code"]);
  });
});

describe("credential literals", () => {
  it("pass as references and are caught when written out", () => {
    expect(secretReason("Authorization", "Bearer {{token}}")).toBeNull();
    expect(secretReason("password", "{{password}}")).toBeNull();
    expect(secretReason("Authorization", "Bearer eyJabc.def")).not.toBeNull();
    expect(secretReason("X-Thing", "ghp_0123456789")).not.toBeNull();
    expect(secretReason("client_secret", "hunter2")).not.toBeNull();
    expect(secretReason("X-Trace", "abc")).toBeNull();
  });

  it("points at the header that carries one", () => {
    const d = draftForOperation("party", UPDATE_PARTY);
    d.headers = [
      { key: "X-Trace", value: "1" },
      { key: "Authorization", value: "Bearer sk-live-123" },
    ];
    expect(secretHeaders(d)).toEqual([1]);
  });
});

describe("the small checks a tab makes before Send", () => {
  it("reads a body with references in it", () => {
    expect(bodyError({ kind: "json", text: '{ "id": {{id}}, "n": "{{n}}" }' })).toBeNull();
    expect(bodyError({ kind: "json", text: "{ nope" })).not.toBeNull();
    expect(bodyError(null)).toBeNull();
  });

  it("knows the three capture sources", () => {
    expect(isSelector("$.data.id")).toBe(true);
    expect(isSelector("header:Location")).toBe(true);
    expect(isSelector("status")).toBe(true);
    expect(isSelector("token.substring(7)")).toBe(false);
  });

  it("shows filled parameters in place", () => {
    expect(pathSegments("/p/{id}/c", [{ key: "id", value: "7" }])).toEqual([
      { text: "/p/", kind: "plain" },
      { text: "7", kind: "filled" },
      { text: "/c", kind: "plain" },
    ]);
    expect(pathSegments("/p/{id}", [])[1]).toEqual({ text: "{id}", kind: "param" });
  });

  it("recognises a spec that names no host", () => {
    const spec = { servers: [{ url: "/", description: "Development server" }] } as MorseSpecDto;
    expect(relativeOnly(spec)).toBe(true);
    expect(hostOf("https://Staging.Serpa.id:8443/x")).toBe("staging.serpa.id");
    expect(hostOf("/")).toBeNull();
  });

  it("tells a send from a scenario in history", () => {
    expect(runKey("send:party/getParty")).toEqual({ kind: "send", operation: "party/getParty" });
    expect(runKey("register")).toEqual({ kind: "scenario", name: "register" });
  });

  it("treats empty rows as nothing when judging whether a step is saved", () => {
    const a = draftForOperation("party", UPDATE_PARTY);
    const b = { ...a, headers: [{ key: "", value: "" }] };
    expect(sameStep(a, b)).toBe(true);
    expect(sameStep(a, { ...a, status: 201 })).toBe(false);
  });
});

describe("body shapes", () => {
  it("finds the variables of every shape", () => {
    const d = draftForOperation("party", UPDATE_PARTY);
    d.body = { kind: "form", fields: [{ key: "user", value: "{{email}}" }] };
    expect(usedVars(d)).toContain("email");
    d.body = { kind: "raw", media_type: "application/xml", text: "<u>{{email}}</u>", file: null };
    expect(usedVars(d)).toContain("email");
    d.body = { kind: "multipart", parts: [{ name: "t", value: "{{title}}", file: null, media_type: null }] };
    expect(usedVars(d)).toContain("title");
  });

  it("refuses what the server will refuse", () => {
    expect(bodyError({ kind: "raw", media_type: "xml", text: "a", file: null })).toMatch(/media type/);
    expect(bodyError({ kind: "raw", media_type: "text/plain; charset=utf-8", text: "a", file: null })).toBeNull();
    expect(bodyError({ kind: "raw", media_type: "image/png", text: null, file: "../x.png" })).toMatch(/climb/);
    expect(
      bodyError({ kind: "multipart", parts: [{ name: "f", value: null, file: ".dit/morse.local.yaml", media_type: null }] }),
    ).toMatch(/own files/);
    expect(filePathProblem("fixtures/a.png")).toBeNull();
    expect(isMediaType("application/vnd.api+json")).toBe(true);
    expect(isMediaType("text/plain\r\nX: y")).toBe(false);
  });

  it("previews each shape the way the fence writer writes it", () => {
    const d = draftForOperation("party", UPDATE_PARTY);
    d.body = { kind: "raw", media_type: "application/xml", text: "<a>\n  <b/>\n</a>", file: null };
    expect(stepPreview(d)).toContain("    raw:\n      type: application/xml\n      text: |\n        <a>\n          <b/>\n        </a>");
    d.body = {
      kind: "multipart",
      parts: [{ name: "avatar", value: null, file: "fixtures/a.png", media_type: "image/png" }],
    };
    expect(stepPreview(d)).toContain("      - { name: avatar, file: fixtures/a.png, type: image/png }");
    d.body = { kind: "form", fields: [{ key: "grant_type", value: "password" }, { key: "", value: "" }] };
    expect(stepPreview(d)).toContain("    form: { grant_type: password }");
  });

  it("keeps JSON text when switching to raw and back", () => {
    const json = { kind: "json" as const, text: '{"a":1}' };
    const raw = emptyBody("raw", json);
    expect(raw?.kind === "raw" && raw.text).toBe('{"a":1}');
    expect(emptyBody("json", raw)).toEqual(json);
    expect(emptyBody("none")).toBeNull();
  });
});

describe("tab verbs", () => {
  it("never claim a method for an operation not looked up yet", () => {
    // A step tab restored after a reload has no draft: it showed GET for a POST.
    expect(tabVerb(undefined)).toBeNull();
    expect(tabVerb({ method: "post" })).toBe("POST");
  });
});
