import type { Chapter } from "./types";
import type {
  AuthorReview,
  WritingPack,
  WritingProposal,
} from "./writingTypes";

type Input = WritingProposal["input"];
export type DecisionAction = "accept" | "reject";
export type TerminalDecision = "accepted" | "rejected" | "cancelled";
export type DecisionAttempt = {
  epoch: string;
  bookId: string;
  chapterId: string;
  id: string;
  clientId: string;
  input: Input;
  baseRevision: number;
  action: DecisionAction;
  policyRevision: number;
  chapterRevision: number;
  frozenPolicyRevision: number;
  requiredMaterialIds: string[];
  confirmedReview: AuthorReview | null;
  conflictsAck: boolean;
};
export type TerminalEvidence = {
  decision: TerminalDecision;
  acceptedRevision: number | null;
  reviewRevision: number | null;
};
type DecisionProof =
  | { kind: "mismatch" | "pending" }
  | { kind: "terminal"; evidence: TerminalEvidence };
export type DecisionTarget = Pick<
  DecisionAttempt,
  "epoch" | "bookId" | "chapterId" | "id" | "action"
>;
type DecisionRecordBase = {
  key: string;
  generation: number;
  retired: boolean;
};
export type DecisionEntry = DecisionRecordBase & {
  kind: "attempt";
  target: DecisionAttempt;
  candidates: Partial<Record<"write" | "full-read", TerminalEvidence>>;
  bytes: number;
};
export type DecisionMarker = DecisionRecordBase & {
  kind: "terminal";
  target: DecisionTarget;
  evidence: TerminalEvidence;
  fingerprint: string | null;
  bodyChecked: boolean;
};
export type DecisionRecord = DecisionEntry | DecisionMarker;

const encoder = new TextEncoder();
const MAX_REVIEW_REVISION = 9_007_199_254_740_000;
const verdicts = new Set(["unreviewed", "conforms", "question", "conflict"]);
const MAX_TARGETS = 32;
const MAX_BYTES = 32 * 1024 * 1024;
// The terminal summary stores only fixed decision strings and two bounded
// numbers, not a second copy of the input, review notes, or chapter.
const TERMINAL_BYTES = 256;
// Rust admits at most 1000 strict proposals per library. Reserve a tombstone
// slot for every active attempt, without evicting at pressure. A separate
// allowance preserves old-library compact evidence with unverified chapters.
const MAX_ACTIVE_MARKERS = 1000;
const MAX_RECORDS = 2 * MAX_ACTIVE_MARKERS;

const sequence = <T>(a: unknown, b: T[], eq: (x: T, y: T) => boolean) =>
  Array.isArray(a) && a.length === b.length && b.every((v, i) => eq(a[i], v));

function sameInput(input: Input, expected: Input) {
  return (
    input?.pack_id === expected.pack_id &&
    input.new_body === expected.new_body &&
    input.idempotency_key === expected.idempotency_key &&
    sequence(
      input.citations,
      expected.citations,
      (a, b) =>
        !!a &&
        a.entry_id === b.entry_id &&
        a.field_key === b.field_key &&
        a.start === b.start &&
        a.end === b.end &&
        a.explanation === b.explanation,
    ) &&
    sequence(
      input.conflicts,
      expected.conflicts,
      (a, b) =>
        !!a &&
        a.explanation === b.explanation &&
        sequence(a.entry_ids, b.entry_ids, (x, y) => x === y),
    )
  );
}

function validReview(r: AuthorReview, a: DecisionAttempt) {
  return (
    r?.proposal_id === a.id &&
    r.pack_id === a.input.pack_id &&
    r.book_id === a.bookId &&
    r.chapter_id === a.chapterId &&
    r.originating_epoch === a.epoch &&
    r.policy_revision === a.frozenPolicyRevision &&
    r.chapter_revision === a.baseRevision &&
    Number.isSafeInteger(r.revision) &&
    r.revision > 0 &&
    r.revision < MAX_REVIEW_REVISION &&
    Array.isArray(r.items) &&
    r.items.length === a.requiredMaterialIds.length &&
    a.requiredMaterialIds.every((id) =>
      r.items.some(
        (i) =>
          i?.material_id === id &&
          verdicts.has(i.verdict) &&
          typeof i.note === "string" &&
          encoder.encode(i.note).length <= 4096,
      ),
    ) &&
    encoder.encode(JSON.stringify(r)).length <= 64 * 1024
  );
}

