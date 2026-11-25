import { useEffect, useRef, useState } from "react";
import { errorMessage } from "./workflowTypes";

type AgentSession = {
  id: string;
  client_name: string;
  status: string;
  book_id: string | null;
};
type AgentStatus = { endpoint: string | null; sessions: AgentSession[] };
export interface AgentAccessAPI {
  status(): Promise<AgentStatus>;
  grant(sessionId: string, bookId: string): Promise<void>;
  revoke(sessionId: string): Promise<void>;
}

export default function AgentAccessPanel({
  api,
  book,
  onClose,
}: {
  api: AgentAccessAPI;
  book: { id: string; title: string };
  onClose: () => void;
}) {
  const [status, setStatus] = useState<AgentStatus>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const action = useRef(false);
  const close = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    close.current?.focus();
    let mounted = true;
    void api
      .status()
      .then((value) => {
        if (mounted) setStatus(value);
      })
      .catch((cause) => {
        if (mounted) setError(errorMessage(cause));
      });
    return () => {
      mounted = false;
      if (previous?.isConnected) previous.focus();
    };
  }, [api]);

  async function refresh(change?: () => Promise<void>) {
    if (action.current) return;
    action.current = true;
    setBusy(true);
    setError("");
    try {
      await change?.();
      setStatus(await api.status());
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      action.current = false;
      setBusy(false);
    }
  }

  const sessions =
    status?.sessions.filter(
      (session) =>
        session.status === "pending" ||
        (session.status === "granted" && session.book_id === book.id),
    ) ?? [];
  return (
    <div className="modal-backdrop">
      <section
        className="modal txt-import"
        role="dialog"
        aria-modal="true"
        aria-label="智能体连接"
        onKeyDown={(event) => {
          if (event.key === "Escape" && !action.current) onClose();
        }}
      >
        <header>
          <h2>智能体连接</h2>
          <button ref={close} disabled={busy} onClick={onClose}>
            关闭
          </button>
        </header>
        <p>当前作品《{book.title}》</p>
        {error && <p role="alert">{error}</p>}
        {status?.endpoint ? (
          <label>
            Windows 本机地址
            <input
              readOnly
              value={status.endpoint}
              onFocus={(event) => event.target.select()}
            />
          </label>
        ) : (
          <p>{status ? "本机连接未能启动。" : "正在读取连接状态…"}</p>
        )}
        <p>客户端调用 session_status 后，在这里选择要授权的连接。</p>
        <button disabled={busy} onClick={() => void refresh()}>
          刷新连接
        </button>
        <ul>
          {sessions.map((session) => (
            <li key={session.id}>
              <span>
                {session.client_name} · {session.id.slice(0, 8)} ·{" "}
                {session.status === "pending" ? "待授权" : "已授权"}
              </span>{" "}
              {session.status === "pending" && (
                <button
                  className="primary"
                  disabled={busy}
                  onClick={() =>
                    void refresh(() => api.grant(session.id, book.id))
                  }
                >
                  授权当前作品
                </button>
              )}
              <button
                disabled={busy}
                onClick={() => void refresh(() => api.revoke(session.id))}
              >
                {session.status === "pending" ? "拒绝" : "撤销授权"}
              </button>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
