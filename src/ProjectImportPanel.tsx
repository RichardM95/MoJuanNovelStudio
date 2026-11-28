import { useEffect, useRef, useState } from "react";
import type { ImportChapter } from "./importTypes";
import type {
  ProjectAssociation,
  ProjectImportAPI,
  ProjectOptions,
  ProjectPreview,
  ProjectSelection,
} from "./projectImportTypes";
import { errorMessage } from "./workflowTypes";
import { Dialog } from "./ui/Dialog";

const roles = [
  ["manuscript", "正文 TXT", "txt"],
  ["world", "核心设定与卷纲", "md"],
  ["outlines", "分章细纲", "md"],
  ["style", "写作风格参考", "md"],
] as const;
const associationNames: Record<ProjectAssociation["status"], string> = {
  same_title: "同号同题",
  different_title: "同号异题",
  missing_chapter: "缺正文",
  ambiguous: "章号或归属不唯一",
};
const cardNames = {
  original: "原文",
  volume_outline: "卷纲",
  chapter_outline: "章纲",
  setting: "设定",
  character: "角色",
};
const chapterName = (chapter: ImportChapter) =>
  chapter.source?.number == null
    ? chapter.title
    : `第${chapter.source.number}${chapter.source.unit ?? "章"} ${chapter.title}`;
const range = (first: number, last: number) =>
  first === last ? String(first) : `${first}–${last}`;
type Receipt = { preview: ProjectPreview; error: string | null };
type Inspection = {
  cardIndex?: number;
  headings: string[];
  bodies?: string[];
  error?: string;
};

