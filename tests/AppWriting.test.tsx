import "./writingWebCrypto";
import "@testing-library/jest-dom/vitest";
import React from "react";
import {
  render,
  screen,
  fireEvent,
  act,
  within,
  waitFor,
  cleanup,
} from "@testing-library/react";
import { it, expect, vi, afterEach } from "vitest";
import App from "../src/App";
import type { LibraryState } from "../src/types";
import type { WritingPolicy, WritingProposal } from "../src/writingTypes";
afterEach(cleanup);
it("flushes draft before loading writing policy and uses saved revision for pack", async () => {
  let state = {
    books: [{ id: "b", title: "书", author: "", deleted_at: null }],
    volumes: [
      { id: "v", book_id: "b", title: "卷", sort_order: 0, deleted_at: null },
    ],
    chapters: [
      {
        id: "c",
        volume_id: "v",
        title: "章",
        body: "旧",
        revision: 0,
        sort_order: 0,
        deleted_at: null,
      },
    ],
  };
  let resolve: (v: any) => void = () => {};
  const apply = vi.fn(
    (cmd: any) =>
      new Promise((r) => {
        resolve = () => {
          state = {
            ...state,
            chapters: [{ ...state.chapters[0], body: cmd.body, revision: 1 }],
          };
          r(state);
        };
      }),
  );
  const policy = {
    chapter_id: "c",
    book_id: "b",
    revision: 0,
    mode: "active",
    goal: "过河",
    required_material_ids: ["m"],
    updated_at: 1,
  };
  const get = vi.fn(async () => ({
      epoch: "e",
      policy,
      packs: [],
      proposals: [],
    })),
    prepare = vi.fn(async () => ({ id: "p" }));
  const api: any = {
    load: async () => state,
    apply,
    exportBook: async () => null,
    backup: async () => null,
    restore: async () => null,
    materials: { list: async () => [] },
    writing: {
      epoch: async () => "e",
      state: get,
      contextDirectory: async () => ({
        epoch: "e",
        book_id: "b",
        chapter_id: "c",
        chapters: [],
        volumes: [],
      }),
      prepare,
      configure: async () => policy,
    },
  };
  render(<App api={api} />);
  fireEvent.click(await screen.findByRole("button", { name: "打开 书" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "章节正文" }), {
    target: { value: "未保存中文🙂" },
  });
  fireEvent.click(screen.getByRole("button", { name: "写作约束" }));
  await waitFor(() => expect(apply).toHaveBeenCalled());
  expect(get).not.toHaveBeenCalled();
  resolve(undefined);
  await screen.findByRole("dialog", { name: "章节写作约束" });
  fireEvent.click(
    await screen.findByRole("button", { name: "冻结当前依据包" }),
  );
  await waitFor(() =>
    expect(prepare).toHaveBeenCalledWith("b", "c", 0, 1, "e"),
  );
  expect(state.chapters[0].body).toBe("未保存中文🙂");
});

