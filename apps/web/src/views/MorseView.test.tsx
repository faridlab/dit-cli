// The Morse screen has to show all four verdicts, and its one control — the
// spec card that opens to reveal a catalogue — has to actually open. These
// mount the view against one small report and read what a person would read.
//
// The data hook is stubbed: what is under test is the screen's behaviour,
// not the index behind it.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { MorseReportDto } from "../lib/types";

const REPORT: MorseReportDto = {
  clean: false,
  specs: [
    {
      id: "auth",
      repo: "backend",
      path: "services/auth/openapi.yaml",
      title: "Acme Auth",
      version: "1.4.0",
      head: "a3f9c2d",
      operations: [
        { operation_id: "createUser", method: "POST", path: "/users", summary: "Register a user" },
        { operation_id: "loginUser", method: "POST", path: "/sessions", summary: null },
      ],
      problem: null,
    },
    {
      id: "billing",
      repo: null,
      path: "api/billing.yaml",
      title: null,
      version: null,
      head: null,
      operations: [],
      problem: "`api/billing.yaml` is not in this workspace at HEAD",
    },
  ],
  scenarios: [
    {
      scenario: "register",
      path: "docs/api/register.md",
      line: 3,
      spec_id: "auth",
      pin: "a3f9c2d",
      env: "local",
      steps: ["create", "login", "me"],
      requires: ["email", "password"],
      health: "fresh",
      stale_by: null,
      reasons: [],
      last_run: null,
    },
    {
      scenario: "checkout",
      path: "docs/api/checkout.md",
      line: 12,
      spec_id: "auth",
      pin: "b1c2d3e",
      env: null,
      steps: ["pay"],
      requires: [],
      health: "stale",
      stale_by: 7,
      reasons: [],
      last_run: null,
    },
    {
      scenario: "renamed",
      path: "docs/api/renamed.md",
      line: 5,
      spec_id: "auth",
      pin: "c4d5e6f",
      env: null,
      steps: ["login"],
      requires: [],
      health: "broken",
      stale_by: null,
      reasons: ["step `login` calls `auth/loginUser`, which the spec no longer describes"],
      last_run: null,
    },
    {
      scenario: "half-written",
      path: "docs/api/broken.md",
      line: 9,
      spec_id: "",
      pin: "",
      env: null,
      steps: [],
      requires: [],
      health: "unreadable",
      stale_by: null,
      reasons: ["`spec:` must name a registered spec and the commit it was checked against"],
      last_run: null,
    },
  ],
};

let report: MorseReportDto | undefined = REPORT;
let pending = false;

let ran: string[] = [];

vi.mock("../lib/queries", () => ({
  useMorse: () => ({
    data: report,
    isPending: pending,
    isError: false,
    error: null,
    refetch: () => undefined,
  }),
  useRunMorse: () => ({
    mutate: (scenario: string) => ran.push(scenario),
    isPending: false,
    variables: undefined,
  }),
}));

const { MorseView } = await import("./MorseView");

let container: HTMLDivElement;
let root: Root;

function render() {
  act(() => {
    root.render(<MorseView />);
  });
}

