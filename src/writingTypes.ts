import type { Book, Chapter, Volume } from "./types";
import type { Material } from "./materialTypes";
export type WritingPolicy = {
  chapter_id: string;
  book_id: string;
  revision: number;
  mode: "active" | "paused" | "disabled";
  goal: string;
  required_material_ids: string[];
  context_chapter_ids?: string[];
  updated_at: number;
};
export type WritingManifest = {
  id: string;
  book_id: string;
  chapter_id: string;
  policy_revision: number;
  chapter_revision: number;
  goal: string;
  expires_at: number;
  archived: boolean;
  entries: {
    id: string;
    title: string;
    kind: string;
    revision: number;
    fields: string[];
    metadata?: Record<string, unknown>;
  }[];
};
export type FrozenContextChapter = { chapter: Chapter; volume: Volume };
export type WritingContextDirectory = {
  epoch: string;
  book_id: string;
  chapter_id: string;
  volumes: Volume[];
  chapters: Pick<
    Chapter,
    "id" | "title" | "volume_id" | "revision" | "sort_order"
  >[];
};
export type WritingPack = {
  handle_version?: number;
  id: string;
  book_id: string;
  chapter_id: string;
  originating_epoch: string;
  created_at: number;
  expires_at: number;
  policy: WritingPolicy;
  book: Book;
  volume: Volume;
  chapter: Chapter;
  materials: Material[];
  context_chapters?: FrozenContextChapter[];
  foreshadow_occurrences?: import("./foreshadowTypes").Occurrence[];
};
export type AuthorReviewVerdict =
  "unreviewed" | "conforms" | "question" | "conflict";
export type AuthorReviewItem = {
  material_id: string;
  verdict: AuthorReviewVerdict;
  note: string;
};
export type AuthorReview = {
  proposal_id: string;
  pack_id: string;
  book_id: string;
  chapter_id: string;
  originating_epoch: string;
  policy_revision: number;
  chapter_revision: number;
  revision: number;
  items: AuthorReviewItem[];
};
export type WritingProposal = {
  id: string;
  book_id: string;
  chapter_id: string;
  client_id: string;
  input: {
    pack_id: string;
    new_body: string;
    idempotency_key: string;
    citations: {
      entry_id: string;
      field_key: string;
      start: number;
      end: number;
      explanation: string;
    }[];
    conflicts: { entry_ids: string[]; explanation: string }[];
  };
  base_revision: number;
  decision: string;
  live_epoch: string | null;
  accepted_revision: number | null;
  author_review?: AuthorReview | null;
};
export type WritingState = {
  epoch: string;
  policy: WritingPolicy | null;
  packs: Pick<
    WritingManifest,
    "id" | "policy_revision" | "chapter_revision" | "expires_at" | "archived"
  >[];
  proposals: {
    id: string;
    pack_id: string;
    decision: string;
    archived: boolean;
    created_at: number;
  }[];
};
export type WritingPredecessorMetadata = Pick<Chapter, "id" | "title" | "revision" | "sort_order"> & {
  volume: Volume;
};
export type WritingPredecessorDiagnosis = {
  epoch: string;
  book_id: string;
  chapter_id: string;
  pack_id: string;
  target: WritingPredecessorMetadata;
  entries: {
    chapter_id: string;
    position: "before" | "after" | "not_visible";
    current: WritingPredecessorMetadata | null;
  }[];
};
export interface WritingAPI {
  epoch(): Promise<string>;
  predecessorDiagnosis?: {
    epoch(): Promise<string>;
    read(book: string, chapter: string, pack: string, epoch: string): Promise<WritingPredecessorDiagnosis>;
  };
  state(book: string, chapter: string): Promise<WritingState>;
  contextDirectory(
    book: string,
    chapter: string,
  ): Promise<WritingContextDirectory>;
  configure(
    book: string,
    chapter: string,
    expected: number | null,
    mode: WritingPolicy["mode"],
    goal: string,
    ids: string[],
    epoch: string,
    contextChapterIds: string[],
  ): Promise<WritingPolicy>;
  prepare(
    book: string,
    chapter: string,
    policyRevision: number,
    chapterRevision: number,
    epoch: string,
  ): Promise<WritingManifest>;
  pack(book: string, id: string): Promise<WritingPack>;
  proposal(book: string, id: string): Promise<WritingProposal>;
  saveReview(
    book: string,
    id: string,
    expectedRevision: number | null,
    epoch: string,
    items: AuthorReviewItem[],
  ): Promise<AuthorReview>;
  acceptReviewed(
    book: string,
    id: string,
    policyRevision: number,
    chapterRevision: number,
    epoch: string,
    expectedReviewRevision: number,
    conflictsAck: boolean,
  ): Promise<WritingProposal>;
  decide(
    book: string,
    id: string,
    approve: boolean,
    policyRevision: number,
    chapterRevision: number,
    epoch: string,
    conflictsAck: boolean,
  ): Promise<WritingProposal>;
}

/** A chapter read may be published only after the caller finishes epoch and body checks. */
export type WritingChapterRead = {
  chapter: Chapter;
  publish?: () => void;
};
