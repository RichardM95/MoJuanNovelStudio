import React from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import App from "../src/App";
import type { BackupJob } from "../src/backupTypes";
import type { API, LibraryState } from "../src/types";

afterEach(cleanup);

function defer<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}
const library = (id: string, title: string): LibraryState => ({
  books: [{ id, title, author: "", deleted_at: null }],
  volumes: [],
  chapters: [],
});
const prior: BackupJob = {
  id: "prior-restore",
  kind: "restore",
  status: "done",
  progress: { phase: "commit", completed: 1, total: 1 },
  cancel_requested: false,
  created_at: 1,
  updated_at: 2,
  expires_at: null,
  source_name: null,
  preview: null,
  confirmation_token: null,
  result: null,
  error_code: null,
  error_message: null,
};
const prepared: BackupJob = {
  ...prior,
  id: "new-restore",
  status: "prepared",
  confirmation_token: "token",
  preview: {
    format: "sqlite",
    books: 1,
    volumes: 0,
    chapters: 0,
    history: 0,
    materials: 0,
    version_labels: 0,
    canonical_bytes: 20,
  },
};
it.each(["success", "failure"] as const)(
  "does not publish a delayed older restore snapshot after the newer shelf reload reports %s",
  async (outcome) => {
    const older = defer<LibraryState>();
    const newer = defer<LibraryState>();
    const initial = library("original", "原书架");
    const latest = library("latest", "最新书架");
    const confirm = vi.fn(async () => ({ ...prior, id: prepared.id }));
    const load = vi
      .fn<API["load"]>()
      .mockResolvedValueOnce(initial)
      .mockReturnValueOnce(older.promise)
      .mockReturnValueOnce(newer.promise)
      .mockResolvedValueOnce(latest);
    const api: API = {
      load,
      apply: vi.fn(),
      backup: vi.fn(),
      restore: vi.fn(),
      exportBook: vi.fn(),
      backups: {
        current: vi.fn(async () => prior),
        startExport: vi.fn(),
        startPrepare: vi.fn(async () => prepared),
        status: vi.fn(),
        cancel: vi.fn(),
        confirm,
        exportJson: vi.fn(),
      },
    };
    render(<App api={api} />);
    await screen.findByRole("button", { name: "打开 原书架" });
    fireEvent.click(screen.getByRole("button", { name: "备份书库" }));
    await screen.findByText("恢复成功，正在重新加载书架…");
    fireEvent.click(screen.getByRole("button", { name: "选择备份并校验" }));
    fireEvent.click(await screen.findByRole("button", { name: "恢复此备份" }));
    fireEvent.click(screen.getByRole("button", { name: "确认替换整个书库" }));
    await act(async () => {});
    expect(load).toHaveBeenCalledTimes(3);
    await act(async () => {
      if (outcome === "failure") newer.reject(new Error("新书架读取失败"));
      else newer.resolve(latest);
    });
    await act(async () => older.resolve(library("older", "旧书架")));
    expect(screen.queryByRole("button", { name: "打开 旧书架" })).toBeNull();
    if (outcome === "failure") {
      expect(screen.getByText(/恢复已完成，书架刷新失败：新书架读取失败/)).toBeTruthy();
      expect(screen.getByText("正在打开本地书库…")).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: "重新加载书架" }));
      await act(async () => {});
      expect(load).toHaveBeenCalledTimes(4);
    } else {
      expect(load).toHaveBeenCalledTimes(3);
    }
    expect(screen.getByRole("button", { name: "打开 最新书架" })).toBeTruthy();
    expect(confirm).toHaveBeenCalledExactlyOnceWith(prepared.id, "token", true);
  },
);

function appApi(load: API["load"]): API {
  return {
    load,
    apply: vi.fn(),
    backup: vi.fn(),
    restore: vi.fn(),
    exportBook: vi.fn(),
  };
}

it.each(["success", "failure"] as const)(
  "ignores a late startup %s after the restored shelf is visible",
  async (outcome) => {
    const startup = defer<LibraryState>();
    const confirm = vi.fn(async () => ({ ...prior, id: prepared.id }));
    const load = vi
      .fn<API["load"]>()
      .mockReturnValueOnce(startup.promise)
      .mockResolvedValueOnce(library("restored", "恢复后的书架"));
    const api: API = {
      ...appApi(load),
      backups: {
        current: vi.fn(async () => null),
        startExport: vi.fn(),
        startPrepare: vi.fn(async () => prepared),
        status: vi.fn(),
        cancel: vi.fn(),
        confirm,
        exportJson: vi.fn(),
      },
    };
    render(<App api={api} />);
    expect(screen.getByText("正在打开本地书库…")).toBeTruthy();
    expect(load).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "备份书库" }));
    const choose = await screen.findByRole<HTMLButtonElement>("button", {
      name: "选择备份并校验",
    });
    await act(async () => {});
    expect(choose.disabled).toBe(false);
    fireEvent.click(choose);
    fireEvent.click(await screen.findByRole("button", { name: "恢复此备份" }));
    fireEvent.click(screen.getByRole("button", { name: "确认替换整个书库" }));
    await screen.findByRole("button", { name: "打开 恢复后的书架" });
    expect(load).toHaveBeenCalledTimes(2);

    await act(async () => {
      if (outcome === "failure") startup.reject(new Error("旧启动读取失败"));
      else startup.resolve(library("old", "启动前的书架"));
    });
    expect(screen.queryByRole("button", { name: "打开 启动前的书架" })).toBeNull();
    expect(screen.queryByText("旧启动读取失败")).toBeNull();
    expect(screen.getByRole("button", { name: "打开 恢复后的书架" })).toBeTruthy();
    expect(confirm).toHaveBeenCalledExactlyOnceWith(prepared.id, "token", true);
  },
);

it.each(["success", "failure"] as const)(
  "StrictMode preserves the replayed startup shelf after the cleaned-up first load reports %s",
  async (outcome) => {
    const first = defer<LibraryState>();
    const active = defer<LibraryState>();
    const load = vi
      .fn<API["load"]>()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(active.promise);
    render(
      <React.StrictMode>
        <App api={appApi(load)} />
      </React.StrictMode>,
    );
    expect(load).toHaveBeenCalledTimes(2);
    await act(async () => active.resolve(library("active", "当前启动书架")));
    expect(screen.getByRole("button", { name: "打开 当前启动书架" })).toBeTruthy();

    await act(async () => {
      if (outcome === "failure") first.reject(new Error("已清理的启动请求"));
      else first.resolve(library("inactive", "过期启动书架"));
    });
    expect(screen.queryByRole("button", { name: "打开 过期启动书架" })).toBeNull();
    expect(screen.queryByText("已清理的启动请求")).toBeNull();
    expect(screen.getByRole("button", { name: "打开 当前启动书架" })).toBeTruthy();
  },
);

it("still reports a genuine current startup load failure", async () => {
  const load = vi.fn<API["load"]>().mockRejectedValueOnce(new Error("当前启动读取失败"));
  render(<App api={appApi(load)} />);
  expect(await screen.findByText("当前启动读取失败")).toBeTruthy();
  expect(screen.getByText("正在打开本地书库…")).toBeTruthy();
});
