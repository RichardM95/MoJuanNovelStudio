import type {
  Material,
  MaterialSummary,
  CreateMaterialInput,
  SaveMaterialInput,
} from "./materialTypes";

export type OccurrenceRole = "plant" | "hint" | "payoff";
export type SourceStatus = "current" | "stale" | "unavailable";
export type SourcePosition = {
  chapter_id: string;
  chapter_title: string;
  chapter_revision: number;
  chapter_sort_order: number;
  volume_id: string;
  volume_title: string;
  volume_sort_order: number;
};
export type SelectedExcerpt = { start: number; end: number; quote: string };
export type Occurrence = SelectedExcerpt & {
  id: string;
  book_id: string;
  material_id: string;
  source: SourcePosition;
  role: OccurrenceRole;
  note: string;
  revision: number;
  deleted_at: number | null;
  created_at: number;
};
export type OccurrenceView = {
  occurrence: Occurrence;
  source_status: SourceStatus;
  current: SourcePosition | null;
};
export type ForeshadowSummary = {
  material: MaterialSummary;
  occurrence_count: number;
  review_count: number;
};
export type OccurrenceInput = SelectedExcerpt & {
  book_id: string;
  material_id: string;
  chapter_id: string;
  chapter_revision: number;
  role: OccurrenceRole;
  note: string;
  epoch: string;
  request_key: string;
};
export type SavedSourceChapter = { position: SourcePosition; body: string };

export type SourceWindow = { position: SourcePosition; before: string; at_range: string; after: string };
export type ForeshadowSourceReader = {
  epoch(): Promise<string>;
  sourceWindow(book: string, chapter: string, epoch: string, start: number, end: number): Promise<SourceWindow>;
};

export interface ForeshadowAPI {
  sourceEpoch?: ForeshadowSourceReader["epoch"];
  sourceWindow?: ForeshadowSourceReader["sourceWindow"];
  proposals?: ForeshadowProposalAPI;
  epoch(): Promise<string>;
  overview(book: string, epoch: string): Promise<ForeshadowSummary[]>;
  directory(book: string, epoch: string): Promise<SourcePosition[]>;
  chapter(
    book: string,
    chapter: string,
    epoch: string,
  ): Promise<SavedSourceChapter>;
  list(
    book: string,
    material: string,
    epoch: string,
  ): Promise<OccurrenceView[]>;
  get(
    book: string,
    material: string,
    id: string,
    epoch: string,
  ): Promise<OccurrenceView>;
  lookup(
    book: string,
    material: string,
    key: string,
    epoch: string,
  ): Promise<OccurrenceView | null>;
  create(input: OccurrenceInput): Promise<OccurrenceView>;
  setDeleted(
    book: string,
    material: string,
    id: string,
    expectedRevision: number,
    epoch: string,
    deleted: boolean,
  ): Promise<OccurrenceView>;
  material(book: string, id: string, epoch: string): Promise<Material>;
  createCard(input: CreateMaterialInput, epoch: string): Promise<Material>;
  saveCard(
    book: string,
    input: SaveMaterialInput,
    epoch: string,
  ): Promise<Material>;
}

export const occurrenceRoles: Record<OccurrenceRole, string> = {
  plant: "铺设",
  hint: "提示",
  payoff: "回收片段",
};

/** A textarea normalizes CRLF/CR to LF and reports UTF-16 boundaries.
 * Translate both offsets back to the exact saved text and count Unicode scalars.
 */
export function exactExcerpt(
  raw: string,
  visible: string,
  start: number,
  end: number,
): SelectedExcerpt {
  if (
    !Number.isInteger(start) ||
    !Number.isInteger(end) ||
    start < 0 ||
    end <= start ||
    end > visible.length
  )
    throw Error("请在已保存的正文中选择非空片段");
  if (raw.replace(/\r\n?/g, "\n") !== visible)
    throw Error("当前显示与保存正文不一致，请重新读取");
  let normalized = 0,
    scalar = 0;
  let first: { raw: number; scalar: number } | undefined;
  for (let i = 0; i <= raw.length;) {
    if (normalized === start) first = { raw: i, scalar };
    if (normalized === end && first) {
      const quote = raw.slice(first.raw, i);
      if (new TextEncoder().encode(quote).length > 4096)
        throw Error("引文最多 4096 UTF-8 字节");
      return { start: first.scalar, end: scalar, quote };
    }
    if (normalized > end || (!first && normalized > start) || i === raw.length)
      break;
    if (raw[i] === "\r" && raw[i + 1] === "\n") {
      i += 2;
      scalar += 2;
      normalized += 1;
    } else {
      const point = raw.codePointAt(i)!;
      if (point >= 0xd800 && point <= 0xdfff) break;
      const width = point > 0xffff ? 2 : 1;
      i += width;
      normalized += width;
      scalar++;
    }
  }
  throw Error("不能在 Unicode 字符中间选取边界");
}

export type ForeshadowProposalStatus = "pending" | "accepted" | "rejected" | "cancelled";
export type ForeshadowProposalReceipt = {
  id: string;
  book_id: string;
  material_id: string;
  material_revision: number;
  chapter_id: string;
  chapter_revision: number;
  status: ForeshadowProposalStatus;
  created_at: number;
  decided_at: number | null;
  accepted_occurrence_id: string | null;
};
export type ForeshadowProposalSummary = {
  receipt: ForeshadowProposalReceipt;
  material_title: string;
  archived: boolean;
};
export type ForeshadowProposal = ForeshadowProposalSummary & SelectedExcerpt & {
  source: SourcePosition;
  source_status: SourceStatus;
  card_current: boolean;
  role: OccurrenceRole;
  note: string;
  reason: string;
  accepted_occurrence: OccurrenceView | null;
};
export type DecideForeshadowProposalInput = {
  book_id: string;
  material_id: string;
  proposal_id: string;
  expected_epoch: string;
  expected_material_revision: number;
  action: "accept" | "reject";
};
export type ForeshadowProposalPage = {
  items: ForeshadowProposalSummary[];
  pending_count: number;
  next_after_id: string | null;
};
export interface ForeshadowProposalAPI {
  list(book: string, material: string | null, pendingOnly: boolean, after: string | null, epoch: string): Promise<ForeshadowProposalPage>;
  get(book: string, material: string, id: string, epoch: string): Promise<ForeshadowProposal>;
  decide(input: DecideForeshadowProposalInput): Promise<ForeshadowProposal>;
}
