import React, { useEffect, useRef, useState } from "react";
import type { Book } from "./types";
import type { FanqieAPI, FanqieDirectory, FanqiePacket } from "./fanqieTypes";
import { MAX_FANQIE_BATCH, queueItems, type QueueItem } from "./fanqieQueue";

const systemCopy = (value: string) => navigator.clipboard.writeText(value);
const errorText = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

export default function FanqieWorkspace({
  books,
  api,
  onExit,
  copyText = systemCopy,
}: {
  books: Book[];
  api: FanqieAPI;
  onExit: () => void;
  copyText?: (value: string) => Promise<void>;
}) {
  const [bookId, setBookId] = useState("");
  const [targetBook, setTargetBook] = useState<string | null>(null);
  const [directory, setDirectory] = useState<FanqieDirectory | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [chosen, setChosen] = useState(new Set<string>());
  const [volumes, setVolumes] = useState<Record<string, string>>({});
  const [first, setFirst] = useState<Record<string, string>>({});
  const [overrides, setOverrides] = useState<Record<string, string>>({});
  const [queue, setQueue] = useState<QueueItem[]>([]);
  const [cursor, setCursor] = useState(0);
  const [packet, setPacket] = useState<FanqiePacket | null>(null);
  const [reading, setReading] = useState(false);
  const [copying, setCopying] = useState(false);
  const clipboardPending = useRef(false);
  const [blocked, setBlocked] = useState(false);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const sequence = useRef(0);
  const pending = useRef(false);
  const item = queue[cursor];
  useEffect(() => {
    let stopped = false;
    if (bookId)
      void api
        .directory(bookId)
        .then((value) => {
          if (!stopped) setDirectory(value);
        })
        .catch((reason) => {
          if (!stopped) setError(errorText(reason));
        });
    return () => {
      stopped = true;
    };
  }, [api, bookId, refresh]);
  useEffect(
    () => () => {
      sequence.current++;
    },
    [],
  );

  function reset() {
    sequence.current++;
    pending.current = false;
    setReading(false);
    setDirectory(null);
    setTargetBook(null);
    setChosen(new Set());
    setVolumes({});
    setFirst({});
    setOverrides({});
    setQueue([]);
    setCursor(0);
    setPacket(null);
    setBlocked(false);
    setError("");
    setStatus("");
  }

  function toggle(id: string) {
    const next = new Set(chosen);
    if (next.has(id)) next.delete(id);
    else if (next.size < MAX_FANQIE_BATCH) next.add(id);
    else {
      setError("一次最多选择 50 章");
      return;
    }
    setError("");
    setChosen(next);
  }

  function build() {
    if (!directory) return;
    try {
      if (!(targetBook ?? directory.book_title).trim())
        throw Error("请输入番茄目标作品");
      setQueue(
        queueItems(directory.chapters, chosen, volumes, first, overrides),
      );
      setCursor(0);
      setError("");
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function read() {
    if (!directory || !item || pending.current || packet || blocked) return;
    const request = ++sequence.current;
    pending.current = true;
    setReading(true);
    setError("");
    try {
      const result = await api.packet(
        directory.book_id,
        directory.epoch,
        item.chapter,
      );
      if (sequence.current === request) setPacket(result);
    } catch (e) {
      if (sequence.current === request) {
        setPacket(null);
        setBlocked(true);
        setError(errorText(e));
      }
    } finally {
      if (sequence.current === request) {
        pending.current = false;
        setReading(false);
      }
    }
  }

  async function copy(label: string, value: string) {
    if (clipboardPending.current) return;
    const request = sequence.current;
    clipboardPending.current = true;
    setCopying(true);
    try {
      await copyText(value);
      if (sequence.current === request) setStatus(label + "已复制");
    } catch {
      if (sequence.current === request)
        setStatus("剪贴板不可用，请直接选中文本复制");
    } finally {
      clipboardPending.current = false;
      if (sequence.current === request) setCopying(false);
    }
  }

  function advance() {
    if (!packet || !item || pending.current || clipboardPending.current) return;
    sequence.current++;
    setPacket(null);
    setStatus("");
    setCursor(cursor + 1);
  }

  const rows = directory?.chapters ?? [];
  const grouped = new Map<string, typeof rows>();
  for (const row of rows) {
    const group = grouped.get(row.volume_id);
    if (group) group.push(row);
    else grouped.set(row.volume_id, [row]);
  }
  return (
    <section className="fanqie-workspace" aria-label="番茄章稿工作区">
      <header className="fanqie-header">
        <div>
          <h1>番茄章稿</h1>
          <p>在墨卷备稿，在番茄逐章核对、存草稿。</p>
        </div>
        <button type="button" onClick={onExit} disabled={reading || copying}>
          {queue.length && item ? "退出并清空队列" : "返回书架"}
        </button>
      </header>
      {error && (
        <p role="alert" className="fanqie-error">
          {error}
        </p>
      )}
      {!queue.length ? (
        <div className="fanqie-card">
          <label className="fanqie-field">
            墨卷作品
            <select
              value={bookId}
              onChange={(e) => {
                reset();
                setBookId(e.target.value);
              }}
            >
              <option value="">选择作品</option>
              {books
                .filter((b) => b.deleted_at === null)
                .map((b) => (
                  <option key={b.id} value={b.id}>
                    {b.title}
                  </option>
                ))}
            </select>
          </label>
          {directory && (
            <label className="fanqie-field">
              番茄目标作品
              <input
                value={targetBook ?? directory.book_title}
                onChange={(e) => setTargetBook(e.target.value)}
              />
            </label>
          )}
          {bookId && !directory && !error && <p role="status">正在读取目录…</p>}
          {bookId && !directory && error && (
            <button
              onClick={() => {
                setError("");
                setRefresh((v) => v + 1);
              }}
            >
              重读目录
            </button>
          )}
          {directory && rows.length === 0 && <p>作品还没有章节。</p>}
          {directory &&
            [...grouped].map(([id, group]) => {
              const picked = group.filter((row) => chosen.has(row.chapter_id));
              return (
                <details
                  key={id}
                  className="fanqie-volume"
                  open={picked.length ? true : undefined}
                >
                  <summary>
                    第 {group[0].volume_ordinal} 卷 · {group[0].volume_title}
                    （已选 {picked.length} / {group.length}）
                  </summary>
                  <div className="fanqie-options">
                    {group.map((row) => (
                      <label key={row.chapter_id}>
                        <input
                          type="checkbox"
                          checked={chosen.has(row.chapter_id)}
                          onChange={() => toggle(row.chapter_id)}
                        />
                        <span>
                          第{row.ordinal}章 {row.title}
                        </span>
                      </label>
                    ))}
                  </div>
                  {picked.length > 0 && (
                    <div className="fanqie-mapping">
                      <label className="fanqie-field">
                        番茄目标卷
                        <input
                          value={volumes[id] ?? group[0].volume_title}
                          onChange={(e) =>
                            setVolumes({ ...volumes, [id]: e.target.value })
                          }
                        />
                      </label>
                      <label className="fanqie-field">
                        番茄起始章号
                        <input
                          inputMode="numeric"
                          value={first[id] ?? ""}
                          onChange={(e) =>
                            setFirst({ ...first, [id]: e.target.value })
                          }
                        />
                      </label>
                      {picked.map((row, offset) => (
                        <label key={row.chapter_id} className="fanqie-field">
                          番茄第{row.ordinal}章章号
                          <input
                            inputMode="numeric"
                            value={
                              overrides[row.chapter_id] ??
                              (/^[1-9]\d{0,8}$/.test(first[id] ?? "")
                                ? String(Number(first[id]) + offset)
                                : "")
                            }
                            onChange={(e) =>
                              setOverrides({
                                ...overrides,
                                [row.chapter_id]: e.target.value,
                              })
                            }
                          />
                        </label>
                      ))}
                    </div>
                  )}
                </details>
              );
            })}
          {chosen.size > 0 && (
            <button type="button" className="primary" onClick={build}>
              生成队列
            </button>
          )}
        </div>
      ) : !item ? (
        <div className="fanqie-card">
          <h2>已记录 {queue.length} 章</h2>
          <p>依据你的存稿确认，不是番茄回执。</p>
          <button
            onClick={() => {
              reset();
              setRefresh((v) => v + 1);
            }}
          >
            准备下一批
          </button>
        </div>
      ) : (
        <div className="fanqie-card">
          <p className="fanqie-counter">
            第 {cursor + 1} / {queue.length} 章
          </p>
          <h2>{item.chapter.title}</h2>
          <p>
            {targetBook ?? directory?.book_title} · 目标卷：
            <strong>{item.targetVolume}</strong> · 第 {item.number} 章
          </p>
          {(!cursor ||
            queue[cursor - 1].targetVolume !== item.targetVolume) && (
            <p>先在番茄核对作品、目标卷和已有草稿。</p>
          )}
          {!packet && !blocked && (
            <button
              className="primary"
              onClick={() => void read()}
              disabled={reading}
            >
              核对并读取正文
            </button>
          )}
          {packet && (
            <>
              <div className="fanqie-mapping">
                <label className="fanqie-field">
                  章号
                  <input readOnly value={item.number} />
                </label>
                <label className="fanqie-field">
                  标题
                  <input readOnly value={packet.title} />
                </label>
              </div>
              <label className="fanqie-field">
                待填正文
                <textarea readOnly value={packet.body} rows={10} />
              </label>
              <div className="fanqie-actions">
                <button
                  disabled={copying}
                  onClick={() => void copy("章号", item.number)}
                >
                  复制章号
                </button>
                <button
                  disabled={copying}
                  onClick={() => void copy("标题", packet.title)}
                >
                  复制标题
                </button>
                <button
                  disabled={copying}
                  onClick={() => void copy("正文", packet.body)}
                >
                  复制正文
                </button>
                <button
                  disabled={copying}
                  onClick={() => {
                    const book = (
                      targetBook ??
                      directory?.book_title ??
                      ""
                    ).trim();
                    if (!book) return;
                    void copy(
                      "本章章稿",
                      JSON.stringify({
                        book,
                        volume: item.targetVolume,
                        number: item.number,
                        title: packet.title,
                        body: packet.body,
                      }),
                    );
                  }}
                >
                  复制本章给浏览器扩展
                </button>
              </div>
              {status && <p role="status">{status}</p>}
              <p>
                请在番茄检查填入内容并存草稿。若不确定是否保存，先查草稿箱。
              </p>
              <button className="primary" disabled={copying} onClick={advance}>
                我已核对存稿，下一章
              </button>
            </>
          )}
          {blocked && (
            <button
              onClick={() => {
                reset();
                setRefresh((v) => v + 1);
              }}
            >
              重建队列
            </button>
          )}
        </div>
      )}
    </section>
  );
}
