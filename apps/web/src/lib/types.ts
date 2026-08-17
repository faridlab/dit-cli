// Types mirroring the server API one-to-one. Field names must never drift
// from the wire format — renaming here silently breaks requests. The
// vocabulary is the glossary: `assignees` (plural), `seq`, `short_ref`,
// priorities p0..p4, types task/bug/story/spike/chore.

export type IssueType = "task" | "bug" | "story" | "spike" | "chore";
export type Priority = "p0" | "p1" | "p2" | "p3" | "p4";
export type StatusCategory = "todo" | "doing" | "done";

export interface IssueDto {
  id: string;
  short_ref: string;
  title: string;
  type: IssueType;
  status: string;
  priority: Priority | null;
  reporter: string | null;
  assignees: string[];
  labels: string[];
  epic: string | null;
  estimate: number | null;
  sprint: string | null;
  due: string | null;
  created: string;
  updated: string;
  body: string;
  body_html: string;
}

export interface CommentDto {
  id: string;
  issue_id: string;
  author: string;
  created: string;
  body: string;
  body_html: string;
}

export interface FieldEventDto {
  seq: number;
  field: string;
  old_value: string | null;
  new_value: string | null;
  author: string;
  ts: string;
  commit_sha: string;
}

export interface StatusDto {
  id: string;
  label: string;
  category: StatusCategory;
  terminal?: boolean;
  wip_limit?: number;
}

export interface SchemaDto {
  workflow: {
    statuses: StatusDto[];
    transitions: Array<{ from: string[]; to: string }>;
    derived: Array<{ on: string; implies: string }>;
  };
}

export interface BoardIssueDto {
  id: string;
  short_ref: string;
  title: string;
  priority: Priority | null;
  type: IssueType;
  assignees: string[];
  labels: string[];
  estimate: number | null;
  updated: string;
}

// Flat on purpose: the stray "not in workflow" column has no StatusDto
// behind it, so the wire sends the id and label directly. Categories live
// in the schema this client already fetched.
export interface BoardColumnDto {
  id: string;
  label: string;
  wip_limit: number | null;
  issues: BoardIssueDto[];
}

export interface BoardDto {
  columns: BoardColumnDto[];
}

export interface StatusInfo {
  ok: boolean;
  version: string;
  repo: string;
  branch: string;
  head: string | null;
  dirty: boolean;
  me: string | null;
}

export interface IssueListDto {
  total: number;
  items: IssueDto[];
}

// The set of fields the PATCH endpoint accepts inside { set: ... }. Absent
// fields are untouched — v0.1 has no way to clear a field, by design.
export interface FieldPatch {
  title?: string;
  status?: string;
  priority?: string;
  type?: string;
  assignees?: string[];
  labels?: string[];
  reporter?: string;
  estimate?: number;
  sprint?: string;
  due?: string;
}

export interface NewIssueInput {
  title: string;
  type?: string;
  status?: string;
  priority?: string;
  labels?: string[];
  assignees?: string[];
  body?: string;
}
