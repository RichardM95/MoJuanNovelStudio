import React from "react";
import {
  cleanup,
  render,
  screen,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import App from "../src/App";
import FanqieWorkspace from "../src/FanqieWorkspace";
import type { API, LibraryState } from "../src/types";
import type { FanqieAPI } from "../src/fanqieTypes";
afterEach(cleanup);
const book = { id: "b", title: "灯塔", author: "", deleted_at: null };
const row = {
  chapter_id: "c",
  volume_id: "v",
  volume_title: "归途",
  volume_ordinal: 1,
  title: "初见",
  revision: 1,
  ordinal: 4,
};
const row2 = { ...row, chapter_id: "d", ordinal: 5 };
const index = {
  book_id: "b",
  book_title: "灯塔",
  epoch: "e",
  chapters: [row, row2],
};
function setup() {
  const api: FanqieAPI = {
    directory: vi.fn(async () => index),
    packet: vi.fn(async (_b, _e, chapter) => ({
      ...chapter,
      body: "「灯亮了。」🙂\r\n\r\n雨停了。",
    })),
  };
  return api;
}
it("keeps the active packet single, confirms before advancing, and keeps exact clipboard bytes", async () => {
  const api = setup();
  const copy = vi.fn(async (_value: string) => {});
  render(
    <FanqieWorkspace
      books={[book]}
      api={api}
      onExit={() => {}}
      copyText={copy}
    />,
  );
  fireEvent.change(screen.getByLabelText("墨卷作品"), {
    target: { value: "b" },
  });
  fireEvent.click(await screen.findByLabelText("第4章 初见"));
  fireEvent.click(screen.getByLabelText("第5章 初见"));
  fireEvent.change(screen.getByLabelText("番茄第4章章号"), {
    target: { value: "11" },
  });
  fireEvent.change(screen.getByLabelText("番茄第5章章号"), {
    target: { value: "11" },
  });
  fireEvent.click(screen.getByRole("button", { name: "生成队列" }));
  await screen.findByRole("alert");
  expect(api.packet).not.toHaveBeenCalled();
  fireEvent.change(screen.getByLabelText("番茄第5章章号"), {
    target: { value: "12" },
  });
  fireEvent.click(screen.getByRole("button", { name: "生成队列" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "核对并读取正文" }),
  );
  await screen.findByLabelText("待填正文");
  fireEvent.click(screen.getByRole("button", { name: "复制正文" }));
  await waitFor(() =>
    expect(copy).toHaveBeenCalledWith("「灯亮了。」🙂\r\n\r\n雨停了。"),
  );
  expect(screen.queryByText("第 2 / 2 章")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "我已核对存稿，下一章" }));
  await screen.findByText("第 2 / 2 章");
  expect(screen.queryByLabelText("待填正文")).toBeNull();
});
it("stops on a stale source instead of serving the old packet", async () => {
  const api = setup();
  vi.mocked(api.packet).mockRejectedValueOnce(Error("章稿已变，请重建队列"));
  render(<FanqieWorkspace books={[book]} api={api} onExit={() => {}} />);
  fireEvent.change(screen.getByLabelText("墨卷作品"), {
    target: { value: "b" },
  });
  fireEvent.click(await screen.findByLabelText("第4章 初见"));
  fireEvent.change(screen.getByLabelText("番茄起始章号"), {
    target: { value: "4" },
  });
  fireEvent.click(screen.getByRole("button", { name: "生成队列" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "核对并读取正文" }),
  );
  await screen.findByText("章稿已变，请重建队列");
  expect(
    screen.queryByRole("button", { name: "我已核对存稿，下一章" }),
  ).toBeNull();
});
it("does not open the section while the manuscript is composing; flushes before leaving the editor", async () => {
  let saved = "";
  let data: LibraryState = {
    books: [book],
    volumes: [
      { id: "v", book_id: "b", title: "归途", deleted_at: null, sort_order: 0 },
    ],
    chapters: [
      {
        id: "c",
        volume_id: "v",
        title: "初见",
        body: "原稿",
        revision: 0,
        sort_order: 0,
        deleted_at: null,
      },
    ],
  };
  const api: API = {
    load: async () => data,
    apply: async (c) => {
      saved = String(c.body);
      data = {
        ...data,
        chapters: [{ ...data.chapters[0], body: saved, revision: 1 }],
      };
      return data;
    },
    backup: async () => null,
    restore: async () => null,
    exportBook: async () => null,
    fanqie: setup(),
  };
  render(<App api={api} />);
  fireEvent.click(await screen.findByRole("button", { name: "打开 灯塔" }));
  const editor = await screen.findByRole("textbox", { name: "章节正文" });
  fireEvent.compositionStart(editor);
  fireEvent.change(editor, { target: { value: "新稿" } });
  fireEvent.click(screen.getByRole("button", { name: "番茄章稿" }));
  await screen.findByText("请先完成输入法组字，再打开番茄章稿。");
  expect(saved).toBe("");
  fireEvent.compositionEnd(editor);
  fireEvent.click(screen.getByRole("button", { name: "番茄章稿" }));
  await screen.findByRole("heading", { name: "番茄章稿" });
  expect(saved).toBe("新稿");
});

it("does not advance or exit while the current chapter clipboard write is pending", async () => {
  const api = setup();
  let complete!: () => void;
  const copy = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        complete = resolve;
      }),
  );
  const exit = vi.fn();
  render(
    <FanqieWorkspace books={[book]} api={api} onExit={exit} copyText={copy} />,
  );
  fireEvent.change(screen.getByLabelText("墨卷作品"), {
    target: { value: "b" },
  });
  fireEvent.click(await screen.findByLabelText("第4章 初见"));
  fireEvent.change(screen.getByLabelText("番茄起始章号"), {
    target: { value: "4" },
  });
  fireEvent.click(screen.getByRole("button", { name: "生成队列" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "核对并读取正文" }),
  );
  fireEvent.click(await screen.findByRole("button", { name: "复制正文" }));
  const advance = screen.getByRole("button", { name: "我已核对存稿，下一章" });
  expect(advance).toHaveProperty("disabled", true);
  expect(screen.getByRole("button", { name: "退出并清空队列" })).toHaveProperty(
    "disabled",
    true,
  );
  complete();
  await screen.findByText("正文已复制");
  expect(advance).toHaveProperty("disabled", false);
});

it("copies one explicit author-targeted packet for the browser extension", async () => {
  const copy = vi.fn(async (_value: string) => {});
  render(
    <FanqieWorkspace
      books={[book]}
      api={setup()}
      onExit={() => {}}
      copyText={copy}
    />,
  );
  fireEvent.change(screen.getByLabelText("墨卷作品"), {
    target: { value: "b" },
  });
  fireEvent.click(await screen.findByLabelText("第4章 初见"));
  fireEvent.change(screen.getByLabelText("番茄起始章号"), {
    target: { value: "19" },
  });
  fireEvent.change(screen.getByLabelText("番茄目标作品"), {
    target: { value: "番茄书" },
  });
  fireEvent.click(screen.getByRole("button", { name: "生成队列" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "核对并读取正文" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "复制本章给浏览器扩展" }),
  );
  await waitFor(() => expect(copy).toHaveBeenCalled());
  expect(JSON.parse(copy.mock.calls[0][0] as string)).toEqual({
    book: "番茄书",
    volume: "归途",
    number: "19",
    title: "初见",
    body: "「灯亮了。」🙂\r\n\r\n雨停了。",
  });
});
