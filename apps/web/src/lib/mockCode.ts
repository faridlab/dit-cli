// The code map in mock mode (?mock=1): a small, made-up webapp and service,
// with the three /api/code answers derived from one file list the way the
// server derives them from the index — so folder counts, edges and a file's
// neighbours always agree with each other.

import type {
  CodeNeighbourDto,
  CodeNeighbourhoodDto,
  CodeOverviewDto,
  CodeRootsDto,
  CodeUnitDto,
} from "./types";

interface MockImport {
  to: string;
  names: string[];
}

interface MockFile {
  root: string;
  path: string;
  generated?: boolean;
  defines: string[];
  imports: MockImport[];
  external: string[];
}

const G = (path: string, defines: string[], imports: MockImport[] = []): MockFile => ({
  root: "web",
  path,
  generated: true,
  defines,
  imports,
  external: ["zod"],
});

const W = (path: string, defines: string[], imports: MockImport[], external: string[] = []): MockFile => ({
  root: "web",
  path,
  defines,
  imports,
  external,
});

const i = (to: string, ...names: string[]): MockImport => ({ to, names });

const FILES: MockFile[] = [
  W("src/main.tsx", [], [i("src/App.tsx", "App"), i("src/lib/api.ts", "installApi")], ["react", "react-dom"]),
  W("src/App.tsx", ["App"], [i("src/shell/AppShell.tsx", "AppShell"), i("src/auth/AuthProvider.tsx", "AuthProvider")], ["react"]),
  W("src/shell/AppShell.tsx", ["AppShell"], [
    i("src/shell/Sidebar.tsx", "Sidebar"),
    i("src/shell/Header.tsx", "Header"),
    i("src/resources/index.ts", "resources"),
    i("src/crud/ResourcePage.tsx", "ResourcePage"),
  ], ["react", "@mantine/core"]),
  W("src/shell/Sidebar.tsx", ["Sidebar"], [i("src/data/menu.ts", "menu"), i("src/lib/cn.ts", "cn")], ["@mantine/core", "lucide-react"]),
  W("src/shell/Header.tsx", ["Header"], [i("src/auth/useSession.ts", "useSession"), i("src/lib/cn.ts", "cn")], ["@mantine/core"]),
  W("src/auth/AuthProvider.tsx", ["AuthProvider"], [i("src/lib/api.ts", "api"), i("src/auth/tokenStore.ts", "tokenStore")], ["react"]),
  W("src/auth/useSession.ts", ["useSession"], [i("src/auth/AuthProvider.tsx", "AuthContext")], ["react"]),
  W("src/auth/tokenStore.ts", ["tokenStore"], [], []),
  W("src/data/menu.ts", ["menu", "MenuEntry"], [i("src/resources/index.ts", "resources")]),
  W("src/lib/api.ts", ["api", "installApi", "ApiError"], [i("src/auth/tokenStore.ts", "tokenStore")], ["ky"]),
  W("src/lib/cn.ts", ["cn"], [], ["clsx", "tailwind-merge"]),
  W("src/lib/format.ts", ["formatMoney", "formatDate"], [], []),
  W("src/crud/hooks.ts", ["useList", "useRecord", "usePatch", "useCreate"], [
    i("src/lib/api.ts", "api", "ApiError"),
    i("src/crud/queryParams.ts", "toQuery"),
  ], ["@tanstack/react-query"]),
  W("src/crud/queryParams.ts", ["toQuery", "fromQuery"], [], []),
  W("src/crud/ResourcePage.tsx", ["ResourcePage"], [
    i("src/crud/hooks.ts", "useList", "useRecord"),
    i("src/crud/CrudForm.tsx", "CrudForm"),
    i("src/components/datatable/DataTable.tsx", "DataTable"),
  ], ["react"]),
  W("src/crud/CrudForm.tsx", ["CrudForm"], [
    i("src/crud/hooks.ts", "usePatch", "useCreate"),
    i("src/components/form/index.ts", "CurrencyInput", "PhoneInput"),
    i("src/lib/format.ts", "formatMoney"),
  ], ["react-hook-form", "@hookform/resolvers", "@mantine/core"]),
  W("src/crud/columns.tsx", ["columnsFor"], [i("src/lib/format.ts", "formatMoney", "formatDate")], ["@tanstack/react-table"]),
  W("src/components/datatable/DataTable.tsx", ["DataTable"], [
    i("src/crud/columns.tsx", "columnsFor"),
    i("src/components/datatable/Toolbar.tsx", "Toolbar"),
  ], ["@tanstack/react-table", "@mantine/core"]),
  W("src/components/datatable/Toolbar.tsx", ["Toolbar"], [i("src/lib/cn.ts", "cn")], ["@mantine/core"]),
  W("src/components/form/index.ts", ["CurrencyInput", "PhoneInput", "EmailInput"], [
    i("src/components/form/CurrencyInput.tsx", "CurrencyInput"),
    i("src/components/form/PhoneInput.tsx", "PhoneInput"),
    i("src/components/form/EmailInput.tsx", "EmailInput"),
  ]),
  W("src/components/form/CurrencyInput.tsx", ["CurrencyInput"], [i("src/lib/format.ts", "formatMoney")], ["@mantine/core"]),
  W("src/components/form/PhoneInput.tsx", ["PhoneInput"], [], ["@mantine/core"]),
  W("src/components/form/EmailInput.tsx", ["EmailInput"], [], ["@mantine/core"]),
  W("src/resources/index.ts", ["resources", "descriptorFor"], [
    i("src/resources/product/index.ts", "listSchema"),
    i("src/resources/invoice/index.ts", "listSchema"),
    i("src/generated/backbone/index.ts", "entities"),
  ]),
  W("src/resources/product/index.ts", ["schema", "listSchema", "includes"], [
    i("src/generated/backbone/domain/entity/Product.schema.ts", "productSchema"),
  ]),
  W("src/resources/invoice/index.ts", ["schema", "listSchema", "editors"], [
    i("src/generated/backbone/domain/entity/Invoice.schema.ts", "invoiceSchema"),
  ]),
  W("src/desks/people/PeopleDesk.tsx", ["PeopleDesk"], [
    i("src/desks/people/self/SelfAnnouncementsPage.tsx", "SelfAnnouncementsPage"),
    i("src/crud/hooks.ts", "useList"),
  ], ["react", "@mantine/core"]),
  W("src/desks/people/self/SelfAnnouncementsPage.tsx", ["SelfAnnouncementsPage"], [
    i("src/crud/hooks.ts", "useList"),
    i("src/lib/format.ts", "formatDate"),
  ], ["react", "@mantine/core"]),
  W("src/desks/finance/LedgerDesk.tsx", ["LedgerDesk"], [
    i("src/crud/hooks.ts", "useList", "usePatch"),
    i("src/components/datatable/DataTable.tsx", "DataTable"),
    i("src/lib/format.ts", "formatMoney"),
  ], ["react"]),
  G("src/generated/backbone/index.ts", ["entities"], [
    i("src/generated/backbone/domain/entity/Product.schema.ts", "productSchema"),
    i("src/generated/backbone/domain/entity/Invoice.schema.ts", "invoiceSchema"),
    i("src/generated/backbone/domain/entity/Customer.schema.ts", "customerSchema"),
  ]),
  G("src/generated/backbone/domain/entity/Product.schema.ts", ["productSchema", "Product"], [
    i("src/generated/shared/money.ts", "money"),
  ]),
  G("src/generated/backbone/domain/entity/Invoice.schema.ts", ["invoiceSchema", "Invoice"], [
    i("src/generated/shared/money.ts", "money"),
    i("src/generated/backbone/domain/entity/Customer.schema.ts", "customerSchema"),
  ]),
  G("src/generated/backbone/domain/entity/Customer.schema.ts", ["customerSchema", "Customer"]),
  G("src/generated/shared/money.ts", ["money"]),
  {
    root: "service",
    path: "src/main.rs",
    defines: ["main"],
    imports: [i("src/routes.rs", "router"), i("src/config.rs", "Config")],
    external: ["tokio", "axum"],
  },
  { root: "service", path: "src/routes.rs", defines: ["router"], imports: [i("src/handlers/invoice.rs", "list_invoices")], external: ["axum"] },
  { root: "service", path: "src/config.rs", defines: ["Config"], imports: [], external: ["serde"] },
  { root: "service", path: "src/handlers/invoice.rs", defines: ["list_invoices"], imports: [], external: ["sqlx"] },
];

