import React from "react";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  cleanup,
  act,
} from "@testing-library/react";
import { it, expect, vi, afterEach } from "vitest";
import BackupPanel from "../src/BackupPanel";
import type { BackupAPI, BackupJob } from "../src/backupTypes";
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
const job = (status: BackupJob["status"] = "prepared"): BackupJob => ({
  id: "job",
  kind: "restore",
  status,
  progress: { phase: "validate", completed: 4, total: 8 },
  cancel_requested: false,
  created_at: 1,
  updated_at: 2,
  expires_at: 9999999999,
  source_name: "书库.sqlite",
  preview: {
    format: "sqlite",
    books: 2,
    volumes: 3,
    chapters: 4,
    history: 5,
    materials: 6,
    version_labels: 7,
    canonical_bytes: 1000,
  },
  confirmation_token: "token",
  result:
    status === "done"
      ? {
          output_path: null,
          recovery_path: "/safe/before.sqlite",
          summary: null,
        }
      : null,
  error_code: null,
  error_message: null,
});
function setup(initial: BackupJob | null = null, overrides: Partial<BackupAPI> = {}) {
  const api: BackupAPI = {
    current: vi.fn(async () => initial),
    startExport: vi.fn(async () => ({
      ...job("running"),
      kind: "export_sqlite" as const,
    })),
    startPrepare: vi.fn(async () => job()),
    status: vi.fn(async () => job()),
    cancel: vi.fn(async () => job("cancelled")),
    confirm: vi.fn(async () => job("done")),
    exportJson: vi.fn(async () => "/backup.json"),
    ...overrides,
  };
  const onClose = vi.fn(),
    onPendingChange = vi.fn(),
    onRestored = vi.fn(async () => {});
  const view = render(
    <BackupPanel
      api={api}
      onClose={onClose}
      onPendingChange={onPendingChange}
      onRestored={onRestored}
    />,
  );
  return { api, onClose, onPendingChange, onRestored, view };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
it("previews validated counts and requires a separate explicit confirmation", async () => {
  const { api, onPendingChange } = setup();
  await waitFor(() =>
    expect(
      (
        screen.getByRole("button", {
          name: "选择备份并校验",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false),
  );
  fireEvent.click(screen.getByRole("button", { name: "选择备份并校验" }));
  await screen.findByText("书库.sqlite");
  expect(screen.getByText(/计数包含回收项/)).toBeTruthy();
  expect(api.confirm).not.toHaveBeenCalled();
  expect(onPendingChange).toHaveBeenLastCalledWith(true);
  fireEvent.click(screen.getByRole("button", { name: "恢复此备份" }));
  expect(api.confirm).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "确认替换整个书库" }));
  await waitFor(() => expect(api.confirm).toHaveBeenCalledWith("job", "token", true));
  await screen.findByText("/safe/before.sqlite");
});
it("protects pending work from closing and cancels a prepared candidate before allowing close", async () => {
  const { onClose, api } = setup();
  await waitFor(() =>
    expect(
      (
        screen.getByRole("button", {
          name: "选择备份并校验",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false),
  );
  fireEvent.click(screen.getByRole("button", { name: "选择备份并校验" }));
  await screen.findByText("书库.sqlite");
  fireEvent.click(screen.getByRole("button", { name: "关闭备份面板" }));
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(onClose).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "放弃候选" }));
  await screen.findByText("已取消");
  fireEvent.click(screen.getByRole("button", { name: "关闭备份面板" }));
  expect(api.cancel).toHaveBeenCalledWith("job");
  expect(onClose).toHaveBeenCalledTimes(1);
});
it("keeps restore success distinct from reload failure and retries reload only", async () => {
  const { api, onRestored } = setup();
  onRestored.mockRejectedValueOnce(new Error("读取失败"));
  await waitFor(() =>
    expect(
      (
        screen.getByRole("button", {
          name: "选择备份并校验",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false),
  );
  fireEvent.click(screen.getByRole("button", { name: "选择备份并校验" }));
  await screen.findByText("书库.sqlite");
  fireEvent.click(screen.getByRole("button", { name: "恢复此备份" }));
  fireEvent.click(screen.getByRole("button", { name: "确认替换整个书库" }));
  await screen.findByText(/恢复已完成，书架刷新失败/);
  expect(screen.getByText("/safe/before.sqlite")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "重新加载书架" }));
  await waitFor(() => expect(onRestored).toHaveBeenCalledTimes(2));
  expect(api.confirm).toHaveBeenCalledTimes(1);
});
it("treats native file dialog cancellation as no job", async () => {
  const { api, onPendingChange } = setup();
  vi.mocked(api.startPrepare).mockResolvedValue(null);
  await waitFor(() =>
    expect(
      (
        screen.getByRole("button", {
          name: "选择备份并校验",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false),
  );
  fireEvent.click(screen.getByRole("button", { name: "选择备份并校验" }));
  await waitFor(() => expect(onPendingChange).toHaveBeenLastCalledWith(false));
  expect(api.status).not.toHaveBeenCalled();
  expect(api.confirm).not.toHaveBeenCalled();
});
it("keeps a cancellation reply when the user clicks before the running job's poll effect", async () => {
  const { api } = setup();
  vi.mocked(api.startPrepare).mockResolvedValue(job("running"));
  vi.mocked(api.cancel).mockResolvedValue({
    ...job("running"),
    cancel_requested: true,
  });
  // Only the action reply can acknowledge cancellation in this test.
  vi.mocked(api.status).mockReturnValue(deferred<BackupJob>().promise);
  const prepare = screen.getByRole<HTMLButtonElement>("button", {
    name: "选择备份并校验",
  });
  await waitFor(() => expect(prepare.disabled).toBe(false));

  // Click as soon as React commits the button, without waitFor yielding to the poll effect.
  const observer = new MutationObserver(() => {
    const cancel = screen.queryByRole<HTMLButtonElement>("button", {
      name: "请求取消",
    });
    if (cancel && !cancel.disabled) {
      observer.disconnect();
      fireEvent.click(cancel);
    }
  });
  observer.observe(document.body, {
    childList: true,
    subtree: true,
    attributes: true,
  });
  try {
    fireEvent.click(prepare);
    await waitFor(() => expect(api.cancel).toHaveBeenCalledTimes(1));
    expect(api.cancel).toHaveBeenCalledWith("job");
    await screen.findByText(/已请求取消/);
    expect(screen.queryByRole("heading", { name: "已取消" })).toBeNull();
  } finally {
    observer.disconnect();
  }
});
it("ignores an old poll after cancellation, then polls for a terminal result before allowing close", async () => {
  vi.useFakeTimers();
  const oldPoll = deferred<BackupJob>();
  const cancellation = deferred<BackupJob>();
  const { api, onClose, onPendingChange } = setup(job("running"));
  vi.mocked(api.status)
    .mockReturnValueOnce(oldPoll.promise)
    .mockResolvedValueOnce(job("cancelled"));
  vi.mocked(api.cancel).mockReturnValueOnce(cancellation.promise);
  await act(async () => {});
  await act(async () => {
    vi.advanceTimersByTime(1000);
  });
  expect(api.status).toHaveBeenCalledTimes(1);
  expect(api.status).toHaveBeenCalledWith("job");

  const cancel = screen.getByRole<HTMLButtonElement>("button", {
    name: "请求取消",
  });
  const refresh = screen.getByRole("button", { name: "刷新作业状态" });
  const close = screen.getByRole("button", { name: "关闭备份面板" });
  act(() => {
    fireEvent.click(cancel);
    fireEvent.click(cancel);
    fireEvent.click(refresh);
  });
  expect(cancel.disabled).toBe(true);
  expect(api.cancel).toHaveBeenCalledTimes(1);
  expect(api.cancel).toHaveBeenCalledWith("job");
  await act(async () => {
    vi.advanceTimersByTime(5000);
  });
  expect(api.status).toHaveBeenCalledTimes(1);

  await act(async () => {
    cancellation.resolve({ ...job("running"), cancel_requested: true });
  });
  expect(screen.getByText(/已请求取消/)).toBeTruthy();
  expect(screen.queryByRole("heading", { name: "已取消" })).toBeNull();
  expect(cancel.disabled).toBe(true);
  expect(onPendingChange).toHaveBeenLastCalledWith(true);
  fireEvent.click(close);
  expect(onClose).not.toHaveBeenCalled();

  await act(async () => {
    oldPoll.resolve(job("running"));
  });
  expect(screen.getByText(/已请求取消/)).toBeTruthy();
  expect(cancel.disabled).toBe(true);
  await act(async () => {
    vi.advanceTimersByTime(1000);
  });
  expect(api.status).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("heading", { name: "已取消" })).toBeTruthy();
  expect(onPendingChange).toHaveBeenLastCalledWith(false);
  fireEvent.click(close);
  expect(onClose).toHaveBeenCalledTimes(1);
});

it("does not resume polling or reload the library for replies arriving after unmount", async () => {
  vi.useFakeTimers();
  const oldPoll = deferred<BackupJob>();
  const cancellation = deferred<BackupJob>();
  const { api, onPendingChange, onRestored, view } = setup(job("running"));
  vi.mocked(api.status).mockReturnValueOnce(oldPoll.promise);
  vi.mocked(api.cancel).mockReturnValueOnce(cancellation.promise);
  await act(async () => {});
  await act(async () => {
    vi.advanceTimersByTime(1000);
  });
  fireEvent.click(screen.getByRole("button", { name: "请求取消" }));
  expect(api.status).toHaveBeenCalledTimes(1);
  expect(api.cancel).toHaveBeenCalledTimes(1);

  view.unmount();
  expect(onPendingChange).toHaveBeenLastCalledWith(false);
  onPendingChange.mockClear();
  await act(async () => {
    oldPoll.resolve(job("running"));
    cancellation.resolve(job("done"));
  });
  await act(async () => {
    vi.advanceTimersByTime(5000);
  });
  expect(api.status).toHaveBeenCalledTimes(1);
  expect(onRestored).not.toHaveBeenCalled();
  expect(onPendingChange).not.toHaveBeenCalled();
});

it("recovers the existing prepared job on mount without opening a file dialog", async () => {
  const { api, onPendingChange } = setup(job());
  await screen.findByText("书库.sqlite");
  expect(api.current).toHaveBeenCalledTimes(1);
  expect(api.startPrepare).not.toHaveBeenCalled();
  expect(onPendingChange).toHaveBeenLastCalledWith(true);
});
it("recovers an unknown start outcome through current and keeps closing protected while disconnected", async () => {
  const { api, onClose } = setup();
  await waitFor(() =>
    expect(
      (
        screen.getByRole("button", {
          name: "选择备份并校验",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false),
  );
  vi.mocked(api.startPrepare).mockRejectedValueOnce(new Error("回执丢失"));
  vi.mocked(api.current).mockRejectedValueOnce(new Error("离线"));
  fireEvent.click(screen.getByRole("button", { name: "选择备份并校验" }));
  await screen.findByRole("button", { name: "刷新当前任务" });
  fireEvent.click(screen.getByRole("button", { name: "关闭备份面板" }));
  expect(onClose).not.toHaveBeenCalled();
  vi.mocked(api.current).mockResolvedValueOnce(job("running"));
  fireEvent.click(screen.getByRole("button", { name: "刷新当前任务" }));
  await screen.findByRole("heading", { name: "处理中" });
  expect(api.startPrepare).toHaveBeenCalledTimes(1);
});
it("stops automatic polling on status errors and manual refresh can reveal committed success", async () => {
  vi.useFakeTimers();
  const { api, onRestored } = setup(job("running"));
  await act(async () => {});
  vi.mocked(api.status).mockRejectedValueOnce(new Error("连接断开"));
  await act(async () => {
    vi.advanceTimersByTime(1000);
  });
  expect(screen.getByText(/自动刷新已暂停/)).toBeTruthy();
  await act(async () => {
    vi.advanceTimersByTime(5000);
  });
  expect(api.status).toHaveBeenCalledTimes(1);
  vi.mocked(api.status).mockResolvedValueOnce(job("done"));
  fireEvent.click(screen.getByRole("button", { name: "刷新作业状态" }));
  await act(async () => {});
  expect(screen.getByText("/safe/before.sqlite")).toBeTruthy();
  expect(onRestored).toHaveBeenCalledTimes(1);
});
it.each([
  ["running", "请求取消"],
  ["prepared", "放弃候选"],
] as const)(
  "requires a successful status read before another cancel of a %s job after an uncertain reply",
  async (status, label) => {
    vi.useFakeTimers();
    const { api } = setup(job(status));
    await act(async () => {});
    const cancel = screen.getByRole<HTMLButtonElement>("button", { name: label });
    const refresh = screen.getByRole("button", { name: "刷新作业状态" });
    vi.mocked(api.cancel).mockRejectedValueOnce(new Error("取消回执丢失"));

    fireEvent.click(cancel);
    await act(async () => {});
    expect(screen.getByRole("alert").textContent).toContain("取消回执丢失");
    expect(screen.getByRole("status").textContent).toContain("作业结果未确认");
    fireEvent.click(cancel);
    expect(api.cancel).toHaveBeenCalledTimes(1);
    expect(cancel.disabled).toBe(true);
    await act(async () => {
      vi.advanceTimersByTime(5000);
    });
    expect(api.cancel).toHaveBeenCalledTimes(1);
    expect(api.status).not.toHaveBeenCalled();

    vi.mocked(api.status).mockRejectedValueOnce(new Error("仍无法读取状态"));
    fireEvent.click(refresh);
    await act(async () => {});
    expect(screen.getByRole("alert").textContent).toContain("仍无法读取状态");
    fireEvent.click(cancel);
    expect(api.cancel).toHaveBeenCalledTimes(1);
    expect(cancel.disabled).toBe(true);

    vi.mocked(api.status).mockResolvedValueOnce(job(status));
    fireEvent.click(refresh);
    await act(async () => {});
    expect(api.status).toHaveBeenCalledTimes(2);
    expect(api.status).toHaveBeenLastCalledWith("job");
    expect(api.cancel).toHaveBeenCalledTimes(1);
    expect(cancel.disabled).toBe(false);
    fireEvent.click(cancel);
    await act(async () => {});
    expect(api.cancel).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("heading", { name: "已取消" })).toBeTruthy();
  },
);
it("never offers cancel during commit and prevents closing", async () => {
  const { api, onClose } = setup(job("committing"));
  await screen.findByText("正在提交，不能取消");
  expect(screen.queryByRole("button", { name: "请求取消" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "关闭备份面板" }));
  expect(onClose).not.toHaveBeenCalled();
  expect(api.cancel).not.toHaveBeenCalled();
});
it("keeps confirmation failure uncertain and does not retry restoring when status reveals done", async () => {
  const { api } = setup(job());
  await screen.findByText("书库.sqlite");
  vi.mocked(api.confirm).mockRejectedValueOnce(new Error("提交回执丢失"));
  fireEvent.click(screen.getByRole("button", { name: "恢复此备份" }));
  fireEvent.click(screen.getByRole("button", { name: "确认替换整个书库" }));
  await screen.findByText("提交回执丢失");
  expect(
    (
      screen.getByRole("button", {
        name: "确认替换整个书库",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
  vi.mocked(api.status).mockResolvedValue(job("done"));
  fireEvent.click(screen.getByRole("button", { name: "刷新作业状态" }));
  await screen.findByText("/safe/before.sqlite");
  expect(api.confirm).toHaveBeenCalledTimes(1);
});
it("disables expired candidates and requires cleanup instead of restoring", async () => {
  const { api } = setup({ ...job(), expires_at: 1 });
  await screen.findByText("书库.sqlite");
  expect(
    (screen.getByRole("button", { name: "恢复此备份" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(api.confirm).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "放弃候选" }));
  await screen.findByText("已取消");
});
it("checks expiry again at the final confirmation even if the UI has not rerendered", async () => {
  vi.useFakeTimers();
  const deadline = Math.floor(Date.now() / 1000) + 2;
  const { api } = setup({ ...job(), expires_at: deadline });
  await act(async () => {});
  fireEvent.click(screen.getByRole("button", { name: "恢复此备份" }));
  vi.setSystemTime((deadline + 1) * 1000);
  fireEvent.click(screen.getByRole("button", { name: "确认替换整个书库" }));
  expect(api.confirm).not.toHaveBeenCalled();
  expect(screen.getByRole("alert").textContent).toContain("过期");
});
it.each([
  ["waiting", "等待处理"],
  ["prepare", "准备候选"],
  ["prepared", "等待确认"],
  ["awaiting_confirmation", "等待确认"],
  ["validate", "校验备份"],
  ["import", "导入数据"],
  ["export", "导出备份"],
  ["recovery", "保存恢复前留底"],
  ["commit", "提交恢复"],
  ["future_unknown_phase", "处理中"],
])("renders phase %s as a fixed human-readable label", async (phase, label) => {
  setup({ ...job(), progress: { phase, completed: 4, total: 8 } });
  await screen.findByText(`阶段：${label} · 已处理 4 / 8`);
  expect(screen.queryByText(new RegExp(`阶段：${phase} ·`))).toBeNull();
});

const stopLabel = "停止本次状态等待";
const currentLabel = "刷新当前任务";

it("stops a stalled initial read without assuming no job, then trusts only the new current reply", async () => {
  const initial = deferred<BackupJob | null>();
  const retry = deferred<BackupJob | null>();
  const current = vi.fn<BackupAPI["current"]>()
    .mockReturnValueOnce(initial.promise)
    .mockReturnValueOnce(retry.promise);
  const { api, onClose, onPendingChange, onRestored } = setup(null, { current });
  expect(current).toHaveBeenCalledTimes(1);
  const stop = screen.getByRole("button", { name: stopLabel });
  const close = screen.getByRole("button", { name: "关闭备份面板" });
  const create = screen.getByRole<HTMLButtonElement>("button", { name: "创建SQLite备份" });
  act(() => {
    fireEvent.click(stop);
    fireEvent.click(stop);
    fireEvent.click(close);
    fireEvent.click(create);
  });
  expect(screen.getByRole("status").textContent).toMatch(/后台.*可能仍/);
  expect(onClose).not.toHaveBeenCalled();
  expect(create.disabled).toBe(true);
  expect(onPendingChange).toHaveBeenLastCalledWith(true);
  expect(api.cancel).not.toHaveBeenCalled();
  expect(api.startExport).not.toHaveBeenCalled();

  const readAgain = screen.getByRole("button", { name: currentLabel });
  act(() => {
    fireEvent.click(readAgain);
    fireEvent.click(readAgain);
  });
  expect(current).toHaveBeenCalledTimes(2);
  await act(async () => initial.resolve(job("done")));
  expect(screen.getByRole("button", { name: stopLabel })).toBeTruthy();
  expect(screen.queryByRole("heading", { name: "已完成" })).toBeNull();
  expect(onRestored).not.toHaveBeenCalled();
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(onClose).not.toHaveBeenCalled();
  expect(create.disabled).toBe(true);

  await act(async () => retry.resolve(null));
  expect(onPendingChange).toHaveBeenLastCalledWith(false);
  expect(create.disabled).toBe(false);
  fireEvent.click(close);
  expect(onClose).toHaveBeenCalledTimes(1);
});

it("stops a stalled manual status read without resubmitting restore and ignores its late rejection", async () => {
  vi.useFakeTimers();
  const write = deferred<BackupJob>();
  const first = deferred<BackupJob>();
  const retry = deferred<BackupJob>();
  const { api, onClose, onPendingChange, onRestored } = setup(job());
  await act(async () => {});
  vi.mocked(api.confirm).mockReturnValueOnce(write.promise);
  vi.mocked(api.status).mockReturnValueOnce(first.promise).mockReturnValueOnce(retry.promise);
  fireEvent.click(screen.getByRole("button", { name: "恢复此备份" }));
  const confirm = screen.getByRole<HTMLButtonElement>("button", { name: "确认替换整个书库" });
  fireEvent.click(confirm);
  expect(api.confirm).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("button", { name: stopLabel })).toBeNull();
  expect(confirm.disabled).toBe(true);
  await act(async () => write.reject(new Error("提交回执丢失")));

  const refresh = screen.getByRole("button", { name: "刷新作业状态" });
  fireEvent.click(refresh);
  expect(api.status).toHaveBeenCalledTimes(1);
  expect(api.status).toHaveBeenLastCalledWith("job");
  fireEvent.click(screen.getByRole("button", { name: stopLabel }));
  expect(screen.getByRole("status").textContent).toContain("不会重新提交恢复");
  expect(confirm.disabled).toBe(true);
  fireEvent.click(confirm);
  fireEvent.click(screen.getByRole("button", { name: "关闭备份面板" }));
  expect(onClose).not.toHaveBeenCalled();
  expect(onPendingChange).toHaveBeenLastCalledWith(true);
  expect(api.confirm).toHaveBeenCalledTimes(1);
  expect(api.cancel).not.toHaveBeenCalled();

  fireEvent.click(refresh);
  await act(async () => first.reject(new Error("迟到的断线")));
  expect(api.status).toHaveBeenCalledTimes(2);
  expect(screen.queryByText("迟到的断线")).toBeNull();
  expect(screen.getByRole("button", { name: stopLabel })).toBeTruthy();
  expect(confirm.disabled).toBe(true);
  await act(async () => retry.resolve(job("done")));
  expect(screen.getByText("/safe/before.sqlite")).toBeTruthy();
  expect(onRestored).toHaveBeenCalledTimes(1);
  expect(onPendingChange).toHaveBeenLastCalledWith(false);
  expect(api.confirm).toHaveBeenCalledTimes(1);
});

it.each(["no job", "new job"] as const)(
  "stops only the fallback read after start rejects, and trusts fresh current: %s",
  async (outcome) => {
    vi.useFakeTimers();
    const start = deferred<BackupJob | null>();
    const fallback = deferred<BackupJob | null>();
    const retry = deferred<BackupJob | null>();
    const { api, onClose, onPendingChange } = setup(job("cancelled"));
    await act(async () => {});
    vi.mocked(api.startPrepare).mockReturnValueOnce(start.promise);
    vi.mocked(api.current).mockReturnValueOnce(fallback.promise).mockReturnValueOnce(retry.promise);
    const prepare = screen.getByRole<HTMLButtonElement>("button", { name: "选择备份并校验" });
    fireEvent.click(prepare);
    expect(api.startPrepare).toHaveBeenCalledTimes(1);
    expect(api.current).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: stopLabel })).toBeNull();
    expect(prepare.disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "关闭备份面板" }));
    expect(onClose).not.toHaveBeenCalled();

    await act(async () => start.reject(new Error("启动回执丢失")));
    expect(api.current).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: stopLabel }));
    expect(onPendingChange).toHaveBeenLastCalledWith(true);
    expect(prepare.disabled).toBe(true);
    expect(screen.queryByRole("button", { name: "刷新作业状态" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: currentLabel }));
    expect(api.current).toHaveBeenCalledTimes(3);
    await act(async () => fallback.resolve(null));
    expect(prepare.disabled).toBe(true);
    expect(screen.getByRole("button", { name: stopLabel })).toBeTruthy();
    expect(api.startPrepare).toHaveBeenCalledTimes(1);
    expect(api.cancel).not.toHaveBeenCalled();

    const next = outcome === "no job" ? null : { ...job("running"), id: "new-job" };
    await act(async () => retry.resolve(next));
    if (!next) {
      expect(screen.queryByRole("region", { name: "备份作业" })).toBeNull();
      expect(onPendingChange).toHaveBeenLastCalledWith(false);
      expect(prepare.disabled).toBe(false);
      fireEvent.click(screen.getByRole("button", { name: "关闭备份面板" }));
      expect(onClose).toHaveBeenCalledTimes(1);
      return;
    }
    expect(screen.getByRole("heading", { name: "处理中" })).toBeTruthy();
    expect(onPendingChange).toHaveBeenLastCalledWith(true);
    vi.mocked(api.status).mockResolvedValueOnce({ ...job("failed"), id: "new-job" });
    fireEvent.click(screen.getByRole("button", { name: "刷新作业状态" }));
    await act(async () => {});
    expect(api.status).toHaveBeenCalledWith("new-job");
    expect(screen.getByRole("heading", { name: "失败" })).toBeTruthy();
    expect(onPendingChange).toHaveBeenLastCalledWith(false);
    expect(prepare.disabled).toBe(false);
  },
);

it("keeps a known running job uncertain after stopping status and ignores an older result after a successful retry", async () => {
  vi.useFakeTimers();
  const old = deferred<BackupJob>();
  const { api, onPendingChange, onRestored } = setup(job("running"));
  await act(async () => {});
  vi.mocked(api.status)
    .mockReturnValueOnce(old.promise)
    .mockResolvedValueOnce(job("done"));
  const refresh = screen.getByRole("button", { name: "刷新作业状态" });
  const cancel = screen.getByRole<HTMLButtonElement>("button", { name: "请求取消" });
  fireEvent.click(refresh);
  fireEvent.click(screen.getByRole("button", { name: stopLabel }));
  expect(cancel.disabled).toBe(true);
  fireEvent.click(cancel);
  expect(api.cancel).not.toHaveBeenCalled();
  expect(onPendingChange).toHaveBeenLastCalledWith(true);
  await act(async () => vi.advanceTimersByTime(5000));
  expect(api.status).toHaveBeenCalledTimes(1);

  fireEvent.click(refresh);
  await act(async () => {});
  expect(screen.getByRole("heading", { name: "已完成" })).toBeTruthy();
  expect(onRestored).toHaveBeenCalledTimes(1);
  await act(async () => old.resolve(job("running")));
  expect(screen.getByRole("heading", { name: "已完成" })).toBeTruthy();
  expect(onRestored).toHaveBeenCalledTimes(1);
  expect(onPendingChange).toHaveBeenLastCalledWith(false);
});

it("detaches a foreground current read on unmount without publishing late results", async () => {
  const read = deferred<BackupJob | null>();
  const { view, onPendingChange, onRestored } = setup(null, {
    current: vi.fn(() => read.promise),
  });
  expect(screen.getByRole("button", { name: stopLabel })).toBeTruthy();
  view.unmount();
  expect(onPendingChange).toHaveBeenLastCalledWith(false);
  onPendingChange.mockClear();
  await act(async () => read.resolve(job("done")));
  expect(onPendingChange).not.toHaveBeenCalled();
  expect(onRestored).not.toHaveBeenCalled();
});

it.each(["success", "failure"] as const)(
  "preserves shelf reload %s while a rejected start's current read is stopped and retried",
  async (outcome) => {
    const prior = { ...job("done"), id: "prior-restore" };
    const shelf = deferred<void>();
    const fallback = deferred<BackupJob | null>();
    const retry = deferred<BackupJob | null>();
    const { api, onPendingChange, onRestored } = setup(prior);
    onRestored.mockReturnValueOnce(shelf.promise);
    await act(async () => {});
    expect(screen.getByText("恢复成功，正在重新加载书架…")).toBeTruthy();

    vi.mocked(api.startExport).mockRejectedValueOnce(new Error("启动回执丢失"));
    vi.mocked(api.current).mockReturnValueOnce(fallback.promise).mockReturnValueOnce(retry.promise);
    fireEvent.click(screen.getByRole("button", { name: "创建SQLite备份" }));
    await act(async () => {});
    expect(api.current).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: stopLabel }));
    fireEvent.click(screen.getByRole("button", { name: currentLabel }));

    await act(async () => {
      if (outcome === "failure") shelf.reject(new Error("读取书架失败"));
      else shelf.resolve();
    });
    expect(api.current).toHaveBeenCalledTimes(3);
    expect(onPendingChange).toHaveBeenLastCalledWith(true);
    await act(async () => retry.resolve(prior));
    expect(screen.getByText("书库恢复已完成。")).toBeTruthy();
    expect(screen.queryByText("恢复成功，正在重新加载书架…")).toBeNull();
    expect(onRestored).toHaveBeenCalledTimes(1);
    expect(api.startExport).toHaveBeenCalledTimes(1);
    expect(api.confirm).not.toHaveBeenCalled();
    expect(onPendingChange).toHaveBeenLastCalledWith(false);

    await act(async () => fallback.resolve(prior));
    expect(onRestored).toHaveBeenCalledTimes(1);
    if (outcome === "failure") {
      expect(screen.getByRole("alert").textContent).toContain("读取书架失败");
      const reload = screen.getByRole("button", { name: "重新加载书架" });
      const manual = deferred<void>();
      onRestored.mockReturnValueOnce(manual.promise);
      act(() => {
        fireEvent.click(reload);
        fireEvent.click(reload);
      });
      expect(onRestored).toHaveBeenCalledTimes(2);
      await act(async () => manual.resolve());
      expect(screen.queryByRole("alert")).toBeNull();
      expect(screen.queryByText("恢复成功，正在重新加载书架…")).toBeNull();
    }
    expect(screen.queryByRole("button", { name: "重新加载书架" })).toBeNull();
  },
);

