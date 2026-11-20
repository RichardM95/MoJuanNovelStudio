import "@testing-library/jest-dom/vitest";
import React from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import App from "../src/App";
import type { API, LibraryState } from "../src/types";

afterEach(cleanup);
const BASE = "原稿\r\n灯";
const DRAFT = "决意🙂\n送灯";
const NEWER = "后来新稿\n仍未保存";
const initial: LibraryState = {
  books: [{ id: "b", title: "潮", author: "", deleted_at: null }],
  volumes: [
    { id: "v", book_id: "b", title: "归途", sort_order: 0, deleted_at: null },
  ],
  chapters: [
    {
      id: "c",
      volume_id: "v",
      title: "归舟",
      body: BASE,
      revision: 3,
      sort_order: 0,
      deleted_at: null,
    },
  ],
};
const readButton = () => screen.getByRole("button", { name: "核对已存正文" });
const save = () =>
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
const editor = () => screen.getByRole("textbox", { name: "章节正文" });
const change = (text: string) =>
  fireEvent.change(editor(), { target: { value: text } });
const start = async (
  kind: "lost" | "wrong" | "not-committed",
  withEpoch = true,
  observe: (() => Promise<string>) | null = null,
) => {
  let state = structuredClone(initial);
  let epoch = "old";
  const api: API = {
    load: vi.fn(async () => structuredClone(state)),
    apply: vi.fn(async (cmd) => {
      if (cmd.type !== "save_chapter") throw Error("unexpected mutation");
      if (kind === "lost" || vi.mocked(api.apply).mock.calls.length > 1) {
        state.chapters[0] = {
          ...state.chapters[0],
          body: String(cmd.body),
          revision: Number(cmd.expected_revision) + 1,
        };
      }
      if (vi.mocked(api.apply).mock.calls.length > 1)
        return structuredClone(state);
      if (kind === "wrong") {
        const reply = structuredClone(state);
        reply.chapters[0] = {
          ...reply.chapters[0],
          body: "不是拟写的稿",
          revision: 4,
        };
        return reply;
      }
      throw Error("未收到写入应答");
    }),
    exportBook: vi.fn(async () => "/unused"),
    backup: vi.fn(async () => "/unused"),
    restore: vi.fn(async () => null),
  };
  if (withEpoch)
    api.foreshadow = {
      sourceEpoch: vi.fn(observe ?? (async () => epoch)),
      chapter: vi.fn(async (_: string, id: string, expectedEpoch: string) => {
        if (expectedEpoch !== epoch) throw Error("wrong epoch");
        const row = state.chapters.find((c) => c.id === id)!;
        return {
          position: { chapter_id: row.id, chapter_revision: row.revision },
          body: row.body,
        };
      }),
    } as unknown as API["foreshadow"];
  const { rerender } = render(<App api={api} />);
  fireEvent.click(await screen.findByRole("button", { name: "打开 潮" }));
  await screen.findByRole("textbox", { name: "章节正文" });
  await act(async () => {});
  change(DRAFT);
  save();
  await screen.findByText(/保存结果未确认，稿件保留在编辑器/);
  return {
    api,
    rerender,
    state: () => state,
    setEpoch: (value: string) => (epoch = value),
  };
};

it("does not replay a lost receipt and exact committed readback preserves a newer dirty edit", async () => {
  const { api } = await start("lost");
  change(NEWER);
  fireEvent.click(screen.getByRole("button", { name: "返回书架" }));
  fireEvent.keyDown(window, { ctrlKey: true, key: "s" });
  save();
  expect(api.apply).toHaveBeenCalledTimes(1);
  expect(editor()).toHaveValue(NEWER);
  fireEvent.click(readButton());
  await screen.findByText(/核对到上一份正文已保存.*最新修改仍未保存/);
  expect(editor()).toHaveValue(NEWER);
  expect(api.apply).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "返回书架" }));
  expect(api.apply).toHaveBeenCalledTimes(1);
  await screen.findByText("请手动保存当前稿件，再继续其他操作");
  save();
  await screen.findByText("已保存");
  expect(api.apply).toHaveBeenNthCalledWith(2, {
    type: "save_chapter",
    id: "c",
    body: NEWER,
    expected_revision: 4,
  });
});

it("does not retry or navigate even if an exact original base is visible", async () => {
  const { api } = await start("not-committed");
  fireEvent.click(readButton());
  await screen.findByText(/存档仍是原版本.*保存结果仍未确认/);
  fireEvent.click(screen.getByRole("button", { name: "返回书架" }));
  await act(async () => {});
  fireEvent.keyDown(window, { ctrlKey: true, key: "s" });
  save();
  await act(async () => {});
  expect(api.apply).toHaveBeenCalledTimes(1);
  expect(editor()).toHaveValue(DRAFT);
});
it("wrong receipt and missing epoch cannot claim saved or retry, and manual copy selects latest draft", async () => {
  const { api } = await start("wrong", false);
  change(NEWER);
  save();
  expect(api.apply).toHaveBeenCalledTimes(1);
  expect(readButton()).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "复制当前稿件" }));
  await screen.findByText(/已选择当前稿件.*手动复制/);
  expect(editor()).toHaveValue(NEWER);
  expect((editor() as HTMLTextAreaElement).selectionEnd).toBe(NEWER.length);
});

