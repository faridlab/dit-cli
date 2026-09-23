// The workbench's behaviour against one small report: an operation opens as
// a draft pre-filled from the spec, Send posts exactly that draft with the
// chosen environment's *name*, the response panel shows what crossed the
// boundary and nothing else, a credential literal blocks Save, and a broken
// scenario offers no Run.
//
// The data hooks are stubbed: what is under test is the screen, not the
// index or the network behind it (those are pinned in Rust).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { MorseEnvsDto, MorseReportDto, MorseRunDto, MorseScenarioDetailDto, MorseSendDto } from "../../lib/types";

const REPORT: MorseReportDto = {
  clean: false,
  specs: [
    {
      id: "party",
      repo: "serpa-service",
      path: "docs/openapi/party.openapi.yaml",
      title: "Backbone API",
      version: "1.0.0",
      head: "e358a9f",
      servers: [{ url: "/", description: "Development server" }],
      operations: [
        {
          operation_id: "getParty",
          method: "GET",
          path: "/api/v1/party/parties/{id}",
          summary: "Get Party by ID",
          tag: "Parties",
          params: [{ name: "id", location: "path", required: true }],
          body: [],
          responses: ["200", "404"],
        },
        {
          operation_id: "createParty",
          method: "POST",
          path: "/api/v1/party/parties",
          summary: "Create Party",
          tag: "Parties",
          params: [],
          body: [
            { name: "name", kind: "string", required: true },
            { name: "notes", kind: "string", required: false },
          ],
          responses: ["201", "400"],
        },
      ],
      problem: null,
    },
  ],
  scenarios: [
    {
      scenario: "renamed",
      path: "docs/api/renamed.md",
      line: 5,
      spec_id: "party",
      pin: "c4d5e6f",
      env: null,
      steps: ["login"],
      requires: [],
      health: "broken",
      stale_by: null,
      reasons: ["step `login` calls `party/loginUser`, which the spec no longer describes"],
      last_run: null,
    },
  ],
};

const ENVS: MorseEnvsDto = {
  envs: [{ name: "local", server: "http://127.0.0.1:18080", vars: ["token"] }],
  allow_hosts: ["127.0.0.1"],
};

const DETAIL: MorseScenarioDetailDto = {
  scenario: "renamed",
  path: "docs/api/renamed.md",
  line: 5,
  spec_id: "party",
  pin: "c4d5e6f",
  env: null,
  requires: [],
  requests: [],
  steps: [],
  fence: "scenario: renamed\n",
  editable: true,
};

const SENT: MorseRunDto = {
  scenario: "send:party/getParty",
  ran_at: Math.floor(Date.now() / 1000),
  passed: true,
  refused: null,
  steps: [{ id: "getParty", method: "GET", status: 200, duration_ms: 6, bytes: 175, passed: true, detail: "captured party_name" }],
};

let sent: MorseSendDto[] = [];
let ran: unknown[] = [];

const mutation = <T,>(record: (input: T) => void, result: unknown) => ({
  mutate: (input: T, opts?: { onSuccess?: (r: unknown) => void }) => {
    record(input);
    opts?.onSuccess?.(result);
  },
  isPending: false,
});

vi.mock("../../lib/queries", () => ({
  useMorse: () => ({ data: REPORT, isPending: false, isError: false, error: null }),
  useMorseEnvs: () => ({ data: ENVS }),
  useMorseRuns: () => ({ data: [] }),
  useMorseScenario: (name: string | null) => ({ data: name ? DETAIL : undefined, isError: false, error: null }),
  useSendMorse: () => mutation((i: MorseSendDto) => sent.push(i), SENT),
  useRunMorse: () => mutation((i: unknown) => ran.push(i), SENT),
  useSaveMorseStep: () => mutation(() => undefined, DETAIL),
  useCreateMorseScenario: () => mutation(() => undefined, DETAIL),
}));

const { MorseView } = await import("./MorseView");

let container: HTMLDivElement;
let root: Root;

