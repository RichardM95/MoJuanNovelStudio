import { useEffect, useRef, useState } from "react";
import type { MaterialKind, MaterialsAPI } from "./materialTypes";
import { errorMessage } from "./workflowTypes";

type Organize = NonNullable<MaterialsAPI["organize"]>;
type Preview = Awaited<ReturnType<Organize["preview"]>>;
const labels = { setting: "设定", character: "角色", outline: "大纲" };

export default function MaterialOrganizationReview({
  api,
  bookId,
  fieldLabels,
  onClose,
  onPending,
  onApplied,
}: {
  api: Organize;
  bookId: string;
  fieldLabels: Record<MaterialKind, Record<string, string>>;
  onClose: () => void;
  onPending: (pending: boolean) => void;
  onApplied: (count: number) => Promise<void>;
}) {
  const [preview, setPreview] = useState<Preview>();
  const [selected, setSelected] = useState<number[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const lock = useRef(false);
  const nextRead = useRef(0);
  const read = async () => {
    const generation = ++nextRead.current;
    setError("");
    setPreview(undefined);
    setSelected([]);
    try {
      const result = await api.preview(bookId);
      if (generation === nextRead.current) setPreview(result);
    } catch (cause) {
      if (generation === nextRead.current) setError(errorMessage(cause));
    }
  };
  useEffect(() => {
    void read();
    return () => {
      nextRead.current++;
    };
  }, [api, bookId]);

  async function apply() {
    if (!preview || !selected.length || lock.current) return;
    lock.current = true;
    onPending(true);
    setBusy(true);
    setError("");
    const id = preview.id;
    setPreview(undefined);
    try {
      await onApplied(await api.apply(id, selected));
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      lock.current = false;
      setBusy(false);
      onPending(false);
    }
  }

  return (
    <section className="organization-review" aria-label="整理已有资料">
      <header>
        <h3>整理已有资料</h3>
        <button disabled={busy} onClick={onClose}>
          返回资料库
        </button>
      </header>
      {error && <p role="alert">{error}</p>}
      {!preview && !error && (
        <p role="status">{busy ? "正在保存…" : "正在整理来源文档…"}</p>
      )}
      {error && !preview && (
        <button disabled={busy} onClick={() => void read()}>
          重新预览
        </button>
      )}
      {preview && (
        <>
          <p>
            {preview.source_count} 份来源文档 · {preview.cards.length} 项待采纳
          </p>
          {!!preview.cards.length && (
            <button
              disabled={busy}
              onClick={() =>
                setSelected(
                  selected.length ? [] : preview.cards.map((_, index) => index),
                )
              }
            >
              {selected.length ? "清空选择" : "全选"}
            </button>
          )}
          <ul className="project-list">
            {preview.cards.map((card, index) => (
              <li key={index}>
                <label>
                  <input
                    type="checkbox"
                    checked={selected.includes(index)}
                    disabled={busy}
                    onChange={() => {
                      setSelected((old) =>
                        old.includes(index)
                          ? old.filter((i) => i !== index)
                          : [...old, index],
                      );
                    }}
                  />{" "}
                  {card.previous_fields ? "更新" : "新增"} · {card.title}
                </label>
                <details>
                  <summary>
                    {labels[card.kind as keyof typeof labels] ?? card.kind} ·{" "}
                    {card.source_files}
                  </summary>
                  {card.previous_fields && (
                    <>
                      <p>更新前</p>
                      {Object.entries(card.previous_fields).map(
                        ([key, value]) => (
                          <div key={key}>
                            <strong>
                              {fieldLabels[card.kind][key] ?? key}
                            </strong>
                            <pre>{value || "空白"}</pre>
                          </div>
                        ),
                      )}
                      <p>整理后的内容</p>
                    </>
                  )}
                  <pre>{card.content}</pre>
                </details>
              </li>
            ))}
          </ul>
          <button
            className="primary"
            disabled={busy || !selected.length}
            onClick={() => void apply()}
          >
            采纳所选 {selected.length} 项
          </button>
        </>
      )}
    </section>
  );
}