function copyInput(input: Input): Input {
  return {
    pack_id: input.pack_id,
    new_body: input.new_body,
    idempotency_key: input.idempotency_key,
    citations: input.citations.map(
      ({ entry_id, field_key, start, end, explanation }) => ({
        entry_id,
        field_key,
        start,
        end,
        explanation,
      }),
    ),
    conflicts: input.conflicts.map(({ entry_ids, explanation }) => ({
      entry_ids: [...entry_ids],
      explanation,
    })),
  };
}

export function captureDecision(
  q: WritingProposal,
  pack: WritingPack,
  context: Pick<
    DecisionAttempt,
    | "epoch"
    | "bookId"
    | "chapterId"
    | "action"
    | "policyRevision"
    | "chapterRevision"
    | "conflictsAck"
  >,
  confirmedReviewRevision: number | null,
): DecisionAttempt {
  const r = context.action === "accept" ? q.author_review : null;
  const attempt: DecisionAttempt = {
    ...context,
    id: q.id,
    clientId: q.client_id,
    baseRevision: q.base_revision,
    input: copyInput(q.input),
    frozenPolicyRevision: pack.policy.revision,
    requiredMaterialIds: [...pack.policy.required_material_ids],
    confirmedReview: r
      ? {
          proposal_id: r.proposal_id,
          pack_id: r.pack_id,
          book_id: r.book_id,
          chapter_id: r.chapter_id,
          originating_epoch: r.originating_epoch,
          policy_revision: r.policy_revision,
          chapter_revision: r.chapter_revision,
          revision: r.revision,
          items: r.items.map(({ material_id, verdict, note }) => ({
            material_id,
            verdict,
            note,
          })),
        }
      : null,
  };
  if (
    q.book_id !== attempt.bookId ||
    q.chapter_id !== attempt.chapterId ||
    q.decision !== "pending" ||
    q.accepted_revision !== null ||
    q.live_epoch !== attempt.epoch ||
    pack.originating_epoch !== attempt.epoch ||
    pack.id !== attempt.input.pack_id ||
    !q.id ||
    !q.client_id ||
    !Number.isSafeInteger(q.base_revision) ||
    q.base_revision < 0 ||
    q.base_revision !== pack.chapter.revision ||
    (attempt.action === "accept" &&
      (attempt.policyRevision !== attempt.frozenPolicyRevision ||
        attempt.chapterRevision !== attempt.baseRevision ||
        !r ||
        !validReview(r, attempt) ||
        r.revision !== confirmedReviewRevision ||
        (attempt.input.conflicts.length > 0 && !attempt.conflictsAck) ||
        !r.items.length ||
        r.items.some((item) => item.verdict !== "conforms")))
  )
    throw Error("提案或逐项审阅已变，未提交正文决定；请重新读取。");
  return attempt;
}

export function decisionProof(
  q: WritingProposal,
  a: DecisionAttempt,
): DecisionProof {
  if (
    q?.id !== a.id ||
    q.book_id !== a.bookId ||
    q.chapter_id !== a.chapterId ||
    q.client_id !== a.clientId ||
    q.base_revision !== a.baseRevision ||
    q.live_epoch !== a.epoch ||
    !sameInput(q.input, a.input) ||
    (q.author_review != null && !validReview(q.author_review, a))
  )
    return { kind: "mismatch" };

  const r = q.author_review;
  if (q.decision === "accepted") {
    if (
      !Number.isSafeInteger(q.accepted_revision) ||
      q.accepted_revision !== a.baseRevision + 1 ||
      !r ||
      !r.items.length ||
      r.items.some((item) => item.verdict !== "conforms") ||
      (a.action === "accept" &&
        (!a.confirmedReview ||
          r.revision !== a.confirmedReview.revision ||
          !a.confirmedReview.items.every((i) =>
            r.items.some(
              (j) =>
                i.material_id === j.material_id &&
                i.verdict === j.verdict &&
                i.note === j.note,
            ),
          )))
    )
      return { kind: "mismatch" };
  } else {
    if (q.accepted_revision !== null) return { kind: "mismatch" };
    if (q.decision === "pending") return { kind: "pending" };
    if (q.decision !== "rejected" && q.decision !== "cancelled")
      return { kind: "mismatch" };
  }

  return {
    kind: "terminal",
    evidence: {
      decision: q.decision,
      acceptedRevision: q.accepted_revision,
      reviewRevision: r?.revision ?? null,
    },
  };
}

export const decisionKey = (
  epoch: string,
  book: string,
  chapter: string,
  id: string,
) => JSON.stringify([epoch, book, chapter, id]);

export const requestedDecision = (action: DecisionAction) =>
  action === "accept" ? "accepted" : "rejected";