const strictChapter = {
  id: "c", volume_id: "v", title: "甲章", body: "作者原稿", revision: 0,
  sort_order: 0, deleted_at: null,
};
const separateChapter = {
  ...strictChapter, id: "d", title: "乙章", body: "另一章原稿", sort_order: 1,
};
const strictPolicy: WritingPolicy = {
  chapter_id: "c", book_id: "b", revision: 0, mode: "active",
  goal: "渡河", required_material_ids: ["m"], updated_at: 1,
};
const strictProposal: WritingProposal = {
  id: "q", book_id: "b", chapter_id: "c", client_id: "agent", base_revision: 0,
  decision: "pending", live_epoch: "e", accepted_revision: null,
  input: { pack_id: "p", new_body: "坐船\r\n🚣到岸", idempotency_key: "k", citations: [], conflicts: [] },
  author_review: {
    proposal_id: "q", book_id: "b", chapter_id: "c", pack_id: "p",
    originating_epoch: "e", policy_revision: 0, chapter_revision: 0,
    revision: 1, items: [{ material_id: "m", verdict: "conforms", note: "" }],
  },
};
const written: WritingProposal = { ...strictProposal, decision: "accepted", accepted_revision: 1 };
const storedChapter = { ...strictChapter, revision: 1, body: strictProposal.input.new_body };
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
function strictApp(withSource = false) {
  const fixture = {
    library: {
      books: [{ id: "b", title: "书", author: "", deleted_at: null }],
      volumes: [{ id: "v", book_id: "b", title: "卷", sort_order: 0, deleted_at: null }],
      chapters: [strictChapter, separateChapter],
    } as LibraryState,
    proposal: strictProposal,
  };
  const card = { id: "m", book_id: "b", title: "规则", kind: "setting", fields: {}, revision: 0, deleted_at: null };
  const position = {
    chapter_id: "d", chapter_title: separateChapter.title, chapter_revision: 0,
    chapter_sort_order: 1, volume_id:"v", volume_title:"卷", volume_sort_order:0,
  };
  const sourceWindow = vi.fn(async () => ({
    position, before:"另一章", at_range:"原稿", after:"",
  }));
  const api = {
    load: vi.fn(async () => fixture.library),
    apply: vi.fn(async () => fixture.library),
    exportBook: async () => null,
    backup: async () => null,
    restore: vi.fn(async (): Promise<LibraryState | null> => null),
    foreshadow: withSource ? { sourceEpoch:async () => "e", sourceWindow } : undefined,
    materials: { list: async () => [card] },
    writing: {
      epoch: vi.fn(async () => "e"),
      state: vi.fn(async (_b: string, c: string) => ({
        epoch: "e", policy: c === "c" ? strictPolicy : null, packs: [],
        proposals: c === "c" ? [{ id: "q", pack_id: "p", decision: fixture.proposal.decision, archived: false, created_at: 0 }] : [],
      })),
      contextDirectory: async (b: string, c: string) => ({
        epoch: "e", book_id: b, chapter_id: c, chapters: [], volumes: [],
      }),
      pack: vi.fn(async () => ({
        id: "p", book_id: "b", chapter_id: "c", originating_epoch: "e",
        expires_at: Date.now() / 1000 + 1800, policy: strictPolicy, chapter: strictChapter,
        foreshadow_occurrences: withSource ? [{
          id:"foreshadow",book_id:"b",material_id:"m",source:position,start:3,end:5,
          quote:"原稿",role:"plant",note:"",revision:0,deleted_at:null,created_at:1,
        }] : [],
        materials: [card],
      })),
      proposal: vi.fn(async () => fixture.proposal),
      acceptReviewed: vi.fn(async (): Promise<WritingProposal> => {
        throw Error("写入回执未送达");
      }),
      decide: vi.fn(async () => fixture.proposal),
    },
  };
  render(<App api={api as any} />);
  return { api, fixture, sourceWindow };
}
async function openStrictChapter(openBook = true) {
  if (openBook)
    fireEvent.click(await screen.findByRole("button", { name: "打开 书" }));
  fireEvent.click(await screen.findByRole("button", { name: "写作约束" }));
  return screen.findByRole("dialog", { name: "章节写作约束" });
}
async function confirmAdoption() {
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  fireEvent.click(await screen.findByRole("button", { name: "采纳建议" }));
  const submit = screen.getByRole("button", { name: "确认采纳" });
  act(() => { fireEvent.click(submit); fireEvent.click(submit); });
}
function blockedUnload() {
  const event = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(event);
  return event.defaultPrevented;
}

it("blocks Escape and window close during the real decision, but can stop the later read", async () => {
  const { api } = strictApp();
  const commit = deferred<WritingProposal>();
  const read = deferred<WritingProposal>();
  api.writing.acceptReviewed.mockImplementationOnce(() => commit.promise);
  await openStrictChapter();
  await confirmAdoption();
  const dialog = screen.getByRole("dialog", { name: "章节写作约束" });
  fireEvent.keyDown(dialog, { key: "Escape" });
  expect(dialog).toBeInTheDocument();
  expect(blockedUnload()).toBe(true);
  expect(screen.queryByRole("button", { name: "停止等待核对" })).not.toBeInTheDocument();
  expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
  api.writing.proposal.mockImplementationOnce(() => read.promise);
  await act(async () => commit.resolve(written));
  fireEvent.click(await screen.findByRole("button", { name: "停止等待核对" }));
  expect(blockedUnload()).toBe(false);
  fireEvent.keyDown(dialog, { key: "Escape" });
  expect(dialog).not.toBeInTheDocument();
  await act(async () => read.resolve(written));
  expect(api.load).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("textbox", { name: "章节正文" })).toHaveValue("作者原稿");
});

