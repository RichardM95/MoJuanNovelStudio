import { useEffect, useRef, useState } from "react";
import type {
  Occurrence,
  ForeshadowSourceReader,
  SourceWindow,
} from "./foreshadowTypes";

function positionDiffers(current: SourceWindow["position"], source: Occurrence["source"]) {
  return current.chapter_title !== source.chapter_title ||
    current.chapter_sort_order !== source.chapter_sort_order ||
    current.volume_id !== source.volume_id ||
    current.volume_title !== source.volume_title ||
    current.volume_sort_order !== source.volume_sort_order;
}

type Read =
  | { kind: "closed" | "loading" }
  | { kind: "error"; text: string }
  | { kind: "ready"; window: SourceWindow };

export default function FrozenForeshadowSource({
  sourceReader,
  occurrence,
  bookId,
  epoch,
  isScopeCurrent,
  onSourceProblem,
}: {
  sourceReader: ForeshadowSourceReader;
  occurrence: Occurrence;
  bookId: string;
  epoch: string;
  isScopeCurrent: () => boolean;
  onSourceProblem?: () => void;
}) {
  const [read, setRead] = useState<Read>({ kind: "closed" });
  const generation = useRef(0);
  const button = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);
  // Changing the bound proposal, pack, occurrence or epoch unmounts this child.
  useEffect(
    () => () => {
      generation.current++;
    },
    [],
  );
  useEffect(() => {
    if (read.kind === "closed" && restoreFocus.current) {
      restoreFocus.current = false;
      button.current?.focus();
    }
  }, [read.kind]);
  const source = occurrence.source;
  const scopeError = "书库或审阅范围已变化，未显示原文。";
  function close() {
    generation.current++;
    restoreFocus.current = true;
    setRead({ kind: "closed" });
  }
  async function open() {
    const token = ++generation.current;
    const active = () => generation.current === token;
    const scope = () => active() && isScopeCurrent();
    setRead({ kind: "loading" });
    try {
      if (!scope() || (await sourceReader.epoch()) !== epoch || !scope()) {
        if (active()) setRead({ kind: "error", text: scopeError });
        return;
      }
      const window = await sourceReader.sourceWindow(
        bookId,
        source.chapter_id,
        epoch,
        occurrence.start,
        occurrence.end,
      );
      if (!scope() || (await sourceReader.epoch()) !== epoch || !scope()) {
        if (active()) setRead({ kind: "error", text: scopeError });
        return;
      }
      if (window.position.chapter_id !== source.chapter_id) {
        setRead({ kind: "error", text: "返回的来源章节不匹配，未显示原文。" });
        return;
      }
      if (
        window.position.chapter_revision !== source.chapter_revision ||
        positionDiffers(window.position, source) ||
        window.at_range !== occurrence.quote
      ) onSourceProblem?.();
      setRead({ kind: "ready", window });
    } catch (error) {
      if (!active()) return;
      const code =
        typeof error === "object" && error
          ? (error as { code?: string }).code
          : undefined;
      setRead({
        kind: "error",
        text:
          !scope() || code === "epoch_changed"
            ? scopeError
            : code === "not_found"
              ? "当前已保存来源不可用；冻结引文仍保留。"
              : code === "busy"
                ? "书库正忙，返回审阅后可再试。"
                : "未能核对当前已保存来源；冻结引文仍保留。",
      });
    }
  }
  const current = read.kind === "ready" ? read.window.position : null;
  const versionChanged =
    !!current && current.chapter_revision !== source.chapter_revision;
  const positionChanged = !!current && positionDiffers(current, source);
  return (
    <>
      <button
        ref={button}
        type="button"
        aria-expanded={read.kind !== "closed"}
        hidden={read.kind !== "closed"}
        onClick={() => void open()}
      >
        核对已保存来源原文
      </button>
      {read.kind !== "closed" && (
        <section aria-label="来源原文核对">
          <button type="button" autoFocus onClick={close}>
            返回审阅
          </button>
          {read.kind === "loading" && (
            <p role="status">正在只读核对；可随时返回审阅。</p>
          )}
          {read.kind === "error" && <p role="alert">{read.text}</p>}
          {read.kind === "ready" && current && (
            <>
              <p>
                当前已保存：{current.volume_title} · {current.chapter_title} ·
                正文 v{current.chapter_revision}
              </p>
              {versionChanged && (
                <p>
                  正文版本已变化：冻结 v{source.chapter_revision}，当前 v
                  {current.chapter_revision}。
                </p>
              )}
              {positionChanged && <p>来源卷章位置或名称已变化。</p>}
              {!versionChanged &&
                !positionChanged &&
                read.window.at_range === occurrence.quote && (
                  <p>已保存来源位置与版本匹配</p>
                )}
              {read.window.at_range !== occurrence.quote && (
                <p>当前原坐标不再匹配冻结引文，未自动寻找替代位置。</p>
              )}
              {read.window.before ||
              read.window.at_range ||
              read.window.after ? (
                <blockquote
                  aria-label="当前已保存正文周边"
                  className="foreshadow-quote"
                >
                  {read.window.before}
                  {read.window.at_range === occurrence.quote ? (
                    <mark>{read.window.at_range}</mark>
                  ) : (
                    read.window.at_range
                  )}
                  {read.window.after}
                </blockquote>
              ) : (
                <p>当前已保存正文在原坐标附近已无内容。</p>
              )}
              <p>只读对照不更新冻结依据或作者审阅。</p>
            </>
          )}
        </section>
      )}
    </>
  );
}