it("rejects different epoch and ignores a late stopped read", async () => {
  const { api, state, setEpoch } = await start("lost");
  let complete!: (s: LibraryState) => void;
  vi.mocked(api.load).mockImplementationOnce(
    () =>
      new Promise((r) => {
        complete = r;
      }),
  );
  fireEvent.click(readButton());
  await screen.findByRole("button", { name: "停止等待核对" });
  await act(async () => {});
  fireEvent.click(screen.getByRole("button", { name: "停止等待核对" }));
  await act(async () => {
    complete(structuredClone(state()));
  });
  expect(screen.queryByText(/上一份正文已保存/)).toBeNull();
  setEpoch("new");
  fireEvent.click(readButton());
  await screen.findByText(/无法确认属于同一书库/);
  expect(editor()).toHaveValue(DRAFT);
  expect(api.apply).toHaveBeenCalledTimes(1);
});

it("copies exact latest text; rejected clipboard leaves it available for manual copy", async () => {
  const { api } = await start("lost");
  change(NEWER);
  const original = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  const writeText = vi
    .fn()
    .mockRejectedValueOnce(Error("denied"))
    .mockResolvedValueOnce(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
  try {
    const copy = () =>
      fireEvent.click(screen.getByRole("button", { name: "复制当前稿件" }));
    copy();
    await screen.findByText(/已选择当前稿件.*手动复制/);
    expect(editor()).toHaveValue(NEWER);
    expect(writeText).toHaveBeenNthCalledWith(1, NEWER);
    expect((editor() as HTMLTextAreaElement).selectionStart).toBe(0);
    expect((editor() as HTMLTextAreaElement).selectionEnd).toBe(NEWER.length);
    copy();
    await screen.findByText("已复制点击时的稿件。");
    expect(writeText).toHaveBeenNthCalledWith(2, NEWER);
    expect(api.apply).toHaveBeenCalledTimes(1);
  } finally {
    if (original) Object.defineProperty(navigator, "clipboard", original);
    else Reflect.deleteProperty(navigator, "clipboard");
  }
});

it("ignores late confirmation when the App owner changes; does not publish old data", async () => {
  const { api, state, rerender } = await start("lost");
  let complete!: (s: LibraryState) => void;
  vi.mocked(api.load).mockImplementationOnce(
    () =>
      new Promise((r) => {
        complete = r;
      }),
  );
  fireEvent.click(readButton());
  await act(async () => {});
  change(NEWER);
  const other = {
    ...api,
    load: vi.fn(async () => structuredClone(initial)),
    apply: vi.fn(api.apply),
  };
  rerender(<App api={other} />);
  await act(async () => {
    complete(structuredClone(state()));
  });
  expect(
    screen.queryByText(/上一份正文已保存|核对到这份正文已保存/),
  ).toBeNull();
  expect(editor()).toHaveValue(NEWER);
  expect(api.apply).toHaveBeenCalledTimes(1);
  expect(other.apply).not.toHaveBeenCalled();
});

it("takes no late epoch observation as proof for an earlier write", async () => {
  let finish!: (value: string) => void;
  const { api } = await start(
    "lost",
    true,
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  await act(async () => finish("old"));
  expect(api.apply).toHaveBeenCalledTimes(1);
  expect(readButton()).toBeDisabled();
});

it("does not let a late failed copy select the editor after owner replacement", async () => {
  const { api, rerender } = await start("lost");
  change(NEWER);
  const original = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  let deny!: (e: Error) => void;
  const writeText = vi.fn(
    () =>
      new Promise<void>((_, reject) => {
        deny = reject;
      }),
  );
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
  try {
    const copy = screen.getByRole("button", { name: "复制当前稿件" });
    fireEvent.click(copy);
    expect(writeText).toHaveBeenCalledExactlyOnceWith(NEWER);
    const other = {
      ...api,
      load: vi.fn(async () => structuredClone(initial)),
      apply: vi.fn(api.apply),
    };
    rerender(<App api={other} />);
    const currentEditor = editor() as HTMLTextAreaElement;
    currentEditor.setSelectionRange(1, 1);
    await act(async () => deny(Error("clipboard request expired")));
    expect(currentEditor).toHaveValue(NEWER);
    expect(currentEditor.selectionStart).toBe(1);
    expect(currentEditor.selectionEnd).toBe(1);
    expect(screen.queryByText(/请手动复制/)).toBeNull();
    expect(other.apply).not.toHaveBeenCalled();
  } finally {
    if (original) Object.defineProperty(navigator, "clipboard", original);
    else Reflect.deleteProperty(navigator, "clipboard");
  }
});
