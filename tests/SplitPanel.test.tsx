import "@testing-library/jest-dom/vitest";
import React from "react";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  cleanup,
} from "@testing-library/react";
import { it, expect, vi, afterEach } from "vitest";
import SplitPanel from "../src/SplitPanel";
import { rawCursorOffset } from "../src/splitTypes";
afterEach(cleanup);
const p = {
  id: "p",
  original_title: "旧章",
  new_title: "后半",
  before: "前🙂\r\n",
  after: "后",
  before_bytes: 9,
  after_bytes: 3,
  before_truncated: false,
  after_truncated: false,
  expires_at: Date.now() / 1000 + 600,
};
function mount(overrides: any = {}, reload: any = async () => {}) {
  const api = {
    prepare: vi.fn(async () => p),
    detail: vi.fn(async () => "完整正文"),
    commit: vi.fn(async () => ({
      book_id: "b",
      original_chapter_id: "c",
      new_chapter_id: "n",
    })),
    discard: vi.fn(async () => {}),
    ...overrides,
  };
  const close = vi.fn(),
    pending = vi.fn();
  render(
    <SplitPanel
      api={api}
      target={{
        bookId: "b",
        chapterId: "c",
        revision: 1,
        offset: 5,
        title: "旧章",
      }}
      onClose={close}
      onPendingChange={pending}
      onCommitted={reload}
    />,
  );
  return { api, close, pending };
}
it("maps textarea normalized cursor to original raw UTF16 and rejects mismatch/range", () => {
  expect(rawCursorOffset("甲🙂\r\n乙\r丙", "甲🙂\n乙\n丙", 4, 4)).toBe(5);
  expect(rawCursorOffset("甲\r乙", "甲\n乙", 2, 2)).toBe(2);
  expect(() => rawCursorOffset("abc", "abc", 1, 2)).toThrow();
  expect(() => rawCursorOffset("甲🙂乙", "甲🙂乙", 2, 2)).toThrow();
  expect(() => rawCursorOffset("abc", "xyz", 1, 1)).toThrow();
});
it("requires preview then explicit confirmation; details and association warning visible", async () => {
  const { api } = mount();
  fireEvent.change(screen.getByLabelText("新章名"), {
    target: { value: "后半" },
  });
  fireEvent.click(screen.getByText("生成拆章预览"));
  await screen.findByText("前🙂");
  fireEvent.click(screen.getByText("查看后半全文"));
  await screen.findByText("完整正文");
  expect(screen.getByText(/新章不继承关联/)).toBeInTheDocument();
  fireEvent.click(screen.getByText("拆分为两章"));
  expect(api.commit).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("确认拆分"));
  await screen.findByText("拆章已完成");
  expect(api.commit).toHaveBeenCalledTimes(1);
});
it("invalidates edited and late previews", async () => {
  let resolve: any;
  const { api } = mount({
    prepare: vi.fn(() => new Promise((r) => (resolve = r))),
  });
  fireEvent.click(screen.getByText("生成拆章预览"));
  fireEvent.change(screen.getByLabelText("新章名"), {
    target: { value: "新名字" },
  });
  resolve(p);
  await waitFor(() => expect(api.discard).toHaveBeenCalledWith("p"));
  expect(screen.queryByText("拆分为两章")).toBeNull();
});
it("blocks double commit/close and retries only reading after load error", async () => {
  let resolve: any;
  const reload = vi
    .fn()
    .mockRejectedValueOnce(Error("读取失败"))
    .mockResolvedValue(undefined);
  const { api, close } = mount(
    { commit: vi.fn(() => new Promise((r) => (resolve = r))) },
    reload,
  );
  fireEvent.click(screen.getByText("生成拆章预览"));
  await screen.findByText("拆分为两章");
  fireEvent.click(screen.getByText("拆分为两章"));
  fireEvent.click(screen.getByText("确认拆分"));
  fireEvent.click(screen.getByText("确认拆分"));
  fireEvent.click(screen.getByLabelText("关闭拆章面板"));
  expect(close).not.toHaveBeenCalled();
  resolve({ book_id: "b", original_chapter_id: "c", new_chapter_id: "n" });
  await screen.findByText("读取失败");
  fireEvent.click(screen.getByText("重新读取核对"));
  await screen.findByText("拆章已完成");
  expect(api.commit).toHaveBeenCalledTimes(1);
  expect(reload).toHaveBeenCalledTimes(2);
});
it.each([undefined, {}, "wrong"])(
  "unknown response never confirms success %s",
  async (value) => {
    const reload = vi.fn(async () => {});
    const { api } = mount({ commit: vi.fn(async () => value) }, reload);
    fireEvent.click(screen.getByText("生成拆章预览"));
    await screen.findByText("拆分为两章");
    fireEvent.click(screen.getByText("拆分为两章"));
    fireEvent.click(screen.getByText("确认拆分"));
    await screen.findByText(/已重新读取，请核对目录/);
    expect(screen.queryByText("拆章已完成")).toBeNull();
    expect(api.commit).toHaveBeenCalledTimes(1);
    expect(reload).toHaveBeenCalledWith(undefined);
  },
);
it("expired candidate cannot commit", async () => {
  const { api } = mount({
    prepare: vi.fn(async () => ({ ...p, expires_at: 1 })),
  });
  fireEvent.click(screen.getByText("生成拆章预览"));
  await screen.findByText("拆分为两章");
  fireEvent.click(screen.getByText("拆分为两章"));
  await screen.findByText(/预览已过期/);
  expect(api.commit).not.toHaveBeenCalled();
});
it("shows conflict reason after safe reconciliation without repeating commit", async () => {
  const { api } = mount({
    commit: vi.fn(async () => {
      throw Error("正文版本已变更");
    }),
  });
  fireEvent.click(screen.getByText("生成拆章预览"));
  await screen.findByText("拆分为两章");
  fireEvent.click(screen.getByText("拆分为两章"));
  fireEvent.click(screen.getByText("确认拆分"));
  await screen.findByText(/正文版本已变更/);
  expect(api.commit).toHaveBeenCalledTimes(1);
});
