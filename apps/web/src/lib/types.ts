// The wire shapes live in ./schema/ — generated from the Rust DTOs by
// ts-rs (`cargo test -p dit-server` regenerates them; CI fails on drift).
// This file keeps only what generation cannot express: the union aliases
// for values the server deliberately sends as plain strings, and the
// narrowings of those fields that consumers rely on. Field names still
// follow the glossary — `assignees`, `seq`, `short_ref` — on both sides.

export type IssueType = "task" | "bug" | "story" | "spike" | "chore";
export type Priority = "p0" | "p1" | "p2" | "p3" | "p4";
export type StatusCategory = "todo" | "doing" | "done";
export type Layout = "root" | "dotdir";
export type NumberingPolicy = "local" | "on-merge";

import type { BoardColumnDto as WireBoardColumnDto } from "./schema/BoardColumnDto";
import type { BoardDto as WireBoardDto } from "./schema/BoardDto";
import type { BoardIssueDto as WireBoardIssueDto } from "./schema/BoardIssueDto";
import type { CommentDto as WireCommentDto } from "./schema/CommentDto";
import type { DerivedDto } from "./schema/DerivedDto";
import type { DocBodyDto as WireDocBodyDto } from "./schema/DocBodyDto";
import type { DocEntryDto as WireDocEntryDto } from "./schema/DocEntryDto";
import type { FieldEventDto as WireFieldEventDto } from "./schema/FieldEventDto";
import type { FieldPatchDto } from "./schema/FieldPatchDto";
import type { IssueDto as WireIssueDto } from "./schema/IssueDto";
import type { IssueListDto as WireIssueListDto } from "./schema/IssueListDto";
import type { NewIssueDto } from "./schema/NewIssueDto";
import type { SchemaDto as WireSchemaDto } from "./schema/SchemaDto";
import type { SetSettingsDto } from "./schema/SetSettingsDto";
import type { SettingsDto as WireSettingsDto } from "./schema/SettingsDto";
import type { StatusDto as WireStatusDto } from "./schema/StatusDto";
import type { StatusInfo as WireStatusInfo } from "./schema/StatusInfo";
import type { TransitionDto } from "./schema/TransitionDto";

// The unions above narrow the generated `string` fields for consumers; the
// generated base still pins every field name and shape, so a wire change
// that touches anything else is a compile error here, not a runtime bug.
export interface IssueDto extends WireIssueDto {
  type: IssueType;
  priority: Priority | null;
}

export type CommentDto = WireCommentDto;
export type FieldEventDto = WireFieldEventDto;

// §13 pages: plain Markdown under the doc roots; `updated_ms` is display
// metadata (the file's mtime), the real history is git.
export type DocEntryDto = WireDocEntryDto;
export type DocBodyDto = WireDocBodyDto;

export interface StatusDto extends WireStatusDto {
  category: StatusCategory;
}

export interface SchemaDto extends WireSchemaDto {
  workflow: {
    statuses: StatusDto[];
    transitions: TransitionDto[];
    derived: DerivedDto[];
  };
}

export interface BoardIssueDto extends WireBoardIssueDto {
  type: IssueType;
  priority: Priority | null;
}

// Flat on purpose: the stray "not in workflow" column has no StatusDto
// behind it, so the wire sends the id and label directly. Categories live
// in the schema this client already fetched.
export interface BoardColumnDto extends WireBoardColumnDto {
  issues: BoardIssueDto[];
}

export interface BoardDto extends WireBoardDto {
  columns: BoardColumnDto[];
}

export type StatusInfo = WireStatusInfo;

export interface SettingsDto extends WireSettingsDto {
  layout: Layout;
  numbering: NumberingPolicy;
}

// Absent fields are untouched — the same contract as the issue patch.
export type SetSettingsInput = SetSettingsDto;

export interface IssueListDto extends WireIssueListDto {
  items: IssueDto[];
}

// The set of fields the PATCH endpoint accepts inside { set: ... }. Absent
// fields are untouched — v0.1 has no way to clear a field, by design.
export type FieldPatch = FieldPatchDto;

export type NewIssueInput = NewIssueDto;
