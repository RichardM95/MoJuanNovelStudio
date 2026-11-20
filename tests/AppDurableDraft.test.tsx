import "@testing-library/jest-dom/vitest";
import React from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import App from "../src/App";
import type { API, LibraryState } from "../src/types";
import type { DraftState } from "../src/draftRecoveryTypes";
import { useDraftRecovery } from "../src/useDraftRecovery";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const base = "已存原稿🙂\r\n灯";
const recovered = "重开前的较新稿🙂\r\n保留行尾";
const editor = () => screen.getByRole("textbox", { name: "章节正文" });
const recover = () => screen.getByRole("button", { name: "载入副本继续编辑" });
const save = () => screen.getByRole("button", { name: "保存" });
const file: LibraryState = {
  books: [{ id: "b", title: "潮书", author: "", deleted_at: null }],
  volumes: [
    { id: "v", book_id: "b", title: "旧潮", deleted_at: null, sort_order: 0 },
  ],
  chapters: [
    {
      id: "c",
      volume_id: "v",
      title: "原章",
      body: base,
      revision: 1,
      sort_order: 0,
      deleted_at: null,
    },
  ],
};
const scope = { epoch: "e", book_id: "b", chapter_id: "c" };
const record = {
  token: "t",
  base_revision: 1,
  base_body: base,
  draft_body: recovered,
  state: "matches_base",
};
type ReadView = {
  epoch: string;
  book_id: string;
  chapter_id: string;
  current_revision: number;
  current_body: string;
  record: typeof record | null;
};

async function open(view: ReadView) {
  vi.useFakeTimers();
  const api: API = {
    load: vi.fn(async () => structuredClone(file)),
    apply: vi.fn(async (command) => ({
      ...file,
      chapters: [
        { ...file.chapters[0], body: String(command.body), revision: 2 },
      ],
    })),
    exportBook: async () => null,
    backup: async () => null,
    restore: async () => null,
  };
  const recovery = {
    start: vi.fn(async () => ({ session_id: "s", epoch: "e" })),
    read: vi.fn(async () => structuredClone(view)),
    checkpoint: vi.fn(async () => ({
      token: "new",
      state: "matches_base" as DraftState,
    })),
  };
  Object.assign(api, {
    draftRecovery: recovery,
    foreshadow: {
      sourceEpoch: async () => "e",
      chapter: async () => ({
        position: { chapter_id: "c", chapter_revision: 1 },
        body: base,
      }),
    },
  });
  const { rerender } = render(<App api={api} />);
  await act(async () => {});
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "打开 潮书" }));
  });
  return { api, recovery, rerender };
}

it("compares and loads an exact-base recovery only on explicit choice; no auto save or overwrite", async () => {
  const { api, recovery } = await open({
    ...scope,
    current_revision: 1,
    current_body: base,
    record,
  });
  expect(screen.getByLabelText("当前已存正文")).toHaveValue(
    base.replaceAll("\r\n", "\n"),
  );
  expect(screen.getByLabelText("本地恢复副本")).toHaveValue(
    recovered.replaceAll("\r\n", "\n"),
  );
  expect(editor()).toHaveValue(base.replaceAll("\r\n", "\n"));
  expect(api.apply).not.toHaveBeenCalled();
  expect(recovery.checkpoint).not.toHaveBeenCalled();
  await act(async () => {
    fireEvent.click(recover());
  });
  expect(recovery.read).toHaveBeenCalledTimes(2);
  expect(editor()).toHaveValue(recovered.replaceAll("\r\n", "\n"));
  await act(async () => {
    vi.advanceTimersByTime(1200);
  });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "返回书架" }));
  });
  expect(editor()).toBeInTheDocument();
  expect(api.apply).not.toHaveBeenCalled();
  expect(recovery.checkpoint).not.toHaveBeenCalled();
  await act(async () => {
    fireEvent.click(save());
  });
  expect(api.apply).toHaveBeenCalledExactlyOnceWith({
    type: "save_chapter",
    id: "c",
    body: recovered,
    expected_revision: 1,
  });
});

it("cannot load a stale checkpoint or revive an acknowledged canonical duplicate", async () => {
  const view = {
    ...scope,
    current_revision: 2,
    current_body: "另一份已存稿",
    record: { ...record, state: "stale" },
  };
  const { api } = await open(view);
  expect(screen.getByLabelText("当前已存正文")).toHaveValue(view.current_body);
  expect(screen.getByLabelText("本地恢复副本")).toHaveValue(
    recovered.replaceAll("\r\n", "\n"),
  );
  expect(screen.queryByRole("button", { name: "载入副本继续编辑" })).toBeNull();
  expect(api.apply).not.toHaveBeenCalled();
});