it("ignores late shelf errors after a different completed restore takes ownership", async () => {
  const prior = { ...job("done"), id: "prior-restore" };
  const next = { ...job(), id: "next-restore" };
  const oldShelf = deferred<void>();
  const newShelf = deferred<void>();
  const { api, onRestored } = setup(prior);
  onRestored.mockReturnValueOnce(oldShelf.promise).mockReturnValueOnce(newShelf.promise);
  vi.mocked(api.startPrepare).mockResolvedValueOnce(next);
  vi.mocked(api.confirm).mockResolvedValueOnce({ ...job("done"), id: next.id });
  await act(async () => {});
  expect(onRestored).toHaveBeenCalledTimes(1);

  fireEvent.click(screen.getByRole("button", { name: "选择备份并校验" }));
  await act(async () => {});
  fireEvent.click(screen.getByRole("button", { name: "恢复此备份" }));
  fireEvent.click(screen.getByRole("button", { name: "确认替换整个书库" }));
  await act(async () => {});
  expect(api.confirm).toHaveBeenCalledExactlyOnceWith(next.id, "token", true);
  expect(onRestored).toHaveBeenCalledTimes(2);
  await act(async () => oldShelf.reject(new Error("旧书架读取失败")));
  expect(screen.queryByText(/旧书架读取失败/)).toBeNull();
  expect(screen.getByText("恢复成功，正在重新加载书架…")).toBeTruthy();
  await act(async () => newShelf.reject(new Error("新书架读取失败")));
  expect(screen.getByRole("alert").textContent).toContain("新书架读取失败");
  expect(screen.queryByText("恢复成功，正在重新加载书架…")).toBeNull();
  expect(screen.getByRole("button", { name: "重新加载书架" })).toBeTruthy();
});
