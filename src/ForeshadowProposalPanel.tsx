import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { errorMessage } from "./workflowTypes";
import {
  occurrenceRoles,
  type DecideForeshadowProposalInput,
  type ForeshadowProposal,
  type ForeshadowProposalAPI,
  type ForeshadowProposalPage,
  type ForeshadowProposalReceipt,
  type ForeshadowProposalSummary,
  type OccurrenceView,
  type SourcePosition,
} from "./foreshadowTypes";

type Action = DecideForeshadowProposalInput["action"];
export type ProposalMode = { materialId?: string };
type Props = {
  api: ForeshadowProposalAPI;
  session?: ForeshadowReviewSession;
  bookId: string;
  epoch: string;
  getEpoch: () => Promise<string>;
  listOccurrences: (book: string, card: string, epoch: string) => Promise<OccurrenceView[]>;
  mode: ProposalMode | null;
  onOpen: () => void;
  onClose: () => void;
  onBlocking: (value: boolean) => void;
  onChanged: () => void;
  onEpochChanged: (error: unknown) => void;
  disabled: boolean;
  invalidated: boolean;
  isEpochInvalidated: () => boolean;
};
type Attempt = { action: Action; evidence: ForeshadowProposal };
type SessionEntry = { epoch: string; attempt?: Attempt; terminal?: ForeshadowProposal };
// Only in-memory for this running App. Epoch/owner is part of every key.
export type ForeshadowReviewSession = Map<string, SessionEntry>;
const statuses = {pending:"待审",accepted:"已采纳",rejected:"已拒绝",cancelled:"已撤回"};
const sourceStatus = {current:"来源仍匹配提交版本",stale:"来源已变化",unavailable:"来源不可用"};
const statusCode = (err: unknown) =>
  typeof err === "object" && err !== null && "code" in err ? err.code : undefined;
const epochError = {code:"epoch_changed", message:"书库已更换，仅保留旧书库证据。"};

function sameIdentity(a: ForeshadowProposalReceipt, b: ForeshadowProposalReceipt) {
  return a.id === b.id && a.book_id === b.book_id && a.material_id === b.material_id &&
    a.material_revision === b.material_revision && a.chapter_id === b.chapter_id &&
    a.chapter_revision === b.chapter_revision && a.created_at === b.created_at;
}
function samePosition(a: SourcePosition, b: SourcePosition) {
  return a.chapter_id === b.chapter_id && a.chapter_title === b.chapter_title &&
    a.chapter_revision === b.chapter_revision && a.chapter_sort_order === b.chapter_sort_order &&
    a.volume_id === b.volume_id && a.volume_title === b.volume_title && a.volume_sort_order === b.volume_sort_order;
}
function sameEvidence(a: ForeshadowProposal, b: ForeshadowProposal) {
  return sameIdentity(a.receipt,b.receipt) && samePosition(a.source,b.source) &&
    a.start === b.start && a.end === b.end && a.quote === b.quote &&
    a.role === b.role && a.note === b.note && a.reason === b.reason;
}
function linkedMatches(p: ForeshadowProposal) {
  const o = p.accepted_occurrence?.occurrence;
  return !!o && !!p.receipt.accepted_occurrence_id && o.id === p.receipt.accepted_occurrence_id &&
    o.book_id === p.receipt.book_id && o.material_id === p.receipt.material_id &&
    samePosition(o.source,p.source) && o.start === p.start && o.end === p.end &&
    o.quote === p.quote && o.role === p.role && o.note === p.note &&
    Number.isSafeInteger(o.created_at) && o.created_at >= 0 &&
    Number.isSafeInteger(o.revision) && o.revision >= 0 &&
    o.revision % 2 === Number(o.deleted_at !== null) &&
    (o.deleted_at === null || (Number.isSafeInteger(o.deleted_at) && o.deleted_at >= o.created_at)) &&
    p.receipt.decided_at !== null && o.created_at <= p.receipt.decided_at;
}
function terminalShape(data: ForeshadowProposal, allowArchivedMissing = false) {
  const {receipt:r} = data;
  return r.decided_at !== null && Number.isSafeInteger(r.decided_at) && r.decided_at >= r.created_at &&
    (r.status === "accepted"
      ? linkedMatches(data) || (allowArchivedMissing && data.archived && !!r.accepted_occurrence_id && data.accepted_occurrence === null)
      : (r.status === "rejected" || r.status === "cancelled") && r.accepted_occurrence_id === null && data.accepted_occurrence === null);
}
function isAttemptedOutcome(data: ForeshadowProposal, attempt: Attempt) {
  return data.receipt.status === (attempt.action === "accept" ? "accepted" : "rejected");
}
function terminalMessage(data: ForeshadowProposal, attempt?: Attempt) {
  const actual = data.receipt.status === "accepted"
    ? data.accepted_occurrence?.occurrence.deleted_at !== null
      ? "已核实采纳，关联出现现已回收。"
      : "已登记出现。"
    : data.receipt.status === "rejected" ? "已拒绝建议。" : "该建议已撤回。";
  return attempt && !isAttemptedOutcome(data,attempt)
    ? actual + (attempt.action === "accept" ? "本次采纳未生效。" : "本次拒绝未生效。")
    : actual;
}

