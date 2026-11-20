import "./writingWebCrypto";
import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { useWritingDecision } from "../src/useWritingDecision";
import type { WritingAPI, WritingProposal } from "../src/writingTypes";
import {
  WritingDecisionSession,
  type DecisionAttempt,
} from "../src/writingDecision";

function attempt(
  id: string,
  epoch = "original",
  body = "新稿",
): DecisionAttempt {
  return {
    epoch,
    id,
    bookId: "b",
    chapterId: "c",
    clientId: "client",
    baseRevision: 0,
    chapterRevision: 0,
    policyRevision: 0,
    frozenPolicyRevision: 0,
    input: {
      pack_id: "p",
      idempotency_key: id,
      new_body: body,
      citations: [],
      conflicts: [],
    },
    requiredMaterialIds: ["m"],
    confirmedReview: null,
    action: "reject",
    conflictsAck: false,
  };
}

it("never evicts a previous unresolved decision to admit a 33rd one", () => {
  const session = new WritingDecisionSession();
  const entries = Array.from({ length: 32 }, (_, i) =>
    session.admit(attempt(String(i))),
  );
  expect(() => session.admit(attempt("33rd"))).toThrow(/32 条或 32 MiB/);
  expect(session.entries()).toEqual(entries);
  session.clearOld(entries[0]);
  expect(() => session.admit(attempt("33rd"))).toThrow();
  session.invalidateLibrary();
  session.clearOld(entries[0]);
  expect(session.admit(attempt("33rd", "replacement")).target.id).toBe("33rd");
});

it("reserves encoded UTF-8 bytes at admission and does not evict a large previous input", () => {
  const session = new WritingDecisionSession();
  const body = "🚣".repeat(256 * 1024); // 1MiB UTF-8, 0.5M UTF-16 code units.
  const entries = Array.from({ length: 31 }, (_, i) =>
    session.admit(attempt(String(i), "original", body)),
  );
  expect(() => session.admit(attempt("limit", "original", body))).toThrow(
    /32 MiB/,
  );
  expect(session.entries()).toEqual(entries);
  expect(session.admit(attempt("small")).target.id).toBe("small");
});

it("keeps an earlier entry retired across another independently observed replacement", () => {
  const session = new WritingDecisionSession();
  const first = session.admit(attempt("same-id", "e1"));
  session.observeEpoch("e2");
  expect(session.isOld(first)).toBe(true);
  // A fresh panel may open against e3 before any recovery has independently
  // observed e3. Its new decision must not inherit e1's or e2's isolation.
  const third = session.admit(attempt("same-id", "e3"));
  expect(session.isOld(third)).toBe(false);
  session.observeEpoch("e3");
  expect(session.isOld(first)).toBe(true);
  expect(session.isOld(third)).toBe(false);
  session.clearOld(third);
  expect(session.entries()).toContain(third);
});