// The people desk is big on purpose: over thirty units in one folder, so
// mock mode draws it in layers, and enough importers of `crud/hooks.ts` that
// its focus view folds past twelve. Built from fixed lists, never at random.
const PEOPLE = "src/desks/people";
const PEOPLE_PAGES = [
  "EmployeeListPage", "EmployeeDetailPage", "RecordChangesPage", "ContractRenewalPage", "ContractHistoryPage",
  "LeaveBalancePage", "LeaveRequestPage", "LeaveApprovalPage", "AttendanceBoardPage", "OvertimeClaimPage",
  "PayslipArchivePage", "BenefitEnrollmentPage", "OrgChartPage", "PositionCatalogPage", "OnboardingChecklistPage",
  "OffboardingChecklistPage", "TrainingPlanPage", "PerformanceReviewPage", "DocumentVaultPage", "AnnouncementComposerPage",
];
const PEOPLE_PARTS = [
  "EmployeeCard", "EmployeePicker", "ContractBadge", "LeaveCalendar", "ApprovalTimeline", "OrgNode",
  "PositionSelect", "ChecklistItem", "ReviewScoreInput", "DocumentPreview", "people-route-guard.test",
  "employee-contract-guard.test", "peopleColumns", "peopleFormats",
];
/** Helpers named `people…` are plain `.ts`; components and tests are `.tsx`. */
const partPath = (name: string) =>
  `${PEOPLE}/${name}.${name.startsWith("people") && !name.endsWith(".test") ? "ts" : "tsx"}`;
