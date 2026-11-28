import { occurrenceRoles, type ForeshadowSourceReader } from "./foreshadowTypes";
import FrozenForeshadowSource from "./FrozenForeshadowSource";
import FrozenPredecessorDiagnosis from "./FrozenPredecessorDiagnosis";
import React, { useEffect, useRef, useState } from "react";
import type { Chapter } from "./types";
import type { MaterialSummary, MaterialsAPI } from "./materialTypes";
import type {
  AuthorReview,
  AuthorReviewItem,
  AuthorReviewVerdict,
  WritingAPI,
  WritingContextDirectory,
  FrozenContextChapter,
  WritingState,
  WritingPolicy,
  WritingPack,
  WritingProposal,
  WritingChapterRead,
} from "./writingTypes";
import { errorMessage } from "./workflowTypes";
import { Dialog } from "./ui/Dialog";
import {
  captureDecision, decisionKey, WritingDecisionSession,
} from "./writingDecision";
import { useWritingDecision } from "./useWritingDecision";
import { WritingDecisionRecovery } from "./WritingDecisionRecovery";
import FrozenOutlineComparison from "./FrozenOutlineComparison";
const verdictLabels: Record<AuthorReviewVerdict, string> = {
  unreviewed: "未审阅",
  conforms: "符合",
  question: "有疑问",
  conflict: "有冲突",
};
const encoder = new TextEncoder();
const MAX_REVIEW_REVISION = 9_007_199_254_740_000;
function sameItems(a: AuthorReviewItem[], b: AuthorReviewItem[]) {
  return (
    a.length === b.length &&
    a.every((item) =>
      b.some(
        (other) =>
          other.material_id === item.material_id &&
          other.verdict === item.verdict &&
          other.note === item.note,
      ),
    )
  );
}
function reviewBinding(q: WritingProposal, pack: WritingPack) {
  return {
    proposal_id: q.id,
    pack_id: pack.id,
    book_id: q.book_id,
    chapter_id: q.chapter_id,
    originating_epoch: pack.originating_epoch,
    policy_revision: pack.policy.revision,
    chapter_revision: pack.chapter.revision,
  };
}
function validReview(r: AuthorReview, q: WritingProposal, pack: WritingPack) {
  return (
    Number.isSafeInteger(r.revision) &&
    r.revision > 0 &&
    r.revision < MAX_REVIEW_REVISION &&
    q.base_revision === pack.chapter.revision &&
    Object.entries(reviewBinding(q, pack)).every(
      ([key, value]) => r[key as keyof AuthorReview] === value,
    ) &&
    Array.isArray(r.items) &&
    r.items.length === pack.policy.required_material_ids.length &&
    pack.policy.required_material_ids.every((id) =>
      r.items.some(
        (item) =>
          item.material_id === id &&
          Object.hasOwn(verdictLabels, item.verdict) &&
          typeof item.note === "string" &&
          encoder.encode(item.note).length <= 4096,
      ),
    ) &&
    encoder.encode(JSON.stringify(r)).length <= 64 * 1024
  );
}
function storedItems(
  q: WritingProposal,
  pack: WritingPack,
): AuthorReviewItem[] {
  return pack.materials.map(
    (material) =>
      q.author_review?.items.find(
        (item) => item.material_id === material.id,
      ) ?? { material_id: material.id, verdict: "unreviewed", note: "" },
  );
}
type ReviewAttempt = {
  epoch: string;
  diagnostic?: string;
  pack: WritingPack;
  proposal: WritingProposal;
  items: AuthorReviewItem[];
  revision: number;
  acknowledged: boolean;
};
type ConfigAttempt = {
  baseEpoch: string;
  baseRevision: number | null;
  goal: string;
  mode: WritingPolicy["mode"];
  ids: string[];
  contextIds: string[];
};
type DirectoryRead =
  | { kind: "loading" }
  | { kind: "unavailable"; reason: string }
  | { kind: "ready"; value: WritingContextDirectory };