function click(element: Element | null | undefined) {
  expect(element, "the control under test is not on screen").toBeTruthy();
  act(() => {
    element!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}

/** The Run button inside one scenario's row, if it has one. */
function runButton(name: string): HTMLButtonElement | undefined {
  return [...scenarioRow(name).querySelectorAll<HTMLButtonElement>("button")].find((b) =>
    (b.textContent ?? "").toLowerCase().includes("run"),
  );
}

/** The row whose scenario name is this. */
function scenarioRow(name: string): HTMLElement {
  const found = [...container.querySelectorAll<HTMLElement>(".morse-row")].find((r) =>
    [...r.querySelectorAll(".morse-name")].some((n) => n.textContent === name),
  );
  expect(found, `no row for ${name}`).toBeTruthy();
  return found!;
}

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  report = REPORT;
  pending = false;
  ran = [];
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("MorseView", () => {
  it("shows every verdict, and says how far a stale one has drifted", () => {
    render();
    expect(scenarioRow("register").textContent).toContain("fresh");
    // The count is the actionable part: "stale" alone says nothing about
    // whether this is one commit behind or a year.
    expect(scenarioRow("checkout").textContent).toContain("stale · 7");
    expect(scenarioRow("renamed").textContent).toContain("broken");
    expect(scenarioRow("half-written").textContent).toContain("unreadable");
  });

  it("names the reason a scenario cannot be run, not just that it cannot", () => {
    render();
    expect(scenarioRow("renamed").textContent).toContain("auth/loginUser");
    expect(scenarioRow("half-written").textContent).toContain("`spec:` must name");
  });

  it("shows a scenario's chain in order and what the environment must supply", () => {
    render();
    const steps = [...scenarioRow("register").querySelectorAll(".morse-steps li")].map(
      (li) => li.textContent,
    );
    expect(steps).toEqual(["create", "login", "me"]);
    const text = scenarioRow("register").textContent ?? "";
    expect(text).toContain("email, password");
    expect(text, "the screen must not imply values live in the repo").toContain("names only");
  });

  it("opens a spec card onto its catalogue and closes it again", () => {
    render();
    expect(container.querySelectorAll(".morse-ops li")).toHaveLength(0);

    const toggle = [...container.querySelectorAll<HTMLButtonElement>(".morse-spec-top")].find((b) =>
      (b.textContent ?? "").includes("Acme Auth"),
    );
    click(toggle);
    const rows = [...container.querySelectorAll(".morse-ops li")].map((li) => li.textContent);
    expect(rows).toHaveLength(2);
    expect(rows[0]).toContain("POST");
    expect(rows[0]).toContain("/users");
    expect(rows[0]).toContain("createUser");

    click(toggle);
    expect(container.querySelectorAll(".morse-ops li")).toHaveLength(0);
  });

  it("lists a spec it could not read rather than hiding it", () => {
    render();
    const text = container.textContent ?? "";
    expect(text).toContain("billing");
    expect(text).toContain("is not in this workspace at HEAD");
  });

  it("tells an empty workspace what to do next instead of showing nothing", () => {
    report = { clean: true, specs: [], scenarios: [] };
    render();
    const text = container.textContent ?? "";
    expect(text).toContain("specs:");
    expect(text).toContain("never a URL");
    expect(text).toContain("dit-morse");
  });

  it("offers Run only where there is something that could be sent", () => {
    render();
    // fresh and stale can be run; broken and unreadable have nothing to send,
    // and a button that could only fail is worse than no button.
    expect(runButton("register")).toBeTruthy();
    expect(runButton("checkout")).toBeTruthy();
    expect(runButton("renamed")).toBeUndefined();
    expect(runButton("half-written")).toBeUndefined();

    click(runButton("register"));
    expect(ran).toEqual(["register"]);
  });

  it("names a host this machine never trusted, and hands over the command", () => {
    report = {
      ...REPORT,
      scenarios: [
        {
          ...REPORT.scenarios[0]!,
          last_run: {
            scenario: "register",
            ran_at: 0,
            passed: false,
            refused:
              "api.staging.acme.com is not allowed on this machine, so nothing was sent. If you trust it, run:  dit morse allow api.staging.acme.com",
            steps: [],
          },
        },
      ],
    };
    render();
    const text = scenarioRow("register").textContent ?? "";
    expect(text).toContain("api.staging.acme.com");
    expect(text).toContain("dit morse allow api.staging.acme.com");
    // The page hands over the command; it must not offer to do it.
    const labels = [...container.querySelectorAll("button")].map((b) =>
      (b.textContent ?? "").toLowerCase(),
    );
    expect(labels.some((l) => l.includes("allow this host") || l.includes("trust"))).toBe(false);
  });

  it("shows what the last run did, step by step", () => {
    report = {
      ...REPORT,
      scenarios: [
        {
          ...REPORT.scenarios[0]!,
          last_run: {
            scenario: "register",
            ran_at: 0,
            passed: false,
            refused: null,
            steps: [
              { id: "create", method: "POST", status: 201, duration_ms: 12, passed: true, detail: "captured user_id" },
              { id: "login", method: "POST", status: 500, duration_ms: 4, passed: false, detail: "expected status 200, got 500" },
            ],
          },
        },
      ],
    };
    render();
    const text = scenarioRow("register").textContent ?? "";
    expect(text).toContain("captured user_id");
    expect(text).toContain("expected status 200, got 500");
    expect(text, "and that it is disposable, so nobody reads it as a record").toContain(
      "gone at the next reindex",
    );
  });
});
