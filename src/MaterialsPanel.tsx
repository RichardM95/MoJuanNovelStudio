import MaterialHistoryPanel from "./MaterialHistoryPanel";
import MaterialOrganizationReview from "./MaterialOrganizationReview";
import React, { useEffect, useMemo, useRef, useState } from "react";
import type { Chapter, Volume } from "./types";
import type {
  Material,
  MaterialKind,
  MaterialSummary,
  MaterialsAPI,
  MaterialProposal,
} from "./materialTypes";
import { errorMessage } from "./workflowTypes";
import { Dialog } from "./ui/Dialog";
const names: Record<MaterialKind, string> = {
  setting: "设定",
  character: "角色",
  outline: "大纲",
  summary: "摘要",
  timeline: "时间线",
  foreshadow: "伏笔",
  document: "来源文档",
};
const fields: Record<MaterialKind, Record<string, string>> = {
  setting: {
    synopsis: "简介",
    genre: "题材",
    world: "世界观",
    style: "风格",
    rules: "规则",
  },
  character: {
    identity: "身份",
    personality: "性格",
    appearance: "外貌",
    goal: "目标",
    relationships: "关系",
    status: "状态",
    aliases: "别名",
  },
  outline: { content: "内容" },
  summary: { content: "内容" },
  timeline: { content: "事件内容" },
  foreshadow: { setup: "埋设内容", resolution: "回收说明" },
  document: { content: "原文" },
};
const catalogKind = (item: Pick<Material, "kind" | "title">): MaterialKind =>
  item.title.startsWith("原文 · ") && /\.(txt|md)$/i.test(item.title) &&
  (item.kind === "setting" || item.kind === "outline") ? "document" : item.kind;
function editorFields(draft: Material) {
  if (catalogKind(draft) === "document" && draft.kind === "setting") {
    return draft.fields.world !== undefined ? { world: "原文" } : { style: "原文" };
  }
  return fields[draft.kind];
}
const trackingLabels: Record<string, string> = {
  planned: "计划",
  written: "已发生",
  planted: "已铺设",
  resolved: "已回收",
  abandoned: "已放弃",
};
const trackingName = (kind: MaterialKind, status: string | null | undefined) =>
  status === "planned" && kind === "foreshadow"
    ? "待铺设"
    : (trackingLabels[status ?? "planned"] ?? "未知状态");
const isTracked = (kind: MaterialKind) =>
  kind === "timeline" || kind === "foreshadow";
// Scoped primary editing uses the same fields/rules as the ordinary card
// editor. Recovery compares only persisted editable fields, not derived status.
const primaryFields = (m: Material) => ({
  title: m.title,
  fields: Object.fromEntries(Object.entries(m.fields).sort(([a], [b]) => a.localeCompare(b))),
  volume_id: m.volume_id,
  chapter_id: m.chapter_id,
  source_chapter_id: m.source_chapter_id ?? null,
  source_revision: m.source_revision ?? null,
  story_order: m.story_order ?? 0,
  story_time: m.story_time ?? null,
  tracking_status: m.tracking_status ?? null,
  payoff_chapter_id: m.payoff_chapter_id ?? null,
  payoff_revision: m.payoff_revision ?? null,
});
const primaryCode = (e: unknown) => e && typeof e === "object" && "code" in e ? String(e.code) : "";
const primaryDefinite = new Set(["invalid_input", "limit_exceeded", "not_found", "deleted", "invalid_state", "revision_conflict", "database_error", "clock_error"]);
type PrimaryRecovery =
  | { mode: "save"; before: Material; attempted: Material; latest?: Material }
  | { mode: "create"; before: string[]; checked?: MaterialSummary[] };
function rebasePrimary(current: Material, before: Material, local: Material): Material {
  const a=primaryFields(before), b=primaryFields(local);
  const patch=Object.fromEntries(Object.entries(b).filter(([k,v])=>k!=="fields"&&v!==a[k as keyof typeof a]));
  // Source identity/revision are one selection; do not pair a locally
  // edited revision with a remotely edited chapter in a rebased draft.
  if (a.source_chapter_id!==b.source_chapter_id||a.source_revision!==b.source_revision) {
    patch.source_chapter_id=b.source_chapter_id; patch.source_revision=b.source_revision;
  }
  if (a.payoff_chapter_id!==b.payoff_chapter_id||a.payoff_revision!==b.payoff_revision) {
    patch.payoff_chapter_id=b.payoff_chapter_id; patch.payoff_revision=b.payoff_revision;
  }
  const fields={...current.fields};
  for (const name of new Set([...Object.keys(a.fields),...Object.keys(b.fields)])) {
    if (a.fields[name]===b.fields[name]) continue;
    if (b.fields[name]===undefined) delete fields[name];
    else fields[name]=b.fields[name];
  }
  return {...current,...patch,fields};
}

