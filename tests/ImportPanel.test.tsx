import "@testing-library/jest-dom/vitest";
import React from "react";
import {
  render,
  screen,
  fireEvent,
  cleanup,
  waitFor,
  act,
} from "@testing-library/react";
import { it, expect, afterEach, vi } from "vitest";
import ImportPanel from "../src/ImportPanel";
import type { ImportAPI, ImportPreview } from "../src/importTypes";
afterEach(cleanup);
const p = (id = "p"): ImportPreview => ({
  id,
  source_name: "稿件.txt",
  title: "稿件",
  author: "",
  mode: "headings",
  expires_at: Date.now() / 1000 + 600,
  volumes: [
    {
      title: "卷一",
      chapters: [{ index: 0, title: "第一章", bytes: 9, snippet: "片段" }],
    },
  ],
  total_chapters: 1,
  total_bytes: 9,
});
function setup(changes: Partial<ImportAPI> = {}, load = vi.fn(async () => {})) {
  const api: ImportAPI = {
    choose: vi.fn(async () => p()),
    revise: vi.fn(async (_, title, author, mode) => ({
      ...p("new"),
      title,
      author,
      mode,
    })),
    detail: vi.fn(async () => ({ index: 0, body: "完整正文\r\n" })),
    commit: vi.fn(async () => ({ book_id: "b", volumes: 1, chapters: 1 })),
    discard: vi.fn(async () => {}),
    ...changes,
  };
  const pending = vi.fn(),
    close = vi.fn();
  const result = render(
    <ImportPanel
      api={api}
      onClose={close}
      onPendingChange={pending}
      onImported={load}
    />,
  );
  return { api, pending, close, load, ...result };
}
async function choose() {
  fireEvent.click(screen.getByRole("button", { name: "选择TXT文件" }));
  await screen.findByText("稿件.txt");
}
async function confirm() {
  fireEvent.click(screen.getByRole("button", { name: "导入为新作品" }));
  fireEvent.click(screen.getByRole("button", { name: "确认导入" }));
}
it("selects single mode before choosing and cancellation does not write", async () => {
  const s = setup({ choose: vi.fn(async () => null) });
  fireEvent.change(screen.getByLabelText("分章模式"), {
    target: { value: "single" },
  });
  fireEvent.click(screen.getByText("选择TXT文件"));
  await waitFor(() => expect(s.api.choose).toHaveBeenCalledWith("single"));
  expect(s.api.commit).not.toHaveBeenCalled();
  expect(screen.queryByText("导入为新作品")).toBeNull();
});
it("loads only selected chapter detail and confirms twice", async () => {
  const s = setup();
  await choose();
  expect(s.api.detail).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "查看 第一章" }));
  expect(await screen.findByText("完整正文", { exact: false })).toBeTruthy();
  fireEvent.click(screen.getByText("导入为新作品"));
  expect(s.api.commit).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("确认导入"));
  await screen.findByText("已导入新作品：1卷、1章。");
  expect(s.load).toHaveBeenCalledWith({
    book_id: "b",
    volumes: 1,
    chapters: 1,
  });
});
it("editing metadata invalidates confirmation until the frozen source is revised", async () => {
  const s = setup();
  await choose();
  fireEvent.click(screen.getByText("导入为新作品"));
  fireEvent.change(screen.getByLabelText("书名"), {
    target: { value: "新书" },
  });
  expect(screen.queryByText("确认导入")).toBeNull();
  expect(screen.queryByText("导入为新作品")).toBeNull();
  fireEvent.click(screen.getByText("更新预览"));
  await waitFor(() =>
    expect(s.api.revise).toHaveBeenCalledWith("p", "新书", "", "headings"),
  );
  await screen.findByText("导入为新作品");
  await confirm();
  await waitFor(() => expect(s.api.commit).toHaveBeenCalledWith("new"));
});
it("failed revision retains the source id for retry but cannot submit old config", async () => {
  const revise = vi
    .fn()
    .mockRejectedValueOnce(Error("配置失败"))
    .mockImplementation(async () => ({ ...p("new"), title: "新书" }));
  const s = setup({ revise });
  await choose();
  fireEvent.change(screen.getByLabelText("书名"), {
    target: { value: "新书" },
  });
  fireEvent.click(screen.getByText("更新预览"));
  await screen.findByText("配置失败");
  expect(screen.queryByText("导入为新作品")).toBeNull();
  fireEvent.click(screen.getByText("更新预览"));
  await screen.findByText("导入为新作品");
  expect(revise.mock.calls.map((c) => c[0])).toEqual(["p", "p"]);
  expect(s.api.commit).not.toHaveBeenCalled();
});
it("ignores late detail after inputs change", async () => {
  let resolve!: (v: any) => void;
  setup({
    detail: () =>
      new Promise<any>((r) => {
        resolve = r;
      }),
  });
  await choose();
  fireEvent.click(screen.getByRole("button", { name: "查看 第一章" }));
  fireEvent.change(screen.getByLabelText("作者"), {
    target: { value: "作者" },
  });
  await act(async () => resolve({ index: 0, body: "迟到正文" }));
  expect(screen.queryByText("迟到正文")).toBeNull();
});
it("late revised preview cannot overwrite newer draft but retains new source id", async () => {
  let resolve!: (v: any) => void;
  const revise = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise<any>((r) => {
          resolve = r;
        }),
    )
    .mockImplementation(async () => ({ ...p("latest"), title: "再次修改" }));
  setup({ revise });
  await choose();
  fireEvent.change(screen.getByLabelText("书名"), {
    target: { value: "修改" },
  });
  fireEvent.click(screen.getByText("更新预览"));
  fireEvent.change(screen.getByLabelText("书名"), {
    target: { value: "再次修改" },
  });
  await act(async () => resolve({ ...p("new"), title: "修改" }));
  expect(screen.getByLabelText("书名")).toHaveProperty("value", "再次修改");
  expect(screen.queryByText("导入为新作品")).toBeNull();
  fireEvent.click(screen.getByText("更新预览"));
  await waitFor(() => expect(revise.mock.calls[1][0]).toBe("new"));
});
it("blocks close and repeat commit while writing", async () => {
  let resolve!: (v: any) => void;
  const s = setup({
    commit: vi.fn(
      () =>
        new Promise<any>((r) => {
          resolve = r;
        }),
    ),
  });
  await choose();
  await confirm();
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(s.close).not.toHaveBeenCalled();
  expect(s.pending).toHaveBeenLastCalledWith(true);
  expect(s.api.commit).toHaveBeenCalledTimes(1);
  await act(async () => resolve({ book_id: "b", volumes: 1, chapters: 1 }));
  await screen.findByText("已导入新作品：1卷、1章。");
});
it("successful commit with failed reload retries only reading", async () => {
  const load = vi
    .fn()
    .mockRejectedValueOnce(Error("读取失败"))
    .mockResolvedValue(undefined);
  const s = setup({}, load);
  await choose();
  await confirm();
  await screen.findByText("读取失败");
  expect(screen.getByLabelText("关闭导入面板")).toBeDisabled();
  fireEvent.click(screen.getByText("重新加载书架"));
  await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
  expect(s.api.commit).toHaveBeenCalledTimes(1);
  await waitFor(() =>
    expect(screen.getByLabelText("关闭导入面板")).not.toBeDisabled(),
  );
});
it("unknown outcome requires read reconciliation and never retries import", async () => {
  const s = setup({
    commit: vi.fn(async () => {
      throw Error("连接中断");
    }),
  });
  await choose();
  await confirm();
  await screen.findByText(/导入结果未确认/);
  fireEvent.click(screen.getByText("刷新书架核对"));
  await screen.findByText(/已重新读取书架/);
  expect(screen.queryByText(/导入结果未确认/)).toBeNull();
  expect(s.load).toHaveBeenCalledWith(undefined);
  expect(s.api.commit).toHaveBeenCalledTimes(1);
});
it("rejects oversized metadata and expired candidate before commit", async () => {
  const s = setup({ choose: vi.fn(async () => ({ ...p(), expires_at: 1 })) });
  await choose();
  expect(screen.getByText("导入为新作品")).toBeDisabled();
  fireEvent.change(screen.getByLabelText("书名"), {
    target: { value: "中".repeat(1366) },
  });
  fireEvent.click(screen.getByText("更新预览"));
  await screen.findByText(/4096/);
  expect(s.api.revise).not.toHaveBeenCalled();
  expect(s.api.commit).not.toHaveBeenCalled();
});
it("cleans a late selection after closing", async () => {
  let resolve!: (v: any) => void;
  const s = setup({
    choose: vi.fn(
      () =>
        new Promise<any>((r) => {
          resolve = r;
        }),
    ),
  });
  fireEvent.click(screen.getByText("选择TXT文件"));
  s.unmount();
  await act(async () => resolve(p()));
  expect(s.api.discard).toHaveBeenCalledWith("p");
});

it("explains TXT encoding, byte cap and that import creates a separate work before choosing", () => {
  const { api } = setup();
  expect(screen.getByText(/仅UTF-8.*最多16 MiB.*单章最多10 MiB/)).toBeInTheDocument();
  expect(screen.getByText(/每次导入都会新建作品.*不会自动合并/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "选择TXT文件" })).toBeEnabled();
  expect(api.choose).not.toHaveBeenCalled();
  expect(api.commit).not.toHaveBeenCalled();
});