export function terminalLabel(e: TerminalEvidence, a: DecisionTarget) {
  const decision =
    e.decision === "accepted"
      ? `已采纳（正文版本 ${e.acceptedRevision}）`
      : e.decision === "rejected"
        ? "已拒绝"
        : "已取消";
  return e.decision === requestedDecision(a.action)
    ? decision
    : `已由其他决定处理：${decision}；并非本次${a.action === "accept" ? "采纳" : "拒绝"}。`;
}

export function acceptedBody(
  c: Chapter,
  a: DecisionTarget,
  revision: number,
  newBody: string,
) {
  if (c?.id !== a.chapterId || c.deleted_at !== null)
    throw Error("章节已删除、不可用或归属不匹配");
  if (!Number.isSafeInteger(c.revision) || c.revision < revision)
    throw Error("当前章节版本未能达到已采纳版本");
  if (c.revision === revision && c.body !== newBody)
    throw Error("同版本正文不匹配，未验证当前正文");
  return c.revision === revision ? "exact" : "later";
}

/** The fingerprint seals the exact authenticated payload when the attempt's
 * manuscript is released. Optional legacy timestamps aren't safety fields. */
export async function decisionFingerprint(q: WritingProposal): Promise<string> {
  const r = q.author_review;
  const payload = JSON.stringify([
    q.id,
    q.book_id,
    q.chapter_id,
    q.client_id,
    q.base_revision,
    q.live_epoch,
    q.decision,
    q.accepted_revision,
    copyInput(q.input),
    r == null
      ? null
      : [
          r.proposal_id,
          r.pack_id,
          r.book_id,
          r.chapter_id,
          r.originating_epoch,
          r.policy_revision,
          r.chapter_revision,
          r.revision,
          r.items.map((i) => [i.material_id, i.verdict, i.note]),
        ],
  ]);
  const bytes = await globalThis.crypto.subtle.digest(
    "SHA-256",
    encoder.encode(payload),
  );
  return Array.from(new Uint8Array(bytes), (v) =>
    v.toString(16).padStart(2, "0"),
  ).join("");
}

export function matchesTerminal(q: WritingProposal, marker: DecisionMarker) {
  const { target: a, evidence: e } = marker;
  return (
    q?.id === a.id &&
    q.book_id === a.bookId &&
    q.chapter_id === a.chapterId &&
    q.live_epoch === a.epoch &&
    q.decision === e.decision &&
    q.accepted_revision === e.acceptedRevision &&
    (q.author_review?.revision ?? null) === e.reviewRevision
  );
}

/** App-owned unknown attempts and compact authenticated tombstones. Retiring
 * complete old-epoch markers frees the replay cache; unresolved old evidence
 * remains accessible and can only be cleared by explicit user action. */
export type ReviewNoteScope = {
  epoch: string;
  bookId: string;
  chapterId: string;
};
type ReviewNoteCopy = Readonly<ReviewNoteScope & {
  generation: number;
  proposalId: string;
  packId: string;
  text: string;
}>;
const MAX_REVIEW_NOTE_COPY_BYTES = 64 * 1024;

export class WritingDecisionSession {
  private reviewNoteCopy: ReviewNoteCopy | null = null;

  retainReviewNotes(scope: ReviewNoteScope, proposalId: string, packId: string, text: string) {
    const value: ReviewNoteCopy = { ...scope, generation: this.generation, proposalId, packId, text };
    if (text.length > MAX_REVIEW_NOTE_COPY_BYTES ||
      new TextEncoder().encode(JSON.stringify(value)).length > MAX_REVIEW_NOTE_COPY_BYTES)
      return "too_large";
    const old = this.reviewNoteCopy;
    if (old && (old.generation !== value.generation || old.epoch !== value.epoch ||
      old.bookId !== value.bookId || old.chapterId !== value.chapterId ||
      old.proposalId !== proposalId || old.packId !== packId || old.text !== text))
      return "occupied";
    this.reviewNoteCopy = Object.freeze(value);
    this.change();
    return "retained";
  }

  readReviewNotes(scope: ReviewNoteScope) {
    const copy = this.reviewNoteCopy;
    return copy?.generation === this.generation && copy.epoch === scope.epoch &&
      copy.bookId === scope.bookId && copy.chapterId === scope.chapterId ? copy : null;
  }

  hasReviewNoteCopy() { return this.reviewNoteCopy !== null; }

  clearReviewNoteCopy() {
    this.reviewNoteCopy = null;
    this.change();
  }

  private attempts = new Map<string, DecisionEntry>();
  private markers = new Map<string, DecisionMarker>();
  private bytes = 0;
  private version = 0;
  private generation = 0;
  private listeners = new Set<() => void>();