export default function MaterialsPanel({
  bookId,
  api,
  volumes,
  chapters,
  onClose,
  onPendingChange,
  initialMaterialId,
  foreshadowFocus = false,
  primaryScope,
}: {
  bookId: string;
  api: MaterialsAPI;
  volumes: Volume[];
  chapters: Chapter[];
  onClose: () => void;
  initialMaterialId?: string;
  foreshadowFocus?: boolean;
  primaryScope?: {
    invalidated: boolean;
    onEpochChanged: (error: unknown) => void;
    onValidatedResult: (id: string) => void;
    chapterLabels: ReadonlyMap<string, string>;
  };
  onPendingChange?: (pending: boolean) => void;
}) {
  const [kind, setKind] = useState<MaterialKind>(foreshadowFocus ? "foreshadow" : "setting"),
    [recycle, setRecycle] = useState(false),
    [items, setItems] = useState<MaterialSummary[]>([]),
    [card, setCard] = useState<Material | null>(null),
    [draft, setDraft] = useState<Material | null>(null),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [busy, setBusy] = useState(false),
    [reading, setReading] = useState(false);
  const [newSource, setNewSource] = useState(""),
    [verifySource, setVerifySource] = useState(false),
    [reviewing, setReviewing] = useState(false);
  const [statusFilter, setStatusFilter] = useState("all"),
    [verifyPayoff, setVerifyPayoff] = useState(false);
  const [historyOpen,setHistoryOpen]=useState(false),[historyPending,setHistoryPending]=useState(false);
  const [organizing,setOrganizing]=useState(false),[organizationPending,setOrganizationPending]=useState(false);
  const [primaryRecovery, setPrimaryRecovery] = useState<PrimaryRecovery | null>(null);
  const [primaryFailure, setPrimaryFailure] = useState("");
  const primaryLocked = !!primaryRecovery || !!primaryScope?.invalidated ||
    primaryFailure==="not_found" || primaryFailure==="deleted";
  const reportPrimary = (e: unknown) => {
    if (primaryScope && primaryCode(e)==="epoch_changed") primaryScope.onEpochChanged(e);
    setError(errorMessage(e));
  };
  const historyOpenRef=useRef(false);historyOpenRef.current=historyOpen;
  const initialHandled = useRef(false);
  const listGeneration = useRef(0),
    cardGeneration = useRef(0),
    mutating = useRef(false);
  const dirty =
    !!card && !!draft && JSON.stringify(card) !== JSON.stringify(draft);
  const pendingCallback = useRef(onPendingChange);
  pendingCallback.current = onPendingChange;
  useEffect(() => {
    pendingCallback.current?.(dirty || busy || historyPending || organizationPending || !!primaryRecovery);
  }, [dirty, busy, historyPending, organizationPending, primaryRecovery]);
  useEffect(() => () => pendingCallback.current?.(false), []);
  const saveRef = useRef(() => {});
  saveRef.current = () => {
    if (
      draft &&
      dirty &&
      !busy &&
      !reading &&
      !reviewing &&
      !historyOpen &&
      !primaryLocked &&
      draft.deleted_at === null
    )
      void mutate(
        () =>
          api.save({
            id: draft.id,
            title: draft.title,
            fields: draft.fields,
            volume_id: draft.volume_id,
            chapter_id: draft.chapter_id,
            source_chapter_id: draft.source_chapter_id ?? null,
            source_revision: draft.source_revision ?? null,
            story_order: draft.story_order ?? 0,
            story_time: draft.story_time ?? null,
            tracking_status: draft.tracking_status ?? null,
            payoff_chapter_id: draft.payoff_chapter_id ?? null,
            payoff_revision: draft.payoff_revision ?? null,
            expected_revision: draft.revision,
          }),
        "save",
      );
  };
  const requestClose = useRef(() => {});
  const canLeave = () => {
    if (mutating.current || organizationPending) return false;
    if (primaryRecovery) {
      setError("请先核对资料写入结果，或明确处理旧草稿。");
      return false;
    }
    if(historyOpen){setError("请先关闭资料历史");return false;}
    if (dirty) {
      setError("请先保存当前资料，再切换或关闭。");
      return false;
    }
    return true;
  };
  const close = () => {
    if (canLeave()) onClose();
  };
  requestClose.current = close;
  async function loadList() {
    const request = ++listGeneration.current;
    try {
      const next = await api.list(bookId, true);
      if (request === listGeneration.current) {
        const sameBook = next.filter((i) => i.book_id === bookId);
        setItems(sameBook);
        if (!initialHandled.current) {
          initialHandled.current = true;
          const initial = sameBook.find(
            (i) => i.id === initialMaterialId && i.deleted_at === null,
          );
          if (initial) {
            setKind(catalogKind(initial));
            void select(initial);
          }
        }
      }
    } catch (e) {
      if (request === listGeneration.current) reportPrimary(e);
    }
  }
  useEffect(() => {
    void loadList();
    return () => {
      listGeneration.current++;
      cardGeneration.current++;
    };
  }, [bookId, api]);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        e.stopPropagation();
        saveRef.current();
      }
    };
    document.addEventListener("keydown", key, true);
    return () => document.removeEventListener("keydown", key, true);
  }, []);
  function changeCategory(next: MaterialKind | "recycle") {
    if (!canLeave()) return;
    cardGeneration.current++;
    setReading(false);
    setReviewing(false);
    setVerifySource(false);
    setVerifyPayoff(false);
    setCard(null);
    setDraft(null);
    setError("");
    setNotice("");
    setStatusFilter("all");
    setRecycle(next === "recycle");
    if (next !== "recycle") setKind(next);
    void loadList();
  }
  async function select(item: MaterialSummary) {
    if (!canLeave()) return;
    setReviewing(false);
    setVerifySource(false);
    setVerifyPayoff(false);
    const request = ++cardGeneration.current;
    setReading(true);
    setError("");
    setNotice("");
    try {
      const next = await api.get(item.id, recycle);
      if (request !== cardGeneration.current) return;
      if (next.book_id !== bookId || next.id !== item.id)
        throw new Error("资料所属作品不匹配");
      setCard(next);
      setDraft(structuredClone(next));
    } catch (e) {
      if (request === cardGeneration.current) reportPrimary(e);
    } finally {
      if (request === cardGeneration.current) setReading(false);
    }
  }
  function update(patch: Partial<Material>) {
    if (!draft || busy || reading || primaryLocked || draft.deleted_at !== null) return;
    setVerifySource(false);
    setVerifyPayoff(false);
    setDraft({ ...draft, ...patch });
    setNotice("");
    setError("");
  }
  async function mutate(
    action: () => Promise<Material>,
    mode: "save" | "create" | "trash" | "restore" | "proposal",
  ) {
    if (mutating.current || reading || primaryLocked) return;
    if (mode === "save" && draft) {
      const order = draft.story_order ?? 0;
      if (!Number.isInteger(order) || order < 0 || order > 1e9) {
        setError("故事位置必须是 0 到 1000000000 的整数。");
        return;
      }
      if (new TextEncoder().encode(draft.story_time ?? "").length > 256) {
        setError("故事时间描述过长，最多 256 UTF-8 字节。");
        return;
      }
      if (isTracked(draft.kind)) {
        const state = draft.tracking_status ?? "planned";
        const allowed =
          draft.kind === "timeline"
            ? ["planned", "written"]
            : ["planned", "planted", "resolved", "abandoned"];
        if (!allowed.includes(state)) {
          setError("请选择有效跟踪状态。");
          return;
        }
        const sourceCurrent =
          !!draft.source_chapter_id &&
          activeChapters.some(
            (c) =>
              c.id === draft.source_chapter_id &&
              c.revision === draft.source_revision,
          );
        const payoffCurrent =
          !!draft.payoff_chapter_id &&
          activeChapters.some(
            (c) =>
              c.id === draft.payoff_chapter_id &&
              c.revision === draft.payoff_revision,
          );
        const transition = state !== (card?.tracking_status ?? "planned");
        const sourceChanged =
          draft.source_chapter_id !== card?.source_chapter_id ||
          draft.source_revision !== card?.source_revision;
        const payoffChanged =
          draft.payoff_chapter_id !== card?.payoff_chapter_id ||
          draft.payoff_revision !== card?.payoff_revision;
        if (
          state === "resolved" &&
          ((transition && (!sourceCurrent || !payoffCurrent)) ||
            (sourceChanged && !sourceCurrent) ||
            (payoffChanged && !payoffCurrent) ||
            !draft.fields.resolution?.trim())
        ) {
          setError(
            "标记已回收需要两组当前来源及回收说明；已有回收记录只需复核本次改动的来源，回收说明不能为空。",
          );
          return;
        }
        if (
          (state === "written" || state === "planted") &&
          (transition || sourceChanged) &&
          !sourceCurrent
        ) {
          setError("此状态需要当前有效来源章节版本，请先选择来源或复核。");
          return;
        }
      }
    }
    mutating.current = true;
    listGeneration.current++;
    setBusy(true);
    setError("");
    setNotice("");
    cardGeneration.current++;
    if (primaryScope && mode==="save") setPrimaryFailure("");
    let creationBefore: string[] | undefined;
    let dispatched = false;
    const attempted = draft && structuredClone(draft);
    const original = card && structuredClone(card);
    try {
      if (primaryScope && mode==="create") {
        const snapshot=await api.list(bookId,false);
        creationBefore=snapshot.filter(m=>m.book_id===bookId&&m.kind==="foreshadow").map(m=>m.id);
      }
      dispatched = true;
      const next = await action();
      if (next.book_id !== bookId) throw new Error("资料所属作品不匹配");
      if (primaryScope && mode==="save" &&
          (!attempted || !original || next.id!==attempted.id || next.kind!=="foreshadow" || next.deleted_at!==null || next.revision<=original.revision ||
           JSON.stringify(primaryFields(next))!==JSON.stringify(primaryFields(attempted))))
        throw Error("资料保存回执不匹配，请读取核对");
      if (primaryScope && mode==="create" && (!next.id || next.kind!=="foreshadow" || next.deleted_at!==null || creationBefore?.includes(next.id)))
        throw Error("新增资料回执不匹配，请读取核对");
      if (mode==="create" || mode==="save") primaryScope?.onValidatedResult(next.id);
      setItems((old) => [
        ...old.filter((i) => i.id !== next.id),
        (({ fields, ...summary }) => summary)(next),
      ]);
      if (mode === "trash" || mode === "restore") {
        setCard(null);
        setDraft(null);
      } else {
        setCard(next);
        setDraft(structuredClone(next));
      }
      setVerifySource(false);
      setVerifyPayoff(false);
      setReviewing(false);
      setNotice(
        mode === "proposal"
          ? "提案已处理"
          : mode === "save"
            ? "已保存"
            : mode === "create"
              ? "已新建，请填写资料"
              : mode === "restore"
                ? "已恢复"
                : "已移入回收站",
      );
    } catch (e) {
      if (primaryScope && dispatched && primaryCode(e)!=="epoch_changed") {
        const code=primaryCode(e);
        if (mode==="save" && attempted && original) {
          if (!primaryDefinite.has(code)||code==="revision_conflict")
            setPrimaryRecovery({mode,before:original,attempted});
          else setPrimaryFailure(code);
        }
        if (mode==="create" && creationBefore && !primaryDefinite.has(code))
          setPrimaryRecovery({mode,before:creationBefore});
      }
      reportPrimary(e);
    } finally {
      mutating.current = false;
      setBusy(false);
    }
  }
  async function reconcilePrimary() {
    if (!primaryRecovery || mutating.current || primaryScope?.invalidated) return;
    mutating.current=true;
    setBusy(true);
    setError("");
    const pending=primaryRecovery;
    try {
      if(pending.mode==="create") {
        setPrimaryRecovery({...pending,checked:undefined});
        const next=(await api.list(bookId,false)).filter(m=>m.book_id===bookId&&m.kind==="foreshadow"&&m.deleted_at===null);
        setItems(next);
        setPrimaryRecovery({...pending,checked:next.filter(m=>!pending.before.includes(m.id))});
      } else {
        const latest=await api.get(pending.attempted.id,false);
        if(latest.id!==pending.attempted.id||latest.book_id!==bookId||latest.kind!=="foreshadow"||latest.deleted_at!==null)
          throw Error("资料身份不匹配，不能核对草稿");
        if(latest.revision>pending.before.revision&&JSON.stringify(primaryFields(latest))===JSON.stringify(primaryFields(pending.attempted))) {
          setCard(latest); setDraft(structuredClone(latest)); setPrimaryRecovery(null);
          primaryScope?.onValidatedResult(latest.id);
          setNotice("已核对：草稿已保存，未再次写入。");
        } else {
          setPrimaryRecovery({...pending,latest});
        }
      }
    } catch(e) {
      reportPrimary(e);
    } finally {mutating.current=false;setBusy(false);}
  }
  function resolvePrimary(keep: boolean) {
    if(primaryRecovery?.mode!=="save"||busy||primaryScope?.invalidated) return;
    const latest=primaryRecovery.latest;
    if(keep&&!latest) return;
    const saved=latest??primaryRecovery.before;
    setCard(saved);
    setDraft(keep?rebasePrimary(saved,primaryRecovery.before,primaryRecovery.attempted):structuredClone(saved));
    setPrimaryRecovery(null);
    setError("");
    setNotice(keep?"已换用最新版本，保留本地修改。请核对后手动保存。":"已放弃本地修改；没有再次写入。");
  }
  const { activeVolumes, activeChapters } = useMemo(() => {
    const activeVolumes = volumes.filter(v => v.book_id === bookId && v.deleted_at === null);
    const activeVolumeIds = new Set(activeVolumes.map(v => v.id));
    return {
      activeVolumes,
      activeChapters: chapters.filter(c => c.deleted_at === null && activeVolumeIds.has(c.volume_id)),
    };
  }, [volumes, chapters, bookId]);
  const chapterLabel = (c: Chapter) => primaryScope?.chapterLabels.get(c.id) ?? c.title;
  const target = draft?.chapter_id
    ? `chapter:${draft.chapter_id}`
    : draft?.volume_id
      ? `volume:${draft.volume_id}`
      : "";
  const targetExists =
    !target ||
    activeVolumes.some((v) => target === `volume:${v.id}`) ||
    activeChapters.some((c) => target === `chapter:${c.id}`);
  const sourceChapter = activeChapters.find(
    (c) => c.id === draft?.source_chapter_id,
  );
  const sourceLabels = {
    none: "未记录来源",
    current: "来源版本未变化",
    stale: "来源正文已更新，待复核",
    unavailable: "来源不可用",
  };
  const payoffChapter = activeChapters.find(
    (c) => c.id === draft?.payoff_chapter_id,
  );
  const visible = items.filter((i) =>
    recycle
      ? i.deleted_at !== null
      : i.deleted_at === null &&
        catalogKind(i) === kind &&
        (kind !== "foreshadow" ||
          statusFilter === "all" ||
          (i.tracking_status ?? "planned") === statusFilter),
  );
  if (!recycle && isTracked(kind))
    visible.sort(
      (a, b) =>
        (a.story_order ?? 0) - (b.story_order ?? 0) ||
        (a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
    );
  return (
    <Dialog
      title={foreshadowFocus ? "伏笔资料编辑" : "创作资料库"}
      label={foreshadowFocus ? "伏笔资料编辑" : "创作资料库"}
      subtitle="按类别整理设定、资料与伏笔；修改只写入资料卡，不改正文。"
      closeLabel={foreshadowFocus ? "返回全书伏笔" : "关闭资料库"}
      className="materials-panel"
      size="xl"
      dismissible={!busy}
      onClose={close}
      headerActions={
        api.organize && !foreshadowFocus && !organizing ? (
          <button onClick={() => { if (canLeave()) setOrganizing(true); }}>
            整理已有资料
          </button>
        ) : undefined
      }
    >
      <div className="materials-body">
        {organizing && api.organize && <MaterialOrganizationReview bookId={bookId} api={api.organize} fieldLabels={fields}
          onPending={setOrganizationPending} onClose={() => setOrganizing(false)}
          onApplied={async count => {
            cardGeneration.current++;
            setReading(false);
            setCard(null);
            setDraft(null);
            setReviewing(false);
            await loadList();
            setNotice("已采纳 " + count + " 项资料。");
            setOrganizing(false);
          }} />}
        {!organizing && !foreshadowFocus && <nav aria-label="资料类别" className="materials-nav">
          {(Object.keys(names) as MaterialKind[]).map((k) => (
            <button
              key={k}
              aria-pressed={!recycle && kind === k}
              disabled={busy}
              onClick={() => changeCategory(k)}
            >
              {names[k]}
            </button>
          ))}
          <button
            aria-pressed={recycle}
            disabled={busy}
            onClick={() => changeCategory("recycle")}
          >
            回收站
          </button>
        </nav>}
        {error && (
          <p role="alert" className="review-error">
            {error}
          </p>
        )}
        {notice && <p role="status">{notice}</p>}
        {dirty && (
          <p className="material-unsaved">有未保存修改，请先保存再切换。</p>
        )}
        {primaryScope?.invalidated && (dirty||primaryRecovery) ? (
          <section aria-label="处理旧书库资料">
            <p>书库已更换，旧内容不能写入新库。可先复制草稿，再明确放弃。</p>
            {draft && dirty && <label>旧书库资料草稿<textarea readOnly value={JSON.stringify(primaryFields(draft),null,2)} /></label>}
            <button disabled={busy} onClick={()=>{setPrimaryRecovery(null);setDraft(null);onClose();}}>放弃旧书库资料草稿并返回</button>
          </section>
        ) : primaryRecovery && (
          <section aria-label="核对资料写入">
            {primaryRecovery.mode==="save" ? <>
              <p>资料保存未确认。先读回当前版本；不会自动再次保存。</p>
              <label>本地资料草稿<textarea readOnly value={JSON.stringify(primaryFields(primaryRecovery.attempted),null,2)} /></label>
              {primaryRecovery.latest && <>
                <label>当前已保存资料<textarea readOnly value={JSON.stringify(primaryFields(primaryRecovery.latest),null,2)} /></label>
                <button disabled={busy} onClick={()=>resolvePrimary(true)}>保留我的修改并换用当前版本</button>
              </>}
              <button disabled={busy} onClick={()=>void reconcilePrimary()}>读取核对资料</button>
              <button disabled={busy} onClick={()=>resolvePrimary(false)}>放弃本地资料草稿</button>
            </> : <>
              <p>新增回执未知。请成功读取并检查新增候选，再决定是否另建。</p>
              <button disabled={busy} onClick={()=>void reconcilePrimary()}>重新读取核对新增伏笔卡</button>
              {primaryRecovery.checked && <>
                <p>{primaryRecovery.checked.length?`可能新增：${primaryRecovery.checked.map(m=>m.title).join("、")}`:"重新读取未发现新增卡。"}</p>
                <button disabled={busy} onClick={()=>{setPrimaryRecovery(null);setError("");}}>已核对列表，解除新增锁定</button>
              </>}
            </>}
          </section>
        )}
        {primaryScope && !primaryScope.invalidated && !primaryRecovery && primaryFailure && dirty && draft && (
          <section aria-label="处理无法保存的资料">
            <p>本次保存已被拒绝。可先复制草稿，再明确放弃；不会再次写入。</p>
            <label>无法保存的资料草稿<textarea readOnly value={JSON.stringify(primaryFields(draft),null,2)} /></label>
            <button disabled={busy} onClick={()=>{setDraft(null);onClose();}}>放弃无法保存的资料草稿并返回</button>
          </section>
        )}
        <div className="materials-layout" hidden={organizing}>
          <aside aria-label="资料条目">
            {!foreshadowFocus && !recycle && kind === "foreshadow" && (
              <label>
                伏笔状态筛选
                <select
                  value={statusFilter}
                  disabled={busy}
                  onChange={(e) => {
                    if (canLeave()) {
                      cardGeneration.current++;
                      setReading(false);
                      setCard(null);
                      setDraft(null);
                      setReviewing(false);
                      setStatusFilter(e.target.value);
                    }
                  }}
                >
                  <option value="all">全部状态</option>
                  {["planned", "planted", "resolved", "abandoned"].map((s) => (
                    <option key={s} value={s}>
                      {trackingName("foreshadow", s)}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {!recycle && kind === "summary" && (
              <label>
                新摘要来源章节
                <select
                  value={newSource}
                  disabled={busy || dirty}
                  onChange={(e) => setNewSource(e.target.value)}
                >
                  <option value="">请选择来源章节</option>
                  {activeChapters.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.title}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {!recycle && (
              <button
                disabled={
                  primaryLocked ||
                  busy ||
                  dirty ||
                  reading ||
                  (kind === "summary" &&
                    !activeChapters.some((c) => c.id === newSource))
                }
                onClick={() =>
                  void mutate(
                    () =>
                      api.create({
                        book_id: bookId,
                        kind,
                        title: `未命名${names[kind]}`,
                        fields: Object.fromEntries(
                          Object.keys(fields[kind]).map((k) => [k, ""]),
                        ),
                        volume_id: null,
                        chapter_id: null,
                        story_order: 0,
                        story_time: null,
                        tracking_status: isTracked(kind) ? "planned" : null,
                        payoff_chapter_id: null,
                        payoff_revision: null,
                        source_chapter_id:
                          kind === "summary" ? newSource : null,
                        source_revision:
                          kind === "summary"
                            ? activeChapters.find((c) => c.id === newSource)!
                                .revision
                            : null,
                      }),
                    "create",
                  )
                }
              >
                新增{names[kind]}
              </button>
            )}
            {visible.map((item) => (
              <button
                key={item.id}
                disabled={busy || primaryLocked}
                aria-pressed={card?.id === item.id}
                onClick={() => void select(item)}
              >
                {isTracked(item.kind)
                  ? `${item.story_order ?? 0} · ${trackingName(item.kind, item.tracking_status)} · ${item.title}`
                  : item.title}
              </button>
            ))}
            {visible.length === 0 && <p>暂无资料</p>}
          </aside>
          <main aria-busy={reading} inert={primaryLocked}>
            {reading ? (
              <p>读取中…</p>
            ) : draft && reviewing && api.review ? (
              <MaterialReview
                key={draft.id}
                bookId={bookId}
                card={draft}
                sourceChapter={sourceChapter}
                api={api.review}
                busy={busy}
                onBack={() => setReviewing(false)}
                onDecide={(p, approve) =>
                  mutate(
                    () => api.review!.decide(p.id, approve, p.base_revision),
                    "proposal",
                  )
                }
              />
            ) : historyOpen && card && api.history ? <MaterialHistoryPanel api={api.history} card={card} onPendingChange={setHistoryPending} onClose={()=>setHistoryOpen(false)} onReload={async()=>{const m=await api.get(card.id,false);if(m.id!==card.id||m.book_id!==bookId)throw Error("资料核对失败");setCard(m);setDraft(structuredClone(m));setItems(old=>old.map(i=>i.id===m.id?(({fields,...summary})=>summary)(m):i));return m;}}/> : draft ? (
              <>
                {draft.fields.source_files && <p>文件来源 · {draft.fields.source_files}</p>}
                {catalogKind(draft) !== "document" && <>
                {(!draft.fields.source_files || draft.source_chapter_id) && <p>{sourceLabels[draft.source_status ?? "none"]}</p>}
                <label>
                  来源章节
                  <select
                    value={draft.source_chapter_id ?? ""}
                    disabled={busy || draft.deleted_at !== null}
                    onChange={(e) => {
                      const selectedSource = activeChapters.find(
                        (c) => c.id === e.target.value,
                      );
                      update({
                        source_chapter_id: selectedSource?.id ?? null,
                        source_revision: selectedSource?.revision ?? null,
                      });
                    }}
                  >
                    <option value="" disabled={draft.kind === "summary"}>
                      {draft.kind === "summary"
                        ? "请选择来源章节"
                        : "未记录来源"}
                    </option>
                    {draft.source_chapter_id && !sourceChapter && (
                      <option value={draft.source_chapter_id}>
                        原来源不可用：{draft.source_chapter_id}
                      </option>
                    )}
                    {activeChapters.map((c) => (
                      <option key={c.id} value={c.id}>
                        {chapterLabel(c)}
                      </option>
                    ))}
                  </select>
                </label>
                </>}
                {(draft.source_chapter_id !== card?.source_chapter_id ||
                  draft.source_revision !== card?.source_revision) && (
                  <p>来源修改尚未保存；状态将在保存后重新检查。</p>
                )}
                {draft.source_chapter_id && (
                  <p>
                    来源：{sourceChapter ? chapterLabel(sourceChapter) : draft.source_chapter_id} ·
                    记录版本 {draft.source_revision ?? "未记录"}
                  </p>
                )}
                {api.review &&
                  !isTracked(draft.kind) &&
                  draft.deleted_at === null && (
                    <button
                      disabled={busy}
                      onClick={() => {
                        if (canLeave()) {
                          setVerifySource(false);
                          setVerifyPayoff(false);
                          setReviewing(true);
                        }
                      }}
                    >
                      待审资料提案
                    </button>
                  )}
                {sourceChapter && draft.deleted_at === null && (
                  <button disabled={busy} onClick={() => setVerifySource(true)}>
                    按当前章节版本复核
                  </button>
                )}
                {verifySource && sourceChapter && (
                  <div className="review-confirm">
                    <p>
                      请确认已核对资料内容与当前章节版本。这不会自动生成或校正内容，仍需保存资料。
                    </p>
                    <button onClick={() => setVerifySource(false)}>
                      取消复核
                    </button>
                    <button
                      onClick={() =>
                        update({ source_revision: sourceChapter.revision })
                      }
                    >
                      已核对内容，更新来源版本
                    </button>
                  </div>
                )}
                <label>
                  资料标题
                  <input
                    value={draft.title}
                    readOnly={draft.deleted_at !== null}
                    disabled={busy}
                    onChange={(e) => update({ title: e.target.value })}
                  />
                </label>
                {isTracked(draft.kind) && (
                  <>
                    <label>
                      故事位置
                      <input
                        type="number"
                        min="0"
                        max="1000000000"
                        step="1"
                        value={draft.story_order ?? 0}
                        disabled={busy}
                        readOnly={draft.deleted_at !== null}
                        onChange={(e) =>
                          update({ story_order: Number(e.target.value) })
                        }
                      />
                    </label>
                    <label>
                      故事时间
                      <input
                        value={draft.story_time ?? ""}
                        placeholder="例如：第三日夜"
                        disabled={busy}
                        readOnly={draft.deleted_at !== null}
                        onChange={(e) =>
                          update({ story_time: e.target.value || null })
                        }
                      />
                    </label>
                    <label>
                      跟踪状态
                      <select
                        value={draft.tracking_status ?? "planned"}
                        disabled={busy || draft.deleted_at !== null}
                        onChange={(e) =>
                          update({ tracking_status: e.target.value })
                        }
                      >
                        {(draft.kind === "timeline"
                          ? ["planned", "written"]
                          : ["planned", "planted", "resolved", "abandoned"]
                        ).map((s) => (
                          <option key={s} value={s}>
                            {trackingName(draft.kind, s)}
                          </option>
                        ))}
                      </select>
                    </label>
                  </>
                )}
                {draft.kind === "foreshadow" && (
                  <>
                    <label>
                      回收章节
                      <select
                        value={draft.payoff_chapter_id ?? ""}
                        disabled={busy || draft.deleted_at !== null}
                        onChange={(e) => {
                          const c = activeChapters.find(
                            (c) => c.id === e.target.value,
                          );
                          update({
                            payoff_chapter_id: c?.id ?? null,
                            payoff_revision: c?.revision ?? null,
                          });
                        }}
                      >
                        <option value="">未记录回收章节</option>
                        {draft.payoff_chapter_id && !payoffChapter && (
                          <option value={draft.payoff_chapter_id}>
                            原回收章节不可用：{draft.payoff_chapter_id}
                          </option>
                        )}
                        {activeChapters.map((c) => (
                          <option key={c.id} value={c.id}>
                            {chapterLabel(c)}
                          </option>
                        ))}
                      </select>
                    </label>
                    <p>
                      回收来源：{sourceLabels[draft.payoff_status ?? "none"]} ·
                      记录版本 {draft.payoff_revision ?? "未记录"}
                    </p>
                    {(draft.payoff_chapter_id !== card?.payoff_chapter_id ||
                      draft.payoff_revision !== card?.payoff_revision) && (
                      <p>回收来源修改尚未保存；状态将在保存后重新检查。</p>
                    )}
                    {payoffChapter && draft.deleted_at === null && (
                      <button
                        disabled={busy}
                        onClick={() => setVerifyPayoff(true)}
                      >
                        按当前回收章节版本复核
                      </button>
                    )}
                    {verifyPayoff && payoffChapter && (
                      <div className="review-confirm">
                        <p>
                          请确认已核对回收说明与当前回收章节内容，更新版本后仍需保存。
                        </p>
                        <button onClick={() => setVerifyPayoff(false)}>
                          取消回收复核
                        </button>
                        <button
                          onClick={() =>
                            update({ payoff_revision: payoffChapter.revision })
                          }
                        >
                          已核对回收内容，更新版本
                        </button>
                      </div>
                    )}
                  </>
                )}
                {draft.kind === "outline" && (
                  <label>
                    关联范围
                    <select
                      value={target}
                      disabled={busy || draft.deleted_at !== null}
                      onChange={(e) => {
                        const value = e.target.value;
                        update({
                          volume_id: value.startsWith("volume:")
                            ? value.slice(7)
                            : null,
                          chapter_id: value.startsWith("chapter:")
                            ? value.slice(8)
                            : null,
                        });
                      }}
                    >
                      <option value="">全书</option>
                      {!targetExists && (
                        <option value={target}>原关联已回收或不可用</option>
                      )}
                      {activeVolumes.map((v) => (
                        <option key={v.id} value={`volume:${v.id}`}>
                          卷：{v.title}
                        </option>
                      ))}
                      {activeChapters.map((c) => (
                        <option key={c.id} value={`chapter:${c.id}`}>
                          章：{c.title}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
                {Object.entries(editorFields(draft)).map(([key, label]) => (
                  <label key={key}>
                    {label}
                    <textarea
                      value={draft.fields[key] ?? ""}
                      readOnly={draft.deleted_at !== null}
                      disabled={busy}
                      onChange={(e) =>
                        update({
                          fields: { ...draft.fields, [key]: e.target.value },
                        })
                      }
                    />
                  </label>
                ))}
                {draft.content_review_required&&<p role="status">内容已恢复，关联待人工复核</p>}
                {api.history && draft.deleted_at===null && <button disabled={busy||dirty||reading} onClick={()=>setHistoryOpen(true)}>内容历史与复核</button>}
                <footer>
                  {draft.deleted_at !== null ? (
                    <button
                      disabled={busy || dirty}
                      onClick={() =>
                        void mutate(
                          () => api.restore(draft.id, draft.revision),
                          "restore",
                        )
                      }
                    >
                      恢复资料
                    </button>
                  ) : (
                    <>
                      {!foreshadowFocus && <button
                        disabled={busy || dirty}
                        onClick={() =>
                          void mutate(
                            () => api.trash(draft.id, draft.revision),
                            "trash",
                          )
                        }
                      >
                        移入回收站
                      </button>}
                      <button
                        className="primary"
                        disabled={busy || !dirty || primaryLocked}
                        onClick={() =>
                          void mutate(
                            () =>
                              api.save({
                                id: draft.id,
                                title: draft.title,
                                fields: draft.fields,
                                volume_id: draft.volume_id,
                                chapter_id: draft.chapter_id,
                                source_chapter_id:
                                  draft.source_chapter_id ?? null,
                                source_revision: draft.source_revision ?? null,
                                story_order: draft.story_order ?? 0,
                                story_time: draft.story_time ?? null,
                                tracking_status: draft.tracking_status ?? null,
                                payoff_chapter_id:
                                  draft.payoff_chapter_id ?? null,
                                payoff_revision: draft.payoff_revision ?? null,
                                expected_revision: draft.revision,
                              }),
                            "save",
                          )
                        }
                      >
                        保存资料
                      </button>
                    </>
                  )}
                </footer>
              </>
            ) : (
              <p className="empty">点击一条资料开始查看或编辑。</p>
            )}
          </main>
        </div>
      </div>
    </Dialog>
  );
}

function MaterialReview({
  bookId,
  card,
  sourceChapter,
  api,
  busy,
  onBack,
  onDecide,
}: {
  bookId: string;
  card: Material;
  sourceChapter?: Chapter;
  api: NonNullable<MaterialsAPI["review"]>;
  busy: boolean;
  onBack: () => void;
  onDecide: (p: MaterialProposal, approve: boolean) => Promise<void>;
}) {
  const [proposals, setProposals] = useState<MaterialProposal[]>([]),
    [selected, setSelected] = useState(""),
    [error, setError] = useState(""),
    [loading, setLoading] = useState(true),
    [confirm, setConfirm] = useState<MaterialProposal | null>(null);
  const generation = useRef(0);
  async function refresh() {
    const request = ++generation.current;
    setLoading(true);
    setConfirm(null);
    try {
      const list = await api.list(bookId);
      if (request === generation.current) {
        setProposals(
          list.filter(
            (p) =>
              p.material_id === card.id &&
              p.old_snapshot.id === card.id &&
              p.old_snapshot.book_id === bookId &&
              p.status === "pending",
          ),
        );
        setError("");
      }
    } catch (e) {
      if (request === generation.current) setError(errorMessage(e));
    } finally {
      if (request === generation.current) setLoading(false);
    }
  }
  useEffect(() => {
    void refresh();
    return () => {
      generation.current++;
    };
  }, [bookId, card.id, card.revision, api]);
  useEffect(() => {
    setConfirm(null);
  }, [sourceChapter?.id, sourceChapter?.revision]);
  const sourceBlocked = (p: MaterialProposal) =>
    !!card.source_chapter_id &&
    (!sourceChapter || p.new_source_revision !== sourceChapter.revision);
  const sourceStatus = !card.source_chapter_id
    ? "未记录来源"
    : !sourceChapter
      ? "来源不可用"
      : card.source_revision === sourceChapter.revision
        ? "来源版本未变化"
        : "来源正文已更新，待复核";
  const proposal = proposals.find((p) => p.id === selected) ?? proposals[0];
  const rows = proposal
    ? [
        ["资料标题", proposal.old_snapshot.title, proposal.new_title],
        ...Array.from(
          new Set([
            ...Object.keys(fields[card.kind]),
            ...Object.keys(proposal.old_snapshot.fields),
            ...Object.keys(proposal.new_fields),
          ]),
        ).map((key) => [
          fields[card.kind][key] ?? key,
          proposal.old_snapshot.fields[key] ?? "",
          proposal.new_fields[key] ?? "",
        ]),
        [
          "来源版本",
          String(proposal.old_snapshot.source_revision ?? "未记录"),
          String(proposal.new_source_revision ?? "未记录"),
        ],
      ]
    : [];
  return (
    <section aria-label="资料提案审阅">
      <header>
        <h3>待审资料提案</h3>
        <button disabled={busy} onClick={onBack}>
          返回资料编辑
        </button>
        <button
          disabled={busy || loading || !!confirm}
          onClick={() => void refresh()}
        >
          刷新资料提案
        </button>
      </header>
      {error && <p role="alert">{error}</p>}
      {loading ? (
        <p>读取提案中…</p>
      ) : proposal ? (
        <>
          <label>
            选择资料提案
            <select
              value={proposal.id}
              disabled={busy || !!confirm}
              onChange={(e) => setSelected(e.target.value)}
            >
              {proposals.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.reason || p.id}
                </option>
              ))}
            </select>
          </label>
          <p>
            客户端：{proposal.client_id} · 基于资料版本 {proposal.base_revision}
          </p>
          <p>
            来源章节：
            {sourceChapter?.title ??
              proposal.old_snapshot.source_chapter_id ??
              "未记录来源"}
          </p>
          <p>
            {sourceStatus} · 当前来源版本：{sourceChapter?.revision ?? "不可用"}{" "}
            · 提案来源版本：{proposal.new_source_revision ?? "未记录"}
          </p>
          {sourceBlocked(proposal) && (
            <p className="review-error">
              来源正文已变化或不可用，请重新提交提案。最终版本仍由保存时的检查裁决。
            </p>
          )}
          <p>{proposal.reason}</p>
          {proposal.base_revision !== card.revision && (
            <p className="review-error">
              资料已更新，此提案版本过期，不能采纳。
            </p>
          )}
          <div className="material-diff">
            {rows.map(([label, before, after]) => (
              <section key={label}>
                <h4>{label}</h4>
                <div className="diff-columns">
                  <article>
                    <h5>提交时原值</h5>
                    <pre>{before || "（空白）"}</pre>
                  </article>
                  <article>
                    <h5>建议值</h5>
                    <pre>{after || "（空白）"}</pre>
                  </article>
                </div>
              </section>
            ))}
          </div>
          <footer>
            <button
              disabled={busy || !!confirm}
              onClick={() => void onDecide(proposal, false)}
            >
              拒绝资料提案
            </button>
            <button
              disabled={
                busy ||
                !!confirm ||
                proposal.base_revision !== card.revision ||
                sourceBlocked(proposal)
              }
              onClick={() => setConfirm(structuredClone(proposal))}
            >
              采纳资料提案
            </button>
          </footer>
          {confirm && (
            <div
              role="alertdialog"
              aria-label="确认资料变更"
              className="review-confirm"
            >
              <p>
                确认将上述建议保存到此资料卡？此操作不修改章节正文，也不新增或删除资料。
              </p>
              <button disabled={busy} onClick={() => setConfirm(null)}>
                取消采纳
              </button>
              <button
                disabled={busy || sourceBlocked(confirm)}
                onClick={() => {
                  if (
                    !sourceBlocked(confirm) &&
                    confirm.material_id === card.id &&
                    confirm.base_revision === card.revision
                  )
                    void onDecide(confirm, true);
                }}
              >
                确认采纳资料提案
              </button>
            </div>
          )}
        </>
      ) : (
        <p>当前资料没有待审提案。</p>
      )}
    </section>
  );
}