it("keeps an unresolved target across reopen and another chapter without resending", async () => {
  const { api, fixture } = strictApp();
  await openStrictChapter();
  await confirmAdoption();
  await screen.findByText(/仍待核对.*完整提案仍待审/);
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  fireEvent.click(screen.getByRole("button", { name: "第2章 乙章" }));
  await openStrictChapter(false);
  expect(screen.queryByRole("button", { name: "只读核对建议 q" })).not.toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  expect(screen.getByRole("textbox", { name: "章节正文" })).toHaveValue("另一章原稿");
  fireEvent.click(screen.getByRole("button", { name: "第1章 甲章" }));
  await openStrictChapter(false);
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  expect(await screen.findByRole("button", { name: "采纳建议" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "拒绝建议" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "刷新依据与建议" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "只读核对建议 q" })).toBeEnabled());
  fixture.proposal = written;
  fixture.library = { ...fixture.library, chapters: [storedChapter, separateChapter] };
  fireEvent.click(screen.getByRole("button", { name: "只读核对建议 q" }));
  await screen.findByText(/当前正文已核对/);
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  expect(screen.getByRole("textbox", { name: "章节正文" })).toHaveValue("坐船\n🚣到岸");
  expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
  expect(api.apply).not.toHaveBeenCalled();
});