  readonly subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  readonly snapshot = () => this.version;
  get libraryGeneration() {
    return this.generation;
  }
  entries() {
    return [...this.attempts.values()];
  }
  recoveries(): DecisionRecord[] {
    return [
      ...this.attempts.values(),
      ...[...this.markers.values()].filter((r) => !r.bodyChecked),
    ];
  }
  get(key: string) {
    return this.attempts.get(key);
  }
  terminal(key: string) {
    return this.markers.get(key);
  }
  has(r: DecisionRecord) {
    return (
      (r.kind === "attempt" ? this.get(r.key) : this.terminal(r.key)) === r
    );
  }
  isOld(r: DecisionRecord) {
    return r.generation !== this.generation || r.retired;
  }

  private change() {
    this.version++;
    this.listeners.forEach((listener) => listener());
  }
  private retireMarkers(obsolete: (r: DecisionMarker) => boolean) {
    let changed = false;
    for (const r of this.markers.values()) {
      if (!r.retired && obsolete(r)) {
        if (r.bodyChecked) this.markers.delete(r.key);
        else r.retired = true;
        changed = true;
      }
    }
    return changed;
  }
  observeEpoch(epoch: string) {
    let changed = this.retireMarkers(
      (r) => this.isOld(r) || r.target.epoch !== epoch,
    );
    for (const r of this.attempts.values()) {
      if (!this.isOld(r) && r.target.epoch !== epoch) {
        r.retired = true;
        changed = true;
      }
    }
    if (changed) this.change();
  }
  invalidateLibrary() {
    this.generation++;
    this.retireMarkers((r) => this.isOld(r));
    this.change();
  }
  admit(a: DecisionAttempt) {
    const key = decisionKey(a.epoch, a.bookId, a.chapterId, a.id);
    // Service identities are short. Even a malformed bridge cannot migrate
    // an oversized scope string out of the attempt budget into a marker.
    if (encoder.encode(key).byteLength > 2048)
      throw Error("正文决定的目标身份超限，未提交新决定。");
    if (this.get(key)) throw Error("该建议已有决定仍待核对，未重发。");
    if (this.terminal(key)) throw Error("已独立核对该建议的终态，未重发。");
    const r: DecisionEntry = {
      kind: "attempt",
      target: a,
      key,
      generation: this.generation,
      retired: false,
      candidates: {},
      bytes: 0,
    };
    r.bytes = encoder.encode(JSON.stringify(r)).length + TERMINAL_BYTES;
    if (this.attempts.size >= MAX_TARGETS || this.bytes + r.bytes > MAX_BYTES)
      throw Error(
        "待核对的正文决定已达 32 条或 32 MiB 上限，未提交新决定；请先核对或清理旧库证据。",
      );
    const liveCount = [
      ...this.attempts.values(),
      ...this.markers.values(),
    ].filter((r) => !this.isOld(r) && r.target.epoch === a.epoch).length;
    if (
      liveCount >= MAX_ACTIVE_MARKERS ||
      this.attempts.size + this.markers.size >= MAX_RECORDS
    )
      throw Error("正文决定终态记录已达上限，未提交新决定；请先清理旧库证据。");
    this.attempts.set(key, r);
    this.bytes += r.bytes;
    this.change();
    return r;
  }
  preserve(
    r: DecisionEntry,
    evidence: TerminalEvidence,
    source: "write" | "full-read",
  ) {
    if (!this.has(r) || this.isOld(r)) return;
    r.candidates[source] = evidence;
    this.change();
  }
  authenticate(
    r: DecisionEntry,
    evidence: TerminalEvidence,
    fingerprint: string | null,
  ) {
    if (!this.has(r) || this.isOld(r)) return null;
    const { epoch, bookId, chapterId, id, action } = r.target;
    const marker: DecisionMarker = {
      kind: "terminal",
      key: r.key,
      generation: r.generation,
      retired: false,
      target: { epoch, bookId, chapterId, id, action },
      evidence,
      fingerprint,
      bodyChecked: evidence.decision !== "accepted",
    };
    this.markers.set(r.key, marker);
    this.removeAttempt(r);
    this.change();
    return marker;
  }
  confirmBody(r: DecisionMarker) {
    if (this.has(r) && !this.isOld(r) && !r.bodyChecked) {
      r.bodyChecked = true;
      this.change();
    }
  }
  clearOld(r: DecisionRecord) {
    if (!this.has(r) || !this.isOld(r)) return;
    if (r.kind === "attempt") this.removeAttempt(r);
    else this.markers.delete(r.key);
    this.change();
  }
  private removeAttempt(r: DecisionEntry) {
    this.attempts.delete(r.key);
    this.bytes -= r.bytes;
  }
}