export default function ForeshadowProposalPanel({
  api,session,bookId,epoch,getEpoch,listOccurrences,mode,onOpen,onClose,onBlocking,onChanged,onEpochChanged,disabled,invalidated,isEpochInvalidated,
}: Props) {
  const [page,setPage] = useState<ForeshadowProposalPage>();
  const [count,setCount] = useState<number>();
  const [listError,setListError] = useState("");
  const [listLoading,setListLoading] = useState(false);
  const [picked,setPicked] = useState<ForeshadowProposalSummary>();
  const [detail,setDetail] = useState<ForeshadowProposal>();
  const [detailLoading,setDetailLoading] = useState(false);
  const [detailError,setDetailError] = useState("");
  const [compared,setCompared] = useState<OccurrenceView[]>();
  const [comparisonError,setComparisonError] = useState("");
  const [confirm,setConfirm] = useState(false);
  const [writing,setWriting] = useState(false);
  const [readWaiting,setReadWaiting] = useState(false);
  const busy = writing || readWaiting;
  const [unknown,setUnknown] = useState<Attempt>();
  const [decisionMessage,setDecisionMessage] = useState("");
  const [decisionError,setDecisionError] = useState("");
  const [obsolete,setObsolete] = useState(false);
  const isObsolete = invalidated || obsolete;
  const writeLock = useRef(false);
  const readRun = useRef<symbol | null>(null);
  const life = useRef(0), listGen = useRef(0), detailGen = useRef(0), comparisonGen = useRef(0);
  const notifyBlocking = useRef(onBlocking);
  notifyBlocking.current = onBlocking;
  const material = mode?.materialId ?? null;
  const open = mode !== null;
  const openButton = useRef<HTMLButtonElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const fallbackSession = useRef<ForeshadowReviewSession>(new Map());
  const memory = session ?? fallbackSession.current;
  const key = (r: ForeshadowProposalReceipt) => JSON.stringify([epoch,r.book_id,r.material_id,r.id]);
  const entry = (r: ForeshadowProposalReceipt) => memory.get(key(r));
  const withoutAttempt = (r: ForeshadowProposalReceipt) => {
    const terminal = entry(r)?.terminal;
    if (terminal) memory.set(key(r),{epoch,terminal});
    else memory.delete(key(r));
  };

  function rememberTerminal(data: ForeshadowProposal) {
    const scope = key(data.receipt), prior = memory.get(scope), old = prior?.terminal;
    if (old) {
      const a = data.receipt, b = old.receipt;
      if (!sameEvidence(data,old) || a.status !== b.status || a.decided_at !== b.decided_at || a.accepted_occurrence_id !== b.accepted_occurrence_id) {
        throw Error("读取内容与已核实的终态不符");
      }
      const next = data.accepted_occurrence?.occurrence, prev = old.accepted_occurrence?.occurrence;
      if (next && prev) {
        if (next.created_at !== prev.created_at || (next.revision === prev.revision && next.deleted_at !== prev.deleted_at)) {
          throw Error("读取关联与已核实的终态不符");
        }
        if (next.revision < prev.revision) return old;
      }
    }
    memory.set(scope,{...prior,epoch,terminal:data});
    return data;
  }

  function projectPage(next: ForeshadowProposalPage) {
    let suppressed = 0;
    const items = next.items.flatMap(item => {
      const known = entry(item.receipt)?.terminal;
      if (!known || entry(item.receipt)?.attempt || !sameIdentity(known.receipt,item.receipt)) return [item];
      if (item.receipt.status === "pending") suppressed++;
      return material === null ? [] : [{receipt:known.receipt,material_title:known.material_title,archived:known.archived}];
    });
    return {...next,items,pending_count:Math.max(0,next.pending_count-suppressed)};
  }

  function closeReview() {
    if (writeLock.current || readRun.current) return;
    onClose();
    if (!material) requestAnimationFrame(() => openButton.current?.focus());
  }

  useLayoutEffect(() => {
    // Invalidate before continuations can affect a committed new scope.
    setReadWaiting(false);
    return () => { readRun.current = null; };
  }, [bookId,epoch,api,open,material,invalidated,memory]);
  useLayoutEffect(() => {
    // A real write has no stop control. During an explicit readback, the
    // author can stop waiting and release only that read's closure guard.
    notifyBlocking.current(busy);
  }, [busy]);
  useLayoutEffect(() => () => notifyBlocking.current(false), []);

  function endReadWait() {
    if (!readRun.current) return;
    readRun.current = null;
    setReadWaiting(false);
  }
  function stopReadWait() {
    if (!readRun.current) return;
    endReadWait();
    setDecisionMessage("已停止等待核对。停止等待不代表原写入失败，不会重发。");
    heading.current?.focus();
  }
  function lostLibrary() {
    endReadWait();
    setObsolete(true);
    listGen.current++;
    detailGen.current++;
    comparisonGen.current++;
    onEpochChanged(epochError);
  }
  useEffect(() => {
    life.current++;
    // An old window keeps its evidence on a detected restore. Opening a fresh
    // window with a verified new epoch no longer needs old-epoch cache entries.
    if (epoch) for (const [scope, note] of memory) if (note.epoch !== epoch) memory.delete(scope);
    setObsolete(false);
    writeLock.current = false;
    setWriting(false);
    setPage(undefined);
    setCount(undefined);
    setListError("");
    setUnknown(undefined);
    setDecisionMessage("");
    setDecisionError("");
    return () => { life.current++; };
  }, [bookId,epoch,api]);

  async function loadPage(after: string | null = null) {
    if (!epoch || isObsolete) return;
    const g = ++listGen.current, l = life.current;
    const active = () => l === life.current && g === listGen.current;
    setListError("");
    setListLoading(true);
    try {
      const next = await api.list(bookId,material,material === null,after,epoch);
      if (!active()) return;
      if (!Number.isInteger(next.pending_count) || next.pending_count < 0 || next.pending_count > 100 ||
        next.items.some(item => item.receipt.book_id !== bookId ||
          (material !== null && item.receipt.material_id !== material) ||
          (material === null && (item.archived || item.receipt.status !== "pending"))) ||
        (next.next_after_id !== null && next.next_after_id !== next.items.at(-1)?.receipt.id)) {
        throw Error("建议列表身份不匹配");
      }
      const projected = projectPage(next);
      setPage(projected);
      if (material === null) setCount(projected.pending_count);
    } catch (error) {
      if (!active()) return;
      if (statusCode(error) === "epoch_changed") lostLibrary();
      else {
        if (material === null) setCount(undefined);
        setListError(`建议列表读取失败：${errorMessage(error)}`);
      }
    } finally {
      if (active()) setListLoading(false);
    }
  }

  useEffect(() => {
    setPage(undefined);
    setPicked(undefined);
    setDetail(undefined);
    setCompared(undefined);
    setComparisonError("");
    setDetailError("");
    setConfirm(false);
    setUnknown(undefined);
    setDecisionMessage("");
    setDecisionError("");
    setDetailLoading(false);
    detailGen.current++;
    comparisonGen.current++;
    if (!isObsolete) void loadPage();
    // Mode and scope define a fresh list and detail lifetime.
  }, [bookId,epoch,api,open,material]);
  useEffect(() => {
    if (open) heading.current?.focus();
  }, [open]);

  async function compare(row: ForeshadowProposal) {
    const g = ++comparisonGen.current, l = life.current;
    const active = () => comparisonGen.current === g && life.current === l;
    setComparisonError("");
    setCompared(undefined);
    try {
      const rows = await listOccurrences(bookId,row.receipt.material_id,epoch);
      if (!active()) return;
      if (rows.some(v=>v.occurrence.book_id !== bookId || v.occurrence.material_id !== row.receipt.material_id)) {
        throw Error("正式出现身份不匹配");
      }
      setCompared(rows);
    } catch (error) {
      if (!active()) return;
      if (statusCode(error) === "epoch_changed") lostLibrary();
      else setComparisonError(`对照读取失败：${errorMessage(error)}`);
    }
  }
  async function select(item: ForeshadowProposalSummary, preserveDecisionError = false) {
    if (isObsolete || writeLock.current || readRun.current) return;
    const gen = ++detailGen.current, l = life.current;
    const active = () => gen === detailGen.current && l === life.current;
    const r = item.receipt, cached = entry(r), known = cached?.terminal;
    const held = cached?.attempt;
    comparisonGen.current++;
    setPicked(item);
    setUnknown(held);
    setDetail(known ?? held?.evidence);
    setCompared(undefined);
    setComparisonError("");
    setDetailError("");
    setConfirm(false);
    if (!known || picked?.receipt.id !== r.id) setDecisionMessage("");
    if (held) {
      setDetailLoading(false);
      setDecisionError("这条建议的上一决定仍待只读核对。离开不等于取消或失败，不会重发。");
      void compare(known ?? held.evidence);
      return;
    }
    if (!preserveDecisionError) setDecisionError("");
    setDetailLoading(true);
    try {
      const result = await api.get(bookId,r.material_id,r.id,epoch);
      if (!active()) return;
      if (!sameIdentity(r,result.receipt) || result.archived !== item.archived ||
        result.source.chapter_id !== r.chapter_id || result.source.chapter_revision !== r.chapter_revision) {
        throw Error("建议详情或正式出现关联不匹配");
      }
      if (known && !terminalShape(result,true)) throw Error("读取内容不能覆盖已核实的终态");
      if (result.receipt.status === "pending"
        ? result.receipt.decided_at !== null || result.receipt.accepted_occurrence_id !== null || result.accepted_occurrence !== null
        : !terminalShape(result,true)) {
        throw Error("建议决定或关联出现格式不匹配");
      }
      const current = result.receipt.status === "pending" ? result : rememberTerminal(result);
      if (known && !known.archived && !current.archived && known.accepted_occurrence?.occurrence.revision !== current.accepted_occurrence?.occurrence.revision) {
        setDecisionMessage(terminalMessage(current));
      }
      setDetail(current);
      void compare(current);
    } catch (error) {
      if (!active()) return;
      if (statusCode(error) === "epoch_changed") lostLibrary();
      else setDetailError(`建议详情读取失败：${errorMessage(error)}`);
    } finally {
      if (active()) setDetailLoading(false);
    }
  }

  async function checkEpoch() {
    if (await getEpoch() !== epoch) throw epochError;
  }
  function settle(result: ForeshadowProposal, attempt: Attempt) {
    withoutAttempt(result.receipt);
    setDetail(result);
    setUnknown(undefined);
    setConfirm(false);
    setDecisionError("");
    setDecisionMessage(terminalMessage(result,attempt));
    setPage(old => old ? projectPage(old) : old);
    if (material === null) setCount(old => old === undefined ? old : Math.max(0, old-1));
    void loadPage();
    void compare(result);
    onChanged();
  }
  async function decide(action: Action) {
    if (disabled || !detail || !mutable || writeLock.current || readRun.current || (action === "accept" && (!canAccept || !confirm))) return;
    const target = {action,evidence:detail};
    const r = detail.receipt, l = life.current;
    writeLock.current = true;
    setWriting(true);
    setDecisionError("");
    let invoked = false, exact = false;
    let reselect: ForeshadowProposalSummary | undefined;
    try {
      await checkEpoch();
      if (l !== life.current) return;
      invoked = true;
      memory.set(key(r),{epoch,attempt:target});
      const result = await api.decide({
        book_id:bookId,material_id:r.material_id,proposal_id:r.id,expected_epoch:epoch,
        expected_material_revision:r.material_revision,action,
      });
      if (l !== life.current) return;
      if (!sameEvidence(result,target.evidence) || result.archived || !terminalShape(result) || !isAttemptedOutcome(result,target)) {
        throw Error("决定回执与建议或正式出现不一致");
      }
      const evidence = rememberTerminal(result);
      exact = true;
      // Keep an exact receipt as old evidence even if the next epoch check
      // detects a restore or a later read fails.
      setDetail(evidence);
      await checkEpoch();
      if (l === life.current) settle(evidence,target);
    } catch (error) {
      if (l !== life.current) return;
      const code = statusCode(error);
      if (code === "epoch_changed") { lostLibrary(); setConfirm(false); }
      else if (!invoked || (!exact && (code === "revision_conflict" || code === "invalid_state" || code === "not_found" || code === "invalid_input" || code === "limit_exceeded"))) {
        if (invoked) withoutAttempt(r);
        setDecisionError(errorMessage(error));
        setConfirm(false);
        if (invoked) reselect = picked;
      } else {
        setUnknown(target);
        setDecisionError(`决定结果未核实：${errorMessage(error)}。请只读核对，不要重发。`);
      }
    } finally {
      if (l === life.current) {
        writeLock.current = false;
        setWriting(false);
        if (reselect) void select(reselect,true);
      }
    }
  }

  async function reconcile() {
    if (!open || !unknown || isObsolete || isEpochInvalidated() || writeLock.current || readRun.current) return;
    const attempt = unknown;
    const run = Symbol();
    const active = () => readRun.current === run && !isEpochInvalidated();
    readRun.current = run;
    setReadWaiting(true);
    setDecisionError("");
    setDecisionMessage("");
    try {
      await checkEpoch();
      if (!active()) return;
      const r = attempt.evidence.receipt;
      const result = await api.get(bookId,r.material_id,r.id,epoch);
      if (!active()) return;
      const same = sameEvidence(result,attempt.evidence) && !result.archived;
      const terminal = same && terminalShape(result) ? rememberTerminal(result) : undefined;
      // Keep exact, terminal evidence already returned for the old epoch,
      // regardless of whether the independent follow-up check finishes.
      if (terminal) setDetail(terminal);
      await checkEpoch();
      if (!active()) return;
      if (terminal) settle(terminal,attempt);
      else setDecisionError(same && result.receipt.status === "pending" &&
        result.receipt.decided_at === null && result.receipt.accepted_occurrence_id === null && result.accepted_occurrence === null
        ? "该建议仍待审，原写入尚无可核实的回执。请稍后再次只读核对，不要重发。"
        : "读取的决定或关联出现与本次操作不一致，不能确认结果。");
    } catch (error) {
      if (!active()) return;
      if (statusCode(error) === "epoch_changed") lostLibrary();
      else setDecisionError(`只读核对失败：${errorMessage(error)}`);
    } finally {
      if (active()) endReadWait();
    }
  }

  const locked = busy || (!!unknown && !isObsolete);
  const mutable = !!detail && detail.receipt.status === "pending" && !detail.archived && !isObsolete && !locked && !detailLoading;
  const canAccept = mutable && detail!.card_current && detail!.source_status === "current" && compared !== undefined;
  if (!epoch) return null;
  // A decision could have reached the terminal state while the author was
  // away. Keep its same-epoch recovery route reachable even without a live
  // pending-list item (or a currently active card).
  const unlisted = !open || isObsolete ? [] : [...memory.values()].flatMap(({epoch:origin, attempt}) => {
    if (!attempt || origin !== epoch) return [];
    const {receipt:r,material_title,archived} = attempt.evidence;
    return r.book_id === bookId && (material === null || r.material_id === material)
      && !page?.items.some(item => item.receipt.id === r.id && item.receipt.material_id === r.material_id)
      ? [{receipt:r,material_title,archived}] : [];
  });
  return (
    <>
      <button ref={openButton} hidden={open} disabled={disabled || isObsolete} onClick={onOpen}>
        出现建议 · {count === undefined ? listLoading ? "正在读取数量" : "数量不可用" : `${count} 条待审`}
      </button>
      {open && <section className="foreshadow-proposals" aria-label="伏笔出现建议">
        <header>
          <h3 ref={heading} tabIndex={-1}>{material ? "此卡建议历史" : "待审出现建议"}</h3>
          <button type="button" disabled={locked} onClick={closeReview}>返回全书伏笔</button>
        </header>
        <p>这是建议，不是已回收。采纳只登记出现，不改正文或作者状态。</p>
        {isObsolete && <p role="alert">书库已更换，仅保留旧书库证据。请退出后重新打开，不会向新库写入。</p>}
        {listError && <p role="alert">{listError}</p>}
        {decisionError && <p role="alert">{decisionError}</p>}
        {decisionMessage && <p role="status">{decisionMessage}</p>}
        {!isObsolete && <button disabled={busy} onClick={() => void loadPage()}>刷新建议</button>}
        {listLoading && !isObsolete && <p>读取建议列表中…</p>}
        {page?.items.length === 0 && !listError && !listLoading && !isObsolete && <p>{
          page.next_after_id ? "本页没有可显示的建议，可翻到下一页。"
            : !material && page.pending_count > 0 ? "本页没有可显示的建议，可刷新回到第一页。"
              : material ? "此卡没有建议历史。" : "没有待审出现建议。"
        }</p>}
        {!!page?.items.length && <ul aria-label="建议列表">{page.items.map(item=>
          <li key={item.receipt.id}><button aria-pressed={picked?.receipt.id === item.receipt.id} disabled={busy || isObsolete}
            onClick={() => void select(item)}>
            {item.material_title} · {statuses[item.receipt.status]}{item.archived ? "（归档）" : ""} · 正文 v{item.receipt.chapter_revision}
          </button></li>
        )}</ul>}
        {!!unlisted.length && <ul aria-label="仍待只读核对的决定">{unlisted.map(item =>
          <li key={item.receipt.id}><button aria-pressed={picked?.receipt.id === item.receipt.id} disabled={busy}
            onClick={() => void select(item)}>{item.material_title} · 待核对</button></li>
        )}</ul>}
        {page?.next_after_id && <button disabled={busy || isObsolete} onClick={() => void loadPage(page.next_after_id)}>下一页建议</button>}
        {detailLoading && !isObsolete && <p>读取建议详情中…</p>}
        {detailError && <p role="alert">{detailError}</p>}
        {picked && !isObsolete && !unknown && (detail || detailError) &&
          <button disabled={busy || detailLoading} onClick={() => void select(picked)}>重读建议详情</button>}
        {detail && <article aria-label="所选建议证据">
          <h4>{detail.material_title} · {statuses[detail.receipt.status]}{detail.archived && "（归档）"}</h4>
          <p>{detail.source.volume_title}（位置 {detail.source.volume_sort_order + 1}） / {detail.source.chapter_title}（位置 {detail.source.chapter_sort_order + 1}） · 已保存正文 v{detail.source.chapter_revision} · 伏笔卡 v{detail.receipt.material_revision}</p>
          <p>{sourceStatus[detail.source_status]} · 标量区间 [{detail.start}, {detail.end})</p>
          <p>建议角色：{occurrenceRoles[detail.role]}</p>
          <blockquote className="foreshadow-quote">{detail.quote}</blockquote>
          <h5>建议备注</h5><p className="foreshadow-quote">{detail.note || "未填写"}</p>
          <h5>建议理由</h5><p className="foreshadow-quote">{detail.reason || "未填写"}</p>
          {detail.archived && <p>备份证据，仅供查看，不能采纳或拒绝。</p>}
          {detail.receipt.status === "pending" && !detail.archived && (!detail.card_current || detail.source_status !== "current") &&
            <p>伏笔卡或来源不可用、已变化或待复核，不能采纳。请让智能体按当前版本重新提交；作者仍可拒绝。</p>}
          {detail.receipt.accepted_occurrence_id && <p>关联正式出现：{detail.receipt.accepted_occurrence_id}
            {detail.accepted_occurrence ? detail.accepted_occurrence.occurrence.deleted_at !== null ? "（已回收）" : "（已登记）" : "（现已不可用）"}</p>}
          {unknown && detail.receipt.status !== "pending" && <p>已保留终态证据，尚未完成同一书库的核对。</p>}
          <section aria-label="正式出现对照">
            <h5>正式出现对照</h5>
            {comparisonError ? <p role="alert">{comparisonError} {!isObsolete && <button disabled={locked} onClick={() => void compare(detail)}>重读对照</button>}</p> :
              compared ? compared.length ? <ul>{compared.map(({occurrence:o})=>
                <li key={o.id}>{occurrenceRoles[o.role]} · {o.source.volume_title} / {o.source.chapter_title} · 正文 v{o.source.chapter_revision}{o.deleted_at !== null && "（已回收）"}
                  <blockquote className="foreshadow-quote">{o.quote}</blockquote>
                </li>
              )}</ul> : <p>此卡还没有正式出现。</p> : !isObsolete && <p>读取正式出现对照中…</p>}
          </section>
          {detail.receipt.status === "pending" && !detail.archived && !isObsolete &&
            <div className="foreshadow-proposal-actions">
              <button disabled={!canAccept || confirm || disabled} onClick={() => setConfirm(true)}>审阅后采纳</button>
              <button disabled={!mutable || confirm || disabled} onClick={() => void decide("reject")}>拒绝建议</button>
              {confirm && !unknown && <section aria-label="确认登记">
                <p>将这个精确片段登记为正式出现，不改伏笔作者状态。此卡上基于旧版本的其他建议会失效，需重新提交。</p>
                <button disabled={!canAccept || disabled} onClick={() => void decide("accept")}>确认并登记出现</button>
                <button disabled={locked} onClick={() => setConfirm(false)}>取消确认</button>
              </section>}
            </div>
          }
        </article>}
        {unknown && !isObsolete && <section aria-label="待核对决定">
          <p>{readWaiting ? "正在只读核对。若一直没有返回，可停止等待后退出。"
            : "可以暂离。离开不等于取消或失败；本次运行中，重开后仍要先只读核对这条建议。"}</p>
          <button disabled={busy} onClick={() => void reconcile()}>只读核对决定</button>
          {readWaiting && <button onClick={stopReadWait}>停止等待核对</button>}
          <button disabled={busy} onClick={closeReview}>暂离，稍后核对</button>
        </section>}
      </section>}
    </>
  );
}