it("discards a same-batch old chapter publication before the replacement library renders", async () => {
  const { api, fixture } = strictApp();
  const finalEpoch = deferred<string>();
  const restore = deferred<LibraryState | null>();
  api.restore.mockImplementationOnce(() => restore.promise);
  api.writing.epoch.mockResolvedValueOnce("e").mockResolvedValueOnce("e")
    .mockImplementationOnce(() => finalEpoch.promise);
  api.writing.acceptReviewed.mockImplementationOnce(async () => {
    fixture.proposal = written;
    fixture.library = { ...fixture.library, chapters: [storedChapter, separateChapter] };
    return written;
  });
  await openStrictChapter();
  await confirmAdoption();
  await waitFor(() => expect(api.writing.epoch).toHaveBeenCalledTimes(3));
  // Trigger the App's real restore completion while the read continuation
  // still has a captured old chapter. JSDOM dispatch bypasses the inert rail.
  fireEvent.click(screen.getByRole("button", { name: "恢复备份" }));
  fireEvent.click(within(screen.getByRole("dialog", { name: "确认操作" })).getByRole("button", { name: "确认" }));
  await waitFor(() => expect(api.restore).toHaveBeenCalledOnce());
  const replacement = {
    ...fixture.library,
    books: [{ ...fixture.library.books[0], title: "替换后的书" }],
    chapters: [{ ...strictChapter, body: "替换后正文" }, separateChapter],
  };
  await act(async () => {
    fixture.library = replacement;
    restore.resolve(replacement);
    await Promise.resolve();
    finalEpoch.resolve("e");
  });
  expect(screen.getByText(/此界面属于旧书库/)).toBeInTheDocument();
  expect(screen.getByText(/精确终态证据：已采纳/)).toBeInTheDocument();
  expect(screen.queryByText(/当前正文已核对/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "停止等待核对" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  fireEvent.click(await screen.findByRole("button", { name: "打开 替换后的书" }));
  expect(await screen.findByRole("textbox", { name: "章节正文" })).toHaveValue("替换后正文");
  expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
});

it("can reopen detached recovery after the source disappears, without refreshing the other editor", async () => {
  const { api, fixture } = strictApp();
  await openStrictChapter();
  await confirmAdoption();
  await screen.findByText(/仍待核对.*完整提案仍待审/);
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  fireEvent.click(screen.getByRole("button", { name: "第2章 乙章" }));
  fixture.proposal = written;
  fixture.library = { ...fixture.library, chapters: [separateChapter] };
  fireEvent.click(screen.getByText("待核对的正文决定"));
  const route = screen.getByRole("button", { name: "核对正文决定 · 书 / 甲章" });
  await waitFor(() => expect(route).toBeEnabled());
  fireEvent.click(route);
  fireEvent.click(await screen.findByRole("button", { name: "只读核对建议 q" }));
  await screen.findByText(/已采纳.*当前正文未核对.*章节已删除/);
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  expect(screen.getByRole("textbox", { name: "章节正文" })).toHaveValue("另一章原稿");
  expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
});

it("does not publish an accepted revision with nonidentical raw line endings", async () => {
  const { api, fixture } = strictApp();
  api.writing.acceptReviewed.mockImplementationOnce(async () => {
    fixture.proposal = written;
    fixture.library = {
      ...fixture.library,
      chapters: [{ ...storedChapter, body: "坐船\n🚣到岸" }, separateChapter],
    };
    return written;
  });
  await openStrictChapter();
  await confirmAdoption();
  await screen.findByText(/同版本正文不匹配/);
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  expect(screen.getByRole("textbox", { name: "章节正文" })).toHaveValue("作者原稿");
  expect(api.apply).not.toHaveBeenCalled();
});

it("keeps a newer unsaved author draft even after verifying historical adoption", async () => {
  const { api, fixture } = strictApp();
  const finalEpoch = deferred<string>();
  api.writing.epoch.mockResolvedValueOnce("e").mockResolvedValueOnce("e")
    .mockImplementationOnce(() => finalEpoch.promise);
  api.writing.acceptReviewed.mockImplementationOnce(async () => {
    fixture.proposal = written;
    fixture.library = {
      ...fixture.library,
      chapters: [{ ...storedChapter, revision: 3, body: "较新的书库稿" }, separateChapter],
    };
    return written;
  });
  await openStrictChapter();
  await confirmAdoption();
  await waitFor(() => expect(api.writing.epoch).toHaveBeenCalledTimes(3));
  // Emulate an editor update racing the completion in the same App process.
  // Ordinary UI cannot edit through the inert modal backdrop.
  fireEvent.change(screen.getByRole("textbox", { name: "章节正文" }), {
    target: { value: "我手上的未保存稿" },
  });
  await act(async () => finalEpoch.resolve("e"));
  await screen.findByText(/曾采纳，正文后来已改变.*编辑器未刷新/);
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  expect(screen.getByRole("textbox", { name: "章节正文" })).toHaveValue("我手上的未保存稿");
  expect(api.apply).not.toHaveBeenCalled();
});

it("uses independently verified cancellation instead of a conflicting provisional acceptance receipt", async () => {
  const { api, fixture } = strictApp();
  api.writing.acceptReviewed.mockImplementationOnce(async () => {
    fixture.proposal = { ...strictProposal, decision: "cancelled" };
    return written;
  });
  await openStrictChapter();
  await confirmAdoption();
  await screen.findByText(/其他决定处理.*已取消.*并非本次采纳/);
  expect(screen.queryByText(/精确终态证据：已采纳|当前正文已核对/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "只读核对建议 q" })).not.toBeInTheDocument();
  expect(api.load).toHaveBeenCalledTimes(2);
  expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
});

it("keeps authenticated terminal status across close and reopen when both reads return stale pending", async () => {
  const { api, fixture } = strictApp();
  api.writing.decide.mockImplementationOnce(async () => {
    fixture.proposal = { ...strictProposal, decision: "rejected" };
    return fixture.proposal;
  });
  await openStrictChapter();
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  fireEvent.click(await screen.findByRole("button", { name: "拒绝建议" }));
  fireEvent.click(screen.getByRole("button", { name: "确认拒绝" }));
  await screen.findByText(/已拒绝.*未按此决定刷新或更改正文/);
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  fixture.proposal = strictProposal;
  await openStrictChapter(false);
  await screen.findByText("rejected");
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  await screen.findByText(/建议状态与已核对的终态冲突/);
  expect(screen.queryByRole("button", { name: "拒绝建议" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "采纳建议" })).not.toBeInTheDocument();
  expect(api.writing.decide).toHaveBeenCalledOnce();
});

it("opens global read-only recovery without a draft save and continues checking while keeping that draft", async () => {
  const { api, fixture } = strictApp();
  const hold = deferred<string>();
  api.writing.epoch.mockImplementationOnce(() => hold.promise);
  api.writing.acceptReviewed.mockImplementationOnce(async () => {
    fixture.proposal = written;
    fixture.library = { ...fixture.library, chapters: [storedChapter, separateChapter] };
    return written;
  });
  api.apply.mockRejectedValue(Error("正文版本冲突"));
  await openStrictChapter();
  await confirmAdoption();
  await waitFor(() => expect(api.writing.epoch).toHaveBeenCalledOnce());
  fireEvent.click(await screen.findByRole("button", { name: "停止等待核对" }));
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  const editor = screen.getByRole("textbox", { name: "章节正文" });
  // Suspends the editor's existing debounce, so any save attempt would have
  // come from opening the global recovery route, not the independent timer.
  fireEvent.compositionStart(editor);
  fireEvent.change(editor, { target: { value: "需保留的作者稿" } });
  fireEvent.click(screen.getByText("待核对的正文决定"));
  const link = screen.getByRole("button", { name: "核对正文决定 · 书 / 甲章" });
  act(() => { fireEvent.click(link); fireEvent.click(link); });
  const dialog = await screen.findByRole("dialog", { name: "章节写作约束" });
  expect(api.apply).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole("button", { name: "只读核对建议 q" }));
  await screen.findByText(/当前正文已核对/);
  expect(api.apply).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole("button", { name: "关闭写作约束" }));
  expect(editor).toHaveValue("需保留的作者稿");
  expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
});