const part = (n: number) => {
  const name = PEOPLE_PARTS[n % PEOPLE_PARTS.length] ?? "EmployeeCard";
  return i(partPath(name), name);
};
for (const [k, name] of PEOPLE_PARTS.entries()) {
  FILES.push(
    W(
      partPath(name),
      name.endsWith(".test") ? [] : [name],
      [...(k > 1 ? [part(k - 2)] : []), i("src/lib/cn.ts", "cn")],
      ["@mantine/core"],
    ),
  );
}
for (const [k, name] of PEOPLE_PAGES.entries()) {
  FILES.push(
    W(
      `${PEOPLE}/${name}.tsx`,
      [name],
      [
        i("src/crud/hooks.ts", k % 2 === 0 ? "useList" : "useRecord"),
        part(k),
        part(k * 3 + 1),
        ...(k % 3 === 0 ? [i("src/lib/format.ts", "formatDate")] : []),
        ...(k > 0 && k % 4 === 0 ? [i(`${PEOPLE}/${PEOPLE_PAGES[k - 1]}.tsx`, PEOPLE_PAGES[k - 1] ?? "")] : []),
      ],
      ["react", "@mantine/core"],
    ),
  );
}
FILES.push(W(`${PEOPLE}/payroll/PayrollRunPage.tsx`, ["PayrollRunPage"], [i("src/crud/hooks.ts", "useList"), i(`${PEOPLE}/EmployeePicker.tsx`, "EmployeePicker")], ["react"]));
FILES.push(W(`${PEOPLE}/payroll/SlipLine.tsx`, ["SlipLine"], [i("src/lib/format.ts", "formatMoney")], ["react"]));

// A barrel: the form inputs are reached through `components/form/index.ts`,
// so the file behind it sees CrudForm as a user *via* the barrel.
const VIA: Record<string, Record<string, string>> = {
  "src/components/form/CurrencyInput.tsx": { "src/crud/CrudForm.tsx": "src/components/form/index.ts" },
  "src/components/form/PhoneInput.tsx": { "src/crud/CrudForm.tsx": "src/components/form/index.ts" },
};