function sameContextIds(a: string[], b: string[]) {
  const ids = new Set(a);
  return (
    ids.size === a.length &&
    a.length === b.length &&
    new Set(b).size === b.length &&
    b.every((id) => ids.has(id))
  );
}
function matchesConfig(policy: WritingPolicy | null, attempt: ConfigAttempt) {
  return (
    policy?.goal === attempt.goal &&
    policy.mode === attempt.mode &&
    JSON.stringify(policy.required_material_ids) ===
      JSON.stringify(attempt.ids) &&
    sameContextIds(policy.context_chapter_ids ?? [], attempt.contextIds)
  );
}
function hasCompleteContext(pack: WritingPack) {
  const ids = pack.policy.context_chapter_ids ?? [];
  const frozen = pack.context_chapters ?? [];
  return (
    ids.length <= 8 &&
    sameContextIds(
      ids,
      frozen.map(({ chapter }) => chapter.id),
    ) &&
    (frozen.length === 0 || pack.handle_version === 3 || pack.handle_version === 4) &&
    frozen.every(
      ({ chapter, volume }) =>
        chapter.id !== pack.chapter_id &&
        chapter.volume_id === volume.id &&
        volume.book_id === pack.book_id &&
        chapter.deleted_at === null &&
        volume.deleted_at === null,
    )
  );
}
function FrozenPriorChapter({
  frozen,
  citations,
}: {
  frozen: FrozenContextChapter;
  citations: React.ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const { chapter, volume } = frozen;
  return (
    <article>
      <details onToggle={(e) => setOpen(e.currentTarget.open)}>
        <summary>
          {volume.title} / {chapter.title} · 版本 {chapter.revision}
        </summary>
        {open && (
          <>
            <p>
              冻结目录排序：卷 {volume.sort_order} / 章 {chapter.sort_order} ·{" "}
              {chapter.id}
            </p>
            <pre className="writing-context-text" tabIndex={0}>
              {chapter.body}
            </pre>
          </>
        )}
      </details>
      {citations}
    </article>
  );
}
function unicodeQuote(value: string, start: number, end: number) {
  let offset = 0;
  let first = 0;
  let index = 0;
  for (const char of value) {
    if (index === start) first = offset;
    if (index === end) break;
    offset += char.length;
    index++;
  }
  return index <= start ? "" : value.slice(first, offset);
}
export default function WritingPanel({
  api,
  bookId,
  chapter,
  materials,
  sourceReader,
  onReload,
  onClose,
  onPendingChange,
  onReadDecisionChapter,
  decisionSession,
  isScopeCurrent = () => true,
  recoveryOnly = false,
}: {
  api: WritingAPI;
  bookId: string;
  chapter: Pick<Chapter, "id" | "title">;
  materials: MaterialsAPI;
  sourceReader?: ForeshadowSourceReader;
  onReload: () => Promise<Chapter>;
  onReadDecisionChapter: () => Promise<WritingChapterRead>;
  decisionSession?: WritingDecisionSession;
  isScopeCurrent?: () => boolean;
  recoveryOnly?: boolean;
  onClose: () => void;
  onPendingChange: (v: boolean) => void;
}) {
  const [state, setState] = useState<WritingState | null>(null),
    [directoryRead, setDirectoryRead] = useState<DirectoryRead>({
      kind: "loading",
    }),
    [contextIds, setContextIds] = useState<string[]>([]),
    [cards, setCards] = useState<MaterialSummary[]>([]),
    [current, setCurrent] = useState<Chapter | null>(null),
    [goal, setGoal] = useState(""),
    [mode, setMode] = useState<WritingPolicy["mode"]>("active"),
    [ids, setIds] = useState<string[]>([]),
    [detail, setDetail] = useState<{
      pack: WritingPack;
      proposal?: WritingProposal;
    } | null>(null),
    [confirm, setConfirm] = useState<"configure" | "accept" | "reject" | null>(
      null,
    ),
    [ack, setAck] = useState(false),
    [confirmedReviewRevision, setConfirmedReviewRevision] = useState<
      number | null
    >(null),
    [draft, setDraft] = useState<AuthorReviewItem[]>([]),
    [reviewBaseChanged, setReviewBaseChanged] = useState(false),
    [sourceProblem, setSourceProblem] = useState<string | null>(null),
    [reviewNotSaved, setReviewNotSaved] = useState(false),
    [discardCopy, setDiscardCopy] = useState(false),
    [copyResult, setCopyResult] = useState<{copy: object; text: string} | null>(null),
    [restoredReview, setRestoredReview] = useState<ReviewAttempt | null>(null),
    [configConflict, setConfigConflict] = useState<ConfigAttempt | null>(null),
    [restoredConfig, setRestoredConfig] = useState<ConfigAttempt | null>(null),
    [busy, setBusy] = useState(false),
    [needsRead, setNeedsRead] = useState(false),
    [message, setMessage] = useState("");
  const receipt = useRef<((s: WritingState) => boolean) | null>(
      null,
    ),
    actionError = useRef(""),
    reviewAttempt = useRef<ReviewAttempt | null>(null),
    configAttempt = useRef<ConfigAttempt | null>(null),
    writeEpoch = useRef<string | null>(null),
    draftGuard = useRef(false),
    decisionGuard = useRef(false),
    copySeq = useRef(0);
  const [localDecisionSession] = useState(() => new WritingDecisionSession());
  const session = decisionSession ?? localDecisionSession;
  const alive = useRef(true),
    lock = useRef(false),
    readRequired = useRef(false),
    seq = useRef(0),
    directorySeq = useRef(0),
    predecessorPicker = useRef<HTMLLegendElement>(null),
    callbacks = useRef({ onReload, onClose, onPendingChange });
  callbacks.current = { onReload, onClose, onPendingChange };
  const decision = useWritingDecision({
    api,
    session,
    isScopeCurrent,
    readChapter: onReadDecisionChapter,
    onBlockingChange: (value) => {
      decisionGuard.current = value;
      callbacks.current.onPendingChange(
        value || lock.current || readRequired.current || draftGuard.current,
      );
    },
    onVerified: (_target, q) => {
      if (detail?.proposal?.id === q.id) {
        setDetail({ pack: detail.pack, proposal: q });
        setDraft(storedItems(q, detail.pack));
        setReviewBaseChanged(false);
      }
    },
    onChapter: (read, publish) => {
      if (publish) read.publish?.();
      setCurrent(read.chapter);
    },
  });
  const attemptEntries = session.recoveries().filter(({ target }) =>
    target.bookId === bookId && target.chapterId === chapter.id,
  );
  const pending = (v: boolean) => {
    setBusy(v);
    callbacks.current.onPendingChange(
      v || decisionGuard.current || readRequired.current || draftGuard.current,
    );
  };
  const close = () => {
    if (!lock.current && !decisionGuard.current && !readRequired.current && !draftGuard.current)
      callbacks.current.onClose();
  };
  async function readDirectory(epoch: string, request: number) {
    try {
      const value = await api.contextDirectory(bookId, chapter.id);
      if (!alive.current || decision.isStale() || request !== directorySeq.current) return;
      if (
        value.epoch !== epoch ||
        value.book_id !== bookId ||
        value.chapter_id !== chapter.id
      )
        throw Error("前文目录读取归属不匹配");
      setDirectoryRead({ kind: "ready", value });
    } catch (err) {
      if (alive.current && !decision.isStale() && request === directorySeq.current)
        setDirectoryRead({ kind: "unavailable", reason: errorMessage(err) });
    }
  }
  async function refresh(reconcile = false) {
    if (
      lock.current ||
      decision.isBusy() ||
      decision.isStale() ||
      (!reconcile && (readRequired.current || draftGuard.current))
    )
      return;
    lock.current = true;
    pending(true);
    const n = ++seq.current;
    // App may publish its chapter before the other scoped reads settle.
    decision.noteChapterRefresh();
    const directoryRequest = ++directorySeq.current;
    setDirectoryRead({ kind: "loading" });
    const attempt = reconcile ? configAttempt.current : null;
    async function confirmConfigEpoch() {
      if (!attempt) return true;
      const epoch = await api.epoch();
      if (!alive.current || decision.isStale() || n !== seq.current) return false;
      if (typeof epoch !== "string" || !epoch.trim())
        throw Error("书库时代读取无效");
      if (epoch === attempt.baseEpoch) return true;

      // The old book/chapter may not exist in the replacement. Never require
      // its scoped read to preserve the old draft or release the receipt lock.
      configAttempt.current = null;
      writeEpoch.current = null;
      receipt.current = null;
      draftGuard.current = true;
      readRequired.current = false;
      setNeedsRead(false);
      setRestoredConfig(attempt);
      setState(null);
      setCards([]);
      setDetail(null);
      setDraft([]);
      setConfigConflict(null);
      setReviewBaseChanged(false);
      clearConfirmation();
      setMessage("无法用当前书库核对旧配置写入。旧草稿仅供查看，未重试写入。");
      return false;
    }
    async function readScope() {
      return Promise.all([
        api.state(bookId, chapter.id),
        materials.list(bookId, false),
        callbacks.current.onReload(),
      ]);
    }
    try {
      if (attempt && !(await confirmConfigEpoch())) return;
      const read = await readScope().then(
        (value) => ({ ok: true as const, value }),
        (error: unknown) => ({ ok: false as const, error }),
      );
      if (!alive.current || decision.isStale() || n !== seq.current) return;
      // Check even when the scoped read failed; restoration may have caused
      // that failure, or a successful scoped read may predate replacement.
      if (attempt && !(await confirmConfigEpoch())) return;
      if (!read.ok) throw read.error;
      const [s, cards, c] = read.value;
      if (
        c.id !== chapter.id ||
        c.deleted_at !== null ||
        !s.epoch ||
        (attempt && s.epoch !== attempt.baseEpoch) ||
        (s.policy &&
          (s.policy.book_id !== bookId || s.policy.chapter_id !== chapter.id))
      )
        throw Error("读取归属不匹配");
      const replaced =
        reconcile &&
        writeEpoch.current !== null &&
        writeEpoch.current !== s.epoch;
      if (reconcile && !replaced && receipt.current && !receipt.current(s))
        throw Error("尚未读取到本次写入结果，请继续只读核对");
      const retainConfig = attempt && !matchesConfig(s.policy, attempt);
      const conflict =
        retainConfig && (s.policy?.revision ?? null) !== attempt.baseRevision
          ? attempt
          : null;
      configAttempt.current = null;
      writeEpoch.current = null;
      receipt.current = null;
      setState(s);
      setCards(cards);
      setCurrent(c);
      decision.noteChapterRefresh({ epoch: s.epoch, bookId, chapter: c });
      setConfigConflict(conflict);
      if (!retainConfig) resetConfig(s.policy);
      setDetail(null);
      setDraft([]);
      setReviewBaseChanged(false);
      setSourceProblem(null);
      setReviewNotSaved(false);
      clearConfirmation();
      readRequired.current = false;
      setNeedsRead(false);
      // Selection is optional. An unavailable or slow directory must never
      // block confirmed state, receipt reconciliation or closing the panel.
      void readDirectory(s.epoch, directoryRequest);
      if (reconcile)
        setMessage(
          (replaced
            ? "检测到书库已恢复，无法用当前书库核对旧操作。未重试写入。"
            : "已重新读取。请核对实际配置与建议状态；未自动重试写入。") +
            (retainConfig ? "配置草稿尚未核对为已保存，已保留。" : "") +
            actionError.current,
        );
    } catch (e) {
      if (alive.current && n === seq.current) {
        setDirectoryRead({ kind: "unavailable", reason: "写作状态尚未核对" });
        setMessage(errorMessage(e));
      }
    } finally {
      if (alive.current && n === seq.current) {
        lock.current = false;
        pending(false);
      }
    }
  }
  useEffect(() => {
    alive.current = true;
    lock.current = false;
    readRequired.current = false;
    draftGuard.current = false;
    reviewAttempt.current = null;
    setSourceProblem(null);
    setReviewNotSaved(false);
    setDiscardCopy(false);
    setCopyResult(null);
    configAttempt.current = null;
    writeEpoch.current = null;
    receipt.current = null;
    setNeedsRead(false);
    setConfigConflict(null);
    setRestoredConfig(null);
    setRestoredReview(null);
    setDetail(null);
    setDraft([]);
    setReviewBaseChanged(false);
    setState(null);
    clearConfirmation();
    if (!recoveryOnly) void refresh();
    return () => {
      alive.current = false;
      seq.current++;
      directorySeq.current++;
      callbacks.current.onPendingChange(false);
    };
  }, [api, bookId, chapter.id, recoveryOnly]);
  function showDetail(id: string, p: WritingPack, q?: WritingProposal) {
    if (
      p.book_id !== bookId ||
      p.chapter_id !== chapter.id ||
      p.id !== (q?.input.pack_id ?? id) ||
      !hasCompleteContext(p) ||
      (q &&
        (q.id !== id ||
          q.book_id !== bookId ||
          q.chapter_id !== chapter.id ||
          (q.author_review && !validReview(q.author_review, q, p))))
    )
      throw Error("依据或提案归属不匹配");
    setDetail({ pack: p, proposal: q });
    setDraft(q ? storedItems(q, p) : []);
    setSourceProblem(null);
    setReviewNotSaved(false);
  }
  async function choose(id: string, proposal: boolean) {
    if (lock.current || decision.isBusy() || decision.isStale() || readRequired.current || reviewDirty) return;
    const marker = proposal && state
      ? session.terminal(decisionKey(state.epoch, bookId, chapter.id, id))
      : undefined;
    if (marker && draftGuard.current) return;
    setDetail(null);
    setDraft([]);
    setReviewBaseChanged(false);
    clearConfirmation();
    setMessage("");
    if (marker) {
      await decision.readDetail(marker, (p, q) => showDetail(id, p, q));
      return;
    }
    lock.current = true;
    pending(true);
    const n = ++seq.current;
    try {
      const q = proposal ? await api.proposal(bookId, id) : undefined;
      if (!alive.current || decision.isStale() || n !== seq.current) return;
      const p = await api.pack(bookId, q?.input.pack_id ?? id);
      if (alive.current && !decision.isStale() && n === seq.current)
        showDetail(id, p, q);
    } catch (e) {
      if (alive.current && n === seq.current) setMessage(errorMessage(e));
    } finally {
      if (alive.current && n === seq.current) {
        lock.current = false;
        pending(false);
      }
    }
  }
  function clearConfirmation() {
    setConfirm(null);
    setAck(false);
    setConfirmedReviewRevision(null);
  }
  async function reconcileReview() {
    const attempt = reviewAttempt.current;
    if (lock.current || decision.isBusy() || decision.isStale() || !attempt) return;
    lock.current = true;
    pending(true);
    const n = ++seq.current;
    const confirmOriginalEpoch = async () => {
      const epoch = await api.epoch();
      if (!alive.current || decision.isStale() || n !== seq.current) return false;
      if (typeof epoch !== "string" || !epoch.trim())
        throw Error("书库时代读取无效");
      if (epoch === attempt.epoch) return true;
      // Restoration can remove the old book/chapter/proposal altogether.
      // Keep only a copyable old draft; no old receipt or proposal is actionable.
      reviewAttempt.current = null;
      readRequired.current = false;
      setNeedsRead(false);
      setRestoredReview(attempt);
      setState(null);
      setDetail(null);
      setDraft([]);
      setReviewBaseChanged(false);
      clearConfirmation();
      setMessage("无法用当前书库核对旧审阅写入。旧草稿仅供复制，未重试写入。");
      return false;
    };
    try {
      if (!(await confirmOriginalEpoch())) return;
      let read:
        { ok: true; proposal: WritingProposal } | { ok: false; error: unknown };
      try {
        read = {
          ok: true,
          proposal: await api.proposal(bookId, attempt.proposal.id),
        };
      } catch (error) {
        read = { ok: false, error };
      }
      if (
        !alive.current ||
        n !== seq.current ||
        !(await confirmOriginalEpoch())
      )
        return;
      if (!read.ok) throw read.error;
      const q = read.proposal;
      if (
        q.id !== attempt.proposal.id ||
        q.book_id !== bookId ||
        q.chapter_id !== chapter.id ||
        q.input.pack_id !== attempt.pack.id ||
        (q.author_review && !validReview(q.author_review, q, attempt.pack))
      )
        throw Error("逐项审阅回读归属不匹配");
      const r = q.author_review;
      const matches =
        r?.revision === attempt.revision && sameItems(r.items, attempt.items);
      if (
        attempt.acknowledged &&
        (!r ||
          r.revision < attempt.revision ||
          (r.revision === attempt.revision && !matches))
      )
        throw Error("尚未读取到本次逐项审阅结果，请继续只读核对");
      setDetail({ pack: attempt.pack, proposal: q });
      setReviewBaseChanged(
        !matches &&
          !!r &&
          (r.revision !== attempt.proposal.author_review?.revision ||
            !sameItems(r.items, attempt.proposal.author_review.items)),
      );
      setReviewNotSaved(!matches);
      if (matches) {
        setDraft(storedItems(q, attempt.pack));
        setMessage("已核对保存的逐项审阅版本 " + r.revision);
      } else {
        setMessage(
          "本次逐项审阅未核对为已保存，草稿已保留。请核对当前版本；未自动重试写入。" +
            (attempt.diagnostic ? " 保存返回信息：" + attempt.diagnostic : ""),
        );
      }
      reviewAttempt.current = null;
      readRequired.current = false;
      setNeedsRead(false);
    } catch (e) {
      if (alive.current && n === seq.current) setMessage(errorMessage(e));
    } finally {
      if (alive.current && n === seq.current) {
        lock.current = false;
        pending(false);
      }
    }
  }
  async function saveReview() {
    if (
      lock.current ||
      decision.isBusy() ||
      decision.isStale() ||
      readRequired.current ||
      !state ||
      !detail ||
      !p ||
      !canEditReview ||
      observedSourceProblem ||
      !reviewDirty ||
      !reviewFits ||
      reviewBaseChanged
    )
      return;
    lock.current = true;
    readRequired.current = true;
    setNeedsRead(true);
    pending(true);
    clearConfirmation();
    setMessage("");
    const n = ++seq.current;
    const attempt: ReviewAttempt = {
      epoch: state.epoch,
      pack: detail.pack,
      proposal: p,
      items: draft,
      revision: nextReviewRevision,
      acknowledged: false,
    };
    reviewAttempt.current = attempt;
    try {
      const r = await api.saveReview(
        bookId,
        p.id,
        p.author_review?.revision ?? null,
        state.epoch,
        draft,
      );
      if (
        r &&
        validReview(r, p, detail.pack) &&
        r.revision === attempt.revision &&
        sameItems(r.items, draft)
      )
        attempt.acknowledged = true;
    } catch (e) {
      if (alive.current && n === seq.current) {
        attempt.diagnostic = errorMessage(e);
        setMessage("逐项审阅结果需核对：" + attempt.diagnostic);
      }
    } finally {
      if (alive.current && n === seq.current) {
        lock.current = false;
        await reconcileReview();
      }
    }
  }
  function changeItem(
    id: string,
    change: Partial<Pick<AuthorReviewItem, "verdict" | "note">>,
  ) {
    if (lock.current || decision.isBusy() || decision.isStale() || readRequired.current || !canEditReview) return;
    setDraft((items) =>
      items.map((i) => (i.material_id === id ? { ...i, ...change } : i)),
    );
    clearConfirmation();
  }
  function resetConfig(policy: WritingPolicy | null) {
    setGoal(policy?.goal ?? "");
    setMode(policy?.mode ?? "active");
    setIds(policy?.required_material_ids ?? []);
    setContextIds(policy?.context_chapter_ids ?? []);
  }
  async function write(kind: "configure" | "prepare" | "accept" | "reject") {
    if (
      lock.current ||
      decision.isBusy() ||
      decision.isStale() ||
      readRequired.current ||
      !state ||
      !current ||
      reviewDirty ||
      configConflict ||
      (kind !== "configure" && configDirty) ||
      (kind === "configure" &&
        (!goal.trim() ||
          ids.length < 1 ||
          ids.length > 32 ||
          contextIds.length > 8 ||
          (mode === "active" && invalidContexts.length > 0))) ||
      (kind === "prepare" && invalidContexts.length > 0) ||
      (kind === "reject" && !canDecide) ||
      (kind === "accept" &&
        (!canAccept ||
          confirmedReviewRevision !== p?.author_review?.revision ||
          (!!p?.input.conflicts.length && !ack)))
    )
      return;
    if (kind === "accept" || kind === "reject") {
      try {
        const attempt = captureDecision(p!, detail!.pack, {
          epoch: state.epoch, bookId, chapterId: chapter.id, action: kind,
          policyRevision: state.policy?.revision ?? -1,
          chapterRevision: current.revision, conflictsAck: kind === "accept" && ack,
        }, confirmedReviewRevision);
        clearConfirmation();
        setMessage("");
        await decision.submit(attempt);
      } catch (e) {
        setMessage(errorMessage(e));
      }
      return;
    }
    lock.current = true;
    readRequired.current = true;
    setNeedsRead(true);
    pending(true);
    clearConfirmation();
    setMessage("");
    const n = ++seq.current;
    receipt.current = null;
    writeEpoch.current = state.epoch;
    configAttempt.current =
      kind === "configure"
        ? {
            baseEpoch: state.epoch,
            baseRevision: state.policy?.revision ?? null,
            goal,
            mode,
            ids,
            contextIds,
          }
        : null;
    actionError.current = "";
    try {
      if (kind === "configure") {
        const r = await api.configure(
          bookId,
          chapter.id,
          state.policy?.revision ?? null,
          mode,
          goal,
          ids,
          state.epoch,
          contextIds,
        );
        if (!alive.current || decision.isStale() || n !== seq.current) return;
        const attempt = configAttempt.current!;
        if (
          r?.chapter_id !== chapter.id ||
          r.book_id !== bookId ||
          !Number.isSafeInteger(r.revision) ||
          r.revision !== (attempt.baseRevision ?? -1) + 1 ||
          !matchesConfig(r, attempt)
        )
          throw Error("写作配置回执与本次提交不匹配");
        receipt.current = (s) =>
          s.epoch === state.epoch &&
          !!s.policy &&
          (s.policy.revision > r.revision ||
            (s.policy.revision === r.revision &&
              matchesConfig(s.policy, attempt)));
      } else {
        const r = await api.prepare(
          bookId,
          chapter.id,
          state.policy!.revision,
          current.revision,
          state.epoch,
        );
        if (!alive.current || decision.isStale() || n !== seq.current) return;
        if (r?.id)
          receipt.current = (s) =>
            s.epoch === state.epoch && s.packs.some((p) => p.id === r.id);
      }
    } catch (e) {
      if (alive.current && n === seq.current) {
        actionError.current = errorMessage(e);
        setMessage("操作结果需核对：" + actionError.current);
      }
    } finally {
      if (alive.current && n === seq.current) {
        lock.current = false;
        await refresh(true);
      }
    }
  }
  const disabled = busy || needsRead || decision.phase !== "idle" || decision.obsolete;
  const directory = directoryRead.kind === "ready" ? directoryRead.value : null;
  const candidates = new Map(directory?.chapters.map((c) => [c.id, c]));
  const candidateVolumes = new Map(directory?.volumes.map((v) => [v.id, v]));
  const invalidContexts = directory
    ? contextIds.filter((id) => !candidates.has(id))
    : [];
  function removeContext(id: string) {
    setContextIds((current) => current.filter((value) => value !== id));
    clearConfirmation();
  }
  function contextLabel(id: string) {
    const c = candidates.get(id);
    return c
      ? `${candidateVolumes.get(c.volume_id)?.title ?? c.volume_id} / ${c.title}`
      : id;
  }
  const configDirty =
    !!configConflict ||
    (!!state &&
      (goal !== (state.policy?.goal ?? "") ||
        mode !== (state.policy?.mode ?? "active") ||
        JSON.stringify(ids) !==
          JSON.stringify(state.policy?.required_material_ids ?? []) ||
        !sameContextIds(contextIds, state.policy?.context_chapter_ids ?? [])));
  const p = detail?.proposal;
  const reviewDirty = !!(
    p &&
    detail &&
    !sameItems(draft, storedItems(p, detail.pack))
  );
  const hasRestoredDraft = !!(restoredReview || restoredConfig);
  draftGuard.current = reviewDirty || configDirty || hasRestoredDraft;
  useEffect(() => {
    callbacks.current.onPendingChange(
      lock.current ||
        decisionGuard.current ||
        readRequired.current ||
        busy || needsRead ||
        reviewDirty ||
        configDirty ||
        hasRestoredDraft,
    );
  }, [busy, needsRead, reviewDirty, configDirty, hasRestoredDraft]);
  const targetKey = p && state ? decisionKey(state.epoch, bookId, chapter.id, p.id) : null;
  const canDecide = p?.decision === "pending" && p.live_epoch === state?.epoch &&
    !!targetKey && !session.get(targetKey) && !session.terminal(targetKey);
  const canEditReview = !!(
    canDecide &&
    detail &&
    !configDirty &&
    invalidContexts.length === 0 &&
    sameContextIds(
      detail.pack.policy.context_chapter_ids ?? [],
      state?.policy?.context_chapter_ids ?? [],
    ) &&
    (!directory ||
      (detail.pack.context_chapters ?? []).every(
        ({ chapter: frozen, volume }) => {
          const c = candidates.get(frozen.id);
          const v = candidateVolumes.get(volume.id);
          return (
            c &&
            v &&
            c.revision === frozen.revision &&
            c.title === frozen.title &&
            c.volume_id === volume.id &&
            c.sort_order === frozen.sort_order &&
            v.sort_order === volume.sort_order &&
            v.title === volume.title
          );
        },
      )) &&
    state?.policy?.mode === "active" &&
    detail.pack.originating_epoch === state.epoch &&
    detail.pack.policy.revision === state.policy.revision &&
    detail.pack.chapter.revision === current?.revision &&
    detail.pack.expires_at * 1000 > Date.now()
  );
  const noteScope = state && !decision.obsolete && isScopeCurrent()
    ? {epoch:state.epoch, bookId, chapterId:chapter.id} : null;
  const noteCopy = noteScope ? session.readReviewNotes(noteScope) : null;
  const sourceProblemKey = detail && p && state
    ? JSON.stringify([state.epoch, bookId, chapter.id, detail.pack.id, p.id]) : null;
  const observedSourceProblem = sourceProblem !== null && sourceProblem === sourceProblemKey;
  const canRetainNotes = !!(noteScope && p && detail && p.live_epoch === noteScope.epoch &&
    detail.pack.originating_epoch === noteScope.epoch && reviewDirty &&
    (observedSourceProblem || reviewNotSaved) && draft.some((i) => i.note.length > 0));
  function retainNotes() {
    if (disabled || lock.current || readRequired.current || !isScopeCurrent() ||
      !canRetainNotes || !noteScope || !p || !detail) return;
    const lines = draft.filter((i) => i.note.length > 0).map((i) =>
      `${detail.pack.materials.find((m) => m.id === i.material_id)?.title ?? i.material_id}：${i.note}`);
    const result = session.retainReviewNotes(noteScope, p.id, detail.pack.id, lines.join("\n"));
    if (result !== "retained") {
      setMessage(result === "occupied"
        ? "已有另一份备注副本，未覆盖，当前编辑仍保留。请先复制或明确移除旧副本。"
        : "备注副本超过 64 KiB 上限，未暂存，当前编辑仍保留。");
      return;
    }
    setDraft(storedItems(p, detail.pack));
    setReviewBaseChanged(false);
    clearConfirmation();
    setMessage("备注已暂存到应用会话；本次未保存编辑已退出，未保存认可或修改正文。");
  }
  async function copyNotes() {
    if (!noteCopy || !noteScope || !isScopeCurrent()) return;
    const n = ++copySeq.current;
    setCopyResult(null);
    const publish = (text: string) => {
      if (alive.current && copySeq.current === n && isScopeCurrent() && session.readReviewNotes(noteScope) === noteCopy)
        setCopyResult({copy:noteCopy, text});
    };
    try {
      await navigator.clipboard.writeText(noteCopy.text);
      publish("已复制纯文本备注。");
    } catch {
      publish("未能复制；备注副本仍在，可手动选择文本复制。");
    }
  }
  const nextReviewRevision = (p?.author_review?.revision ?? 0) + 1;
  const reviewFits = !!(
    p &&
    detail &&
    nextReviewRevision < MAX_REVIEW_REVISION &&
    draft.every((item) => encoder.encode(item.note).length <= 4096) &&
    encoder.encode(
      JSON.stringify({
        ...reviewBinding(p, detail.pack),
        revision: nextReviewRevision,
        items: draft,
      }),
    ).length <=
      64 * 1024
  );
  const canAccept =
    canEditReview &&
    !observedSourceProblem &&
    !reviewDirty &&
    !reviewBaseChanged &&
    !!(
      p?.author_review &&
      p.author_review.items.length > 0 &&
      p.author_review.items.every((i) => i.verdict === "conforms")
    );
  const materialHandle = (pack: WritingPack, id: string) =>
    (pack.handle_version ?? 1) === 1 ? id : `material:${id}`;
  function citation(
    q: WritingProposal["input"]["citations"][number],
    i: number,
  ) {
    return (
      <div key={i}>
        <p>
          {q.entry_id} / {q.field_key} [{q.start},{q.end})
        </p>
        <blockquote>
          {quote(q.entry_id, q.field_key, q.start, q.end)}
        </blockquote>
        <p>{q.explanation}</p>
      </div>
    );
  }
  function quote(id: string, key: string, start: number, end: number) {
    const pack = detail!.pack;
    const version = pack.handle_version ?? 1;
    const source =
      id === (version === 1 ? "$chapter" : `chapter:${pack.chapter_id}`)
        ? pack.chapter
        : version >= 3
          ? pack.context_chapters?.find(
              ({ chapter }) => id === `context:${chapter.id}`,
            )?.chapter
          : undefined;
    const material = pack.materials.find(
      (m) => materialHandle(pack, m.id) === id,
    );
    const value = source
      ? key === "body"
        ? source.body
        : key === "$title"
          ? source.title
          : undefined
      : key === "$title"
        ? material?.title
        : key.startsWith("occurrence:")
          ? pack.foreshadow_occurrences?.find(o=>o.material_id===material?.id&&`occurrence:${o.id}`===key)?.quote
          : material?.fields[key];
    return value === undefined
      ? "引用字段不存在"
      : unicodeQuote(value, start, end);
  }
  const locked =
    busy ||
    needsRead ||
    decision.phase !== "idle" ||
    configDirty ||
    reviewDirty ||
    hasRestoredDraft;
  return (
    <Dialog
      title={`章节写作约束 · ${chapter.title}`}
      label="章节写作约束"
      closeLabel="关闭写作约束"
      className="modal writing-panel"
      size="lg"
      dismissible={!locked}
      onClose={close}
    >
      <div className="writing-body">
        <p className="ui-meta">
          依据引用只检查覆盖与范围，不代表语义一致。作者必须审阅设定、大纲与新稿；不会自动连接模型或发送书稿。
        </p>
        {reviewDirty && (
          <p>有未保存的逐项审阅，保存或明确放弃后才能切换建议或关闭。</p>
        )}
        {noteScope && session.hasReviewNoteCopy() && (
          <aside className="writing-config-comparison" aria-label="会话备注副本">
            {noteCopy ? <>
              <p>旧建议 {noteCopy.proposalId} 的备注副本。仅供查阅，不会带入新建议。关闭应用会丢失，请按需复制。</p>
              <label>旧建议备注副本<textarea value={noteCopy.text} readOnly /></label>
              <button type="button" onClick={() => void copyNotes()}>复制备注副本</button>
              {copyResult?.copy === noteCopy && <p role="status">{copyResult.text}</p>}
            </> : <p>备注槽已被其他范围的副本占用；此处不显示其内容。可返回原范围查看（若仍可用），或明确移除副本。</p>}
            {discardCopy ? <>
              <p>移除会话副本不会修改正式审阅，但无法从会话找回这些备注。</p>
              <button type="button" onClick={() => {
                if (!isScopeCurrent()) return;
                session.clearReviewNoteCopy();
                setDiscardCopy(false);
                setCopyResult(null);
              }}>确认移除备注副本</button>
              <button type="button" onClick={() => setDiscardCopy(false)}>取消移除</button>
            </> : <button type="button" onClick={() => setDiscardCopy(true)}>移除备注副本</button>}
          </aside>
        )}
        {configDirty && (
          <p>有未保存的配置修改，保存或明确放弃后才能刷新、查看已核对的建议或关闭。</p>
        )}
        {restoredConfig && (
          <aside
            className="writing-config-comparison"
            aria-label="旧书库配置草稿"
          >
            <h3>书库已恢复，旧草稿不可带入</h3>
            <label>
              旧写作目标
              <textarea value={restoredConfig.goal} readOnly />
            </label>
            <p>
              旧约束状态：
              {restoredConfig.mode === "active"
                ? "启用"
                : restoredConfig.mode === "paused"
                  ? "暂停"
                  : "停用"}
            </p>
            <p>旧必需资料 ID：{restoredConfig.ids.join("、") || "无"}</p>
            <p>旧前文 ID：{restoredConfig.contextIds.join("、") || "无"}</p>
            <button
              onClick={() => {
                setRestoredConfig(null);
                resetConfig(null);
                setMessage(
                  "旧配置草稿已放弃。要在当前书库继续，请先重新读取。",
                );
              }}
            >
              放弃旧书库草稿
            </button>
          </aside>
        )}
        {restoredReview && (
          <aside
            className="writing-config-comparison"
            aria-label="旧书库逐项审阅草稿"
          >
            <h3>书库已恢复，旧审阅草稿不可带入</h3>
            <p>
              旧建议：{restoredReview.proposal.id} · 旧依据包：
              {restoredReview.pack.id}
            </p>
            {restoredReview.items.map((item) => {
              const title =
                restoredReview.pack.materials.find(
                  (m) => m.id === item.material_id,
                )?.title ?? item.material_id;
              return (
                <label key={item.material_id}>
                  {title} · {verdictLabels[item.verdict]} · 旧草稿备注
                  <textarea value={item.note} readOnly />
                </label>
              );
            })}
            <button
              onClick={() => {
                setRestoredReview(null);
                setMessage(
                  "旧审阅草稿已放弃。要在当前书库继续，请先重新读取。",
                );
              }}
            >
              放弃旧书库审阅草稿
            </button>
          </aside>
        )}
        <WritingDecisionRecovery
          entries={attemptEntries}
          session={session}
          phase={decision.phase}
          disabled={disabled || configDirty || reviewDirty || hasRestoredDraft}
          obsolete={decision.obsolete}
          message={decision.message}
          reconcile={(entry) => {
            if (lock.current || readRequired.current || draftGuard.current) return;
            void decision.reconcile(entry);
          }}
          stop={decision.stop}
        />
        {!recoveryOnly && <button
          disabled={disabled || configDirty || reviewDirty || hasRestoredDraft}
          onClick={() => void refresh()}
        >
          刷新依据与建议
        </button>}
        {state && !decision.obsolete && (
          <>
            <label>
              写作目标
              <textarea
                value={goal}
                disabled={disabled || reviewDirty}
                readOnly={!!configConflict}
                onChange={(e) => {
                  if (configConflict) return;
                  setGoal(e.target.value);
                  clearConfirmation();
                }}
              />
            </label>
            <label>
              约束状态
              <select
                value={mode}
                disabled={disabled || reviewDirty || !!configConflict}
                onChange={(e) => {
                  setMode(e.target.value as WritingPolicy["mode"]);
                  clearConfirmation();
                }}
              >
                <option value="active">启用</option>
                <option value="paused">暂停（仍禁止无依据提案）</option>
                <option value="disabled">停用</option>
              </select>
            </label>
            <fieldset disabled={disabled || reviewDirty || !!configConflict}>
              <legend>必需资料（1 至 32 张，必须完整读取）</legend>
              {cards.map((m) => (
                <label key={m.id}>
                  <input
                    type="checkbox"
                    checked={ids.includes(m.id)}
                    onChange={(e) => {
                      setIds(
                        e.target.checked
                          ? [...ids, m.id]
                          : ids.filter((id) => id !== m.id),
                      );
                      clearConfirmation();
                    }}
                  />
                  {m.title}
                  {m.content_review_required ? " · 待复核" : ""}
                </label>
              ))}
              {ids
                .filter((id) => !cards.some((m) => m.id === id))
                .map((id) => (
                  <p key={id}>
                    不可用资料：{id}（暂停或停用仍可保存）
                    <button
                      type="button"
                      disabled={disabled}
                      onClick={() => {
                        setIds((current) =>
                          current.filter((value) => value !== id),
                        );
                        clearConfirmation();
                      }}
                    >
                      移除不可用资料 {id}
                    </button>
                    <span>仅从本次配置移除，历史依据保留。</span>
                  </p>
                ))}
            </fieldset>
            <fieldset disabled={disabled || reviewDirty || !!configConflict}>
              <legend ref={predecessorPicker} tabIndex={-1}>前文全文（可选，最多 8 章）</legend>
              <p>
                仅显示本书目录中位于目标章之前的活动章节。未选择的章节不会加入冻结依据包。
              </p>
              <div className="writing-context-picker">
                {directory?.chapters.map((prior) => (
                  <label key={prior.id}>
                    <input
                      type="checkbox"
                      checked={contextIds.includes(prior.id)}
                      disabled={
                        contextIds.length >= 8 && !contextIds.includes(prior.id)
                      }
                      onChange={(e) => {
                        setContextIds(
                          e.target.checked
                            ? [...contextIds, prior.id]
                            : contextIds.filter((id) => id !== prior.id),
                        );
                        clearConfirmation();
                      }}
                    />
                    {contextLabel(prior.id)}
                  </label>
                ))}
                {directory && directory.chapters.length === 0 && (
                  <p>没有可选的前文章节。</p>
                )}
              </div>
              {directoryRead.kind === "loading" && <p>正在读取前文目录。</p>}
              {directoryRead.kind === "unavailable" && (
                <p>
                  暂无法读取前文目录：{directoryRead.reason}
                  。已保存的选择仍保留，提交时由书库核对前文。
                </p>
              )}
              {!directory &&
                contextIds.map((id) => (
                  <p key={id}>
                    已选前文（目录未核对）：{id}
                    <button type="button" onClick={() => removeContext(id)}>
                      从选择移除前文 {id}
                    </button>
                    <span>仅从本次配置移除，历史依据保留。</span>
                  </p>
                ))}
              {invalidContexts.map((id) => (
                <p key={id}>
                  不可用前文：{id}
                  （已删除或不再位于本章之前；暂停或停用仍可保存）
                  <button type="button" onClick={() => removeContext(id)}>
                    移除不可用前文 {id}
                  </button>
                  <span>仅从本次配置移除，历史依据保留。</span>
                </p>
              ))}
            </fieldset>
            <button
              disabled={
                disabled ||
                reviewDirty ||
                !!configConflict ||
                !goal.trim() ||
                ids.length < 1 ||
                ids.length > 32 ||
                contextIds.length > 8 ||
                (mode === "active" && invalidContexts.length > 0)
              }
              onClick={() => setConfirm("configure")}
            >
              保存约束配置
            </button>
            {configConflict && (
              <aside
                className="writing-config-comparison"
                aria-label="对照当前保存的写作约束"
              >
                <h3>保存的约束配置已改变</h3>
                <p>草稿依据版本：{configConflict.baseRevision ?? "未配置"}</p>
                {state.policy ? (
                  <>
                    <p>
                      当前保存的版本：{state.policy.revision} ·{" "}
                      {state.policy.mode === "active"
                        ? "启用"
                        : state.policy.mode === "paused"
                          ? "暂停"
                          : "停用"}
                    </p>
                    <p className="author-review-note">
                      当前写作目标：{state.policy.goal}
                    </p>
                    <p>
                      当前必需资料：
                      {state.policy.required_material_ids
                        .map((id) => {
                          const card = cards.find((m) => m.id === id);
                          return card ? `${card.title}（${id}）` : id;
                        })
                        .join("、")}
                    </p>
                    <p>
                      当前前文：
                      {(state.policy.context_chapter_ids ?? [])
                        .map(contextLabel)
                        .join("、") || "无"}
                    </p>
                  </>
                ) : (
                  <p>当前没有保存的写作约束。</p>
                )}
                <button
                  disabled={disabled}
                  onClick={() => {
                    setConfigConflict(null);
                    clearConfirmation();
                  }}
                >
                  已核对当前配置，保留我的草稿
                </button>
              </aside>
            )}
            {configDirty && (
              <button
                disabled={disabled}
                onClick={() => {
                  resetConfig(state.policy);
                  setConfigConflict(null);
                  clearConfirmation();
                }}
              >
                放弃未保存的配置修改
              </button>
            )}
            <button
              disabled={
                disabled ||
                configDirty ||
                reviewDirty ||
                invalidContexts.length > 0 ||
                state.policy?.mode !== "active"
              }
              onClick={() => void write("prepare")}
            >
              冻结当前依据包
            </button>
            <p>
              配置尚未保存时不能冻结新包。停用后允许无约束提案；配置变更会取消本章所有待审正文建议，停用也不会复活旧建议。依据包有效
              30 分钟；资料或所选前文改变需重新冻结。
            </p>
            <h3>依据包</h3>
            {state.packs.map((pack) => (
              <button
                key={pack.id}
                disabled={disabled || reviewDirty}
                onClick={() => void choose(pack.id, false)}
              >
                查看依据 {pack.id}{" "}
                {pack.archived
                  ? "（归档）"
                  : pack.expires_at * 1000 < Date.now()
                    ? "（已过期）"
                    : pack.policy_revision !== state.policy?.revision ||
                        pack.chapter_revision !== current?.revision
                      ? "（版本已改变）"
                      : "（未过期，依赖仍须重核）"}
              </button>
            ))}
            <h3>严格写作建议</h3>
            {state.proposals.map((q) => {
              const marker = session.terminal(
                decisionKey(state.epoch, bookId, chapter.id, q.id),
              );
              return (
                <div key={q.id}>
                  <button
                    disabled={disabled || reviewDirty || (!!marker && draftGuard.current)}
                    onClick={() => void choose(q.id, true)}
                  >
                    查看建议 {q.id}
                  </button>
                  <span>
                    {q.archived ? "归档 · " : ""}
                    {marker?.evidence.decision ?? q.decision}
                  </span>
                </div>
              );
            })}
          </>
        )}
        {detail && !decision.obsolete && (
          <>
            <h3>冻结依据与目标</h3>
            <p>{detail.pack.policy.goal}</p>
            {p && (
              <p>
                {p.author_review
                  ? `已保存逐项作者审阅 · 版本 ${p.author_review.revision}`
                  : p.decision === "accepted"
                    ? "旧版未记录逐项审阅"
                    : "尚未保存逐项作者审阅"}
              </p>
            )}
            {!!detail.pack.context_chapters?.length && (
              <>
                <h4>冻结前文（补充上下文）</h4>
                {api.predecessorDiagnosis && p?.decision === "pending" && state &&
                  p.live_epoch === state.epoch && detail.pack.originating_epoch === state.epoch &&
                  !state.packs.some(x => x.id === detail.pack.id && x.archived) && (
                    <FrozenPredecessorDiagnosis
                      key={sourceProblemKey ?? ""}
                      reader={api.predecessorDiagnosis}
                      pack={detail.pack} epoch={state.epoch} isScopeCurrent={isScopeCurrent}
                      onAfter={() => setSourceProblem(sourceProblemKey)}
                      onChoose={() => {
                        predecessorPicker.current?.scrollIntoView?.({ block:"nearest" });
                        predecessorPicker.current?.focus();
                      }}
                    />
                )}
                <p>
                  前文衔接仍需作者检查。逐项符合不代表系统检查了所有叙事连续性，引用也不是语义证明。
                </p>
                {detail.pack.context_chapters.map((frozen) => {
                  const matches =
                    p?.input.citations.filter(
                      (q) => q.entry_id === `context:${frozen.chapter.id}`,
                    ) ?? [];
                  return (
                    <FrozenPriorChapter
                      key={`${detail.pack.id}:${frozen.chapter.id}`}
                      frozen={frozen}
                      citations={
                        matches.length ? (
                          <>
                            <h4>前文引用（由智能体提供）</h4>
                            {matches.map(citation)}
                          </>
                        ) : null
                      }
                    />
                  );
                })}
              </>
            )}
            {detail.pack.materials.map((m, index) => {
              const item = draft.find((i) => i.material_id === m.id);
              const saved = p?.author_review?.items.find(
                (i) => i.material_id === m.id,
              );
              const matches = p?.input.citations.filter(
                (c) => c.entry_id === materialHandle(detail.pack, m.id),
              );
              return (
                <details
                  key={m.id}
                  open={!!p && (index === 0 || reviewBaseChanged)}
                >
                  <summary>
                    {m.title} · 版本 {m.revision}
                    {canDecide && item
                      ? ` · ${verdictLabels[item.verdict]}`
                      : ""}
                  </summary>
                  <p>
                    关联卷：{m.volume_id ?? "无"} · 关联章：
                    {m.chapter_id ?? "无"}
                  </p>
                  <p>
                    来源：{m.source_chapter_id ?? "无"} / 版本{" "}
                    {m.source_revision ?? "无"} / {m.source_status ?? "none"}
                    ；兑现来源：{m.payoff_chapter_id ?? "无"} / 版本{" "}
                    {m.payoff_revision ?? "无"} / {m.payoff_status ?? "none"}
                  </p>
                  <p>
                    剧情位置：{m.story_order ?? 0} · {m.story_time ?? "未指定"}{" "}
                    · {m.tracking_status ?? "未指定状态"}
                  </p>
                  {Object.entries(m.fields).map(([k, v]) => (
                    <p key={k}>
                      <strong>{k}</strong>
                      <span style={{ whiteSpace: "pre-wrap" }}>{v}</span>
                    </p>
                  ))}
                  {detail.pack.foreshadow_occurrences?.filter(o=>o.material_id===m.id).map(o=>(
                    <section key={o.id}>
                      <h4>冻结伏笔出现 · {occurrenceRoles[o.role]}</h4>
                      <p>{o.source.volume_title} · {o.source.chapter_title} · 正文 v{o.source.chapter_revision}</p>
                      <blockquote className="foreshadow-quote">{o.quote}</blockquote>
                      {o.note&&<p className="foreshadow-quote">{o.note}</p>}
                      {sourceReader && p && state && o.book_id === bookId && !state.packs.some(x => x.id === detail.pack.id && x.archived) &&
                        detail.pack.originating_epoch === state.epoch && p.live_epoch === state.epoch && (
                        <FrozenForeshadowSource
                          key={JSON.stringify([bookId, state.epoch, detail.pack.id, p.id, o.id])}
                          sourceReader={sourceReader} occurrence={o} bookId={bookId}
                          epoch={state.epoch} isScopeCurrent={isScopeCurrent}
                          onSourceProblem={() => setSourceProblem(sourceProblemKey)}
                        />
                      )}
                    </section>
                  ))}
                  {matches?.length ? (
                    <>
                      <h4>本卡引用（由智能体提供）</h4>
                      {matches.map(citation)}
                    </>
                  ) : null}
                  {reviewBaseChanged && saved && (
                    <p className="author-review-note">
                      当前已保存：{verdictLabels[saved.verdict]}
                      {saved.note && ` · ${saved.note}`}
                    </p>
                  )}
                  {item && (canDecide || reviewDirty) ? (
                    <fieldset disabled={disabled || !canEditReview}>
                      <legend>逐项作者审阅</legend>
                      <label>
                        作者判断：{m.title}
                        <select
                          value={item.verdict}
                          onChange={(e) =>
                            changeItem(m.id, {
                              verdict: e.target.value as AuthorReviewVerdict,
                            })
                          }
                        >
                          {Object.entries(verdictLabels).map(
                            ([value, label]) => (
                              <option key={value} value={value}>
                                {label}
                              </option>
                            ),
                          )}
                        </select>
                      </label>
                      <label>
                        作者备注：{m.title}
                        <textarea
                          value={item.note}
                          onChange={(e) =>
                            changeItem(m.id, { note: e.target.value })
                          }
                        />
                      </label>
                      <small>
                        {encoder.encode(item.note).length} / 4096 UTF-8 字节
                      </small>
                    </fieldset>
                  ) : null}
                  {saved && !canDecide && !reviewBaseChanged && (
                    <>
                      <p>作者判断：{verdictLabels[saved.verdict]}</p>
                      {saved.note && (
                        <p className="author-review-note">
                          作者备注：{saved.note}
                        </p>
                      )}
                    </>
                  )}
                </details>
              );
            })}
            {p && (
              <>
                <div className="history-comparison">
                  <article>
                    <h4>冻结原稿</h4>
                    <pre>{detail.pack.chapter.body}</pre>
                  </article>
                  <article>
                    <h4>建议新稿</h4>
                    <pre>{p.input.new_body}</pre>
                  </article>
                </div>
                {p.input.citations
                  .filter(
                    (q) =>
                      !detail.pack.materials.some(
                        (m) => materialHandle(detail.pack, m.id) === q.entry_id,
                      ) &&
                      !(detail.pack.context_chapters ?? []).some(
                        ({ chapter }) => q.entry_id === `context:${chapter.id}`,
                      ),
                  )
                  .map(citation)}
                {canDecide && (
                  <>
                    <p>
                      每项判断均由作者填写。可以保存未完成的审阅，但所有必需资料均已保存为“符合”才能采纳。
                    </p>
                    <button
                      disabled={
                        disabled ||
                        !canEditReview ||
                        observedSourceProblem ||
                        !reviewDirty ||
                        !reviewFits ||
                        reviewBaseChanged
                      }
                      onClick={() => void saveReview()}
                    >
                      保存逐项审阅
                    </button>
                    {reviewDirty && !reviewFits && (
                      <p>每项备注最多4096 UTF-8字节，整份审阅编码最多64KiB。</p>
                    )}
                  </>
                )}
                {reviewBaseChanged && canEditReview && (
                  <button
                    disabled={disabled}
                    onClick={() => setReviewBaseChanged(false)}
                  >
                    已核对较新版本，保留我的草稿
                  </button>
                )}
                {observedSourceProblem && (
                  <p>已核对到此旧建议的来源与冻结依据不一致。不能保存或采纳此旧建议；请先处理来源，再冻结新依据。</p>
                )}
                {canRetainNotes && (
                  <>
                    <p>只暂存备注文本，不保留本次未保存的判断。关闭应用会丢失副本。</p>
                    <button type="button" disabled={disabled} onClick={retainNotes}>
                      保留备注副本并退出本次编辑
                    </button>
                  </>
                )}
                {reviewDirty && (
                  <button
                    disabled={disabled}
                    onClick={() => {
                      setDraft(storedItems(p, detail.pack));
                      setReviewBaseChanged(false);
                      clearConfirmation();
                    }}
                  >
                    放弃未保存的审阅修改
                  </button>
                )}
                <FrozenOutlineComparison
                  key={JSON.stringify([bookId, chapter.id, p.live_epoch, detail.pack.id, p.id])}
                  pack={detail.pack}
                  proposal={p}
                />
                <h4>智能体自报冲突</h4>
                {p.input.conflicts.length ? (
                  p.input.conflicts.map((q, i) => (
                    <p key={i}>
                      {q.entry_ids.join("、")}：{q.explanation}
                    </p>
                  ))
                ) : (
                  <p>未自报冲突，不等于系统验证无冲突。</p>
                )}
                <button
                  disabled={disabled || !canAccept}
                  onClick={() => {
                    setAck(false);
                    setConfirmedReviewRevision(p.author_review!.revision);
                    setConfirm("accept");
                  }}
                >
                  采纳建议
                </button>
                <button
                  disabled={
                    disabled || !canDecide || configDirty || reviewDirty
                  }
                  onClick={() => setConfirm("reject")}
                >
                  拒绝建议
                </button>
              </>
            )}
          </>
        )}
        {confirm && (
          <div role="alertdialog" aria-label="确认写作操作">
            <p>
              {confirm === "configure"
                ? mode === "disabled"
                  ? "确认停用约束，允许以后提交无约束提案，并取消本章全部待审正文建议？"
                  : "确认更新配置并取消本章全部待审正文建议？"
                : confirm === "accept"
                  ? `确认以建议新稿替换正文？将绑定逐项作者审阅版本 ${confirmedReviewRevision}，系统会重新核对全部依赖。`
                  : "确认拒绝该建议？"}
            </p>
            {confirm === "accept" && !!p?.input.conflicts.length && (
              <label>
                <input
                  type="checkbox"
                  checked={ack}
                  onChange={(e) => setAck(e.target.checked)}
                />
                已人工核对并接受上述未解决冲突
              </label>
            )}
            <button disabled={disabled} onClick={clearConfirmation}>
              返回审阅
            </button>
            <button
              disabled={
                disabled ||
                (confirm === "accept" &&
                  (!canAccept ||
                    confirmedReviewRevision !== p?.author_review?.revision)) ||
                (confirm === "accept" && !!p?.input.conflicts.length && !ack)
              }
              onClick={() => void write(confirm)}
            >
              {confirm === "configure"
                ? "确认更新配置"
                : confirm === "accept"
                  ? "确认采纳"
                  : "确认拒绝"}
            </button>
          </div>
        )}
        {needsRead && (
          <button
            disabled={busy}
            onClick={() =>
              void (reviewAttempt.current ? reconcileReview() : refresh(true))
            }
          >
            重新读取核对
          </button>
        )}
        {message && <p role="status">{message}</p>}
      </div>
    </Dialog>
  );
}