it("keeps the authenticated acceptance while detached body recovery detects a changed same-ID input", async () => {
  const { api, fixture } = strictApp();
  api.writing.acceptReviewed.mockImplementationOnce(async () => {
    fixture.proposal = written;
    fixture.library = { ...fixture.library, chapters: [separateChapter] };
    return written;
  });
  await openStrictChapter();
  await confirmAdoption();
  await screen.findByText(/已采纳.*当前正文未核对.*章节已删除/);
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  const changed = { ...written, input: { ...written.input, new_body: "同一版本的伪稿" } };
  fixture.proposal = changed;
  fixture.library = { ...fixture.library, chapters: [{ ...storedChapter, body: changed.input.new_body }, separateChapter] };
  fireEvent.click(screen.getByText("待核对的正文决定"));
  fireEvent.click(screen.getByRole("button", { name: "核对正文决定 · 书 / 甲章" }));
  fireEvent.click(await screen.findByRole("button", { name: "只读核对建议 q" }));
  await screen.findByText(/已采纳.*完整提案与已认证终态证据冲突/);
  expect(screen.getByText(/精确终态证据：已采纳/)).toBeInTheDocument();
  expect(api.load).toHaveBeenCalledTimes(3);
  expect(screen.queryByText(/当前正文已核对/)).not.toBeInTheDocument();

  fixture.proposal = written;
  fixture.library = { ...fixture.library, chapters: [storedChapter, separateChapter] };
  fireEvent.click(screen.getByRole("button", { name: "只读核对建议 q" }));
  await screen.findByText(/当前正文已核对/);
  expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  expect(screen.getByRole("textbox", { name: "章节正文" })).toHaveValue("作者原稿");
});

it("fails closed on later accepted body verification if the original proposal digest was unavailable", async () => {
  const { api, fixture } = strictApp();
  const digest = vi.spyOn(crypto.subtle, "digest").mockRejectedValueOnce(Error("测试环境摘要不可用"));
  try {
    api.writing.acceptReviewed.mockImplementationOnce(async () => {
      fixture.proposal = written;
      fixture.library = { ...fixture.library, chapters: [separateChapter] };
      return written;
    });
    await openStrictChapter();
    await confirmAdoption();
    await screen.findByText(/已采纳.*当前正文未核对.*章节已删除/);
    fixture.library = { ...fixture.library, chapters: [storedChapter, separateChapter] };
    fireEvent.click(screen.getByRole("button", { name: "只读核对建议 q" }));
    await screen.findByText(/完整提案指纹不可用.*保留已认证结果/);
    expect(api.load).toHaveBeenCalledTimes(3);
    expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
    expect(screen.queryByText(/当前正文已核对/)).not.toBeInTheDocument();
  } finally {
    digest.mockRestore();
  }
});

it("can stop a stalled proposal hash without a trailing epoch or delayed authentication", async () => {
  const { api, fixture } = strictApp();
  const hold = deferred<ArrayBuffer>();
  const digest = vi.spyOn(crypto.subtle, "digest").mockImplementationOnce(() => hold.promise);
  try {
    api.writing.acceptReviewed.mockImplementationOnce(async () => {
      fixture.proposal = written;
      fixture.library = { ...fixture.library, chapters: [storedChapter, separateChapter] };
      return written;
    });
    await openStrictChapter();
    await confirmAdoption();
    await waitFor(() => expect(digest).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("button", { name: "停止等待核对" }));
    expect(screen.getByText(/完整提案候选.*尚未完成时代核对/)).toBeInTheDocument();
    await act(async () => hold.resolve(new ArrayBuffer(32)));
    expect(screen.queryByText(/精确终态证据|当前正文已核对/)).not.toBeInTheDocument();
    expect(api.writing.epoch).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "只读核对建议 q" }));
    await screen.findByText(/当前正文已核对/);
    expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
  } finally {
    digest.mockRestore();
  }
});

