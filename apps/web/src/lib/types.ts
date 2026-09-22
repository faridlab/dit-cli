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

import type { ActivityEventDto as WireActivityEventDto } from "./schema/ActivityEventDto";
import type { ActivityPageDto as WireActivityPageDto } from "./schema/ActivityPageDto";
import type { ActivitySummaryDto as WireActivitySummaryDto } from "./schema/ActivitySummaryDto";
import type { BoardColumnDto as WireBoardColumnDto } from "./schema/BoardColumnDto";
import type { CategoryCountsDto as WireCategoryCountsDto } from "./schema/CategoryCountsDto";
import type { ChangeSummaryDto as WireChangeSummaryDto } from "./schema/ChangeSummaryDto";
import type { DayCountDto as WireDayCountDto } from "./schema/DayCountDto";
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
import type { ReleaseDto as WireReleaseDto } from "./schema/ReleaseDto";
import type { ReleasePatchDto } from "./schema/ReleasePatchDto";
import type { WorkspaceCommentDto as WireWorkspaceCommentDto } from "./schema/WorkspaceCommentDto";

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
    lanes: LaneDtoWire[];
    coordination: CoordinationDtoWire;
  };
}

export type LaneDtoWire = WireSchemaDto["workflow"]["lanes"][number];
export type CoordinationDtoWire = WireSchemaDto["workflow"]["coordination"];

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

// The workspace activity feed and the time-travel summary. Both are read
// models: every number in them is recomputed from `field_events` on the
// server, never stored (invariant 5).
export type ActivityEventDto = WireActivityEventDto;
export type ActivityPageDto = WireActivityPageDto;
export type ActivitySummaryDto = WireActivitySummaryDto;
export type CategoryCountsDto = WireCategoryCountsDto;
export type ChangeSummaryDto = WireChangeSummaryDto;
export type DayCountDto = WireDayCountDto;

// A comment with enough of its issue to render in the workspace Timeline
// without a second request per row.
export type WorkspaceCommentDto = WireWorkspaceCommentDto;

// Releases (DESIGN.md §15.2): one folder per version under `.dit/releases/`.
// `target` is the planned date the roadmap draws the milestone at; whether
// the work actually shipped is a question git answers, never a field.
export type ReleaseStatus = "planned" | "in_dev" | "in_uat" | "released" | "rolled_back";

export interface ReleaseDto extends WireReleaseDto {
  status: ReleaseStatus;
}

export type ReleasePatchInput = ReleasePatchDto;

// The flow diagram (ADR 0019 wire types, verbatim from the server).
export type { FlowBoardDto } from "./schema/FlowBoardDto";
export type { FlowClaimDto } from "./schema/FlowClaimDto";
export type { FlowEdgeDto } from "./schema/FlowEdgeDto";
export type { FlowGroupDto } from "./schema/FlowGroupDto";
export type { FlowLaneDto } from "./schema/FlowLaneDto";
export type { FlowPhaseDto } from "./schema/FlowPhaseDto";
export type { FlowShapeProblemDto } from "./schema/FlowShapeProblemDto";
export type { FlowNodeDto } from "./schema/FlowNodeDto";
export type { FlowOutsideBlockerDto } from "./schema/FlowOutsideBlockerDto";
export type { FlowSummaryDto } from "./schema/FlowSummaryDto";
