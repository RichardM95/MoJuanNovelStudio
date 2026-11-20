import "@testing-library/jest-dom/vitest";
import React from "react";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  act,
  cleanup,
} from "@testing-library/react";
import { it, expect, vi, afterEach } from "vitest";
import ExportPanel from "../src/ExportPanel";
afterEach(cleanup);
const preview = {
  id: "p",
  bytes: 3,
  volumes: 1,
  chapters: 1,
  text: "文",
  truncated: false,
  expires_at: Date.now() / 1000 + 600,
  bom: false,
  newline: "crlf",
};
function mount(
  prepare: any = async () => preview,
  save: any = async () => "/new.txt",
) {
  const api = { prepare, save: vi.fn(save), discard: vi.fn(async () => {}) };
  const close = vi.fn();
  render(
    <ExportPanel
      api={api}
      bookId="b"
      volumes={[]}
      chapters={[]}
      onClose={close}
      onPendingChange={() => {}}
    />,
  );
  return { api, close };
}
it("requires preview, then saves only its frozen id and shows success", async () => {
  const { api } = mount();
  expect(screen.queryByRole("button", { name: "保存TXT" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
  await screen.findByText("文");
  fireEvent.click(screen.getByRole("button", { name: "保存TXT" }));
  await screen.findByText("TXT已导出：/new.txt");
  expect(api.save).toHaveBeenCalledWith("p");
  expect(screen.queryByRole("button", { name: "保存TXT" })).toBeNull();
});
it("changing options invalidates preview and a late response", async () => {
  let resolve: any;
  mount(() => new Promise((r) => (resolve = r)));
  fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
  fireEvent.click(screen.getByLabelText("UTF-8 BOM"));
  resolve(preview);
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "生成预览" })).not.toBeDisabled(),
  );
  expect(screen.queryByRole("button", { name: "保存TXT" })).toBeNull();
});
it("cancelled save retains preview but failed save requires new preview", async () => {
  const { api } = mount(undefined, async () => null);
  fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
  fireEvent.click(await screen.findByRole("button", { name: "保存TXT" }));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "保存TXT" })).not.toBeDisabled(),
  );
  api.save.mockRejectedValueOnce(Error("写入失败"));
  fireEvent.click(screen.getByRole("button", { name: "保存TXT" }));
  await screen.findByText(/结果未确认/);
  expect(screen.queryByRole("button", { name: "保存TXT" })).toBeNull();
});
it("does not double-save or close while native save is pending", async () => {
  let done: any;
  const { api, close } = mount(undefined, () => new Promise((r) => (done = r)));
  fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
  const save = await screen.findByRole("button", { name: "保存TXT" });
  fireEvent.click(save);
  fireEvent.click(save);
  fireEvent.keyDown(window, { key: "Escape" });
  expect(api.save).toHaveBeenCalledTimes(1);
  expect(close).not.toHaveBeenCalled();
  done("/ok");
  await screen.findByText("TXT已导出：/ok");
});
it("discards a late preview on unmount", async () => {
  let done: any;
  const { api } = mount(() => new Promise((r) => (done = r)));
  fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
  cleanup();
  done(preview);
  await waitFor(() => expect(api.discard).toHaveBeenCalledWith("p"));
});
it("expired preview cannot start native save", async () => {
  const { api } = mount(async () => ({ ...preview, expires_at: 1 }));
  fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
  fireEvent.click(await screen.findByRole("button", { name: "保存TXT" }));
  await screen.findByText("预览已过期，请重新生成");
  expect(api.save).not.toHaveBeenCalled();
});
for (const result of [undefined, {}, ""]) {
  it(`unknown save response ${JSON.stringify(result)} never claims success or retries`, async () => {
    const { api } = mount(undefined, async () => result);
    fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
    fireEvent.click(await screen.findByRole("button", { name: "保存TXT" }));
    await screen.findByText(/导出结果未确认/);
    expect(screen.queryByText(/TXT已导出/)).toBeNull();
    expect(screen.queryByRole("button", { name: "保存TXT" })).toBeNull();
    expect(api.save).toHaveBeenCalledTimes(1);
  });
}

const limit = 16 * 1024 * 1024;
const warning = /超过16 MiB.*不能作为同一个TXT文件整体再次导入/;
const large = () => ({
  ...preview,
  bytes: limit + 1,
  truncated: true,
  expires_at: Date.now() / 1000 + 60,
});
const generate = () =>
  fireEvent.click(screen.getByRole("button", { name: "生成预览" }));
const findPreview = () => screen.findByLabelText("导出内容预览");