it("labels both conflicting candidates as provisional until the trailing epoch verifies the complete read", async () => {
  const { api, fixture } = strictApp();
  api.writing.epoch.mockResolvedValueOnce("e").mockRejectedValueOnce(Error("时代暂不可读"));
  api.writing.acceptReviewed.mockImplementationOnce(async () => {
    fixture.proposal = { ...strictProposal, decision: "cancelled" };
    return written;
  });
  await openStrictChapter();
  await confirmAdoption();
  await screen.findByText(/仍待核对.*时代暂不可读/);
  expect(screen.getByText(/写入回执候选.*未核实.*已采纳/)).toBeInTheDocument();
  expect(screen.getByText(/完整提案候选.*尚未完成时代核对.*已取消/)).toBeInTheDocument();
  expect(screen.queryByText(/精确终态证据|当前正文已核对/)).not.toBeInTheDocument();
  // Hold the retry read so old provisional text cannot become authoritative yet.
  let releaseEpoch!: (epoch: string) => void;
  const pendingEpoch = new Promise<string>((resolve) => { releaseEpoch = resolve; });
  api.writing.epoch.mockImplementationOnce(() => pendingEpoch);
  fireEvent.click(screen.getByRole("button", { name: "只读核对建议 q" }));
  await waitFor(() => expect(api.writing.epoch).toHaveBeenCalledTimes(3));
  // The former wait matched this old candidate even while the read is blocked.
  expect(screen.getByText(/其他决定处理.*已取消.*并非本次采纳/)).toBeInTheDocument();
  expect(screen.queryAllByText(/写入回执候选|完整提案候选/)).toHaveLength(2);
  expect(screen.queryByText(/原书库精确终态证据/)).not.toBeInTheDocument();
  await act(async () => { releaseEpoch("e"); });
  await screen.findByText(/已由其他决定处理.*已取消.*并非本次采纳.*未按此决定刷新或更改正文/);
  await waitFor(() => {
    expect(screen.queryAllByText(/写入回执候选|完整提案候选/)).toHaveLength(0);
  });
  expect(api.load).toHaveBeenCalledTimes(2);
});

async function reopenAuthenticatedHistory() {
  const app = strictApp();
  app.api.writing.acceptReviewed.mockImplementationOnce(async () => {
    app.fixture.proposal = written;
    app.fixture.library = { ...app.fixture.library, chapters: [storedChapter, separateChapter] };
    return written;
  });
  await openStrictChapter();
  await confirmAdoption();
  await screen.findByText(/当前正文已核对/);
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  await openStrictChapter(false);
  await screen.findByText("accepted");
  return app;
}

it.each(["resolve", "reject"] as const)(
  "can stop a hung history hash and ignore its late %s while a new detail read waits",
  async (lateOutcome) => {
    const { api } = await reopenAuthenticatedHistory();
    const nativeDigest = crypto.subtle.digest.bind(crypto.subtle);
    const first = deferred<ArrayBuffer>();
    let rejectFirst!: (error: Error) => void;
    const rejected = new Promise<ArrayBuffer>((_resolve, reject) => { rejectFirst = reject; });
    const second = deferred<void>();
    const digest = vi.spyOn(crypto.subtle, "digest")
      .mockImplementationOnce(() => lateOutcome === "resolve" ? first.promise : rejected)
      .mockImplementationOnce(async (algorithm, value) => {
        const result = await nativeDigest(algorithm, value);
        await second.promise;
        return result;
      });
    try {
      const choose = screen.getByRole("button", { name: "查看建议 q" });
      fireEvent.click(choose);
      await waitFor(() => expect(digest).toHaveBeenCalledTimes(1));
      const stop = await screen.findByRole("button", { name: "停止等待核对" });
      expect(blockedUnload()).toBe(true);
      fireEvent.click(stop);
      expect(blockedUnload()).toBe(false);
      expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeEnabled();
      fireEvent.click(choose);
      await waitFor(() => expect(digest).toHaveBeenCalledTimes(2));
      await act(async () => {
        if (lateOutcome === "resolve") first.resolve(new ArrayBuffer(32));
        else rejectFirst(Error("旧历史核对失败"));
      });
      expect(screen.queryByText(/旧历史核对失败|建议内容与已核对的终态冲突/)).not.toBeInTheDocument();
      expect(screen.getByRole("button", { name: "停止等待核对" })).toBeEnabled();
      expect(blockedUnload()).toBe(true);
      await act(async () => second.resolve());
      await screen.findByText("建议新稿");
      expect(blockedUnload()).toBe(false);
      expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
      expect(api.writing.pack).toHaveBeenCalledTimes(2);
    } finally {
      digest.mockRestore();
    }
  },
);

