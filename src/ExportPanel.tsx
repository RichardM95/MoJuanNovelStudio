import React, { useState, useRef, useEffect } from "react";
import type { ExportAPI, ExportOptions, ExportPreview } from "./exportTypes";
import type { Volume, Chapter } from "./types";
import { errorMessage } from "./workflowTypes";
const IMPORT_BYTE_LIMIT = 16 * 1024 * 1024;
const EXPORT_BYTE_LIMIT = 32 * 1024 * 1024;
const PREVIEW_TTL_MS = 10 * 60 * 1000;

type PreparedPreview = {
  preview: ExportPreview;
  bookId: string;
  api: ExportAPI;
  options: ExportOptions;
  generation: number;
  receivedAt: number;
};

export default function ExportPanel({
  api,
  bookId,
  volumes,
  chapters,
  onClose,
  onPendingChange,
}: {
  api: ExportAPI;
  bookId: string;
  volumes: Volume[];
  chapters: Chapter[];
  onClose: () => void;
  onPendingChange: (pending: boolean) => void;
}) {
  const [options, setOptions] = useState<ExportOptions>({
    scope: "book",
    target_id: null,
    include_metadata: true,
    include_headings: true,
    newline: "crlf",
    bom: false,
  });
  const [prepared, setPrepared] = useState<PreparedPreview | null>(null),
    [expiredAdvice, setExpiredAdvice] = useState<PreparedPreview | null>(null),
    [busy, setBusy] = useState(false),
    [message, setMessage] = useState("");
  const generation = useRef(0),
    candidate = useRef<string | null>(null),
    alive = useRef(true),
    locked = useRef(false),
    saving = useRef(false);
  const preview = prepared?.preview ?? null;
  const pending = useRef(onPendingChange);
  pending.current = onPendingChange;
  const discard = (id: string) => {
    void api.discard(id).catch(() => {});
  };
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      generation.current++;
      if (candidate.current) discard(candidate.current);
      pending.current(false);
    };
  }, [api, bookId]);
  const change = (patch: Partial<ExportOptions>) => {
    if (saving.current) return;
    generation.current++;
    if (candidate.current) discard(candidate.current);
    candidate.current = null;
    setPrepared(null);
    setMessage("");
    setOptions((o) => ({ ...o, ...patch }));
  };
  const prepare = async () => {
    if (locked.current) return;
    locked.current = true;
    setBusy(true);
    pending.current(true);
    setMessage("");
    if (candidate.current) discard(candidate.current);
    candidate.current = null;
    setPrepared(null);
    const g = ++generation.current;
    try {
      const p = await api.prepare(bookId, options);
      if (!alive.current || g !== generation.current) {
        discard(p.id);
        return;
      }
      candidate.current = p.id;
      setPrepared({
        preview: p,
        bookId,
        api,
        options,
        generation: g,
        receivedAt: Date.now(),
      });
    } catch (e) {
      if (alive.current && g === generation.current)
        setMessage(errorMessage(e));
    } finally {
      locked.current = false;
      if (alive.current) {
        setBusy(false);
        pending.current(false);
      }
    }
  };
  const save = async () => {
    if (locked.current || !preview) return;
    if (preview.expires_at * 1000 <= Date.now()) {
      change({});
      setMessage("预览已过期，请重新生成");
      return;
    }
    locked.current = true;
    saving.current = true;
    setBusy(true);
    pending.current(true);
    setMessage("");
    try {
      const path = await api.save(preview.id);
      if (path !== null) {
        if (typeof path !== "string" || path.length === 0) {
          throw new Error("未收到有效的保存位置");
        }
        candidate.current = null;
        setPrepared(null);
        setMessage("TXT已导出：" + path);
      }
    } catch (e) {
      candidate.current = null;
      setPrepared(null);
      setMessage(
        "导出结果未确认，请核对目标文件后重新预览。" + errorMessage(e),
      );
    } finally {
      saving.current = false;
      locked.current = false;
      if (alive.current) {
        setBusy(false);
        pending.current(false);
      }
    }
  };
  const close = () => {
    if (locked.current) return;
    onClose();
  };
  useEffect(() => {
    const escape = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close();
      }
    };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, []);
  // This is advice only. A truncated preview is not a measurement, and
  // neither questionable metadata nor this timer changes the save workflow.
  const expiresAt = preview ? preview.expires_at * 1000 : 0;
  const targetMatches =
    options.scope === "book"
      ? options.target_id === null
      : (options.scope === "volume" ? volumes : chapters).some(
          (item) => item.id === options.target_id && item.deleted_at === null,
        );
  const advise =
    prepared !== null &&
    preview !== null &&
    prepared !== expiredAdvice &&
    prepared.api === api &&
    prepared.bookId === bookId &&
    prepared.options === options &&
    prepared.generation === generation.current &&
    typeof preview.id === "string" &&
    preview.id.length > 0 &&
    candidate.current === preview.id &&
    targetMatches &&
    Number.isSafeInteger(preview.bytes) &&
    preview.bytes > IMPORT_BYTE_LIMIT &&
    preview.bytes <= EXPORT_BYTE_LIMIT &&
    preview.bom === options.bom &&
    preview.newline === options.newline &&
    Number.isFinite(preview.expires_at) &&
    expiresAt > Date.now() &&
    expiresAt <= prepared.receivedAt + PREVIEW_TTL_MS
      ? prepared
      : null;
  useEffect(() => {
    if (!advise) return;
    const timer = window.setTimeout(
      () => setExpiredAdvice(advise),
      Math.min(PREVIEW_TTL_MS, Math.max(0, expiresAt - Date.now())),
    );
    return () => window.clearTimeout(timer);
  }, [advise, expiresAt]);

  return (
    <div className="modal-backdrop">
      <section
        className="modal txt-export"
        role="dialog"
        aria-modal="true"
        aria-label="导出TXT"
      >
        <header>
          <h2>导出TXT</h2>
          <button onClick={close} disabled={busy} aria-label="关闭导出面板">
            关闭
          </button>
        </header>
        <label>
          导出范围
          <select
            aria-label="导出范围"
            disabled={saving.current}
            value={options.scope}
            onChange={(e) => {
              const scope = e.target.value as ExportOptions["scope"];
              change({
                scope,
                target_id:
                  scope === "volume"
                    ? (volumes[0]?.id ?? null)
                    : scope === "chapter"
                      ? (chapters[0]?.id ?? null)
                      : null,
              });
            }}
          >
            <option value="book">全书</option>
            <option value="volume">单卷</option>
            <option value="chapter">单章</option>
          </select>
        </label>
        {options.scope !== "book" && (
          <label>
            选择卷章
            <select
              aria-label="选择卷章"
              disabled={saving.current}
              value={options.target_id ?? ""}
              onChange={(e) => change({ target_id: e.target.value })}
            >
              <option value="" disabled>
                请选择
              </option>
              {(options.scope === "volume" ? volumes : chapters).map((x) => (
                <option key={x.id} value={x.id}>
                  {x.title || "未命名"}
                </option>
              ))}
            </select>
          </label>
        )}
        <label>
          <input
            type="checkbox"
            checked={options.include_metadata}
            disabled={saving.current}
            onChange={(e) => change({ include_metadata: e.target.checked })}
          />
          书名和作者
        </label>
        <label>
          <input
            type="checkbox"
            checked={options.include_headings}
            disabled={saving.current}
            onChange={(e) => change({ include_headings: e.target.checked })}
          />
          卷章标题
        </label>
        <label>
          <input
            type="checkbox"
            checked={options.bom}
            disabled={saving.current}
            onChange={(e) => change({ bom: e.target.checked })}
          />
          UTF-8 BOM
        </label>
        <label>
          换行
          <select
            aria-label="换行"
            value={options.newline}
            disabled={saving.current}
            onChange={(e) =>
              change({ newline: e.target.value as "lf" | "crlf" })
            }
          >
            <option value="crlf">CRLF（Windows）</option>
            <option value="lf">LF</option>
          </select>
        </label>
        <p>
          卷章在所选范围内从1编号；换行统一为所选格式，正文不裁剪。仅新建文件，不覆盖同名文件。
        </p>
        <p>
          TXT 导出最多32 MiB；要保留作品结构、资料和历史，请使用 SQLite 备份。
        </p>
        <button
          onClick={prepare}
          disabled={busy || (options.scope !== "book" && !options.target_id)}
        >
          生成预览
        </button>
        {preview && (
          <>
            <p>
              {preview.volumes}卷 · {preview.chapters}章 · {preview.bytes}字节 ·
              UTF-8{preview.bom ? " BOM" : ""} · {preview.newline.toUpperCase()}
            </p>
            <p>
              保存的是本次预览时刻的内容，10分钟内有效。
              {preview.truncated
                ? "仅显示前4000字，文件包含完整内容。"
                : "以下为完整文本。"}
              BOM单独标记，不显示为正文。
            </p>
            {advise && (
              <p role="status">
                本次预览超过16
                MiB，不能作为同一个TXT文件整体再次导入。
              </p>
            )}
            <pre aria-label="导出内容预览">{preview.text}</pre>
            <button onClick={save} disabled={busy}>
              保存TXT
            </button>
          </>
        )}
        {message && <p role="status">{message}</p>}
      </section>
    </div>
  );
}
