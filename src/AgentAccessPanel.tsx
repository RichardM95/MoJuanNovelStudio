import { useEffect, useRef, useState } from "react";
import { errorMessage } from "./workflowTypes";
import { Dialog } from "./ui/Dialog";

type AgentSession = {
  id: string;
  client_name: string;
  status: string;
  book_id: string | null;
  browser_worker?: boolean;
  fanqie_send?: boolean;
};
type AgentStatus = { endpoint: string | null; sessions: AgentSession[] };
export interface AgentAccessAPI {
  status(): Promise<AgentStatus>;
  grant(sessionId: string, bookId: string, fanqieSend?: boolean): Promise<void>;
  connectCodex?(): Promise<string>;
  registerBrowser?(): Promise<string>;
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
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const action = useRef(false);
  useEffect(() => {
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
    <Dialog
      title="智能体连接"
      subtitle={`当前作品《${book.title}》`}
      closeLabel="关闭"
      className="agent-access"
      size="md"
      dismissible={!busy}
      onClose={onClose}
    >
      <section className="ui-card">
        <div className="ui-grid-2">
          {api.connectCodex && (
            <button
              disabled={busy || !status?.endpoint}
              onClick={() =>
                void refresh(async () => setNotice(await api.connectCodex!()))
              }
            >
              连接 Codex
            </button>
          )}
          {api.registerBrowser && (
            <button
              disabled={busy || !status?.endpoint}
              onClick={() =>
                void refresh(async () =>
                  setNotice(await api.registerBrowser!()),
                )
              }
            >
              配置番茄浏览器扩展
            </button>
          )}
        </div>
        {notice && <p role="status">{notice}</p>}
        {error && <p role="alert">{error}</p>}
        {status?.endpoint ? (
          <label className="ui-field">
            Windows 本机地址
            <input
              readOnly
              value={status.endpoint}
              onFocus={(event) => event.target.select()}
            />
          </label>
        ) : (
          <p className="ui-meta">
            {status ? "本机连接未能启动。" : "正在读取连接状态…"}
          </p>
        )}
        <p className="ui-meta">
          客户端调用 session_status 后，在这里选择要授权的连接。
        </p>
        <button disabled={busy} onClick={() => void refresh()}>
          刷新连接
        </button>
      </section>
      <section className="ui-card">
        <h3 className="ui-card-title">连接会话</h3>
        {sessions.length === 0 ? (
          <p className="ui-meta">暂无待授权或已授权的连接。</p>
        ) : (
          <ul className="agent-sessions">
            {sessions.map((session) => (
              <li key={session.id}>
                <span>
                  {session.client_name} · {session.id.slice(0, 8)} ·{" "}
                  {session.status === "pending" ? "待授权" : "已授权"}
                  {session.fanqie_send && " · 可提交番茄草稿"}
                </span>
                {session.status === "pending" && (
                  <button
                    className="primary"
                    disabled={busy}
                    onClick={() =>
                      void refresh(() => api.grant(session.id, book.id))
                    }
                  >
                    {session.browser_worker
                      ? "允许此扩展处理本书草稿"
                      : "授权当前作品"}
                  </button>
                )}
                {session.status === "pending" && !session.browser_worker && (
                  <button
                    disabled={busy}
                    onClick={() =>
                      void refresh(() =>
                        api.grant(session.id, book.id, true),
                      )
                    }
                  >
                    授权当前作品并允许番茄存稿
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
        )}
      </section>
    </Dialog>
  );
}