it("renders the authenticated history snapshot, not bridge data mutated during its SHA-256 wait", async () => {
  const { fixture, api } = await reopenAuthenticatedHistory();
  const shared = structuredClone(written);
  fixture.proposal = shared;
  const nativeDigest = crypto.subtle.digest.bind(crypto.subtle);
  const encoded = deferred<void>();
  const release = deferred<void>();
  const digest = vi.spyOn(crypto.subtle, "digest").mockImplementationOnce(async (algorithm, data) => {
    const authentic = await nativeDigest(algorithm, data);
    encoded.resolve();
    await release.promise;
    return authentic;
  });
  try {
    fireEvent.click(screen.getByRole("button", { name: "查看建议 q" }));
    await encoded.promise;
    shared.input.new_body = "未被指纹覆盖的变造展示稿";
    shared.input.pack_id = "变造依据包";
    shared.author_review!.items[0].note = "未被指纹覆盖的变造审核";
    await act(async () => release.resolve());
    const detail = await screen.findByText("建议新稿");
    expect(detail.parentElement?.querySelector("pre")?.textContent).toBe(written.input.new_body);
    expect(screen.queryByText(/变造|依据或提案归属不匹配/)).not.toBeInTheDocument();
    expect(api.writing.pack).toHaveBeenLastCalledWith("b", "p");
  } finally {
    digest.mockRestore();
  }
});

it("does not continue a verified history read when restoration is known in the same render batch", async () => {
  const { api, fixture } = await reopenAuthenticatedHistory();
  const nativeDigest = crypto.subtle.digest.bind(crypto.subtle);
  const hashed = deferred<void>();
  const late = deferred<void>();
  const restore = deferred<LibraryState | null>();
  const digest = vi.spyOn(crypto.subtle, "digest").mockImplementationOnce(async (algorithm, value) => {
    const authentic = await nativeDigest(algorithm, value);
    hashed.resolve();
    await late.promise;
    return authentic;
  });
  try {
    api.restore.mockImplementationOnce(() => restore.promise);
    fireEvent.click(screen.getByRole("button", { name: "查看建议 q" }));
    await hashed.promise;
    fireEvent.click(screen.getByRole("button", { name: "恢复备份" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "确认操作" })).getByRole("button", { name: "确认" }));
    await waitFor(() => expect(api.restore).toHaveBeenCalledOnce());
    const replacement = {
      ...fixture.library,
      books: [{ ...fixture.library.books[0], title: "已替换的书库" }],
    };
    await act(async () => {
      restore.resolve(replacement);
      await Promise.resolve();
      late.resolve();
    });
    expect(screen.getByText(/此界面属于旧书库/)).toBeInTheDocument();
    expect(screen.queryByText("建议新稿")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "停止等待核对" })).not.toBeInTheDocument();
    expect(api.writing.pack).toHaveBeenCalledTimes(1);
    expect(blockedUnload()).toBe(false);
    expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeEnabled();
  } finally {
    digest.mockRestore();
  }
});

it("preserves the dirty configuration before opening authenticated history and can view history after discarding", async () => {
  const { api } = await reopenAuthenticatedHistory();
  const draft = screen.getByRole("textbox", { name: "写作目标" });
  const detail = screen.getByRole("button", { name: "查看建议 q" });
  api.writing.epoch.mockClear();
  api.writing.proposal.mockClear();
  fireEvent.change(draft, { target: { value: "我的未保存配置文字" } });
  expect(detail).toBeDisabled();
  fireEvent.click(detail);
  expect(api.writing.epoch).not.toHaveBeenCalled();
  expect(api.writing.proposal).not.toHaveBeenCalled();
  expect(draft).toHaveValue("我的未保存配置文字");
  expect(blockedUnload()).toBe(true);
  const discard = screen.getByRole("button", { name: "放弃未保存的配置修改" });
  expect(discard).toBeEnabled();
  fireEvent.click(discard);
  expect(detail).toBeEnabled();
  fireEvent.click(detail);
  await screen.findByText("建议新稿");
  expect(draft).toHaveValue("渡河");
  expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
  expect(api.apply).not.toHaveBeenCalled();
  expect(blockedUnload()).toBe(false);
});

it("allows leaving if an authenticated history read discovers a replacement after explicitly discarding the draft", async () => {
  const { api } = await reopenAuthenticatedHistory();
  fireEvent.change(screen.getByRole("textbox", { name: "写作目标" }), {
    target: { value: "另一份配置草稿" },
  });
  api.writing.epoch.mockClear().mockResolvedValueOnce("replacement");
  const detail = screen.getByRole("button", { name: "查看建议 q" });
  fireEvent.click(detail);
  expect(api.writing.epoch).not.toHaveBeenCalled();
  expect(screen.getByDisplayValue("另一份配置草稿")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "放弃未保存的配置修改" }));
  fireEvent.click(detail);
  await screen.findByText(/此界面属于旧书库/);
  expect(blockedUnload()).toBe(false);
  const dialog = screen.getByRole("dialog", { name: "章节写作约束" });
  fireEvent.keyDown(dialog, { key: "Escape" });
  expect(dialog).not.toBeInTheDocument();
  expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
});