it.each(["opening", "publication"] as const)(
  "finishes the stopped %s reconciliation while the bridge promise is still pending",
  async (stage) => {
    const session = new WritingDecisionSession();
    const a = attempt("q");
    const entry = session.admit(a);
    let rejectBridge!: (e: Error) => void;
    const bridge = new Promise<string>((_resolve, reject) => {
      rejectBridge = reject;
    });
    const waiting = vi.fn();
    let epochCalls = 0;
    const epoch = vi.fn(async () => {
      if (++epochCalls === (stage === "opening" ? 1 : 3)) {
        waiting();
        return bridge;
      }
      return a.epoch;
    });
    const proposal = vi.fn(async (): Promise<WritingProposal> => ({
      id: a.id,
      book_id: a.bookId,
      chapter_id: a.chapterId,
      client_id: a.clientId,
      input: a.input,
      base_revision: a.baseRevision,
      live_epoch: a.epoch,
      decision: "accepted",
      accepted_revision: 1,
      author_review: {
        proposal_id: a.id,
        pack_id: a.input.pack_id,
        book_id: a.bookId,
        chapter_id: a.chapterId,
        originating_epoch: a.epoch,
        policy_revision: 0,
        chapter_revision: 0,
        revision: 1,
        items: [{ material_id: "m", verdict: "conforms", note: "已核对" }],
      },
    }));
    const readChapter = vi.fn(async () => ({
      chapter: {
        id: a.chapterId,
        volume_id: "v",
        title: "章",
        body: a.input.new_body,
        revision: 1,
        sort_order: 0,
        deleted_at: null,
      },
      publish: vi.fn(),
    }));
    const publish = vi.fn();
    const { result } = renderHook(() =>
      useWritingDecision({
        api: { epoch, proposal } as unknown as WritingAPI,
        session,
        isScopeCurrent: () => true,
        onBlockingChange: () => {},
        readChapter,
        onVerified: () => {},
        onChapter: publish,
      }),
    );
    let completion!: Promise<void>;
    act(() => {
      completion = result.current.reconcile(entry);
    });
    await waitFor(() => expect(waiting).toHaveBeenCalledOnce());
    await act(async () => {
      result.current.stop();
      // Let all immediately runnable Promise continuations finish without ever
      // settling the fake IPC. The original implementation remains suspended.
      const outcome = await Promise.race([
        completion.then(() => "stopped"),
        new Promise((resolve) =>
          setTimeout(() => resolve("still suspended"), 0),
        ),
      ]);
      expect(outcome).toBe("stopped");
    });
    expect(epoch).toHaveBeenCalledTimes(stage === "opening" ? 1 : 3);
    expect(proposal).toHaveBeenCalledTimes(stage === "opening" ? 0 : 1);
    expect(readChapter).toHaveBeenCalledTimes(stage === "opening" ? 0 : 1);
    expect(publish).not.toHaveBeenCalled();
    if (stage === "opening") expect(session.get(entry.key)).toBe(entry);
    else {
      expect(session.get(entry.key)).toBeUndefined();
      expect(session.terminal(entry.key)?.evidence.decision).toBe("accepted");
    }
    await act(async () => rejectBridge(Error("old IPC finally rejected")));
    // Vitest also fails the run on an unhandled rejection.
    expect(publish).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("idle");
  },
);

it("releases the large attempt after authenticating even when the current body cannot be verified", () => {
  const session = new WritingDecisionSession();
  const a = attempt("large", "e", "🚣".repeat(256 * 1024));
  const entry = session.admit(a);
  const terminal = session.authenticate(
    entry,
    {
      decision: "accepted",
      acceptedRevision: 1,
      reviewRevision: 2,
    },
    "2".repeat(64),
  )!;
  expect(session.entries()).toHaveLength(0);
  expect(session.recoveries()).toEqual([terminal]);
  expect(terminal.target).not.toHaveProperty("input");
  expect(() => session.admit(a)).toThrow(/已独立核对.*未重发/);
  session.clearOld(terminal);
  expect(session.terminal(entry.key)).toBe(terminal);
  session.invalidateLibrary();
  expect(session.isOld(terminal)).toBe(true);
  session.clearOld(terminal);
  expect(session.recoveries()).toHaveLength(0);
});

it("does not evict the first authenticated marker to admit a 1001st current-library decision", () => {
  const session = new WritingDecisionSession();
  const terminal = {
    decision: "rejected",
    acceptedRevision: null,
    reviewRevision: null,
  } as const;
  for (let i = 0; i < 1000; i++)
    session.authenticate(
      session.admit(attempt(String(i))),
      terminal,
      "3".repeat(64),
    );
  const key = JSON.stringify(["original", "b", "c", "0"]);
  expect(() => session.admit(attempt("1001st"))).toThrow(/终态记录已达上限/);
  expect(session.terminal(key)?.evidence).toEqual(terminal);
  session.observeEpoch("e2");
  expect(session.terminal(key)).toBeUndefined();
  expect(session.admit(attempt("0", "e2")).target.epoch).toBe("e2");
});