function filesOf(root: string): MockFile[] {
  return FILES.filter((f) => f.root === root);
}

export function mockCodeRoots(): CodeRootsDto {
  return {
    roots: [
      { id: "web", repo: "webapp", git_ref: "main", files: filesOf("web").length },
      { id: "service", repo: null, git_ref: null, files: filesOf("service").length },
    ],
    parsed: 0,
    problems: [],
  };
}

export function mockCodeOverview(root: string, rawFolder: string): CodeOverviewDto | null {
  const files = filesOf(root);
  if (files.length === 0) return null;
  const folder = rawFolder.replace(/^\/+|\/+$/g, "");
  const prefix = folder.length === 0 ? "" : `${folder}/`;
  const unitOf = (path: string): { path: string; folder: boolean } | null => {
    if (!path.startsWith(prefix)) return null;
    const rest = path.slice(prefix.length);
    const slash = rest.indexOf("/");
    return slash < 0 ? { path, folder: false } : { path: prefix + rest.slice(0, slash), folder: true };
  };
  const units = new Map<string, CodeUnitDto>();
  for (const f of files) {
    const u = unitOf(f.path);
    if (!u) continue;
    const unit = units.get(u.path) ?? { path: u.path, folder: u.folder, files: 0, generated: 0, inbound: 0, outbound: 0 };
    unit.files += 1;
    if (f.generated) unit.generated += 1;
    units.set(u.path, unit);
  }
  const edges = new Map<string, number>();
  for (const f of files) {
    const from = unitOf(f.path);
    for (const imp of f.imports) {
      const to = unitOf(imp.to);
      if (from && to && from.path !== to.path) {
        const key = `${from.path}\u0000${to.path}`;
        edges.set(key, (edges.get(key) ?? 0) + 1);
      } else if (from && !to) {
        const unit = units.get(from.path);
        if (unit) unit.outbound += 1;
      } else if (!from && to) {
        const unit = units.get(to.path);
        if (unit) unit.inbound += 1;
      }
    }
  }
  return {
    root,
    folder,
    units: [...units.values()].sort((a, b) => (a.path < b.path ? -1 : 1)),
    edges: [...edges.entries()].map(([key, imports]) => {
      const [from = "", to = ""] = key.split("\u0000");
      return { from, to, imports };
    }),
  };
}

function fanIn(root: string, path: string): number {
  return filesOf(root).filter((f) => f.imports.some((imp) => imp.to === path)).length;
}

export function mockCodeNode(name: string): CodeNeighbourhoodDto | null {
  const file = FILES.find((f) => f.path === name) ?? FILES.find((f) => f.defines.includes(name));
  if (!file) return null;
  const byPath = (path: string) => FILES.find((f) => f.root === file.root && f.path === path);
  const users: CodeNeighbourDto[] = filesOf(file.root)
    .filter((f) => f.imports.some((imp) => imp.to === file.path))
    .map((f) => ({
      path: f.path,
      generated: f.generated === true,
      users: fanIn(file.root, f.path),
      names: f.imports.find((imp) => imp.to === file.path)?.names ?? [],
      via: null,
    }));
  // Users that reach the file through a barrel.
  for (const [user, barrel] of Object.entries(VIA[file.path] ?? {})) {
    const f = byPath(user);
    if (f && !users.some((u) => u.path === user)) {
      users.push({ path: user, generated: f.generated === true, users: fanIn(file.root, user), names: file.defines.slice(0, 1), via: barrel });
    }
  }
  const uses: CodeNeighbourDto[] = file.imports.map((imp) => ({
    path: imp.to,
    generated: byPath(imp.to)?.generated === true,
    users: fanIn(file.root, imp.to),
    names: imp.names,
    via: null,
  }));
  return {
    root: file.root,
    path: file.path,
    generated: file.generated === true,
    defines: file.defines,
    users,
    uses,
    external: file.external,
  };
}