export default function ProjectImportPanel({
  api,
  onClose,
  onPendingChange,
  onImported,
}: {
  api: ProjectImportAPI;
  onClose: () => void;
  onPendingChange: (pending: boolean) => void;
  onImported: (bookId: string) => Promise<boolean>;
}) {
  const [selection, setSelection] = useState<ProjectSelection | null>(null);
  const [options, setOptions] = useState<ProjectOptions | null>(null);
  const [preview, setPreview] = useState<ProjectPreview | null>(null);
  const [links, setLinks] = useState<Record<number, number>>({});
  const [excluded, setExcluded] = useState<Set<number>>(new Set());
  const [inspection, setInspection] = useState<Inspection | null>(null);
  const [receipt, setReceipt] = useState<Receipt | null>(null);
  const [done, setDone] = useState<ProjectPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [now, setNow] = useState(Date.now);
  const inspectionRoot = useRef<HTMLDivElement>(null);
  const selectionRef = useRef<ProjectSelection | null>(null);
  const detailSeq = useRef(0);
  const mounted = useRef(true);
  const working = useRef(false);
  const pending = useRef(false);
  const callbacks = useRef({ onClose, onPendingChange, onImported });
  callbacks.current = { onClose, onPendingChange, onImported };

  function close() {
    if (!working.current && !pending.current) callbacks.current.onClose();
  }

  useEffect(() => {
    mounted.current = true;
    const clock = setInterval(() => setNow(Date.now()), 1000);
    return () => {
      mounted.current = false;
      detailSeq.current++;
      clearInterval(clock);
      const old = selectionRef.current;
      if (old) void api.discard(old.id).catch(() => {});
      callbacks.current.onPendingChange(false);
    };
  }, [api]);

  useEffect(() => {
    if (inspection && !inspection.bodies && !inspection.error) {
      inspectionRoot.current?.scrollIntoView?.({ block: "nearest" });
    }
  }, [inspection]);

  const expired = !!selection && now >= selection.expires_at * 1000;
  const chapters = preview?.volumes.flatMap((volume) => volume.chapters) ?? [];
  const inspectedIndex = inspection?.cardIndex;
  const inspectedCard =
    inspectedIndex === undefined ? undefined : preview?.cards[inspectedIndex];
  const inspectedAssociation = inspectedCard?.association;
  const inspectedVolume =
    inspectedAssociation?.volume_index == null
      ? undefined
      : preview?.volumes[inspectedAssociation.volume_index];

  function clearPreview() {
    detailSeq.current++;
    setPreview(null);
    setInspection(null);
    setLinks({});
    setExcluded(new Set());
  }

  function edit(patch: Partial<ProjectOptions>) {
    if (working.current || !options) return;
    clearPreview();
    setError("");
    setOptions({ ...options, ...patch });
  }

  async function run(action: () => Promise<void>) {
    if (working.current) return;
    working.current = true;
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (cause) {
      if (mounted.current) setError(errorMessage(cause));
    } finally {
      working.current = false;
      if (mounted.current) setBusy(false);
    }
  }

  function choose() {
    void run(async () => {
      const picked = await api.choose();
      if (!picked) return;
      if (!mounted.current) {
        void api.discard(picked.id).catch(() => {});
        return;
      }
      selectionRef.current = picked;
      clearPreview();
      setDone(null);
      setSelection(picked);
      setOptions({
        roles: picked.suggested_roles,
        title: null,
        author: "",
        omit_separators: true,
        ...(api.automatic ? { automatic: true } : {}),
      });
      setNow(Date.now());
    });
  }

  function prepare() {
    if (
      !selection ||
      !options ||
      (!options.automatic && !options.roles.manuscript) ||
      expired
    )
      return;
    void run(async () => {
      clearPreview();
      const next = await api.prepare(selection.id, options);
      if (!mounted.current) return;
      const suggested: Record<number, number> = {};
      next.cards.forEach(({ association }, index) => {
        if (
          association?.status === "same_title" &&
          association.chapter_index != null
        ) {
          suggested[index] = association.chapter_index;
        }
      });
      setLinks(suggested);
      setPreview(next);
      setNow(Date.now());
    });
  }

  async function inspect(
    cardIndex: number | undefined,
    chapterIndex: number | undefined,
  ) {
    if (!preview) return;
    const request = ++detailSeq.current;
    const items: { kind: "card" | "chapter"; index: number; title: string }[] =
      [];
    if (cardIndex !== undefined) {
      const card = preview.cards[cardIndex];
      items.push({ kind: "card", index: cardIndex, title: card.title });
    }
    const chapter = chapters.find(({ index }) => index === chapterIndex);
    if (chapter)
      items.push({
        kind: "chapter",
        index: chapter.index,
        title: `正文 · ${chapterName(chapter)}`,
      });
    const headings = items.map(({ title }) => title);
    setInspection({ cardIndex, headings });
    try {
      const records = await Promise.all(
        items.map(({ index, kind }) => api.detail(preview.id, kind, index)),
      );
      if (
        records.some((record, index) => record.index !== items[index].index)
      ) {
        throw new Error("预览条目回执不匹配，请重新打开。");
      }
      if (request === detailSeq.current)
        setInspection({
          cardIndex,
          headings,
          bodies: records.map(({ body }) => body),
        });
    } catch (cause) {
      if (request === detailSeq.current)
        setInspection({ cardIndex, headings, error: errorMessage(cause) });
    }
  }

  function changeLink(cardIndex: number, value: string) {
    const next = { ...links };
    if (value === "") delete next[cardIndex];
    else next[cardIndex] = Number(value);
    setLinks(next);
    if (inspection?.cardIndex === cardIndex) {
      void inspect(
        cardIndex,
        next[cardIndex] ??
          preview?.cards[cardIndex].association?.chapter_index ??
          undefined,
      );
    }
  }

  async function check(attempt: Receipt) {
    const found = await callbacks.current.onImported(attempt.preview.book_id);
    if (!mounted.current) return;
    if (!found && !attempt.error)
      throw new Error("已收到导入回执，暂时无法在书架确认作品。");
    setReceipt(null);
    pending.current = false;
    callbacks.current.onPendingChange(false);
    if (found) {
      setDone(attempt.preview);
    } else {
      setError(`书架中未找到本次导入作品。${attempt.error}`);
    }
  }

  function commit() {
    if (!preview || pending.current || Date.now() >= preview.expires_at * 1000)
      return;
    void run(async () => {
      pending.current = true;
      callbacks.current.onPendingChange(true);
      const associations = Object.entries(links)
        .filter(([index]) => !excluded.has(Number(index)))
        .map(([index, chapter_index]) => ({
          card_index: Number(index),
          chapter_index,
        }));
      selectionRef.current = null;
      setSelection(null);
      clearPreview();
      let commitError: string | null = null;
      try {
        const result = excluded.size
          ? await api.commit(preview.id, associations, [...excluded])
          : await api.commit(preview.id, associations);
        if (result.book_id !== preview.book_id)
          throw new Error("导入回执的作品 ID 不匹配。");
      } catch (cause) {
        commitError = errorMessage(cause);
      }
      if (!mounted.current) return;
      const attempt = {
        preview: {
          ...preview,
          cards: preview.cards.filter((_, i) => !excluded.has(i)),
        },
        error: commitError,
      };
      setReceipt(attempt);
      await check(attempt);
    });
  }

  return (
    <Dialog
      title="导入作品文件夹"
      subtitle="从文件夹读取正文、卷纲、章纲与资料卡，导入前逐项核对。"
      closeLabel="关闭"
      className="project-import"
      size="lg"
      dismissible={!busy && !receipt}
      onClose={close}
      footer={
        preview ? (
          <button className="primary" onClick={commit} disabled={busy || expired}>
            确认导入新作品
          </button>
        ) : undefined
      }
    >
      <div className="project-import-body">
        {error && <p role="alert">{error}</p>}
        {receipt ? (
          <div className="ui-card">
            <p role="status">正在核对本次导入结果。{receipt.error}</p>
            <button
              onClick={() => void run(() => check(receipt))}
              disabled={busy}
            >
              核对书架
            </button>
          </div>
        ) : (
          <button onClick={choose} disabled={busy}>
            选择作品文件夹
          </button>
        )}
        {done && (
          <p role="status">
            已导入 {done.volumes.length} 卷、{done.total_chapters} 章和{" "}
            {done.cards.length} 张资料卡。
          </p>
        )}
        {selection && options && (
          <>
            <p>{selection.folder_name}</p>
            {api.automatic && (
              <label className="project-check">
                <input
                  type="checkbox"
                  checked={options.automatic ?? false}
                  disabled={busy || expired}
                  onChange={(event) =>
                    edit({ automatic: event.target.checked })
                  }
                />
                按内容自动整理
              </label>
            )}
            {options.automatic && (
              <p>
                {selection.files.filter((file) => file.selectable).length}{" "}
                份文本可分析。生成预览后可核对识别结果。
              </p>
            )}
            <div className="project-fields">
              {options.automatic && (
                <label>
                  正文候选
                  <select
                    value={options.manuscript_override ?? ""}
                    disabled={busy || expired}
                    onChange={(event) =>
                      edit({ manuscript_override: event.target.value || null })
                    }
                  >
                    <option value="">按内容识别</option>
                    {selection.files
                      .filter((file) => file.selectable)
                      .map((file) => (
                        <option key={file.name} value={file.name}>
                          {file.name}
                        </option>
                      ))}
                  </select>
                </label>
              )}
              {!options.automatic &&
                roles.map(([key, label, kind]) => (
                  <label key={key}>
                    {label}
                    <select
                      value={options.roles[key] ?? ""}
                      disabled={busy || expired}
                      onChange={(event) =>
                        edit({
                          roles: {
                            ...options.roles,
                            [key]: event.target.value || null,
                          },
                        })
                      }
                    >
                      <option value="">
                        {key === "manuscript" ? "请选择正文" : "跳过"}
                      </option>
                      {selection.files
                        .filter((file) => file.kind === kind)
                        .map((file) => (
                          <option
                            key={file.name}
                            value={file.name}
                            disabled={!file.selectable}
                          >
                            {file.name}
                            {file.selectable ? "" : "（超出大小限制）"}
                          </option>
                        ))}
                    </select>
                  </label>
                ))}
              <label>
                书名
                <input
                  value={options.title ?? preview?.title ?? ""}
                  placeholder="从正文读取"
                  disabled={busy || expired}
                  onChange={(event) =>
                    edit({ title: event.target.value || null })
                  }
                />
              </label>
              <label>
                作者
                <input
                  value={options.author}
                  disabled={busy || expired}
                  onChange={(event) => edit({ author: event.target.value })}
                />
              </label>
            </div>
            <label className="project-check">
              <input
                type="checkbox"
                checked={options.omit_separators}
                disabled={busy || expired}
                onChange={(event) =>
                  edit({ omit_separators: event.target.checked })
                }
              />
              排除卷章边界的排版分隔线
            </label>
            <button
              onClick={prepare}
              disabled={
                busy ||
                expired ||
                (!options.automatic && !options.roles.manuscript)
              }
            >
              生成导入预览
            </button>
            {expired && <p role="status">预览已过期，请重新选择作品文件夹。</p>}
          </>
        )}
        {preview && (
          <>
            <p>
              {preview.title} · {preview.volumes.length} 卷 ·{" "}
              {preview.total_chapters} 章 · {preview.cards.length} 张资料卡
            </p>
            {options?.automatic && <p>正文来源 · {preview.source_name}</p>}
            {preview.warnings.map((warning) => (
              <p key={warning}>{warning}</p>
            ))}
            {preview.numbering.length > 0 && (
              <ul className="project-warnings">
                {preview.numbering.map((issue) => (
                  <li key={`${issue.volume}-${issue.kind}-${issue.first}`}>
                    {preview.volumes[issue.volume].title} ·{" "}
                    {issue.kind === "missing" ? "缺少章号" : "重复章号"}{" "}
                    {range(issue.first, issue.last)}
                  </li>
                ))}
              </ul>
            )}
            {preview.separator_lines.length > 0 && (
              <details>
                <summary>
                  {preview.omitted_separators
                    ? "拟排除的边界分隔线"
                    : "保留的边界分隔线"}
                </summary>
                <p>
                  源文件行号{" "}
                  {preview.separator_lines
                    .map(({ first, last }) => range(first, last))
                    .join("、")}
                </p>
              </details>
            )}
            <details>
              <summary>卷章目录</summary>
              <div className="project-list">
                {preview.volumes.map((volume, index) => (
                  <div key={index}>
                    <h3>
                      {volume.source?.number != null &&
                        `第${volume.source.number}${volume.source.unit ?? "卷"} `}
                      {volume.title}
                      {volume.source && (
                        <small> · 源第 {volume.source.line} 行</small>
                      )}
                    </h3>
                    <ul>
                      {volume.chapters.map((chapter) => (
                        <li key={chapter.index}>
                          <button
                            onClick={() =>
                              void inspect(undefined, chapter.index)
                            }
                            disabled={expired}
                          >
                            {chapterName(chapter)}
                          </button>
                          {chapter.source && (
                            <small>源第 {chapter.source.line} 行</small>
                          )}
                        </li>
                      ))}
                    </ul>
                  </div>
                ))}
              </div>
            </details>
            {preview.cards.length > 0 && (
              <details open>
                <summary>资料和章纲关联</summary>
                <ul className="project-list">
                  {preview.cards.map((card, cardIndex) => {
                    const match = card.association;
                    const linkedChapter = chapters[links[cardIndex]];
                    const volume =
                      card.volume_index == null
                        ? undefined
                        : preview.volumes[card.volume_index];
                    return (
                      <li key={cardIndex}>
                        {options?.automatic && card.kind !== "original" && (
                          <input
                            type="checkbox"
                            aria-label={"导入 " + card.title}
                            checked={!excluded.has(cardIndex)}
                            disabled={busy || expired}
                            onChange={(event) => {
                              setExcluded((previous) => {
                                const next = new Set(previous);
                                if (event.target.checked)
                                  next.delete(cardIndex);
                                else next.add(cardIndex);
                                return next;
                              });
                            }}
                          />
                        )}
                        <button
                          onClick={() =>
                            void inspect(
                              cardIndex,
                              links[cardIndex] ??
                                match?.chapter_index ??
                                undefined,
                            )
                          }
                          disabled={expired}
                        >
                          {card.title}
                        </button>
                        <small>
                          {cardNames[card.kind]} · {card.file} · 第{" "}
                          {range(card.first_line, card.last_line)} 行
                        </small>
                        {match && (
                          <span className="project-association">
                            <small>{associationNames[match.status]}</small>
                            <small>
                              {linkedChapter
                                ? `关联 ${chapterName(linkedChapter)}`
                                : match.volume_index == null
                                  ? "待关联 · 归入书级"
                                  : "待关联 · 归入卷级"}
                            </small>
                          </span>
                        )}
                        {card.kind === "volume_outline" && (
                          <small>{volume?.title ?? "归入书级"}</small>
                        )}
                      </li>
                    );
                  })}
                </ul>
              </details>
            )}
            {inspection && (
              <div
                ref={inspectionRoot}
                className="project-inspection import-detail"
                aria-live="polite"
              >
                {inspectedCard &&
                  inspectedIndex !== undefined &&
                  inspectedAssociation && (
                    <label className="project-link">
                      关联正文
                      <select
                        aria-label={`${inspectedCard.title}关联`}
                        value={links[inspectedIndex] ?? ""}
                        onChange={(event) =>
                          changeLink(inspectedIndex, event.target.value)
                        }
                        disabled={expired || !inspectedVolume}
                      >
                        <option value="">
                          {inspectedVolume
                            ? "待关联 · 归入卷级"
                            : "待关联 · 归入书级"}
                        </option>
                        {inspectedVolume?.chapters
                          .filter((chapter) => chapter.source)
                          .map((chapter) => (
                            <option key={chapter.index} value={chapter.index}>
                              {chapterName(chapter)}
                            </option>
                          ))}
                      </select>
                    </label>
                  )}
                {inspection.error && <p role="alert">{inspection.error}</p>}
                {!inspection.bodies && !inspection.error && (
                  <p>正在读取预览…</p>
                )}
                {inspection.bodies &&
                  inspection.headings.map((heading, index) => (
                    <section key={index}>
                      <h3>{heading}</h3>
                      <pre>{inspection.bodies?.[index]}</pre>
                    </section>
                  ))}
              </div>
            )}
          </>
        )}
      </div>
    </Dialog>
  );
}