it("updates a verified v1 body result to historical adoption after refreshing the different v2 body", async () => {
  const { api, fixture } = strictApp();
  api.writing.acceptReviewed.mockImplementationOnce(async () => {
    fixture.proposal = written;
    fixture.library = { ...fixture.library, chapters: [storedChapter, separateChapter] };
    return written;
  });
  await openStrictChapter();
  await confirmAdoption();
  await screen.findByText(/当前正文已核对，书库已保存的正文与建议逐字一致/);
  fixture.library = {
    ...fixture.library,
    chapters: [{ ...storedChapter, revision: 2, body: "后来改写的正文" }, separateChapter],
  };
  const policyRead = deferred<void>();
  const getWritingState = api.writing.state.getMockImplementation()!;
  api.writing.state.mockImplementationOnce(async (...args) => {
    await policyRead.promise;
    return getWritingState(...args);
  });
  fireEvent.click(screen.getByRole("button", { name: "刷新依据与建议" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: "章节正文" })).toHaveValue("后来改写的正文"));
  // The App can publish the new chapter before its writing-state read settles.
  expect(screen.queryByText(/当前正文已核对，书库已保存的正文与建议逐字一致/)).not.toBeInTheDocument();
  await act(async () => policyRead.resolve());
  await screen.findByText(/曾采纳，正文后来已改变.*采纳 v1.*刷新读到 v2/);
  expect(screen.queryByText(/当前正文已核对，书库已保存的正文与建议逐字一致/)).not.toBeInTheDocument();
  expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
  expect(api.apply).not.toHaveBeenCalled();
});

it("routes source comparison through the App without navigating, saving or replacing the target manuscript", async () => {
  const {api, sourceWindow} = strictApp(true);
  const modal = await openStrictChapter();
  fireEvent.click(await within(modal).findByRole("button", {name:"查看建议 q"}));
  fireEvent.click(await within(modal).findByRole("button", {name:"核对已保存来源原文"}));
  expect(await within(modal).findByText("已保存来源位置与版本匹配")).toBeInTheDocument();
  expect(sourceWindow).toHaveBeenCalledWith("b","d","e",3,5);
  fireEvent.click(within(modal).getByRole("button", {name:"返回审阅"}));
  fireEvent.click(within(modal).getByRole("button", {name:"关闭写作约束"}));
  expect(screen.getByRole("textbox", {name:"章节正文"})).toHaveValue("作者原稿");
  expect(api.apply).not.toHaveBeenCalled();
  expect(api.writing.acceptReviewed).not.toHaveBeenCalled();
});

it("retains session-only notes across panel closure without sharing them with another chapter", async () => {
  const {api, sourceWindow} = strictApp(true);
  const window = await sourceWindow.getMockImplementation()!();
  sourceWindow.mockResolvedValue({...window, position:{...window.position,chapter_revision:1}});
  const modal = await openStrictChapter();
  fireEvent.click(await within(modal).findByRole("button", {name:"查看建议 q"}));
  fireEvent.change(await within(modal).findByLabelText("作者备注：规则"), {target:{value:"旧建议原地核对笔记"}});
  fireEvent.click(within(modal).getByRole("button", {name:"核对已保存来源原文"}));
  await within(modal).findByText("正文版本已变化：冻结 v0，当前 v1。");
  fireEvent.click(within(modal).getByRole("button", {name:"返回审阅"}));
  expect(blockedUnload()).toBe(true);
  fireEvent.click(within(modal).getByRole("button", {name:"保留备注副本并退出本次编辑"}));
  expect(blockedUnload()).toBe(false);
  fireEvent.click(within(modal).getByRole("button", {name:"关闭写作约束"}));
  expect(modal).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", {name:"第2章 乙章"}));
  const other = await openStrictChapter(false);
  await within(other).findByText(/备注槽已被其他范围/);
  expect(within(other).queryByLabelText("旧建议备注副本")).not.toBeInTheDocument();
  fireEvent.click(within(other).getByRole("button", {name:"关闭写作约束"}));
  fireEvent.click(screen.getByRole("button", {name:"第1章 甲章"}));
  const reopened = await openStrictChapter(false);
  expect(await within(reopened).findByLabelText("旧建议备注副本")).toHaveValue("规则：旧建议原地核对笔记");
  expect(screen.getByRole("textbox", {name:"章节正文"})).toHaveValue("作者原稿");
  expect(api.apply).not.toHaveBeenCalled();
  expect(api.writing.acceptReviewed).not.toHaveBeenCalled();
});
