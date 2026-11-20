import {
  terminalLabel,
  type DecisionRecord,
  type WritingDecisionSession,
} from "./writingDecision";

export function WritingDecisionRecovery({
  entries,
  session,
  phase,
  disabled,
  obsolete,
  message,
  reconcile,
  stop,
}: {
  entries: DecisionRecord[];
  session: WritingDecisionSession;
  phase: "idle" | "writing" | "reading";
  disabled: boolean;
  obsolete: boolean;
  message: string;
  reconcile: (entry: DecisionRecord) => void;
  stop: () => void;
}) {
  if (!entries.length && !message && !obsolete) return null;
  return (
    <section aria-label="正文决定核对">
      {obsolete && (
        <p>此界面属于旧书库，关闭后再进入当前书库。不会重发决定。</p>
      )}
      {entries.length > 0 && (
        <ul aria-label="待核对或旧库的正文决定">
          {entries.map((entry) => {
            const a = entry.target;
            const old = session.isOld(entry);
            return (
              <li key={entry.key}>
                <p>
                  建议 {a.id} ·{" "}
                  {old
                    ? "旧书库证据（只读）"
                    : entry.kind === "terminal"
                      ? "决定已核对，当前正文待核对"
                      : "决定仍待核对"}
                </p>
                {entry.kind === "terminal" && (
                  <p>
                    原书库精确终态证据：{terminalLabel(entry.evidence, a)}
                    {entry.evidence.reviewRevision !== null &&
                      ` · 审阅版本 ${entry.evidence.reviewRevision}`}
                  </p>
                )}
                {entry.kind === "attempt" && entry.candidates.write && (
                  <p>
                    写入回执候选（未核实）：
                    {terminalLabel(entry.candidates.write, a)}
                  </p>
                )}
                {entry.kind === "attempt" && entry.candidates["full-read"] && (
                  <p>
                    完整提案候选（尚未完成时代核对）：
                    {terminalLabel(entry.candidates["full-read"], a)}
                  </p>
                )}
                {old ? (
                  <button
                    disabled={phase !== "idle"}
                    onClick={() => session.clearOld(entry)}
                  >
                    清理旧库证据 {a.id}
                  </button>
                ) : (
                  <button
                    disabled={disabled || obsolete}
                    onClick={() => reconcile(entry)}
                  >
                    只读核对建议 {a.id}
                  </button>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {phase === "reading" && <button onClick={stop}>停止等待核对</button>}
      {message && <p role="status">{message}</p>}
    </section>
  );
}
