import React, { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import {
  BookOpen,
  Plus,
  Trash2,
  ArrowLeft,
  Download,
  Archive,
  RotateCcw,
  Check,
  Save,
  PenLine,
  Search,
  ListChecks,
  Replace,
  Flag,
  Library,
  Bot,
  History,
} from "lucide-react";
import type { API, LibraryState, Chapter, Command } from "./types";
import { SAVE_UNKNOWN, SaveQueue } from "./saveQueue";
import { useDraftRecovery, type DraftAnchor } from "./useDraftRecovery";
import Directory from "./Directory";
import RecycleBin from "./RecycleBin";
import ReviewPanel from "./ReviewPanel";
import BackupPanel from "./BackupPanel";
import ReplacePanel from "./ReplacePanel";
import WritingPanel from "./WritingPanel";
import { WritingDecisionSession } from "./writingDecision";
import type { WritingChapterRead } from "./writingTypes";
import SplitPanel from "./SplitPanel";
import {rawCursorOffset, type SplitTarget} from "./splitTypes";
import ExportPanel from "./ExportPanel";
import ImportPanel from "./ImportPanel";
import ProjectImportPanel from "./ProjectImportPanel";
import AgentAccessPanel from "./AgentAccessPanel";
import MaterialsPanel from "./MaterialsPanel";
import ForeshadowPanel from "./ForeshadowPanel";
import type { ForeshadowReviewSession } from "./ForeshadowProposalPanel";
import SearchPanel from "./SearchPanel";
import TasksPanel from "./TasksPanel";
import { errorMessage } from "./workflowTypes";
const empty: LibraryState = { books: [], volumes: [], chapters: [] };
type WritingTarget = {
  book: string;
  chapter: Pick<Chapter, "id" | "title">;
  generation: number;
  recoveryOnly?: boolean;
};
const STALE_SAVE_SCOPE = Error("编辑器范围已改变");
type SaveOwner = { api: API; book: string | null; generation: number };
function savedChapter(state: LibraryState, book: string | null, id: string) {
  if (!book || !state.books.some((item) => item.id === book && item.deleted_at === null))
    return undefined;
  const candidate = state.chapters.find((item) => item.id === id && item.deleted_at === null);
  return candidate && state.volumes.some((volume) =>
    volume.id === candidate.volume_id && volume.book_id === book && volume.deleted_at === null)
    ? candidate : undefined;
}
type Form = {
  heading: string;
  title: string;
  author?: string;
  submit: (title: string, author: string) => Promise<void>;
};
const count = (s: string) => Array.from(s.replace(/\s/g, "")).length;
export default function App({ api }: { api: API }) {
  const [data, setData] = useState(empty),
    [bookId, setBook] = useState<string | null>(null),
    [chapterId, setChapter] = useState<string | null>(null),
    [body, setBody] = useState(""),
    [trash, setTrash] = useState(false),
    [search, setSearch] = useState(""),
    [status, setStatus] = useState("已保存"),
    [error, setError] = useState(""),
    [checkMessage, setCheckMessage] = useState(""),
    [checking, setChecking] = useState(false),
    [notice, setNotice] = useState(""),
    [busy, setBusy] = useState(false),
    [form, setForm] = useState<Form | null>(null),
    [confirm, setConfirm] = useState<{
      text: string;
      run: () => Promise<void>;
    } | null>(null),
    [font, setFont] = useState(18),
    [ready, setReady] = useState(false),
    [review, setReview] = useState(false),
    [materialsOpen, setMaterialsOpen] = useState(false),
    [foreshadowOpen, setForeshadowOpen] = useState(false),
    [bookSearchOpen, setBookSearchOpen] = useState(false),
    [tasksOpen, setTasksOpen] = useState(false),
    [backupOpen, setBackupOpen] = useState(false),
    [replaceOpen, setReplaceOpen] = useState(false),
    [exportOpen, setExportOpen] = useState(false),
    [importOpen, setImportOpen] = useState<false | "txt" | "project">(false),
    [agentAccessOpen, setAgentAccessOpen] = useState(false),
    [initialMaterialId, setInitialMaterialId] = useState<string | undefined>();
  const [writingTarget, setWritingTarget] = useState<WritingTarget | null>(null);
  const [writingDecisions] = useState(() => new WritingDecisionSession());
  useSyncExternalStore(writingDecisions.subscribe, writingDecisions.snapshot);
  const writingPending = useRef(false);
  const writingScope = useRef<WritingTarget | null>(null);
  const decisionScopes = new Map(writingDecisions.recoveries().map(({ target }) => [
    JSON.stringify([target.bookId, target.chapterId]), target,
  ]));
  const [splitTarget,setSplitTarget] = useState<SplitTarget|null>(null);
  const splitPending=useRef(false), editor=useRef<HTMLTextAreaElement>(null);
  const materialsPending = useRef(false);
  const foreshadowPending = useRef(false);
  const foreshadowProposalSession = useRef<ForeshadowReviewSession>(new Map());
  const historyPending = useRef(false);
  const backupPending = useRef(false);
  const shelfLoadGeneration = useRef(0);
  const replacePending = useRef(false);
  const exportPending = useRef(false);
  const importPending = useRef(false);
  const replaceTarget = useRef<{bookId:string;chapterId:string|null}|null>(null);
  const materialsOpenRef = useRef(false);
  const queue = useRef<SaveQueue | null>(null),
    timer = useRef<ReturnType<typeof setTimeout> | null>(null),
    composing = useRef(false),
    locked = useRef(false);
  const saveOwner = useRef<SaveOwner>({api, book: bookId, generation: writingDecisions.libraryGeneration});
  saveOwner.current = {api, book: bookId, generation: writingDecisions.libraryGeneration};
  const saveContexts = useRef(new WeakMap<SaveQueue, SaveOwner>());
  const checkToken = useRef(0), copyToken = useRef(0);
  const saveUiMounted = useRef(false);
  useEffect(() => {
    saveUiMounted.current = true;
    return () => {
      saveUiMounted.current = false;
      checkToken.current++;
      copyToken.current++;
    };
  }, []);
  const sameSaveOwner = (owner: SaveOwner) =>
    owner.api === saveOwner.current.api && owner.book === saveOwner.current.book &&
    owner.generation === saveOwner.current.generation;
  const epochRead = api.foreshadow?.sourceEpoch;
  const epochChapter = api.foreshadow?.chapter;
  const selectedSave = queue.current;
  const [draftAnchor, setDraftAnchor] = useState<{anchor:DraftAnchor; owner:SaveOwner} | null>(null);
  const currentDraftAnchor = draftAnchor?.anchor.queue === selectedSave &&
    sameSaveOwner(draftAnchor.owner) ? draftAnchor.anchor : null;
  const draftRecovery = useDraftRecovery(api.draftRecovery, writingDecisions.libraryGeneration, currentDraftAnchor);
  const saveGeneration = writingDecisions.libraryGeneration;
  useEffect(() => {
    if (!selectedSave || !bookId || !epochRead || !epochChapter) return;
    let stopped = false;
    const owner = {api, book: bookId, generation: saveGeneration};
    const base = selectedSave.baseline;
    const current = () => !stopped && queue.current === selectedSave && sameSaveOwner(owner);
    // Standalone epoch never authenticates a chapter from stale editor state.
    // Verify the saved base against the existing book/epoch-scoped point read.
    void Promise.resolve().then(async () => {
      const epoch = await epochRead();
      if (!current() || typeof epoch !== "string" || !epoch) return;
      const anchored = await epochChapter(bookId, selectedSave.id, epoch);
      if (!current() || anchored.position.chapter_id !== selectedSave.id ||
          !Number.isSafeInteger(anchored.position.chapter_revision) ||
          anchored.position.chapter_revision !== base.revision || anchored.body !== base.body) return;
      selectedSave.observeEpoch(epoch);
      if (api.draftRecovery)
        setDraftAnchor({owner,anchor:{
          queue:selectedSave,epoch,book_id:bookId,chapter_id:selectedSave.id,
          valid: () => queue.current === selectedSave && sameSaveOwner(owner),
        }});
    }).catch(() => {});
    return () => { stopped = true; };
  }, [api, bookId, selectedSave, saveGeneration, epochRead, epochChapter]);
  const book = data.books.find((x) => x.id === bookId),
    chapter = data.chapters.find((x) => x.id === chapterId);
  const volumes = data.volumes
    .filter((x) => x.book_id === bookId && x.deleted_at == null)
    .sort((a, b) => a.sort_order - b.sort_order);
  const orderedChapters = volumes.flatMap((v) =>
    data.chapters
      .filter((c) => c.volume_id === v.id && c.deleted_at == null)
      .sort((a, b) => a.sort_order - b.sort_order),
  );
  const chaptersOf = (id: string) =>
    data.chapters
      .filter((x) => x.volume_id === id && x.deleted_at == null)
      .sort((a, b) => a.sort_order - b.sort_order);
  const report = (e: unknown) => {
    if (e !== STALE_SAVE_SCOPE) setError(errorMessage(e));
  };
  useEffect(() => {
    const startup = ++shelfLoadGeneration.current;
    api
      .load()
      .then((x) => {
        if (shelfLoadGeneration.current !== startup) return;
        setData(x);
        setReady(true);
      })
      .catch((e) => {
        if (shelfLoadGeneration.current === startup) report(e);
      });
    return () => {
      // Invalidate any shelf read when this App/API owner goes away.
      shelfLoadGeneration.current++;
    };
  }, [api]);
  const flush = useCallback(async (explicit = false) => {
    if (timer.current) clearTimeout(timer.current);
    const q = queue.current;
    if (!q?.dirty) return;
    const owner = saveOwner.current;
    const current = () => saveUiMounted.current && queue.current === q && sameSaveOwner(owner);
    try {
      if (!q.unknownAttempt && (!q.manualSaveRequired || explicit)) setStatus("保存中");
      await q.flush(explicit);
      if (!current()) throw STALE_SAVE_SCOPE;
      setStatus(q.dirty ? "未保存" : "已保存");
      setError("");
      if (!q.dirty) setCheckMessage("");
    } catch (e) {
      if (!current()) throw STALE_SAVE_SCOPE;
      setStatus(q.unknownAttempt ? "保存待核对" : "未保存");
      throw e;
    }
  }, []);
  const run = async (fn: () => Promise<void>, bubble = false, explicitSave = false) => {
    if (locked.current) return false;
    locked.current = true;
    setBusy(true);
    setError("");
    try {
      await flush(explicitSave);
      await fn();
      return true;
    } catch (e) {
      if (bubble) throw e;
      report(e);
      return false;
    } finally {
      locked.current = false;
      setBusy(false);
    }
  };
  const select = (c: Chapter | null) => {
    setDraftAnchor(null);
    checkToken.current++;
    copyToken.current++;
    setChecking(false);
    setCheckMessage("");
    setChapter(c?.id ?? null);
    setBody(c?.body ?? "");
    setStatus("已保存");
    if (!c) {
      queue.current = null;
      return;
    }
    const q = new SaveQueue(c.id, c.revision, async (id, body, revision) => {
      const owner = saveOwner.current;
      if (owner.api !== api || queue.current !== q) throw STALE_SAVE_SCOPE;
      if (!Number.isSafeInteger(revision) || revision < 0 || !Number.isSafeInteger(revision + 1))
        throw Error("待保存正文的版本无效");
      saveContexts.current.set(q, owner);
      const next = await api.apply({
        type: "save_chapter",
        id,
        body,
        expected_revision: revision,
      });
      const saved = savedChapter(next, owner.book, id);
      if (!saved || !Number.isSafeInteger(saved.revision) || saved.revision !== revision + 1 || saved.body !== body)
        throw Error("保存应答的章节、版本或正文不匹配");
      if (queue.current !== q || !sameSaveOwner(owner)) throw STALE_SAVE_SCOPE;
      setData(next);
      return saved.revision;
    }, c.body);
    queue.current = q;
  };
  const checkSavedDraft = async () => {
    const q = queue.current;
    const attempt = q?.unknownAttempt;
    const owner = q && saveContexts.current.get(q);
    if (!q || !attempt?.epoch || !owner || !sameSaveOwner(owner) || !epochRead || checking) return;
    const token = ++checkToken.current;
    const current = () => saveUiMounted.current && token === checkToken.current && queue.current === q &&
      q.unknownAttempt === attempt && sameSaveOwner(owner);
    setChecking(true);
    setCheckMessage("");
    setError("");
    try {
      const first = await epochRead();
      if (!current()) return;
      if (first !== attempt.epoch) throw Error("无法确认属于同一书库");
      const snapshot = await api.load();
      if (!current()) return;
      const last = await epochRead();
      if (!current()) return;
      if (last !== first) throw Error("无法确认属于同一书库");
      const stored = savedChapter(snapshot, owner.book, q.id);
      if (!stored) throw Error("找不到同一作品中的活动章节");
      const result = q.reconcile(attempt, stored);
      if (result === "committed") {
        setData((state) => ({...state, chapters: state.chapters.map((item) =>
          item.id === q.id ? {...item, body: stored.body, revision: stored.revision} : item)}));
        setStatus(q.dirty ? "未保存" : "已保存");
        setCheckMessage(q.dirty ? "核对到上一份正文已保存，最新修改仍未保存。请手动保存。" : "核对到这份正文已保存。");
      } else if (result === "base") {
        setStatus("保存待核对");
        setCheckMessage("当前存档仍是原版本，但保存结果仍未确认。未再次写入。");
      } else {
        setCheckMessage("已存版本或正文与两份预期都不一致，未覆盖任何稿件。");
      }
    } catch (e) {
      if (current()) setCheckMessage("未能核对已存正文：" + errorMessage(e));
    } finally {
      if (token === checkToken.current && queue.current === q) setChecking(false);
    }
  };
  const stopDraftCheck = () => {
    checkToken.current++;
    setChecking(false);
    setCheckMessage("已停止等待，本次核对不改变任何稿件。");
  };
  const copyDraft = async () => {
    const q = queue.current;
    const owner = saveOwner.current;
    const token = ++copyToken.current;
    const current = () => saveUiMounted.current && token === copyToken.current &&
      queue.current === q && sameSaveOwner(owner);
    try {
      if (!navigator.clipboard?.writeText) throw Error("剪贴板不可用");
      await navigator.clipboard.writeText(body);
      if (current()) setCheckMessage("已复制点击时的稿件。");
    } catch {
      if (!current() || !editor.current) return;
      editor.current.focus();
      editor.current.select();
      setCheckMessage("已选择当前稿件，请手动复制。");
    }
  };
  const open = (id: string) =>
    run(async () => {
      setBook(id);
      const vs = data.volumes
        .filter((v) => v.book_id === id && v.deleted_at == null)
        .sort((a, b) => a.sort_order - b.sort_order);
      select(vs.length ? (chaptersOf(vs[0].id)[0] ?? null) : null);
    });
  const openChapter = (c: Chapter) =>
    run(async () => {
      const latest = await api.load();
      setData(latest);
      select(latest.chapters.find((x) => x.id === c.id) ?? null);
    });
  const mutate = async (command: Command) => {
    const next = await api.apply(command);
    setData(next);
    return next;
  };
  const edit = (value: string) => {
    setBody(value);
    queue.current?.edit(value);
    if (queue.current && !composing.current) draftRecovery.input(queue.current,value);
    setStatus("未保存");
    if (timer.current) clearTimeout(timer.current);
    if (!composing.current && !queue.current?.unknownAttempt && !queue.current?.manualSaveRequired)
      timer.current = setTimeout(() => flush().catch(report), 800);
  };
  useEffect(() => {
    const save = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        if (!materialsOpenRef.current) flush(true).catch(report);
      }
    };
    const leave = (e: BeforeUnloadEvent) => {
      if (queue.current?.dirty || foreshadowPending.current || materialsPending.current || historyPending.current || backupPending.current || replacePending.current || importPending.current || exportPending.current || splitPending.current || writingPending.current) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("keydown", save);
    window.addEventListener("beforeunload", leave);
    return () => {
      window.removeEventListener("keydown", save);
      window.removeEventListener("beforeunload", leave);
      if (timer.current) clearTimeout(timer.current);
    };
  }, [flush]);
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    if ("__TAURI_INTERNALS__" in window)
      import("@tauri-apps/api/window").then(async ({ getCurrentWindow }) => {
        const win = getCurrentWindow();
        const off = await win.onCloseRequested(async (e) => {
          e.preventDefault();
          try {
            if (writingPending.current) throw Error("写作约束操作尚未结束，请先核对结果。");
            if (splitPending.current) throw Error("拆章尚未结束，请完成或核对结果后关闭。");
            if (exportPending.current) throw Error("TXT导出尚未结束，请等待完成后关闭。");
            if (importPending.current)
              throw Error("导入尚未结束，请完成操作或核对书架后关闭。");
            if (replacePending.current)
              throw Error("批量替换尚未结束，请完成操作或核对结果后关闭。");
            if (backupPending.current)
              throw Error("备份或恢复尚未结束，请等待完成或取消任务后关闭。");
            if (historyPending.current)
              throw Error("历史版本名称尚未保存或正在操作，请先保存或取消再关闭。");
            if (foreshadowPending.current)
              throw Error("全书伏笔还有草稿或写入结果待核对，请先处理。");
            if (materialsPending.current)
              throw Error("创作资料尚未保存或正在写入，请先完成保存再关闭。");
            await flush();
            await win.destroy();
          } catch (err) {
            report(err);
          }
        });
        if (cancelled) off();
        else unlisten = off;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [flush]);
  const titleForm = (
    heading: string,
    title: string,
    submit: (t: string, a: string) => Promise<void>,
    author?: string,
  ) => setForm({ heading, title, submit, author });
  const setWritingScope = (target: WritingTarget | null) => {
    writingScope.current = target;
    setWritingTarget(target);
  };
  const isWritingCurrent = (target: WritingTarget) =>
    writingScope.current === target && target.generation === writingDecisions.libraryGeneration;
  const openWriting = () => {
    if (!chapter || !bookId || !api.writing || writingScope.current || locked.current) return;
    const c = chapter, b = bookId;
    void run(async () => {
      if (queue.current?.id !== c.id) throw Error("章节已切换");
      setWritingScope({
        book: b,
        chapter: { id: c.id, title: c.title },
        generation: writingDecisions.libraryGeneration,
      });
    });
  };
  async function readWritingChapter(target: WritingTarget): Promise<WritingChapterRead> {
    const editorQueue = queue.current;
    const next = await api.load();
    if (!isWritingCurrent(target)) throw Error("写作约束界面或书库已切换");
    const b = next.books.find((b) => b.id === target.book && b.deleted_at === null);
    const volumes = new Set(next.volumes
      .filter((v) => v.book_id === b?.id && v.deleted_at === null)
      .map((v) => v.id));
    const c = next.chapters.find((c) =>
      c.id === target.chapter.id && c.deleted_at === null && volumes.has(c.volume_id),
    );
    if (!c) throw Error("章节已删除或不可用");
    const publish = () => {
      if (!isWritingCurrent(target)) throw Error("写作约束界面或书库已切换");
      if (!editorQueue || queue.current !== editorQueue || editorQueue.id !== c.id ||
          editorQueue.dirty || editorQueue.currentRevision > c.revision)
        throw Error("编辑器已更改，保留作者稿，未刷新正文");
      setData(next);
      select(c);
    };
    return { chapter: c, publish: target.recoveryOnly ? undefined : publish };
  }
  const splitChapter = () => {
    if (!chapter || !bookId || !api.split || locked.current) return;
    if (composing.current) { setError("请先完成输入法组合，再拆分章节"); return; }
    const el=editor.current; if (!el) return;
    let offset:number;
    try { offset=rawCursorOffset(body,el.value,el.selectionStart,el.selectionEnd); }
    catch(e) { report(e); return; }
    const targetChapter=chapter.id, targetBook=bookId, targetTitle=chapter.title;
    void run(async()=>{
      const q=queue.current;
      if(!q || q.id!==targetChapter) throw Error("章节已切换，请重新放置光标");
      setSplitTarget({bookId:targetBook,chapterId:targetChapter,revision:q.currentRevision,offset,title:targetTitle});
    });
  };
  const exportBook = () =>
    run(async () => {
      if (bookId) {
        if (api.txtExport) { setExportOpen(true); return; }
        const path = await api.exportBook(bookId);
        if (path) setNotice("TXT 已导出：" + path);
      }
    });
  const rows = data.books.filter(
    (b) =>
      (b.deleted_at != null) === trash &&
      `${b.title} ${b.author}`.toLowerCase().includes(search.toLowerCase()),
  );
  const bookWords = (id: string) => {
    const ids = new Set(
      data.volumes
        .filter((v) => v.book_id === id && v.deleted_at == null)
        .map((v) => v.id),
    );
    return data.chapters
      .filter((c) => ids.has(c.volume_id) && c.deleted_at == null)
      .reduce((n, c) => n + count(c.body), 0);
  };
  return (
    <div className="app">
      <aside className="rail" inert={foreshadowOpen || !!writingTarget}>
        <div className="brand">
          <img className="brand-logo" src="/mojuan-logo.png" alt="" />
          <span>
            墨卷<small>小说写作</small>
          </span>
        </div>
        <nav>
          <button
            className={!trash ? "active" : ""}
            onClick={() =>
              run(async () => {
                setBook(null);
                select(null);
                setTrash(false);
              })
            }
          >
            <BookOpen size={19} />
            书架
          </button>
          <button
            className={trash ? "active" : ""}
            onClick={() =>
              run(async () => {
                setBook(null);
                select(null);
                setTrash(true);
              })
            }
          >
            <Trash2 size={19} />
            回收站
          </button>
        </nav>
        <div className="rail-bottom">
          <button
            onClick={() =>
              run(async () => {
                if (api.backups) { setBackupOpen(true); return; }
                const path = await api.backup();
                if (path) setNotice("书库已备份：" + path);
              })
            }
          >
            <Archive size={18} />
            备份书库
          </button>
          <button
            onClick={() =>
              api.backups ? run(async () => { setBackupOpen(true); }) : setConfirm({
                text: "恢复备份将替换整个书库，包括正文、历史和创作资料。旧版备份不含创作资料，恢复后资料将为空。恢复前会另存当前书库快照；取消不会修改。",
                run: async () => {
                  const next = await api.restore();
                  if (next) {
                    writingDecisions.invalidateLibrary();
                    setData(next);
                    setBook(null);
                    select(null);
                    setNotice("书库已恢复");
                  }
                },
              })
            }
          >
            <RotateCcw size={18} />
            恢复备份
          </button>
          <span className="local-dot">本地书库 · 离线写作</span>
        </div>
      </aside>
      <main
        className={book ? "editor-main" : "shelf-main"}
        inert={foreshadowOpen || !!writingTarget}>
        {api.writing && api.materials && decisionScopes.size > 0 && (
          <details className="writing-recovery-links">
            <summary>待核对的正文决定</summary>
            <ul>
              {Array.from(decisionScopes, ([key, { bookId: b, chapterId: c }]) => {
                const bookTitle = data.books.find((item) => item.id === b)?.title ?? b;
                const title = data.chapters.find((item) => item.id === c)?.title ?? c;
                return (
                  <li key={key}>
                    <button disabled={busy} onClick={() => {
                      if (writingScope.current || locked.current) return;
                      // Detached verification never writes or reselects the editor.
                      setWritingScope({
                        book: b, chapter: { id: c, title },
                        generation: writingDecisions.libraryGeneration, recoveryOnly: true,
                      });
                    }}>
                      核对正文决定 · {bookTitle} / {title}
                    </button>
                  </li>
                );
              })}
            </ul>
          </details>
        )}
        {error || notice ? (
          <div className="toasts">
            {error && (
              <div role="alert" className="error">
                {error}
                <button aria-label="关闭错误提示" onClick={() => setError("")}>
                  ×
                </button>
              </div>
            )}
            {notice && (
              <div role="status" className="notice">
                {notice}
                <button aria-label="关闭提示" onClick={() => setNotice("")}>
                  ×
                </button>
              </div>
            )}
          </div>
        ) : null}
        {!book && trash ? (
          <RecycleBin
            data={data}
            busy={busy}
            apply={async (command) => {
              return run(async () => {
                await mutate(command);
              });
            }}
          />
        ) : !book ? (
          <section className="shelf">
            <header className="shelf-header">
              <div>
                <h1>{trash ? "回收站" : "我的书架"}</h1>
                <p>
                  {trash
                    ? "删除的作品保留在这里，可随时恢复。"
                    : `${rows.length} 部作品，让故事慢慢生长。`}
                </p>
              </div>
              <div className="shelf-actions">
                {!trash && api.txtImport && <button disabled={busy || !ready} onClick={() => void run(async () => {setImportOpen("txt");})}>导入TXT</button>}
                {!trash && api.projectImport && <button disabled={busy || !ready} onClick={() => void run(async () => {setImportOpen("project");})}>导入作品文件夹</button>}
                {!trash && (
                  <button
                    className="primary"
                    disabled={busy || !ready}
                    onClick={() =>
                      titleForm(
                        "新增作品",
                        "",
                        async (title, author) => {
                          await mutate({ type: "create_book", title, author });
                        },
                        "",
                      )
                    }
                  >
                    <Plus size={18} />
                    新增作品
                  </button>
                )}
              </div>
            </header>
            <label className="search">
              <Search size={17} />
              <input
                aria-label="搜索作品"
                placeholder="搜索书名或作者"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </label>
            {!ready ? (
              <div className="empty">正在打开本地书库…</div>
            ) : rows.length === 0 ? (
              <div className="empty">
                <BookOpen size={42} />
                <h2>{trash ? "回收站是空的" : "写下你的第一个故事"}</h2>
                <p>
                  {trash
                    ? "作品删除后仍可在此恢复。"
                    : "新增作品，然后从第一章开始。"}
                </p>
              </div>
            ) : (
              <div className="books">
                {rows.map((b, i) => (
                  <article className="book" key={b.id}>
                    <button
                      aria-label={`打开 ${b.title}`}
                      className={`cover color-${i % 4}`}
                      onClick={() => !trash && open(b.id)}
                      disabled={trash || busy}
                    >
                      <span className="cover-line" />
                      <span className="cover-title">{b.title}</span>
                      <span className="cover-author">
                        {b.author || "未署名"}
                      </span>
                      <span className="cover-mark">墨卷</span>
                    </button>
                    <div className="book-info">
                      <h2>{b.title}</h2>
                      <p>
                        {b.author || "未署名"}
                        <span>{bookWords(b.id).toLocaleString()} 字</span>
                      </p>
                      <div className="book-actions">
                        {trash ? (
                          <button
                            onClick={() =>
                              run(async () => {
                                await mutate({
                                  type: "restore_book",
                                  id: b.id,
                                });
                              })
                            }
                          >
                            <RotateCcw size={15} />
                            恢复
                          </button>
                        ) : (
                          <>
                            <button
                              aria-label={`编辑 ${b.title} 信息`}
                              onClick={() =>
                                titleForm(
                                  "作品信息",
                                  b.title,
                                  async (title, author) => {
                                    await mutate({
                                      type: "update_book",
                                      id: b.id,
                                      title,
                                      author,
                                    });
                                  },
                                  b.author,
                                )
                              }
                            >
                              <PenLine size={15} />
                              信息
                            </button>
                            <button
                              aria-label={`删除 ${b.title}`}
                              onClick={() =>
                                setConfirm({
                                  text: `将《${b.title}》移入回收站？正文和目录都会保留。`,
                                  run: async () => {
                                    await mutate({
                                      type: "trash_book",
                                      id: b.id,
                                    });
                                  },
                                })
                              }
                            >
                              <Trash2 size={15} />
                              删除
                            </button>
                          </>
                        )}
                      </div>
                    </div>
                  </article>
                ))}
              </div>
            )}
          </section>
        ) : (
          <div className="workspace">
            <header className="editor-top">
              <button
                aria-label="返回书架"
                onClick={() =>
                  run(async () => {
                    setBook(null);
                    select(null);
                  })
                }
              >
                <ArrowLeft size={19} />
              </button>
              <div className="work-title">
                <strong>{book.title}</strong>
                <span>{book.author || "未署名"}</span>
              </div>
              <button
                onClick={() => void run(async () => {}, false, true)}
                disabled={busy}
              >
                <Save size={17} />
                保存
              </button>
              <button className="primary" onClick={exportBook} disabled={busy}>
                <Download size={17} />
                导出 TXT
              </button>
            </header>
            <nav className="editor-tools" aria-label="写作工具">
              {api.tasks && (
                <button
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      setTasksOpen(true);
                    })
                  }
                >
                  <ListChecks size={15} />
                  检查任务
                </button>
              )}
              {api.replace && (
                <button disabled={busy} onClick={() => void run(async () => {
                  replaceTarget.current = {bookId: bookId!, chapterId};
                  setReplaceOpen(true);
                })}><Replace size={15} />批量替换</button>
              )}
              {api.searchBook && (
                <button
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      setBookSearchOpen(true);
                    })
                  }
                >
                  <Search size={15} />
                  书内搜索
                </button>
              )}
              {api.foreshadow && (
                <button disabled={busy} onClick={() => void run(async () => {
                  // run() has flushed pending prose before either occurrence review path opens.
                  setForeshadowOpen(true);
                })}><Flag size={15} />全书伏笔</button>
              )}
              {api.materials && (
                <button
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      setInitialMaterialId(undefined);
                      materialsOpenRef.current = true;
                      setMaterialsOpen(true);
                    })
                  }
                >
                  <Library size={15} />
                  创作资料
                </button>
              )}
              {api.agentAccess && (
                <button disabled={busy} onClick={() => void run(async () => setAgentAccessOpen(true))}><Bot size={15} />智能体连接</button>
              )}
              {api.workflow && (
                <button
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      setReview(true);
                    })
                  }
                >
                  <History size={15} />
                  改稿与历史
                </button>
              )}
            </nav>
            <div className="writing-layout">
              <Directory
                bookId={book.id}
                volumes={volumes}
                chapters={data.chapters}
                selected={chapter ?? null}
                busy={busy}
                run={run}
                mutate={mutate}
                select={select}
                confirm={(text, action) => setConfirm({ text, run: action })}
                titleForm={titleForm}
                onOpen={openChapter}
              />
              <section className="manuscript">
                {chapter ? (
                  <>
                    <div className="paper-toolbar">
                      <button
                        aria-label={`删除章节 ${chapter.title}`}
                        disabled={busy}
                        onClick={() =>
                          setConfirm({
                            text: `将章节《${chapter.title}》移入回收站？可恢复。`,
                            run: async () => {
                              await mutate({
                                type: "trash_chapter",
                                id: chapter.id,
                              });
                              select(null);
                            },
                          })
                        }
                      >
                        <Trash2 size={15} />
                        删除章节
                      </button>
                      <div className="paper-toolbar-group">
                        <label>
                          所属卷{" "}
                          <select
                            aria-label="移动章节到卷"
                            value={chapter.volume_id}
                            disabled={busy}
                            onChange={(e) => {
                              const volume_id = e.target.value;
                              void run(async () => {
                                await mutate({
                                  type: "transfer_chapter",
                                  id: chapter.id,
                                  volume_id,
                                });
                              });
                            }}
                          >
                            {volumes.map((v) => (
                              <option key={v.id} value={v.id}>
                                {v.title}
                              </option>
                            ))}
                          </select>
                        </label>
                        <label>
                          字号{" "}
                          <select
                            aria-label="正文字号"
                            value={font}
                            onChange={(e) => setFont(Number(e.target.value))}
                          >
                            {[16, 18, 20, 24, 28].map((n) => (
                              <option key={n}>{n}</option>
                            ))}
                          </select>
                        </label>
                        {api.writing && api.materials && <button onClick={openWriting} disabled={busy}>写作约束</button>}
                        {api.split && <button onClick={splitChapter} disabled={busy}>拆分章节</button>}
                      </div>
                    </div>
                    <button
                      className="chapter-heading"
                      onClick={() =>
                        titleForm("修改章名", chapter.title, async (title) => {
                          await mutate({
                            type: "rename_chapter",
                            id: chapter.id,
                            title,
                          });
                        })
                      }
                    >
                      {chapter.title}
                      <PenLine size={16} />
                    </button>
                    <textarea
                      ref={editor}
                      spellCheck={false}
                      aria-label="章节正文"
                      placeholder="故事从这里开始……"
                      value={body}
                      style={{ fontSize: font }}
                      onCompositionStart={() => {
                        composing.current = true;
                        if (timer.current) clearTimeout(timer.current);
                      }}
                      onCompositionEnd={(e) => {
                        composing.current = false;
                        edit(e.currentTarget.value);
                      }}
                      onChange={(e) => edit(e.target.value)}
                      disabled={busy}
                    />
                    {draftRecovery.record && draftRecovery.saved && (
                      <section className="draft-recovery" aria-label="对照本地恢复副本">
                        <p>发现本地恢复副本。核对两份正文后自行决定；不会覆盖原稿。</p>
                        <div className="draft-recovery-pair">
                        <label>
                          当前已存正文（版本 {draftRecovery.saved.revision}）
                          <textarea readOnly aria-label="当前已存正文" value={draftRecovery.saved.body} />
                        </label>
                        <label>
                          本地恢复副本（原基线 {draftRecovery.record.base_revision}）
                          <textarea readOnly aria-label="本地恢复副本" value={draftRecovery.record.draft_body} />
                        </label>
                        </div>
                        {draftRecovery.checking ? <p>正在核对本次副本……</p> : draftRecovery.canChoose ? (
                          <button onClick={() => void draftRecovery.load((q,text) => {
                            if (queue.current !== q || q.dirty) return;
                            q.editRecovered(text);
                            setBody(text);
                            setStatus("未保存");
                          })}>载入副本继续编辑</button>
                        ) : <p>若存档已变化或正在编辑，只能查看和手动复制，不能替换正文。</p>}
                      </section>
                    )}
                    {draftRecovery.available && queue.current?.dirty &&
                      <p role="status">{draftRecovery.message || "恢复副本尚未确认；正文仍未保存。"}</p>}
                    {(queue.current?.unknownAttempt || queue.current?.manualSaveRequired || checkMessage) && (
                      <div className="notice">
                        {queue.current?.unknownAttempt && (
                          <>
                            <p>{SAVE_UNKNOWN}。</p>
                            {checking ? (
                              <button onClick={stopDraftCheck}>停止等待核对</button>
                            ) : (
                              <button onClick={() => void checkSavedDraft()} disabled={!queue.current.unknownAttempt.epoch || !epochRead}>
                                核对已存正文
                              </button>
                            )}
                            {!queue.current.unknownAttempt.epoch && <p>没有可核对的书库标识。可先复制当前稿件。</p>}
                            <button onClick={() => void copyDraft()}>复制当前稿件</button>
                          </>
                        )}
                        {queue.current?.manualSaveRequired && !queue.current.unknownAttempt && (
                          <button onClick={() => void copyDraft()}>复制当前稿件</button>
                        )}
                        {checkMessage && <p role="status">{checkMessage}</p>}
                      </div>
                    )}
                    <footer>
                      <span>{count(body).toLocaleString()} 字</span>
                      <span className={status === "已保存" ? "saved" : ""}>
                        {status === "已保存" && <Check size={14} />} {status}
                      </span>
                      <span>Ctrl + S 保存</span>
                    </footer>
                  </>
                ) : (
                  <div className="empty">选择章节，或新增一章开始写作。</div>
                )}
              </section>
            </div>
          </div>
        )}
      </main>
      {tasksOpen && bookId && api.tasks && (
        <TasksPanel
          bookId={bookId}
          api={api.tasks}
          onClose={() => setTasksOpen(false)}
          onOpen={(hit) => {
            if (hit.kind === "chapter") {
              const found = orderedChapters.find((c) => c.id === hit.id);
              if (!found) {
                setError("章节已不存在或不属于当前作品");
                return;
              }
              select(found);
            } else {
              if (!api.materials) return;
              setInitialMaterialId(hit.id);
              materialsOpenRef.current = true;
              setMaterialsOpen(true);
            }
            setTasksOpen(false);
          }}
        />
      )}
      {writingTarget && api.writing && api.materials && (
        <WritingPanel
          key={JSON.stringify([writingTarget.book, writingTarget.chapter.id])}
          api={api.writing}
          sourceReader={api.foreshadow?.sourceWindow && api.foreshadow.sourceEpoch
            ? {epoch:api.foreshadow.sourceEpoch, sourceWindow:api.foreshadow.sourceWindow} : undefined}
          materials={api.materials}
          bookId={writingTarget.book}
          chapter={writingTarget.chapter}
          recoveryOnly={writingTarget.recoveryOnly}
          decisionSession={writingDecisions}
          isScopeCurrent={() => isWritingCurrent(writingTarget)}
          onPendingChange={(value) => {
            if (writingScope.current === writingTarget) writingPending.current = value;
          }}
          onClose={() => {
            if (writingScope.current === writingTarget && !writingPending.current)
              setWritingScope(null);
          }}
          onReadDecisionChapter={() => readWritingChapter(writingTarget)}
          onReload={async () => {
            const read = await readWritingChapter(writingTarget);
            // A generic reload is not evidence of an outstanding decision.
            // The editor is refreshed only after the full recovery check.
            if (!writingDecisions.recoveries().some((entry) => !writingDecisions.isOld(entry) &&
                entry.target.bookId === writingTarget.book &&
                entry.target.chapterId === writingTarget.chapter.id))
              read.publish?.();
            return read.chapter;
          }}
        />
      )}
      {splitTarget && api.split && <SplitPanel api={api.split} target={splitTarget}
        onPendingChange={p=>{splitPending.current=p;}}
        onClose={()=>{if(!splitPending.current)setSplitTarget(null);}}
        onCommitted={async(outcome)=>{
          if(timer.current) clearTimeout(timer.current);
          select(null);setData(empty);setReady(false);setError("");
          const next=await api.load();
          const targetBook=next.books.find(b=>b.id===splitTarget.bookId&&b.deleted_at==null);
          const valid=new Set(next.volumes.filter(v=>v.book_id===targetBook?.id&&v.deleted_at==null).map(v=>v.id));
          const id=outcome?.new_chapter_id??splitTarget.chapterId;
          const found=next.chapters.find(c=>c.id===id&&c.deleted_at==null&&valid.has(c.volume_id));
          if(outcome&&(!targetBook||!found))throw Error("拆章结果尚未读到，请重新读取核对");
          setData(next);setReady(true);setBook(targetBook?.id??null);select(found??null);
        }}/>}
      {exportOpen && api.txtExport && bookId && <ExportPanel api={api.txtExport} bookId={bookId} volumes={volumes} chapters={orderedChapters} onPendingChange={p => {exportPending.current=p;}} onClose={() => {if (!exportPending.current) setExportOpen(false);}}/>}
      {importOpen === "txt" && api.txtImport && (
        <ImportPanel api={api.txtImport}
          onPendingChange={(pending) => { importPending.current = pending; }}
          onClose={() => { if (!importPending.current) setImportOpen(false); }}
          onImported={async (outcome) => {
            if (timer.current) clearTimeout(timer.current);
            select(null); setBook(null); setData(empty); setReady(false); setError("");
            const next = await api.load();
            setData(next); setReady(true); setTrash(false);
            if (outcome) {
              const created = next.books.find(b => b.id === outcome.book_id && b.deleted_at == null);
              if (created) {
                setBook(created.id);
                const importedVolumes = next.volumes.filter(v => v.book_id === created.id && v.deleted_at == null).sort((a,b) => a.sort_order-b.sort_order);
                const importedChapters = importedVolumes.flatMap(v => next.chapters.filter(c => c.deleted_at == null && c.volume_id === v.id).sort((a,b) => a.sort_order-b.sort_order));
                select(importedChapters[0] ?? null);
              }
            }
            setNotice("已重新读取书库，请核对导入结果。");
          }} />
      )}
      {importOpen === "project" && api.projectImport && (
        <ProjectImportPanel api={api.projectImport}
          onPendingChange={(pending) => { importPending.current = pending; }}
          onClose={() => { if (!importPending.current) setImportOpen(false); }}
          onImported={async (expectedBookId) => {
            const next = await api.load();
            const created = next.books.find(b => b.id === expectedBookId && b.deleted_at === null);
            setData(next); setReady(true); setTrash(false);
            if (!created) return false;
            if (timer.current) clearTimeout(timer.current);
            setBook(created.id);
            const importedVolumes = next.volumes.filter(v => v.book_id === created.id && v.deleted_at === null)
              .sort((a, b) => a.sort_order - b.sort_order);
            const firstVolume = importedVolumes.find(v => next.chapters.some(c => c.volume_id === v.id && c.deleted_at === null));
            const firstChapter = firstVolume && next.chapters.filter(c => c.volume_id === firstVolume.id && c.deleted_at === null)
              .sort((a, b) => a.sort_order - b.sort_order)[0];
            select(firstChapter ?? null);
            return true;
          }} />
      )}
      {agentAccessOpen && book && api.agentAccess && (
        <AgentAccessPanel api={api.agentAccess} book={book} onClose={() => setAgentAccessOpen(false)} />
      )}
      {replaceOpen && replaceTarget.current && api.replace && (
        <ReplacePanel bookId={replaceTarget.current.bookId} api={api.replace}
          onPendingChange={(pending) => { replacePending.current = pending; }}
          onClose={() => { if (!replacePending.current) setReplaceOpen(false); }}
          onCommitted={async () => {
            const target = replaceTarget.current;
            if (timer.current) clearTimeout(timer.current);
            select(null); setData(empty); setReady(false); setError("");
            const next = await api.load();
            setData(next); setReady(true);
            const targetBook = next.books.find(b => b.id === target?.bookId && b.deleted_at == null);
            setBook(targetBook?.id ?? null);
            const validVolumes = new Set(next.volumes.filter(v => v.book_id === targetBook?.id && v.deleted_at == null).map(v => v.id));
            select(next.chapters.find(c => c.id === target?.chapterId && c.deleted_at == null && validVolumes.has(c.volume_id)) ?? null);
            setNotice("已重新读取书库，请核对正文。");
          }} />
      )}
      {bookSearchOpen && bookId && api.searchBook && (
        <SearchPanel
          bookId={bookId}
          search={api.searchBook}
          onClose={() => setBookSearchOpen(false)}
          onOpen={(hit) => {
            if (hit.kind === "chapter") {
              const found = orderedChapters.find((c) => c.id === hit.id);
              if (!found) {
                setError("章节已不存在或不属于当前作品");
                return;
              }
              select(found);
            } else {
              if (!api.materials) return;
              setInitialMaterialId(hit.id);
              materialsOpenRef.current = true;
              setMaterialsOpen(true);
            }
            setBookSearchOpen(false);
          }}
        />
      )}
      {foreshadowOpen && bookId && api.foreshadow && (
        <ForeshadowPanel key={bookId} bookId={bookId} api={api.foreshadow}
          proposalSession={foreshadowProposalSession.current}
          onPendingChange={value=>{foreshadowPending.current=value;}}
          onClose={()=>setForeshadowOpen(false)}
          onJump={(source, epoch)=>run(async()=>{
            const checkEpoch = async () => {
              if(await api.foreshadow!.epoch()!==epoch)
                throw {code:"epoch_changed",message:"书库已更换，未跳转。"};
            };
            await checkEpoch();
            const next=await api.load();
            await checkEpoch();
            const c=next.chapters.find(x=>x.id===source.chapter_id&&x.deleted_at===null&&x.revision===source.chapter_revision&&
              x.title===source.chapter_title&&x.sort_order===source.chapter_sort_order&&x.volume_id===source.volume_id);
            const v=next.volumes.find(x=>x.id===c?.volume_id&&x.book_id===bookId&&x.deleted_at===null&&
              x.title===source.volume_title&&x.sort_order===source.volume_sort_order);
            if(!c||!v||!next.books.some(x=>x.id===bookId&&x.deleted_at===null))
              throw Error("来源已更改或不可用。请刷新伏笔后再尝试跳转。");
            setData(next);
            select(c);
          },true)}
        />
      )}
      {materialsOpen && bookId && api.materials && (
        <MaterialsPanel
          bookId={bookId}
          api={api.materials}
          initialMaterialId={initialMaterialId}
          volumes={data.volumes}
          chapters={data.chapters}
          onPendingChange={(pending) => {
            materialsPending.current = pending;
          }}
          onClose={() => {
            if (materialsPending.current) return;
            materialsOpenRef.current = false;
            setMaterialsOpen(false);
          }}
        />
      )}
      {backupOpen && api.backups && (
        <BackupPanel api={api.backups}
          onPendingChange={(pending) => { backupPending.current = pending; }}
          onClose={() => { if (!backupPending.current) setBackupOpen(false); }}
          onRestored={async () => {
            const reload = ++shelfLoadGeneration.current;
            // Commit is already successful. Discard stale editor state before reload,
            // and let the panel report reload errors separately without re-importing.
            writingDecisions.invalidateLibrary();
            if (timer.current) clearTimeout(timer.current);
            select(null); setBook(null); setTrash(false); setData(empty); setReady(false);
            setReview(false); setMaterialsOpen(false); setForeshadowOpen(false); setTasksOpen(false); setBookSearchOpen(false); setReplaceOpen(false); setImportOpen(false);
            setError(""); setNotice("书库恢复已提交；恢复前留底路径见备份面板。");
            try {
              const next = await api.load();
              if (shelfLoadGeneration.current === reload) {
                setData(next);
                setReady(true);
              }
            } catch (e) {
              if (shelfLoadGeneration.current === reload) throw e;
            }
          }}
        />
      )}
      {review && bookId && api.workflow && (
        <ReviewPanel
          bookId={bookId}
          chapterTitles={Object.fromEntries(
            data.chapters.map((c) => [c.id, c.title]),
          )}
          chapter={chapter}
          api={api.workflow}
          busy={busy}
          onPendingChange={(pending) => { historyPending.current = pending; }}
          onClose={() => { if (!historyPending.current) setReview(false); }}
          perform={(action) =>
            run(async () => {
              const next = await action();
              setData(next);
              select(next.chapters.find((c) => c.id === chapterId) ?? null);
            })
          }
        />
      )}
      {form && (
        <div className="modal-backdrop">
          <form
            className="modal"
            onSubmit={(e) => {
              e.preventDefault();
              const f = form;
              void run(async () => {
                await f.submit(f.title.trim(), f.author ?? "");
                setForm(null);
              });
            }}
          >
            <h2>{form.heading}</h2>
            <label>
              名称
              <input
                autoFocus
                required
                maxLength={200}
                value={form.title}
                onChange={(e) => setForm({ ...form, title: e.target.value })}
              />
            </label>
            {form.author !== undefined && (
              <label>
                作者
                <input
                  maxLength={100}
                  value={form.author}
                  onChange={(e) => setForm({ ...form, author: e.target.value })}
                />
              </label>
            )}
            <div className="modal-actions">
              <button
                type="button"
                disabled={busy}
                onClick={() => setForm(null)}
              >
                取消
              </button>
              <button className="primary" disabled={busy || !form.title.trim()}>
                {busy ? "保存中…" : "确定"}
              </button>
            </div>
          </form>
        </div>
      )}
      {confirm && (
        <div className="modal-backdrop">
          <section className="modal" role="dialog" aria-label="确认操作">
            <h2>确认操作</h2>
            <p>{confirm.text}</p>
            <div className="modal-actions">
              <button disabled={busy} onClick={() => setConfirm(null)}>
                取消
              </button>
              <button
                className="primary"
                disabled={busy}
                onClick={() => {
                  const c = confirm;
                  void run(async () => {
                    await c.run();
                    setConfirm(null);
                  });
                }}
              >
                确认
              </button>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