it("does not offer already saved recovery as a second draft", async () => {
  const { api, recovery } = await open({
    ...scope,
    current_revision: 1,
    current_body: base,
    record: { ...record, draft_body: base, state: "already_saved" },
  });
  expect(screen.queryByLabelText("本地恢复副本")).toBeNull();
  expect(screen.queryByRole("button", { name: "载入副本继续编辑" })).toBeNull();
  expect(recovery.checkpoint).not.toHaveBeenCalled();
  expect(api.apply).not.toHaveBeenCalled();
});

it("checkpoints only the latest input before debounce and never calls it manuscript saved", async () => {
  const { api, recovery } = await open({
    ...scope,
    current_revision: 1,
    current_body: base,
    record: null,
  });
  fireEvent.change(editor(), { target: { value: "旧输入" } });
  fireEvent.change(editor(), { target: { value: "最新输入🙂\n" } });
  expect(recovery.checkpoint).not.toHaveBeenCalled();
  await act(async () => {
    vi.advanceTimersByTime(500);
  });
  expect(recovery.checkpoint).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({
      ...scope,
      session_id: "s",
      expected_token: null,
      base_revision: 1,
      base_body: base,
      draft_body: "最新输入🙂\n",
    }),
  );
  expect(screen.getByText(/恢复副本已保存.*正文仍未保存/)).toBeInTheDocument();
  expect(api.apply).not.toHaveBeenCalled();
});

it("does not replace later typed text while the selected recovery is rechecked", async () => {
  const { api, recovery } = await open({
    ...scope,
    current_revision: 1,
    current_body: base,
    record,
  });
  let release!: (r: ReadView) => void;
  recovery.read.mockImplementationOnce(
    () =>
      new Promise<ReadView>((r) => {
        release = r;
      }),
  );
  fireEvent.click(recover());
  fireEvent.change(editor(), { target: { value: "核对期间继续写🙂" } });
  await act(async () => {
    release({ ...scope, current_revision: 1, current_body: base, record });
  });
  await act(async () => {
    vi.advanceTimersByTime(500);
  });
  expect(editor()).toHaveValue("核对期间继续写🙂");
  expect(screen.getByLabelText("本地恢复副本")).toHaveValue(
    recovered.replaceAll("\r\n", "\n"),
  );
  expect(recovery.checkpoint).not.toHaveBeenCalled();
  expect(api.apply).not.toHaveBeenCalled();
});

it("does not claim a late checkpoint covered newer input; uses exact token and normal save survives sidecar failure", async () => {
  const { api, recovery } = await open({
    ...scope,
    current_revision: 1,
    current_body: base,
    record: null,
  });
  let finish!: (result: { token: string; state: DraftState }) => void;
  recovery.checkpoint
    .mockImplementationOnce(
      () =>
        new Promise((r) => {
          finish = r;
        }),
    )
    .mockRejectedValueOnce(Error("sidecar quota"));
  fireEvent.change(editor(), { target: { value: "甲输入" } });
  await act(async () => {
    vi.advanceTimersByTime(450);
  });
  fireEvent.change(editor(), { target: { value: "甲之后的新稿" } });
  await act(async () => {
    finish({ token: "token-a", state: "matches_base" });
  });
  expect(screen.queryByText(/恢复副本已保存/)).toBeNull();
  await act(async () => {
    vi.advanceTimersByTime(450);
  });
  expect(recovery.checkpoint).toHaveBeenNthCalledWith(
    2,
    expect.objectContaining({
      expected_token: "token-a",
      draft_body: "甲之后的新稿",
      sequence: 2,
    }),
  );
  expect(screen.getByText(/未能确认最新恢复副本/)).toBeInTheDocument();
  await act(async () => {
    fireEvent.click(save());
  });
  expect(api.apply).toHaveBeenCalledExactlyOnceWith({
    type: "save_chapter",
    id: "c",
    body: "甲之后的新稿",
    expected_revision: 1,
  });
});

it("keeps identical prose at a later unrelated revision copy-only", async () => {
  const { api } = await open({
    ...scope,
    current_revision: 5,
    current_body: recovered,
    record: { ...record, state: "stale" },
  });
  expect(screen.getByLabelText("本地恢复副本")).toHaveValue(
    recovered.replaceAll("\r\n", "\n"),
  );
  expect(screen.queryByRole("button", { name: "载入副本继续编辑" })).toBeNull();
  expect(api.apply).not.toHaveBeenCalled();
});

