import React, {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import MaterialsPanel from "./MaterialsPanel";
import ForeshadowProposalPanel, { type ForeshadowReviewSession, type ProposalMode } from "./ForeshadowProposalPanel";
import type { Material, MaterialsAPI } from "./materialTypes";
import type { Chapter, Volume } from "./types";
import { errorMessage } from "./workflowTypes";
import { Dialog } from "./ui/Dialog";
import {
  exactExcerpt,
  occurrenceRoles,
  type ForeshadowAPI,
  type ForeshadowSummary,
  type OccurrenceInput,
  type OccurrenceRole,
  type OccurrenceView,
  type SavedSourceChapter,
  type SelectedExcerpt,
  type SourcePosition,
} from "./foreshadowTypes";

const authorStates: Record<string, string> = {
  planned: "待铺设",
  planted: "已铺设",
  resolved: "已回收",
  abandoned: "已放弃",
};
const sourceStates = {
  current: "来源版本未变化",
  stale: "来源已变化，待复核",
  unavailable: "来源不可用",
  none: "未设置",
};
const knownFailures = new Set([
  "invalid_input",
  "not_found",
  "limit_exceeded",
  "revision_conflict",
  "deleted",
  "invalid_state",
  "idempotency_conflict",
  "database_error",
  "clock_error",
]);
const code = (e: unknown) =>
  e && typeof e === "object" && "code" in e ? String(e.code) : "";
const needsReview = ({ material: m, review_count }: ForeshadowSummary) =>
  !!review_count ||
  !!m.content_review_required ||
  [m.source_status, m.payoff_status].some(
    (s) => s === "stale" || s === "unavailable",
  );
type Pending =
  | { kind: "create"; input: OccurrenceInput }
  | {
      kind: "lifecycle";
      id: string;
      revision: number;
      deleted: boolean;
      card: string;
      epoch: string;
    };

function indexDirectory(rows: SourcePosition[], bookId: string) {
  const volumes = new Map<string, Volume>();
  const chapters: Chapter[] = [];
  const counts = new Map<string, number>();
  const labels = new Map<string, string>();
  const names: {
    id: string;
    pair: string;
    name: string;
    volumeOrder: number;
    chapterOrder: number;
  }[] = [];
  for (const position of rows) {
    const {
      chapter_id: id,
      chapter_title: title,
      chapter_sort_order: chapterOrder,
      chapter_revision: revision,
      volume_id: volumeId,
      volume_title: volumeTitle,
      volume_sort_order: volumeOrder,
    } = position;
    if (!volumes.has(volumeId))
      volumes.set(volumeId, {
        id: volumeId,
        book_id: bookId,
        title: volumeTitle,
        sort_order: volumeOrder,
        deleted_at: null,
      });
    chapters.push({
      id,
      volume_id: volumeId,
      title,
      sort_order: chapterOrder,
      revision,
      deleted_at: null,
      body: "",
    });
    const pair = JSON.stringify([volumeTitle, title]);
    counts.set(pair, (counts.get(pair) ?? 0) + 1);
    names.push({
      id,
      pair,
      name: `${volumeTitle} · ${title}`,
      volumeOrder,
      chapterOrder,
    });
  }
  for (const { id, pair, name, volumeOrder, chapterOrder } of names) {
    labels.set(
      id,
      (counts.get(pair) ?? 0) > 1
        ? `${name}（卷 ${volumeOrder + 1} / 章 ${chapterOrder + 1} · ${id}）`
        : name,
    );
  }
  return { volumes: [...volumes.values()], chapters, labels };
}

export default function ForeshadowPanel({
  api,
  bookId,
  onClose,
  onJump,
  onPendingChange,
  proposalSession,
}: {
  api: ForeshadowAPI;
  bookId: string;
  onClose: () => void;
  onJump: (source: SourcePosition, epoch: string) => Promise<boolean>;
  onPendingChange?: (pending: boolean) => void;
  proposalSession?: ForeshadowReviewSession;
}) {
  const [epoch, setEpoch] = useState("");
  const [cards, setCards] = useState<ForeshadowSummary[]>([]);
  const [directory, setDirectory] = useState<SourcePosition[]>([]);
  const indexedDirectory = useMemo(
    () => indexDirectory(directory, bookId),
    [directory, bookId],
  );
  const [directoryError, setDirectoryError] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [status, setStatus] = useState("all");
  const [reviewOnly, setReviewOnly] = useState(false);
  const [selected, setSelected] = useState("");
  const [rows, setRows] = useState<OccurrenceView[]>([]);
  const [reading, setReading] = useState(false);
  const [chapterId, setChapterId] = useState("");
  const [chapter, setChapter] = useState<SavedSourceChapter | null>(null);
  const [sourceReading, setSourceReading] = useState(false);
  const [excerpt, setExcerpt] = useState<SelectedExcerpt | null>(null);
  const [role, setRole] = useState<OccurrenceRole>("hint");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [invalidated, setInvalidated] = useState(false);
  const invalidatedRef = useRef(false);
  const [pending, setPending] = useState<Pending | null>(null);
  const [createNotFound, setCreateNotFound] = useState(false);
  const [editor, setEditor] = useState<{ id?: string } | null>(null);
  const [editorPending, setEditorPending] = useState(false);
  const [proposalMode, setProposalMode] = useState<ProposalMode | null>(null);
  const proposalHistoryButton = useRef<HTMLButtonElement>(null);
  const [proposalBlocking, setProposalBlocking] = useState(false);
  const [cardDetail, setCardDetail] = useState<Material | null>(null);
  const afterEditor = useRef<string | undefined>(undefined);
  const life = useRef(0),
    listGen = useRef(0),
    rowGen = useRef(0),
    chapterGen = useRef(0),
    dirGen = useRef(0),
    detailGen = useRef(0);
  const locked = useRef(false);
  const dirty = !!excerpt || !!note || role !== "hint";
  const block = busy || dirty || editorPending || proposalBlocking || (!invalidated && !!pending);
  const pendingCallback = useRef(onPendingChange);
  pendingCallback.current = onPendingChange;

  useLayoutEffect(() => {
    pendingCallback.current?.(block);
  }, [block]);
  useEffect(() => () => pendingCallback.current?.(false), []);

  function failure(e: unknown) {
    if (code(e) === "epoch_changed") {
      // Async child checks must see the epoch failure before React commits it.
      invalidatedRef.current = true;
      setInvalidated(true);
      listGen.current++;
      rowGen.current++;
      dirGen.current++;
      chapterGen.current++;
      detailGen.current++;
      setError("书库已更换。旧草稿不会写入新库，请关闭并重新打开全书伏笔。");
    } else setError(errorMessage(e));
  }
  async function loadCards(e: string) {
    const g = ++listGen.current,
      l = life.current;
    try {
      const result = await api.overview(bookId, e);
      if (g === listGen.current && l === life.current)
        setCards(
          result.filter(
            (i) =>
              i.material.book_id === bookId &&
              i.material.kind === "foreshadow" &&
              i.material.deleted_at === null,
          ),
        );
    } catch (err) {
      if (g === listGen.current && l === life.current) failure(err);
    }
  }
  async function loadDirectory(e: string) {
    const g = ++dirGen.current,
      l = life.current;
    setDirectoryError("");
    try {
      const result = await api.directory(bookId, e);
      if (g === dirGen.current && l === life.current) setDirectory(result);
    } catch (err) {
      if (g !== dirGen.current || l !== life.current) return;
      if (code(err) === "epoch_changed") failure(err);
      else setDirectoryError(errorMessage(err));
    }
  }
  async function begin() {
    const l = ++life.current;
    setError("");
    try {
      const e = await api.epoch();
      if (l !== life.current) return;
      setEpoch(e);
      void loadCards(e);
      void loadDirectory(e);
    } catch (err) {
      if (l === life.current) failure(err);
    }
  }
  useEffect(() => {
    void begin();
    return () => {
      life.current++;
    };
    // App keys the panel by book; a replaced API begins a new read lifetime.
  }, [api, bookId]);
  async function loadDetail(card: string, e: string) {
    const g = ++detailGen.current,
      l = life.current;
    setCardDetail(null);
    try {
      const next = await api.material(bookId, card, e);
      if (g !== detailGen.current || l !== life.current) return;
      if (
        next.id !== card ||
        next.book_id !== bookId ||
        next.kind !== "foreshadow" ||
        next.deleted_at !== null
      )
        throw Error("主要资料身份不匹配，请重新读取");
      setCardDetail(next);
    } catch (err) {
      if (g === detailGen.current && l === life.current) failure(err);
    }
  }
  async function loadRows(card: string, e: string) {
    void loadDetail(card, e);
    const g = ++rowGen.current,
      l = life.current;
    setReading(true);
    try {
      const result = await api.list(bookId, card, e);
      if (g === rowGen.current && l === life.current)
        setRows(
          result.filter(
            (v) =>
              v.occurrence.book_id === bookId &&
              v.occurrence.material_id === card,
          ),
        );
    } catch (err) {
      if (g === rowGen.current && l === life.current) failure(err);
    } finally {
      if (g === rowGen.current && l === life.current) setReading(false);
    }
  }
  function allowLeave() {
    if (invalidated) {
      if (dirty) setError("请先复制或明确放弃旧书库的片段草稿。");
      return !locked.current && !dirty && !editorPending && !proposalBlocking;
    }
    if (locked.current || pending || editorPending || proposalBlocking) {
      setError("请先核对尚未结束的操作。");
      return false;
    }
    if (dirty) {
      setError("请先记录或清除当前片段及备注，再切换或关闭。");
      return false;
    }
    return true;
  }
  const closeRef = useRef(() => {});
  closeRef.current = () => {
    if (allowLeave()) onClose();
  };
  useEffect(() => {
    return () => {
      life.current++;
      listGen.current++;
      rowGen.current++;
      chapterGen.current++;
      dirGen.current++;
      detailGen.current++;
    };
  }, [editor]);

  function clearDraft() {
    setExcerpt(null);
    setRole("hint");
    setNote("");
  }
  function clearSource() {
    chapterGen.current++;
    setChapter(null);
    setChapterId("");
    setSourceReading(false);
    clearDraft();
  }
  function selectCard(id: string) {
    if (invalidated || id === selected || !allowLeave()) return;
    setSelected(id);
    setRows([]);
    setError("");
    setNotice("");
    clearSource();
    void loadRows(id, epoch);
  }
  async function selectChapter(id: string) {
    if (invalidated || !allowLeave()) return;
    clearSource();
    setError("");
    if (!id) return;
    setChapterId(id);
    const g = ++chapterGen.current,
      l = life.current;
    setSourceReading(true);
    try {
      const c = await api.chapter(bookId, id, epoch);
      if (g !== chapterGen.current || l !== life.current) return;
      if (c.position.chapter_id !== id)
        throw Error("来源章节身份不匹配，请重新读取");
      setChapter(c);
    } catch (err) {
      if (g === chapterGen.current && l === life.current) failure(err);
    } finally {
      if (g === chapterGen.current && l === life.current)
        setSourceReading(false);
    }
  }
  function scopeRow(v: OccurrenceView, id: string) {
    if (v.occurrence.book_id !== bookId || v.occurrence.material_id !== id)
      throw Error("出现记录身份不匹配，请读取核对");
    return v.occurrence;
  }
  function checkCreateReceipt(v: OccurrenceView, q: OccurrenceInput) {
    const o = scopeRow(v, q.material_id);
    if (
      !o.id ||
      o.source.chapter_id !== q.chapter_id ||
      o.source.chapter_revision !== q.chapter_revision ||
      o.start !== q.start ||
      o.end !== q.end ||
      o.quote !== q.quote ||
      o.role !== q.role ||
      o.note !== q.note
    )
      throw Error("新增回执与同一请求不匹配，未再次写入");
  }
  function completed(card: string, e: string, msg: string, clear: boolean) {
    setPending(null);
    setCreateNotFound(false);
    setNotice(msg);
    setError("");
    if (clear) clearDraft();
    void loadRows(card, e);
    void loadCards(e);
  }
  async function operate(p: Pending, existing = false) {
    if (locked.current || invalidated || (!existing && pending)) return;
    locked.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    const l = life.current;
    try {
      if (p.kind === "create") {
        const v = await api.create(p.input);
        if (l !== life.current) return;
        checkCreateReceipt(v, p.input);
        completed(
          p.input.material_id,
          p.input.epoch,
          "已记录出现；作者状态不变。",
          true,
        );
      } else {
        const v = await api.setDeleted(
          bookId,
          p.card,
          p.id,
          p.revision,
          p.epoch,
          p.deleted,
        );
        if (l !== life.current) return;
        const o = scopeRow(v, p.card);
        if (
          o.id !== p.id ||
          o.revision !== p.revision + 1 ||
          (o.deleted_at !== null) !== p.deleted
        )
          throw Error("状态回执有差异，请读取核对");
        completed(
          p.card,
          p.epoch,
          p.deleted
            ? "已移入出现记录回收站。"
            : "已恢复原记录，未刷新引文来源。",
          false,
        );
      }
    } catch (err) {
      if (l !== life.current) return;
      const c = code(err);
      if (c === "epoch_changed" || knownFailures.has(c)) {
        if (!existing || c === "epoch_changed") setPending(null);
        failure(err);
      } else {
        setPending(p);
        setCreateNotFound(false);
        setError("写入回执未知。请先读取核对，不会自动再次写入。");
      }
    } finally {
      locked.current = false;
      if (l === life.current) setBusy(false);
    }
  }
  async function reconcile() {
    if (!pending || locked.current || invalidated) return;
    const l = life.current;
    locked.current = true;
    setBusy(true);
    setError("");
    try {
      if (pending.kind === "create") {
        const q = pending.input;
        const found = await api.lookup(
          bookId,
          q.material_id,
          q.request_key,
          q.epoch,
        );
        if (l !== life.current) return;
        if (!found) {
          setCreateNotFound(true);
          setNotice(
            "尚未找到同一请求。可以用原键重试，或明确放弃这份片段草稿。",
          );
          return;
        }
        checkCreateReceipt(found, q);
        completed(
          q.material_id,
          q.epoch,
          "已在书库核对到记录；没有再次写入。",
          true,
        );
      } else {
        const q = pending;
        const r = await api.get(bookId, q.card, q.id, q.epoch);
        if (l !== life.current) return;
        const o = scopeRow(r, q.card);
        if (o.id !== q.id) throw Error("核对到不同记录");
        const matches =
          o.revision === q.revision + 1 &&
          (o.deleted_at !== null) === q.deleted;
        completed(
          q.card,
          q.epoch,
          matches
            ? "已核对提交结果，未重新写入。"
            : "已读取当前记录，未重新写入。",
          false,
        );
      }
    } catch (err) {
      if (l === life.current) {
        if (code(err) === "not_found") {
          if (pending.kind === "create") setCreateNotFound(true);
          else setPending(null);
        }
        failure(err);
      }
    } finally {
      locked.current = false;
      if (l === life.current) setBusy(false);
    }
  }
  function record() {
    if (
      !excerpt ||
      !chapter ||
      !selected ||
      !epoch ||
      invalidated ||
      locked.current ||
      pending
    )
      return;
    if (new TextEncoder().encode(note).length > 1024) {
      setError("备注最多 1024 UTF-8 字节");
      return;
    }
    void operate({
      kind: "create",
      input: {
        ...excerpt,
        book_id: bookId,
        material_id: selected,
        chapter_id: chapter.position.chapter_id,
        chapter_revision: chapter.position.chapter_revision,
        role,
        note,
        epoch,
        request_key: crypto.randomUUID(),
      },
    });
  }
  async function jump(source: SourcePosition) {
    if (invalidated || !allowLeave()) return;
    locked.current = true;
    setBusy(true);
    setError("");
    const l = life.current;
    try {
      if (await onJump(source, epoch)) {
        if (l === life.current) onClose();
      } else if (l === life.current)
        setError("未跳转。当前稿件与伏笔面板仍保留。");
    } catch (err) {
      if (l === life.current) failure(err);
    } finally {
      locked.current = false;
      if (l === life.current) setBusy(false);
    }
  }

  // Only the existing material editor writes primary fields; this adapter scopes
  // its actions to the active book/epoch and hides unrelated categories/history.
  const materialApi = useMemo<MaterialsAPI>(
    () => ({
      list: async () =>
        (await api.overview(bookId, epoch)).map((x) => x.material),
      get: (id) => api.material(bookId, id, epoch),
      create: (input) => api.createCard(input, epoch),
      save: (input) => api.saveCard(bookId, input, epoch),
      trash: async () => {
        throw Error("请在创作资料库回收伏笔卡");
      },
      restore: async () => {
        throw Error("请在创作资料库恢复伏笔卡");
      },
    }),
    [api, bookId, epoch],
  );
  function openEditor(id?: string) {
    if (!epoch || invalidated || !allowLeave()) return;
    afterEditor.current = undefined;
    setEditor({ id });
  }
  if (editor) {
    return (
      <MaterialsPanel
        bookId={bookId}
        api={materialApi}
        foreshadowFocus
        primaryScope={{
          invalidated,
          onEpochChanged: failure,
          onValidatedResult: (id) => {
            afterEditor.current = id;
          },
          chapterLabels: indexedDirectory.labels,
        }}
        initialMaterialId={editor.id}
        volumes={indexedDirectory.volumes}
        chapters={indexedDirectory.chapters}
        onPendingChange={setEditorPending}
        onClose={() => {
          setEditor(null);
          setEditorPending(false);
          if (invalidated) return;
          void loadCards(epoch);
          const next = afterEditor.current ?? selected;
          if (next) {
            setSelected(next);
            if (next !== selected) {
              setRows([]);
              clearSource();
            }
            void loadRows(next, epoch);
          }
        }}
      />
    );
  }

  const visible = cards
    .filter(
      (x) =>
        (status === "all" ||
          (x.material.tracking_status ?? "planned") === status) &&
        (!reviewOnly || needsReview(x)),
    )
    .sort(
      (a, b) =>
        (a.material.story_order ?? 0) - (b.material.story_order ?? 0) ||
        a.material.id.localeCompare(b.material.id),
    );
  const selectedSummary = visible.find((x) => x.material.id === selected);
  const titleOf = (id?: string | null) =>
    id ? (indexedDirectory.labels.get(id) ?? id) : "未设置";
  const primary = (
    id?: string | null,
    revision?: number | null,
    state?: keyof typeof sourceStates,
  ) =>
    id
      ? `${titleOf(id)} · v${revision ?? "?"} · ${sourceStates[state ?? "none"]}`
      : "未设置";
  const active = rows.filter((x) => x.occurrence.deleted_at === null);
  const trash = rows.filter((x) => x.occurrence.deleted_at !== null);
  const off = invalidated || busy || !!pending;
  function renderRow(v: OccurrenceView) {
    const o = v.occurrence;
    const retired = o.deleted_at !== null;
    return (
      <li key={o.id}>
        <p>
          <strong>{occurrenceRoles[o.role]}</strong> ·{" "}
          {sourceStates[v.source_status]}
        </p>
        <p>
          冻结来源：{o.source.volume_title} · {o.source.chapter_title} · 正文 v
          {o.source.chapter_revision} · 标量 [{o.start}, {o.end})
        </p>
        {v.current && v.source_status === "stale" && (
          <p>
            当前位置：{v.current.volume_title} · {v.current.chapter_title} ·
            正文 v{v.current.chapter_revision}
          </p>
        )}
        <blockquote className="foreshadow-quote">{o.quote}</blockquote>
        {o.note && <p className="foreshadow-quote">{o.note}</p>}
        {v.current && !retired && (
          <button disabled={off} onClick={() => void jump(v.current!)}>
            跳转来源章
          </button>
        )}
        <button
          disabled={off || dirty}
          onClick={() =>
            void operate({
              kind: "lifecycle",
              id: o.id,
              revision: o.revision,
              deleted: !retired,
              card: o.material_id,
              epoch,
            })
          }
        >
          {retired ? "恢复出现记录" : "回收出现记录"}
        </button>
      </li>
    );
  }
  return (
    <Dialog
      title="全书伏笔"
      subtitle="主要埋设和回收保留在伏笔卡。出现记录是作者选定的正文引文，不会自动改变作者状态。"
      closeLabel="关闭全书伏笔"
      className="foreshadow-panel"
      size="xl"
      onClose={() => closeRef.current()}
    >
      <div className="foreshadow-body">
        {error && (
          <p role="alert" className="review-error">
            {error}
          </p>
        )}
        {notice && <p role="status">{notice}</p>}
        {invalidated && dirty && (
          <section aria-label="处理旧书库片段">
            <p>这份片段只能留在旧书库。可先复制文字，或明确放弃；不会重发。</p>
            <label>
              旧书库片段草稿
              <textarea
                readOnly
                value={JSON.stringify(
                  { role: occurrenceRoles[role], excerpt, note },
                  null,
                  2,
                )}
              />
            </label>
            <button
              disabled={busy}
              onClick={() => {
                clearDraft();
                setPending(null);
                onClose();
              }}
            >
              放弃旧书库片段草稿并关闭
            </button>
          </section>
        )}
        {directoryError && (
          <p role="status">
            章节目录暂不可用：{directoryError}{" "}
            <button disabled={off} onClick={() => void loadDirectory(epoch)}>
              重读目录
            </button>
          </p>
        )}
        {!epoch && !invalidated && (
          <button onClick={() => void begin()}>读取伏笔</button>
        )}
        {pending && !invalidated && (
          <p>
            <button disabled={busy} onClick={() => void reconcile()}>
              读取核对
            </button>
            {createNotFound && pending.kind === "create" && (
              <>
                <button
                  disabled={busy}
                  onClick={() => void operate(pending, true)}
                >
                  使用原键重试
                </button>
                <button
                  disabled={busy}
                  onClick={() => {
                    setPending(null);
                    setCreateNotFound(false);
                    clearDraft();
                    setError("");
                  }}
                >
                  放弃片段草稿
                </button>
              </>
            )}
          </p>
        )}
        {api.proposals && <ForeshadowProposalPanel
          api={api.proposals}
          session={proposalSession}
          bookId={bookId}
          epoch={epoch}
          getEpoch={api.epoch}
          listOccurrences={api.list}
          mode={proposalMode}
          onOpen={() => { if (allowLeave()) setProposalMode({}); }}
          onClose={() => {
            const fromHistory = !!proposalMode?.materialId;
            setProposalMode(null);
            if (fromHistory) requestAnimationFrame(() => proposalHistoryButton.current?.focus());
          }}
          onBlocking={setProposalBlocking}
          onChanged={() => {
            void loadCards(epoch);
            if (selected) void loadRows(selected, epoch);
          }}
          onEpochChanged={failure}
          disabled={off || dirty}
          invalidated={invalidated}
          isEpochInvalidated={() => invalidatedRef.current}
        />}
        {proposalMode === null && <div className="materials-layout">
          <aside aria-label="全书伏笔卡">
            <button disabled={!epoch || off} onClick={() => openEditor()}>
              新增伏笔卡
            </button>
            <label>
              作者状态筛选
              <select
                disabled={off}
                value={status}
                onChange={(e) => {
                  if (allowLeave()) setStatus(e.target.value);
                }}
              >
                <option value="all">全部状态</option>
                {Object.entries(authorStates).map(([id, label]) => (
                  <option value={id} key={id}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <input
                type="checkbox"
                disabled={off}
                checked={reviewOnly}
                onChange={(e) => {
                  if (allowLeave()) setReviewOnly(e.target.checked);
                }}
              />
              只显示待复核
            </label>
            {visible.map((x) => (
              <button
                key={x.material.id}
                disabled={off}
                aria-pressed={selected === x.material.id}
                onClick={() => selectCard(x.material.id)}
              >
                {x.material.title} ·{" "}
                {authorStates[x.material.tracking_status ?? "planned"] ??
                  "未知状态"}{" "}
                · {x.occurrence_count} 次出现 · {x.review_count} 条待复核
                {needsReview(x) && "（需复核）"}
                <small>
                  主要埋设：
                  {primary(
                    x.material.source_chapter_id,
                    x.material.source_revision,
                    x.material.source_status,
                  )}
                </small>
                <small>
                  主要回收：
                  {primary(
                    x.material.payoff_chapter_id,
                    x.material.payoff_revision,
                    x.material.payoff_status,
                  )}
                </small>
              </button>
            ))}
            {epoch && visible.length === 0 && <p>没有符合筛选的伏笔卡。</p>}
            {epoch && (
              <button
                disabled={off}
                onClick={() => {
                  if (allowLeave()) {
                    void loadCards(epoch);
                    if (selected) void loadRows(selected, epoch);
                  }
                }}
              >
                刷新伏笔
              </button>
            )}
          </aside>
          <main aria-busy={reading}>
            {selectedSummary ? (
              <>
                <h3>{selectedSummary.material.title}</h3>
                {api.proposals && <button ref={proposalHistoryButton} disabled={off} onClick={() => {
                  if (allowLeave()) setProposalMode({materialId:selected});
                }}>查看此卡的建议历史</button>}
                {cardDetail?.id === selected && (
                  <section aria-label="主要伏笔内容">
                    <h4>主要埋设内容</h4>
                    <p className="foreshadow-quote">
                      {cardDetail.fields.setup || "未填写"}
                    </p>
                    <h4>主要回收说明</h4>
                    <p className="foreshadow-quote">
                      {cardDetail.fields.resolution || "未填写"}
                    </p>
                  </section>
                )}
                <button disabled={off} onClick={() => openEditor(selected)}>
                  编辑主要埋设、回收与作者状态
                </button>
                <h4>当前目录中的出现</h4>
                {reading ? (
                  <p>读取中…</p>
                ) : (
                  <ul>{active.filter((x) => x.current).map(renderRow)}</ul>
                )}
                {!reading && !active.some((x) => x.current) && (
                  <p>没有可定位的出现。</p>
                )}
                {!reading && active.some((x) => !x.current) && (
                  <>
                    <h4>来源不可用（冻结位置）</h4>
                    <ul>{active.filter((x) => !x.current).map(renderRow)}</ul>
                  </>
                )}
                {trash.length > 0 && (
                  <details>
                    <summary>出现记录回收站（{trash.length}）</summary>
                    <ul>{trash.map(renderRow)}</ul>
                  </details>
                )}
                <fieldset disabled={off}>
                  <legend>从已保存正文新建出现</legend>
                  <label>
                    选择出现来源章节
                    <select
                      value={chapterId}
                      onChange={(e) => void selectChapter(e.target.value)}
                    >
                      <option value="">请选择章节</option>
                      {directory.map((p) => (
                        <option key={p.chapter_id} value={p.chapter_id}>
                          {indexedDirectory.labels.get(p.chapter_id)}
                        </option>
                      ))}
                    </select>
                  </label>
                  {sourceReading && <p>读取已保存正文…</p>}
                  {chapterId && !sourceReading && (
                    <button
                      disabled={dirty}
                      onClick={() => void selectChapter(chapterId)}
                    >
                      重读来源正文
                    </button>
                  )}
                  {chapter && (
                    <>
                      <label>
                        已保存来源正文
                        <textarea
                          readOnly
                          value={chapter.body}
                          rows={7}
                          onSelect={(e) => {
                            const el = e.currentTarget;
                            if (el.selectionStart === el.selectionEnd) return;
                            try {
                              setExcerpt(
                                exactExcerpt(
                                  chapter.body,
                                  el.value,
                                  el.selectionStart,
                                  el.selectionEnd,
                                ),
                              );
                              setError("");
                            } catch (err) {
                              setExcerpt(null);
                              failure(err);
                            }
                          }}
                        />
                      </label>
                      <p>
                        请框选片段。当前读取版本 v
                        {chapter.position.chapter_revision}。
                      </p>
                      {excerpt && (
                        <blockquote className="foreshadow-quote">
                          {excerpt.quote}
                        </blockquote>
                      )}
                      <label>
                        出现角色
                        <select
                          value={role}
                          onChange={(e) =>
                            setRole(e.target.value as OccurrenceRole)
                          }
                        >
                          {Object.entries(occurrenceRoles).map(
                            ([id, label]) => (
                              <option key={id} value={id}>
                                {label}
                              </option>
                            ),
                          )}
                        </select>
                      </label>
                      <label>
                        出现备注
                        <textarea
                          value={note}
                          onChange={(e) => setNote(e.target.value)}
                        />
                      </label>
                      <button disabled={!excerpt} onClick={record}>
                        记录出现
                      </button>
                      {dirty && (
                        <button
                          onClick={() => {
                            clearDraft();
                            setError("");
                          }}
                        >
                          清除片段草稿
                        </button>
                      )}
                    </>
                  )}
                </fieldset>
              </>
            ) : (
              <p>选择一张伏笔卡查看出现记录。</p>
            )}
          </main>
        </div>}
      </div>
    </Dialog>
  );
}
