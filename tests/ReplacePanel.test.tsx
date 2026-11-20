import React from "react";
import {
  render,
  screen,
  fireEvent,
  cleanup,
  waitFor,
  act,
} from "@testing-library/react";
import { afterEach, it, expect, vi } from "vitest";
import ReplacePanel from "../src/ReplacePanel";
const p = {
  id: "p",
  book_id: "b",
  query: "林溪",
  replacement: "林舟",
  expires_at: Date.now() / 1000 + 600,
  chapters: [
    {
      id: "c",
      title: "第一章",
      revision: 1,
      matches: 2,
      before_snippet: "林溪看海",
      after_snippet: "林舟看海",
    },
  ],
  total_matches: 2,
};
const defer = () => {
  let resolve: any, reject: any;
  const promise = new Promise<any>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
};
function setup(overrides: any = {}, reload = vi.fn(async () => {})) {
  const api = {
    preview: vi.fn(async () => p),
    detail: vi.fn(async () => ({
      chapter_id: "c",
      before: "完整原稿",
      after: "完整新稿",
    })),
    commit: vi.fn(async () => ({ chapters: 1, matches: 2 })),
    discard: vi.fn(async () => {}),
    ...overrides,
  };
  const close = vi.fn(),
    pending = vi.fn();
  render(
    <ReplacePanel
      bookId="b"
      api={api}
      onClose={close}
      onPendingChange={pending}
      onCommitted={reload}
    />,
  );
  return { api, close, pending, reload };
}
async function preview() {
  fireEvent.change(screen.getByLabelText("查找内容"), {
    target: { value: "林溪" },
  });
  fireEvent.change(screen.getByLabelText("替换为"), {
    target: { value: "林舟" },
  });
  fireEvent.click(screen.getByRole("button", { name: "预览替换" }));
  await screen.findByRole("button", { name: "查看 第一章" });
}
afterEach(cleanup);
it("previews metadata then fetches only selected chapter and confirms frozen candidate", async () => {
  const { api } = setup();
  await preview();
  expect(api.detail).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "查看 第一章" }));
  await screen.findByText("完整原稿");
  fireEvent.click(screen.getByRole("button", { name: "替换这些正文" }));
  expect(api.commit).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "确认替换" }));
  await screen.findByText("已替换1章，共2处。");
  expect(api.commit).toHaveBeenCalledWith("p");
});
it("rejects empty and oversized UTF8 query", async () => {
  const { api } = setup();
  fireEvent.click(screen.getByRole("button", { name: "预览替换" }));
  await screen.findByText(/搜索内容须/);
  fireEvent.change(screen.getByLabelText("查找内容"), {
    target: { value: "中".repeat(86) },
  });
  fireEvent.click(screen.getByRole("button", { name: "预览替换" }));
  expect(api.preview).not.toHaveBeenCalled();
});
it("invalidates preview and confirmation when inputs change", async () => {
  const { api } = setup();
  await preview();
  fireEvent.click(screen.getByRole("button", { name: "替换这些正文" }));
  fireEvent.change(screen.getByLabelText("替换为"), {
    target: { value: "新名" },
  });
  expect(screen.queryByRole("button", { name: "确认替换" })).toBeNull();
  expect(screen.queryByRole("button", { name: "查看 第一章" })).toBeNull();
  await waitFor(() => expect(api.discard).toHaveBeenCalledWith("p"));
});
it("ignores and discards late preview after input change", async () => {
  const d = defer();
  const { api } = setup({ preview: vi.fn(() => d.promise) });
  fireEvent.change(screen.getByLabelText("查找内容"), {
    target: { value: "林溪" },
  });
  fireEvent.click(screen.getByRole("button", { name: "预览替换" }));
  fireEvent.change(screen.getByLabelText("查找内容"), {
    target: { value: "别的" },
  });
  await act(async () => d.resolve(p));
  expect(screen.queryByRole("button", { name: "查看 第一章" })).toBeNull();
  expect(api.discard).toHaveBeenCalledWith("p");
});
it("ignores stale detail after changed inputs", async () => {
  const d = defer();
  setup({ detail: vi.fn(() => d.promise) });
  await preview();
  fireEvent.click(screen.getByRole("button", { name: "查看 第一章" }));
  fireEvent.change(screen.getByLabelText("查找内容"), {
    target: { value: "新" },
  });
  await act(async () =>
    d.resolve({ chapter_id: "c", before: "过期原稿", after: "过期新稿" }),
  );
  expect(screen.queryByText("过期原稿")).toBeNull();
});
it("blocks close and double commit while submitting", async () => {
  const d = defer();
  const { api, close, pending } = setup({ commit: vi.fn(() => d.promise) });
  await preview();
  fireEvent.click(screen.getByRole("button", { name: "替换这些正文" }));
  const confirm = screen.getByRole("button", { name: "确认替换" });
  fireEvent.click(confirm);
  fireEvent.click(confirm);
  fireEvent.click(screen.getByRole("button", { name: "关闭替换面板" }));
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(close).not.toHaveBeenCalled();
  expect(pending).toHaveBeenLastCalledWith(true);
  await act(async () => d.resolve({ chapters: 1, matches: 2 }));
  expect(api.commit).toHaveBeenCalledTimes(1);
});
it("keeps success when reload fails and retries only read", async () => {
  const reload = vi
    .fn()
    .mockRejectedValueOnce(Error("读取失败"))
    .mockResolvedValue(undefined);
  const { api } = setup({}, reload);
  await preview();
  fireEvent.click(screen.getByRole("button", { name: "替换这些正文" }));
  fireEvent.click(screen.getByRole("button", { name: "确认替换" }));
  await screen.findByText("读取失败");
  expect(screen.getByText("已替换1章，共2处。")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "重新加载正文" }));
  await waitFor(() => expect(reload).toHaveBeenCalledTimes(2));
  expect(api.commit).toHaveBeenCalledTimes(1);
});
it("does not claim transport failure means no write and offers read-only reconciliation", async () => {
  const { api, reload } = setup({
    commit: vi.fn(async () => {
      throw Error("连接中断");
    }),
  });
  await preview();
  fireEvent.click(screen.getByRole("button", { name: "替换这些正文" }));
  fireEvent.click(screen.getByRole("button", { name: "确认替换" }));
  await screen.findByText(/结果未确认/);
  expect(screen.queryByRole("button", { name: "确认替换" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "刷新正文核对" }));
  await waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
  await screen.findByText(/已重新读取正文/);
  expect(screen.queryByText(/请先刷新正文核对/)).toBeNull();
  expect(screen.getByText(/连接中断/)).toBeTruthy();
  expect(api.commit).toHaveBeenCalledTimes(1);
});
it("zero matches and expired preview cannot commit", async () => {
  setup({
    preview: vi.fn(async () => ({
      ...p,
      query: "x",
      replacement: "",
      chapters: [],
      total_matches: 0,
    })),
  });
  fireEvent.change(screen.getByLabelText("查找内容"), {
    target: { value: "x" },
  });
  fireEvent.click(screen.getByRole("button", { name: "预览替换" }));
  await screen.findByText("没有匹配的正文。");
  expect(screen.queryByRole("button", { name: "替换这些正文" })).toBeNull();
});
it("expired candidates show expiry and disable confirmation", async () => {
  const { api } = setup({
    preview: vi.fn(async () => ({ ...p, expires_at: 1 })),
  });
  await preview();
  expect(
    (screen.getByRole("button", { name: "替换这些正文" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(api.commit).not.toHaveBeenCalled();
});