it("explains the export cap without promising the import will succeed at 16 MiB", async () => {
  mount(async () => ({ ...large(), bytes: limit }));
  expect(screen.getByText(/导出最多32 MiB/)).toBeInTheDocument();
  expect(screen.getByText(/结构.*历史.*SQLite/)).toBeInTheDocument();
  generate();
  await findPreview();
  expect(screen.queryByText(warning)).toBeNull();
  expect(screen.queryByText(/可以导入|能再次导入/)).toBeNull();
});

it("uses the current exact full byte count, not short shown text, and still saves", async () => {
  const { api } = mount(async () => large());
  generate();
  expect(await screen.findByText(warning)).toBeInTheDocument();
  expect(screen.getByLabelText("导出内容预览")).toHaveTextContent("文");
  const save = screen.getByRole("button", { name: "保存TXT" });
  expect(save).not.toBeDisabled();
  fireEvent.click(save);
  await screen.findByText("TXT已导出：/new.txt");
  expect(api.save).toHaveBeenCalledExactlyOnceWith("p");
  expect(screen.queryByText(warning)).toBeNull();
});

it.each([
  ["unknown count", { bytes: undefined }],
  ["nonfinite count", { bytes: Infinity }],
  ["fractional count", { bytes: limit + 0.5 }],
  ["impossible count", { bytes: 2 * limit + 1 }],
  ["wrong BOM", { bom: true }],
  ["wrong newline", { newline: "lf" }],
  ["unknown expiry", { expires_at: Infinity }],
  ["expired", { expires_at: 1 }],
])("does not infer reimport warning from %s metadata", async (_, patch) => {
  mount(async () => ({ ...large(), ...patch }));
  generate();
  await findPreview();
  expect(screen.queryByText(warning)).toBeNull();
});

it("withdraws the advice after expiry without introducing a save action", async () => {
  vi.useFakeTimers();
  try {
    const { api } = mount(async () => ({
      ...large(),
      expires_at: Date.now() / 1000 + 1,
    }));
    await act(async () => {
      generate();
    });
    expect(screen.getByText(warning)).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.queryByText(warning)).toBeNull();
    expect(api.save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "保存TXT" }));
    expect(screen.getByText("预览已过期，请重新生成")).toBeInTheDocument();
    expect(api.save).not.toHaveBeenCalled();
  } finally {
    vi.useRealTimers();
  }
});

it("removes advice on BOM/newline edits and ignores a large late response", async () => {
  const next = vi
    .fn()
    .mockImplementationOnce(async () => large())
    .mockImplementationOnce(
      () =>
        new Promise((r) => {
          resolve = r;
        }),
    );
  let resolve!: (p: ReturnType<typeof large>) => void;
  mount(next);
  generate();
  await screen.findByText(warning);
  fireEvent.click(screen.getByLabelText("UTF-8 BOM"));
  expect(screen.queryByText(warning)).toBeNull();
  generate();
  expect(screen.queryByText(warning)).toBeNull();
  fireEvent.change(screen.getByLabelText("换行"), { target: { value: "lf" } });
  await act(async () => resolve({ ...large(), bom: true }));
  expect(screen.queryByText(warning)).toBeNull();
  expect(screen.queryByLabelText("导出内容预览")).toBeNull();
});

it("does not leave size advice visible after switching book or changing selected target", async () => {
  const api = {
    prepare: vi.fn(async () => large()),
    save: vi.fn(async () => "/new.txt"),
    discard: vi.fn(async () => {}),
  };
  const onClose = vi.fn();
  const onPendingChange = vi.fn();
  const volumes = [
    { id: "v1", book_id: "b", title: "一卷", sort_order: 0, deleted_at: null },
    { id: "v2", book_id: "b", title: "二卷", sort_order: 1, deleted_at: null },
  ];
  const ui = (bookId: string) => (
    <ExportPanel
      api={api}
      bookId={bookId}
      volumes={volumes}
      chapters={[]}
      onClose={onClose}
      onPendingChange={onPendingChange}
    />
  );
  const { rerender } = render(ui("b"));
  fireEvent.change(screen.getByLabelText("导出范围"), {
    target: { value: "volume" },
  });
  generate();
  await screen.findByText(warning);
  expect(api.prepare).toHaveBeenCalledWith(
    "b",
    expect.objectContaining({ target_id: "v1" }),
  );
  fireEvent.change(screen.getByLabelText("选择卷章"), {
    target: { value: "v2" },
  });
  expect(screen.queryByText(warning)).toBeNull();
  generate();
  await screen.findByText(warning);
  rerender(ui("another"));
  expect(screen.queryByText(warning)).toBeNull();
});
