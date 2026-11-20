import React, { useEffect, useRef, useState } from "react";
import type { Chapter, LibraryState } from "./types";
import type { WorkflowAPI, Proposal } from "./workflowTypes";
import HistoryPanel from "./HistoryPanel";
import { errorMessage } from "./workflowTypes";
type Confirmation = {
  kind: "accept";
  id: string;
  chapterId: string;
  expectedRevision: number;
  scope: string;
};
const navigationWarning = "请先保存名称或取消编辑、恢复确认。";
const words = (text: string) => Array.from(text.replace(/\s/g, "")).length;
export default function ReviewPanel({
  bookId,
  chapter,
  api,
  perform,
  onClose,
  busy,
  chapterTitles = {},
  onPendingChange,
}: {
  bookId: string;
  chapter: Chapter | undefined;
  api: WorkflowAPI;
  perform: (action: () => Promise<LibraryState>) => Promise<boolean>;
  onClose: () => void;
  busy: boolean;
  chapterTitles?: Record<string, string>;
  onPendingChange?: (pending: boolean) => void;
}) {
  const [tab, setTab] = useState<"proposals" | "history">("proposals"),
    [proposals, setProposals] = useState<Proposal[]>([]),
    [selected, setSelected] = useState(""),
    [error, setError] = useState(""),
    [confirm, setConfirm] = useState<Confirmation | null>(null),
    [loading, setLoading] = useState(true),
    [working, setWorking] = useState(false);
  const [historyPending, setHistoryPending] = useState(false);
  const pendingCallback = useRef(onPendingChange);
  pendingCallback.current = onPendingChange;
  useEffect(() => {
    pendingCallback.current?.(historyPending || working || !!confirm);
  }, [historyPending, working, confirm]);
  useEffect(() => () => pendingCallback.current?.(false), []);
  useEffect(() => {
    if (!historyPending)
      setError((previous) => (previous === navigationWarning ? "" : previous));
  }, [historyPending]);
  const guardNavigation = () => {
    if (historyPending) {
      setError(navigationWarning);
      return false;
    }
    return true;
  };
  const generation = useRef(0);
  const panel = useRef<HTMLElement>(null);
  const scope = `${bookId}:${chapter?.id ?? ""}:${chapter?.revision ?? ""}`;
  const currentScope = useRef(scope);
  currentScope.current = scope;
  const refresh = async () => {
    // An action started against an older render must not replace a newer view.
    if (currentScope.current !== scope) return;
    const request = ++generation.current;
    setLoading(true);
    const active = () =>
      generation.current === request && currentScope.current === scope;
    try {
      const ps = await api.listProposals(bookId);
      if (!active()) return;
      setProposals(ps.filter((p) => p.status === "pending"));
      setError("");
    } catch (e) {
      if (active()) setError(errorMessage(e));
    } finally {
      if (active()) setLoading(false);
    }
  };
  useEffect(() => {
    setConfirm(null);
    setSelected("");
    if (tab === "proposals") void refresh();
    return () => {
      generation.current++;
    };
  }, [bookId, chapter?.id, chapter?.revision, api, tab]);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const root = panel.current!;
    const focusables = () =>
      Array.from(
        root.querySelectorAll<HTMLElement>(
          'button:not(:disabled),select:not(:disabled),input:not(:disabled),textarea:not(:disabled),a[href],[tabindex="0"]',
        ),
      );
    const focusFirst = () => (focusables()[0] ?? root).focus();
    const guard = (event: FocusEvent) => {
      if (!root.contains(event.target as Node)) focusFirst();
    };
    const trap = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const items = focusables();
      const first = items[0] ?? root,
        last = items.at(-1) ?? root;
      if (
        event.shiftKey &&
        (document.activeElement === first || document.activeElement === root)
      ) {
        event.preventDefault();
        last.focus();
      } else if (
        !event.shiftKey &&
        (document.activeElement === last || document.activeElement === root)
      ) {
        event.preventDefault();
        first.focus();
      }
    };
    focusFirst();
    document.addEventListener("focusin", guard);
    root.addEventListener("keydown", trap);
    return () => {
      document.removeEventListener("focusin", guard);
      root.removeEventListener("keydown", trap);
      if (previous?.isConnected) previous.focus();
    };
  }, []);
  const proposal = proposals.find((p) => p.id === selected) ?? proposals[0];
  const locked = busy || working;
  const execute = async (action: () => Promise<LibraryState>) => {
    if (locked) return;
    setWorking(true);
    try {
      const ok = await perform(action);
      if (ok) {
        setConfirm(null);
        await refresh();
      } else
        setError("操作未完成。请查看保存或版本冲突提示；当前稿件未被替换。");
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setWorking(false);
    }
  };
  return (
    <div className="review-backdrop">
      <section
        ref={panel}
        tabIndex={-1}
        className="review-panel"
        role="dialog"
        aria-modal="true"
        aria-label="改稿与历史"
      >
        <header>
          <div>
            <h2>改稿与历史</h2>
            <p>外部智能体只能提交建议，正式稿件由你决定。</p>
          </div>
          <button
            disabled={locked}
            aria-label="关闭改稿面板"
            onClick={() => {
              if (guardNavigation()) onClose();
            }}
          >
            ×
          </button>
        </header><p>此处为普通正文提案，无冻结约束依据。启用或暂停写作约束的章节不能从这里采纳，请到“写作约束”审阅严格建议。</p>
        <nav className="review-tabs">
          <button
            disabled={locked}
            className={tab === "proposals" ? "active" : ""}
            onClick={() => {
              if (!guardNavigation()) return;
              setError("");
              setTab("proposals");
              setConfirm(null);
            }}
          >
            待采纳提案
          </button>
          <button
            disabled={locked || !chapter}
            className={tab === "history" ? "active" : ""}
            onClick={() => {
              if (!guardNavigation()) return;
              setError("");
              setTab("history");
              setConfirm(null);
            }}
          >
            章节历史
          </button>
          {tab === "proposals" && (
            <button
              disabled={locked || loading || !!confirm}
              onClick={() => void refresh()}
            >
              刷新
            </button>
          )}
        </nav>
        {error && (
          <p role="alert" className="review-error">
            {error}
          </p>
        )}
        {tab === "history" ? (
          chapter && api.versionBrowser ? (
            <HistoryPanel
              key={`${bookId}:${chapter.id}`}
              bookId={bookId}
              chapter={chapter}
              api={api.versionBrowser}
              perform={perform}
              busy={locked}
              onPendingChange={setHistoryPending}
            />
          ) : (
            <p>当前连接不支持分页历史，请更新桌面版本。</p>
          )
        ) : loading ? (
          <p>读取中…</p>
        ) : proposal ? (
          <>
            <label>
              提案
              <select
                aria-label="选择提案"
                disabled={locked || !!confirm}
                value={proposal.id}
                onChange={(e) => setSelected(e.target.value)}
              >
                {proposals.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.source} · {p.reason || "章节改稿"} · {p.id.slice(0, 8)}
                  </option>
                ))}
              </select>
            </label>
            <p>
              章节：
              {chapterTitles[proposal.chapter_id] ?? proposal.chapter_id} ·
              来源：{proposal.source} · 基于版本 {proposal.base_revision} · 字数{" "}
              {words(proposal.old_body)} → {words(proposal.new_body)}
            </p>
            <p>{proposal.reason}</p>
            <div className="diff-columns">
              <article>
                <h3>提交时原稿</h3>
                <pre>{proposal.old_body || "（空白）"}</pre>
              </article>
              <article>
                <h3>建议稿</h3>
                <pre>{proposal.new_body || "（空白）"}</pre>
              </article>
            </div>
            <footer>
              <button
                disabled={locked || !!confirm}
                onClick={() =>
                  void execute(() =>
                    api.decideProposal(
                      proposal.id,
                      false,
                      proposal.base_revision,
                    ),
                  )
                }
              >
                拒绝此提案
              </button>
              <button
                className="primary"
                disabled={locked || !!confirm}
                onClick={() =>
                  setConfirm({
                    kind: "accept",
                    id: proposal.id,
                    chapterId: proposal.chapter_id,
                    expectedRevision: proposal.base_revision,
                    scope,
                  })
                }
              >
                采纳此提案
              </button>
            </footer>
          </>
        ) : (
          <p className="empty">没有待采纳提案。</p>
        )}
        {confirm && confirm.scope === scope && (
          <div
            className="review-confirm"
            role="alertdialog"
            aria-label="确认正文变更"
          >
            <p>
              确认将上述建议稿替换到正式章节？提交后原稿会保留在历史中；版本变化时会拒绝覆盖。
            </p>
            <button disabled={locked} onClick={() => setConfirm(null)}>
              取消
            </button>
            <button
              className="primary"
              disabled={locked}
              onClick={() => {
                if (confirm.scope !== currentScope.current) return;
                const target = confirm;
                if (target.kind === "accept")
                  void execute(() =>
                    api.decideProposal(
                      target.id,
                      true,
                      target.expectedRevision,
                    ),
                  );
              }}
            >
              确认采纳
            </button>
          </div>
        )}
      </section>
    </div>
  );
}
