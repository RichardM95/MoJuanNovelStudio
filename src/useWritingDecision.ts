import { useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { errorMessage } from "./workflowTypes";
import type { Chapter } from "./types";
import {
  acceptedBody,
  decisionProof,
  decisionFingerprint,
  matchesTerminal,
  type DecisionMarker,
  type DecisionRecord,
  type DecisionTarget,
  requestedDecision,
  terminalLabel,
  type DecisionAttempt,
  type DecisionEntry,
  type WritingDecisionSession,
} from "./writingDecision";
import type {
  WritingAPI,
  WritingChapterRead,
  WritingPack,
  WritingProposal,
} from "./writingTypes";

type Options = {
  api: WritingAPI;
  session: WritingDecisionSession;
  isScopeCurrent: () => boolean;
  onBlockingChange: (value: boolean) => void;
  readChapter: () => Promise<WritingChapterRead>;
  onVerified: (a: DecisionTarget, proposal: WritingProposal) => void;
  onChapter: (read: WritingChapterRead, publish: boolean) => void;
};
type Phase = "idle" | "writing" | "reading";
type BodyObservation = {
  target: DecisionTarget;
  label: string;
  acceptedRevision: number;
  observedRevision: number;
  comparison: "exact" | "later";
};
type DecisionNotice = { text: string; body?: BodyObservation };
const stopped = Symbol("writing decision read stopped");

function createReadRun(kind: "reconcile" | "detail") {
  let stop!: () => void;
  const signal = new Promise<void>((resolve) => {
    stop = resolve;
  });
  return {
    kind,
    stop,
    async wait<T>(request: () => Promise<T>): Promise<T> {
      // Release the local coroutine and any loaded chapter/publish snapshot
      // even if IPC never returns. Race consumes late rejection; IPC runs on.
      return Promise.race([
        request(),
        signal.then((): never => {
          throw stopped;
        }),
      ]);
    },
  };
}

export function useWritingDecision(options: Options) {
  const { session } = options;
  useSyncExternalStore(session.subscribe, session.snapshot);
  const latest = useRef(options);
  latest.current = options;
  const [phase, setPhase] = useState<Phase>("idle");
  const [notice, setNotice] = useState<DecisionNotice>({ text: "" });
  const setMessage = (text: string, body?: BodyObservation) =>
    setNotice({ text, body });
  const [obsolete, setObsolete] = useState(false);
  const live = useRef(false);
  const retired = useRef(false);
  const writeRun = useRef<symbol | null>(null);
  const readRun = useRef<ReturnType<typeof createReadRun> | null>(null);
  const generation = useRef(session.libraryGeneration);
  const isStale = () =>
    retired.current ||
    generation.current !== session.libraryGeneration ||
    !latest.current.isScopeCurrent();
  const isBusy = () => !!(readRun.current || writeRun.current);

  function changePhase(value: Phase) {
    latest.current.onBlockingChange(value !== "idle");
    setPhase(value);
  }
  function abandonRead() {
    const run = readRun.current;
    readRun.current = null;
    run?.stop();
  }
  function endRead() {
    abandonRead();
    if (!writeRun.current) changePhase("idle");
  }
  useLayoutEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
      abandonRead();
      writeRun.current = null;
    };
  }, []);
  useLayoutEffect(() => {
    if (!isStale()) return;
    retired.current = true;
    if (readRun.current) endRead();
    setObsolete(true);
    setNotice((previous) =>
      previous.body
        ? {
            text: `${previous.body.label} 仅是原书库的核对记录，未核对当前正文。`,
          }
        : previous,
    );
  });
  function noteChapterRefresh(read?: {
    epoch: string;
    bookId: string;
    chapter: Chapter;
  }) {
    setNotice((previous) => {
      const checked = previous.body;
      if (!checked) return previous;
      const { target, label, acceptedRevision, observedRevision, comparison } =
        checked;
      if (
        read &&
        (read.epoch !== target.epoch ||
          read.bookId !== target.bookId ||
          read.chapter.id !== target.chapterId)
      )
        return {
          text: `${label} 仅是原章节和书库的核对记录，未核对当前正文。`,
        };
      // Ordinary reloads do not hash/compare the proposal or do an epoch
      // sandwich. Preserve the decision, but never reassert current equality.
      if (
        read &&
        Number.isSafeInteger(read.chapter.revision) &&
        read.chapter.revision > acceptedRevision
      ) {
        const history = `曾采纳，正文后来已改变（采纳 v${acceptedRevision}，本次刷新读到 v${read.chapter.revision}）。`;
        return {
          ...previous,
          text: target.action === "accept" ? history : `${label} ${history}`,
        };
      }
      const history =
        comparison === "exact"
          ? `上次核对时正文 v${observedRevision} 与建议逐字一致`
          : `上次核对时读到正文 v${observedRevision}`;
      return {
        ...previous,
        text: `${label} ${history}；本次刷新未重新核对正文。`,
      };
    });
  }
  function stop() {
    const run = readRun.current;
    if (!run) return;
    endRead();
    setMessage(
      run.kind === "detail"
        ? "已停止等待建议详情。未修改正文。"
        : "已停止等待。仍可稍后只读核对；不表示原写入失败，也不会重发。",
    );
  }

  function beginRead(kind: "reconcile" | "detail") {
    const run = createReadRun(kind);
    readRun.current = run;
    changePhase("reading");
    setMessage(
      kind === "detail"
        ? "正在只读核对已处理的建议详情…"
        : "正在只读核对正文决定…",
    );
    return run;
  }
  const readActive = (
    run: ReturnType<typeof createReadRun>,
    record: DecisionRecord,
  ) =>
    live.current &&
    readRun.current === run &&
    !isStale() &&
    !session.isOld(record) &&
    session.has(record);

  async function originalEpoch(
    run: ReturnType<typeof createReadRun>,
    epoch: string,
    active: () => boolean,
  ) {
    const value = await run.wait(() => options.api.epoch());
    if (!active()) return false;
    if (typeof value !== "string" || !value.trim())
      throw Error("书库时代读取无效");
    if (value === epoch) {
      session.observeEpoch(value);
      return true;
    }
    // Fence read effects immediately, before React publishes the next render.
    retired.current = true;
    endRead();
    session.observeEpoch(value);
    setObsolete(true);
    setMessage("书库已更换。原书库证据仅供查看，未向当前书库重发决定。");
    return false;
  }

  async function readDetail(
    marker: DecisionMarker,
    show: (pack: WritingPack, q: WritingProposal) => void,
  ) {
    if (
      !live.current ||
      isStale() ||
      isBusy() ||
      session.isOld(marker) ||
      !session.has(marker)
    )
      return;
    const { epoch, bookId, id } = marker.target;
    const run = beginRead("detail");
    const active = () => readActive(run, marker);
    try {
      if (!(await originalEpoch(run, epoch, active)) || !active()) return;
      const raw = await run.wait(() => options.api.proposal(bookId, id));
      if (!active()) return;
      // The bridge may reuse objects. Check, hash, select the pack and display
      // only the same private snapshot, never the mutable object it handed us.
      const q = structuredClone(raw);
      if (!matchesTerminal(q, marker))
        throw Error("读取的建议状态与已核对的终态冲突；不会恢复决定权限。");
      if (!marker.fingerprint)
        throw Error("完整提案指纹不可用；保留已认证终态，不恢复决定权限。");
      const fingerprint = await run
        .wait(() => decisionFingerprint(q))
        .catch(() => null);
      if (!active()) return;
      if (!fingerprint)
        throw Error("完整提案指纹不可用；保留已认证终态，不恢复决定权限。");
      if (fingerprint !== marker.fingerprint)
        throw Error("读取的建议内容与已核对的终态冲突；不会恢复决定权限。");
      const rawPack = await run.wait(() =>
        options.api.pack(bookId, q.input.pack_id),
      );
      if (!active()) return;
      const pack = structuredClone(rawPack);
      if (!(await originalEpoch(run, epoch, active)) || !active()) return;
      show(pack, q);
      setMessage("");
    } catch (e) {
      if (active()) setMessage(errorMessage(e));
    } finally {
      if (active()) endRead();
    }
  }

  async function reconcile(record: DecisionRecord, diagnostic = "") {
    if (
      !live.current ||
      isStale() ||
      isBusy() ||
      session.isOld(record) ||
      !session.has(record) ||
      (record.kind === "terminal" && record.bodyChecked)
    )
      return;
    const { epoch, bookId, chapterId, id, action } = record.target;
    const a: DecisionTarget = { epoch, bookId, chapterId, id, action };
    const run = beginRead("reconcile");
    const active = () => readActive(run, record);
    try {
      if (!(await originalEpoch(run, epoch, active)) || !active()) return;
      const full = await run
        .wait(() => options.api.proposal(a.bookId, a.id))
        .then(
          (q) => ({ ok: true as const, q: structuredClone(q) }),
          (error: unknown) => ({ ok: false as const, error }),
        );
      if (!active()) return;
      const proof =
        full.ok && record.kind === "attempt"
          ? decisionProof(full.q, record.target)
          : null;
      if (record.kind === "attempt" && proof?.kind === "terminal")
        session.preserve(record, proof.evidence, "full-read");
      // Fingerprint exactly the authenticated payload so chapter-only recovery
      // can drop the stored manuscript, but never compare against a changed
      // same-ID proposal. If unavailable, this original full read still works.
      const candidate =
        full.ok &&
        (proof?.kind === "terminal" ||
          (record.kind === "terminal" && matchesTerminal(full.q, record)));
      const fingerprint =
        candidate && full.ok
          ? await run.wait(() => decisionFingerprint(full.q)).catch(() => null)
          : null;
      if (!active() || !(await originalEpoch(run, epoch, active)) || !active())
        return;
      if (!full.ok) throw full.error;

      if (record.kind === "attempt") {
        if (proof?.kind !== "terminal") {
          const detail =
            proof?.kind === "pending"
              ? "完整提案仍待审，本次结果未核实。"
              : "完整提案的决定、内容或审核与本次预期不匹配。";
          setMessage(`仍待核对：${detail}未重发。${diagnostic}`);
          return;
        }
        // Only this successful epoch sandwich is authoritative. Provisional
        // write/complete-read candidates cannot veto its terminal decision.
        const confirmed = session.authenticate(
          record,
          proof.evidence,
          fingerprint,
        );
        if (!confirmed) return;
        record = confirmed;
      } else if (
        !candidate ||
        !fingerprint ||
        fingerprint !== record.fingerprint
      ) {
        const reason =
          candidate && (!fingerprint || !record.fingerprint)
            ? "完整提案指纹不可用"
            : "完整提案与已认证终态证据冲突";
        setMessage(
          `${terminalLabel(record.evidence, a)} ${reason}，保留已认证结果，不恢复写入权限。`,
        );
        return;
      }

      const evidence = record.evidence;
      const label = terminalLabel(evidence, a);
      latest.current.onVerified(a, full.q);
      if (evidence.decision === "accepted") {
        const chapter = await run.wait(latest.current.readChapter).then(
          (read) => ({ ok: true as const, read }),
          (error: unknown) => ({ ok: false as const, error }),
        );
        if (
          !active() ||
          !(await originalEpoch(run, epoch, active)) ||
          !active()
        )
          return;
        if (!chapter.ok) throw chapter.error;
        const acceptedRevision = evidence.acceptedRevision!;
        const observedRevision = chapter.read.chapter.revision;
        const comparison = acceptedBody(
          chapter.read.chapter,
          a,
          acceptedRevision,
          full.q.input.new_body,
        );
        const observation: BodyObservation = {
          target: a,
          label,
          acceptedRevision,
          observedRevision,
          comparison,
        };
        const history = `曾采纳，正文后来已改变（采纳 v${acceptedRevision}，核对时读到 v${observedRevision}）。`;
        const bodyStatus =
          comparison === "later"
            ? a.action === "accept"
              ? history
              : `${label} ${history}`
            : `${label} 当前正文已核对，书库已保存的正文与建议逐字一致（核对版本 v${observedRevision}）。`;
        session.confirmBody(record);
        try {
          latest.current.onChapter(chapter.read, a.action === "accept");
        } catch (error) {
          setMessage(
            `${bodyStatus} 编辑器未刷新：${errorMessage(error)}`,
            observation,
          );
          return;
        }
        setMessage(bodyStatus, observation);
      } else {
        setMessage(`${label} 未按此决定刷新或更改正文。`);
      }
    } catch (e) {
      if (active()) {
        const detail = errorMessage(e);
        setMessage(
          record.kind === "terminal"
            ? `${terminalLabel(record.evidence, a)} 当前正文未核对：${detail}`
            : `仍待核对：只读核对失败：${detail}。未重发。${diagnostic}`,
        );
      }
    } finally {
      if (active()) endRead();
    }
  }

  async function submit(a: DecisionAttempt) {
    if (!live.current || isStale() || isBusy()) return;
    let entry: DecisionEntry;
    try {
      entry = session.admit(a);
    } catch (e) {
      setMessage(errorMessage(e));
      return;
    }
    const run = Symbol("writing decision write");
    writeRun.current = run;
    changePhase("writing");
    setMessage("正在提交正文决定，请等待写入返回。");
    const stillHere = () => live.current && writeRun.current === run;
    let diagnostic = "";
    try {
      const q =
        a.action === "accept"
          ? await options.api.acceptReviewed(
              a.bookId,
              a.id,
              a.policyRevision,
              a.chapterRevision,
              a.epoch,
              a.confirmedReview!.revision,
              a.conflictsAck,
            )
          : await options.api.decide(
              a.bookId,
              a.id,
              false,
              a.policyRevision,
              a.chapterRevision,
              a.epoch,
              false,
            );
      if (!stillHere() || isStale()) return;
      const proof = decisionProof(q, a);
      if (
        proof.kind === "terminal" &&
        proof.evidence.decision === requestedDecision(a.action)
      )
        session.preserve(entry, proof.evidence, "write");
      else diagnostic = " 写入回执与本次决定不匹配。";
    } catch (e) {
      if (!stillHere() || isStale()) return;
      diagnostic = ` 写入返回：${errorMessage(e)}`;
    } finally {
      if (stillHere()) {
        writeRun.current = null;
        if (isStale() || session.isOld(entry)) changePhase("idle");
        else void reconcile(entry, diagnostic);
      }
    }
  }

  return {
    phase,
    isBusy,
    obsolete,
    isStale,
    message: notice.text,
    noteChapterRefresh,
    submit,
    reconcile,
    readDetail,
    stop,
  };
}