function render() {
  act(() => root.render(<MorseView />));
}

function byText(selector: string, text: string): HTMLElement {
  const found = [...container.querySelectorAll<HTMLElement>(selector)].find((e) => (e.textContent ?? "").includes(text));
  expect(found, `no ${selector} containing "${text}"`).toBeTruthy();
  return found!;
}

function click(el: Element) {
  act(() => {
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}

function type(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(proto, "value")?.set?.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function openOperation(summary: string) {
  click(byText(".mw-tr", "party"));
  click(byText(".mw-tr.d1", "Parties"));
  click(byText(".mw-tr.d2", summary));
}

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.clear();
  sent = [];
  ran = [];
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the Morse workbench", () => {
  it("says up front that a relative server names no host", () => {
    render();
    expect(container.textContent).toContain("1 of 1 specs declare");
    expect(container.textContent).toContain("servers: - url: /");
  });

  it("opens an operation as a draft pre-filled from the spec", () => {
    render();
    openOperation("Create Party");
    const body = container.querySelector<HTMLTextAreaElement>(".mw-code");
    expect(JSON.parse(body?.value ?? "")).toEqual({ name: "" });
    expect(container.querySelector(".mw-url .base")?.textContent, "the spec's `/` is no server").toBe("no server");
  });

  it("sends exactly the draft, with the environment's name and never a URL", () => {
    localStorage.setItem("dit.morse.env", JSON.stringify("local"));
    render();
    openOperation("Get Party by ID");
    type(container.querySelector<HTMLInputElement>('[aria-label="path parameter id"]')!, "pty_7");
    click(container.querySelector(".mw-send")!);

    expect(sent).toHaveLength(1);
    expect(sent[0]!.env).toBe("local");
    expect(sent[0]!.step.operation).toBe("party/getParty");
    expect(sent[0]!.step.params).toEqual([{ key: "id", value: "pty_7" }]);
    expect(JSON.stringify(sent[0]), "the page has no field that could name a host").not.toMatch(/https?:\/\//);

    const panel = container.querySelector(".mw-resp")?.textContent ?? "";
    expect(panel).toContain("200");
    expect(panel).toContain("175 B");
    expect(panel).toContain("captured party_name");
    expect(panel).toContain("stayed on the server");
  });

  it("blocks Save while a header holds a credential written out", () => {
    render();
    openOperation("Get Party by ID");
    click(byText(".mw-subtabs button", "Headers"));
    click(byText(".mw-btn", "Add header"));
    type(container.querySelector<HTMLInputElement>('[aria-label="Header 1 name"]')!, "Authorization");
    type(container.querySelector<HTMLInputElement>('[aria-label="Header 1 value"]')!, "Bearer eyJhbGciOiJIUzI1NiJ9.x.y");
    expect(byText(".mw-btn", "Save to scenario").hasAttribute("disabled")).toBe(true);
    expect(container.textContent).toContain("stay in git history");

    type(container.querySelector<HTMLInputElement>('[aria-label="Header 1 value"]')!, "Bearer {{token}}");
    expect(byText(".mw-btn", "Save to scenario").hasAttribute("disabled")).toBe(false);
  });

  it("offers no Run for a scenario that cannot be run, and says why", () => {
    render();
    click(byText(".mw-seg button", "Scenarios"));
    click(byText(".mw-tr", "renamed"));
    expect(container.textContent).toContain("party/loginUser");
    expect([...container.querySelectorAll(".mw-btn")].some((b) => b.textContent === "Run")).toBe(false);
    expect(ran).toHaveLength(0);
  });

  it("marks an edited draft and asks, in the page, before throwing it away", () => {
    render();
    openOperation("Create Party");
    type(container.querySelector<HTMLTextAreaElement>(".mw-code")!, '{ "name": "Acme" }');
    expect(container.querySelector(".mw-tab.on .dirty")).toBeTruthy();
    click(container.querySelector(".mw-tab.on .x")!);
    expect(document.body.textContent).toContain("Discard this draft?");
  });
});