it("submits newer B against its original anchored base after A commits but gets no receipt", async () => {
  const { api, recovery } = await open({
    ...scope,
    current_revision: 1,
    current_body: base,
    record: null,
  });
  let loseAck!: (reason: Error) => void;
  const savedA: LibraryState = {
    ...file,
    chapters: [{ ...file.chapters[0], body: "甲已提交", revision: 2 }],
  };
  vi.mocked(api.apply).mockImplementationOnce(async () => {
    vi.mocked(api.load).mockResolvedValue(savedA); // the canonical commit precedes the lost reply
    return new Promise((_, r) => {
      loseAck = r;
    });
  });
  recovery.checkpoint
    .mockResolvedValueOnce({ token: "token-a", state: "matches_base" })
    .mockResolvedValueOnce({ token: "token-b", state: "stale" });
  fireEvent.change(editor(), { target: { value: "甲已提交" } });
  await act(async () => {
    vi.advanceTimersByTime(800);
  });
  expect(api.apply).toHaveBeenCalledTimes(1);
  expect(editor()).toBeEnabled();
  expect(recovery.checkpoint).toHaveBeenCalledTimes(1);
  fireEvent.change(editor(), { target: { value: "甲之后的乙草稿🙂" } });
  await act(async () => {
    loseAck(Error("回执丢失"));
  });
  await act(async () => {
    vi.advanceTimersByTime(900);
  });
  expect(recovery.checkpoint).toHaveBeenNthCalledWith(
    2,
    expect.objectContaining({
      base_revision: 1,
      base_body: base,
      draft_body: "甲之后的乙草稿🙂",
      expected_token: "token-a",
    }),
  );
  expect(api.apply).toHaveBeenCalledTimes(1);
  expect(editor()).toHaveValue("甲之后的乙草稿🙂");
  expect(screen.getByText(/恢复副本已保存.*正文仍未保存/)).toBeInTheDocument();
});

it("compares the newly observed token but never applies it on the old choose action", async () => {
  const { api, recovery } = await open({
    ...scope,
    current_revision: 1,
    current_body: base,
    record,
  });
  recovery.read.mockResolvedValueOnce({
    ...scope,
    current_revision: 1,
    current_body: base,
    record: {
      ...record,
      token: "newer-ticket",
      draft_body: "突然出现的新副本",
    },
  });
  await act(async () => {
    fireEvent.click(recover());
  });
  expect(editor()).toHaveValue(base.replaceAll("\r\n", "\n"));
  expect(screen.getByLabelText("本地恢复副本")).toHaveValue("突然出现的新副本");
  expect(api.apply).not.toHaveBeenCalled();
  expect(recovery.checkpoint).not.toHaveBeenCalled();
});

it("ignores a late adopted-read response after the App API owner changes", async () => {
  const { api, recovery, rerender } = await open({
    ...scope,
    current_revision: 1,
    current_body: base,
    record,
  });
  let finish!: (value: ReadView) => void;
  recovery.read.mockImplementationOnce(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  fireEvent.click(recover());
  const nextRead = vi.fn(async () => ({
    ...scope,
    current_revision: 1,
    current_body: base,
    record: null,
  }));
  const next = {
    ...api,
    load: vi.fn(async () => structuredClone(file)),
    draftRecovery: { ...recovery, read: nextRead },
  };
  rerender(<App api={next} />);
  await act(async () => {
    finish({ ...scope, current_revision: 1, current_body: base, record });
  });
  expect(editor()).toHaveValue(base.replaceAll("\r\n", "\n"));
  expect(screen.queryByLabelText("本地恢复副本")).toBeNull();
  expect(api.apply).not.toHaveBeenCalled();
  expect(recovery.checkpoint).not.toHaveBeenCalled();
});

it("serializes superseding lease starts so a late old backend call cannot revoke the new result", async () => {
  let finish!: (value: { session_id: string; epoch: string }) => void;
  const api = {
    start: vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise((r) => {
            finish = r;
          }),
      )
      .mockResolvedValueOnce({ session_id: "s2", epoch: "e2" }),
    read: vi.fn(),
    checkpoint: vi.fn(),
  };
  const { rerender } = renderHook(
    ({ generation }) => useDraftRecovery(api, generation, null),
    { initialProps: { generation: 0 } },
  );
  await act(async () => {});
  expect(api.start).toHaveBeenCalledTimes(1);
  rerender({ generation: 1 });
  rerender({ generation: 2 });
  await act(async () => {});
  expect(api.start).toHaveBeenCalledTimes(1);
  await act(async () => {
    finish({ session_id: "s1", epoch: "e1" });
  });
  expect(api.start).toHaveBeenCalledTimes(2);
  expect(api.read).not.toHaveBeenCalled();
  expect(api.checkpoint).not.toHaveBeenCalled();
});
