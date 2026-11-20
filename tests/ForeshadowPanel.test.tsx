import React from "react";
import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ForeshadowPanel from "../src/ForeshadowPanel";
import {
  exactExcerpt,
  type ForeshadowAPI,
  type OccurrenceView,
} from "../src/foreshadowTypes";
import type { MaterialSummary } from "../src/materialTypes";

afterEach(cleanup);
const position = {
  chapter_id: "c",
  chapter_title: "桥下",
  chapter_revision: 1,
  chapter_sort_order: 0,
  volume_id: "v",
  volume_title: "一卷",
  volume_sort_order: 0,
};
const card = (
  id: string,
  title: string,
  tracking_status: "planned" | "planted",
): MaterialSummary => ({
  id,
  book_id: "b",
  kind: "foreshadow",
  title,
  tracking_status,
  source_chapter_id: "c",
  source_revision: 1,
  source_status: "current",
  payoff_chapter_id: null,
  payoff_revision: null,
  payoff_status: "none",
  volume_id: null,
  chapter_id: null,
  revision: 0,
  updated_at: 0,
  deleted_at: null,
});
const item: OccurrenceView = {
  occurrence: {
    id: "oc",
    book_id: "b",
    material_id: "a",
    source: position,
    start: 1,
    end: 5,
    quote: "😀\r\n线",
    role: "hint",
    note: "备注",
    created_at: 0,
    revision: 0,
    deleted_at: null,
  },
  source_status: "current",
  current: position,
};
const makeAPI = (): ForeshadowAPI => ({
  epoch: vi.fn(async () => "epoch"),
  overview: vi.fn(async () => [
    {
      material: card("a", "铜钥匙", "planned"),
      occurrence_count: 1,
      review_count: 0,
    },
    {
      material: card("b2", "纸伞", "planted"),
      occurrence_count: 0,
      review_count: 0,
    },
  ]),
  directory: vi.fn(async () => [position]),
  chapter: vi.fn(async () => ({ position, body: "甲😀\r\n线乙" })),
  list: vi.fn(async () => [item]),
  get: vi.fn(async () => item),
  lookup: vi.fn(async () => item),
  create: vi.fn(async () => item),
  setDeleted: vi.fn(async () => item),
  material: vi.fn(async () => ({
    ...card("a", "铜钥匙", "planned"),
    fields: { setup: "" },
  })),
  createCard: vi.fn(async () => ({
    ...card("a", "铜钥匙", "planned"),
    fields: { setup: "" },
  })),
  saveCard: vi.fn(async () => ({
    ...card("a", "铜钥匙", "planned"),
    fields: { setup: "" },
  })),
});
const display = (
  api: ForeshadowAPI,
  overrides: Partial<React.ComponentProps<typeof ForeshadowPanel>> = {},
) =>
  render(
    <ForeshadowPanel
      api={api}
      bookId="b"
      onClose={vi.fn()}
      onJump={vi.fn(async () => true)}
      {...overrides}
    />,
  );
const cardButton = (name: string) =>
  screen.findByRole("button", { name: new RegExp(name) });
const select = async () => {
  fireEvent.click(await cardButton("铜钥匙"));
};
test("maps normalized textarea UTF-16 to exact raw scalar boundaries", () => {
  expect(exactExcerpt("甲😀\r\n线乙", "甲😀\n线乙", 1, 5)).toEqual({
    start: 1,
    end: 5,
    quote: "😀\r\n线",
  });
  expect(exactExcerpt("甲\r乙", "甲\n乙", 0, 3).quote).toBe("甲\r乙");
  expect(() => exactExcerpt("甲😀", "甲😀", 1, 2)).toThrow();
  expect(() => exactExcerpt("甲\r\n乙", "甲\r\n乙", 1, 3)).toThrow();
});
test("directory errors do not block the whole-book summary or selected records", async () => {
  const api = makeAPI();
  vi.mocked(api.directory).mockRejectedValue(new Error("可选目录不可用"));
  display(api);
  expect(await cardButton("纸伞")).toBeVisible();
  expect(api.list).not.toHaveBeenCalled();
  await select();
  expect(await screen.findByText("备注")).toBeVisible();
  expect(screen.getByText(/可选目录不可用/)).toBeVisible();
  fireEvent.change(screen.getByLabelText("作者状态筛选"), {
    target: { value: "planted" },
  });
  expect(screen.queryByRole("button", { name: /铜钥匙/ })).toBeNull();
});
test("unknown create checks the original request key and does not retry a write", async () => {
  const api = makeAPI();
  vi.mocked(api.create).mockRejectedValueOnce(new Error("连接中断，回执未知"));
  const onClose = vi.fn();
  display(api, { onClose });
  await select();
  fireEvent.change(await screen.findByLabelText("选择出现来源章节"), {
    target: { value: "c" },
  });
  const body = (await screen.findByLabelText(
    "已保存来源正文",
  )) as HTMLTextAreaElement;
  body.setSelectionRange(1, 5);
  fireEvent.select(body);
  fireEvent.change(screen.getByLabelText("出现备注"), {
    target: { value: "备注" },
  });
  fireEvent.click(screen.getByRole("button", { name: "记录出现" }));
  expect(await screen.findByRole("button", { name: "读取核对" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "关闭全书伏笔" }));
  expect(onClose).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "读取核对" }));
  const [[request]] = vi.mocked(api.create).mock.calls;
  expect(request).toMatchObject({
    quote: "😀\r\n线",
    start: 1,
    end: 5,
    epoch: "epoch",
    note: "备注",
  });
  await waitFor(() =>
    expect(api.lookup).toHaveBeenCalledWith(
      "b",
      "a",
      request.request_key,
      "epoch",
    ),
  );
  expect(api.create).toHaveBeenCalledTimes(1);
  await waitFor(() =>
    expect(
      screen.queryByRole("button", { name: "读取核对" }),
    ).not.toBeInTheDocument(),
  );
});
test("a deferred source read cannot populate a different card, and failed jumps keep the panel open", async () => {
  const api = makeAPI();
  let resolve!: (value: Awaited<ReturnType<ForeshadowAPI["chapter"]>>) => void;
  vi.mocked(api.chapter).mockReturnValueOnce(
    new Promise((r) => {
      resolve = r;
    }),
  );
  vi.mocked(api.list).mockImplementation(async (_b, id) =>
    id === "a" ? [item] : [],
  );
  const onJump = vi.fn(async () => false);
  const onClose = vi.fn();
  display(api, { onJump, onClose });
  await select();
  const selector = await screen.findByLabelText("选择出现来源章节");
  await waitFor(() =>
    expect(screen.getByRole("option", { name: "一卷 · 桥下" })).toBeVisible(),
  );
  fireEvent.change(selector, { target: { value: "c" } });
  fireEvent.click(await cardButton("纸伞"));
  resolve({ position, body: "迟到的正文" });
  await waitFor(() =>
    expect(screen.queryByLabelText("已保存来源正文")).not.toBeInTheDocument(),
  );
  await select();
  fireEvent.click(await screen.findByRole("button", { name: "跳转来源章" }));
  await waitFor(() => expect(onJump).toHaveBeenCalledWith(position, "epoch"));
  expect(onClose).not.toHaveBeenCalled();
  expect(screen.getByRole("dialog", { name: "全书伏笔" })).toBeVisible();
});

test("App saves the current chapter before opening overview and selects an active source only after a fresh load", async () => {
  const { default: App } = await import("../src/App");
  const api = makeAPI();
  const state = {
    books: [{ id: "b", title: "虚构书", author: "", deleted_at: null }],
    volumes: [
      { id: "v", book_id: "b", title: "一卷", sort_order: 0, deleted_at: null },
    ],
    chapters: [
      {
        id: "d",
        volume_id: "v",
        title: "目标",
        body: "旧稿",
        revision: 0,
        sort_order: 0,
        deleted_at: null,
      },
      {
        id: "c",
        volume_id: "v",
        title: "桥下",
        body: "来源稿",
        revision: 1,
        sort_order: 0,
        deleted_at: null,
      },
    ],
  };
  const order: string[] = [];
  vi.mocked(api.overview).mockImplementation(async () => {
    order.push("overview");
    return [
      {
        material: card("a", "铜钥匙", "planned"),
        occurrence_count: 1,
        review_count: 0,
      },
    ];
  });
  vi.mocked(api.chapter).mockImplementation(async () => {
    order.push("read-source");
    return { position, body: "来源稿" };
  });
  render(
    <App
      api={{
        foreshadow: api,
        load: async () => {
          order.push("load");
          return structuredClone(state);
        },
        apply: async (cmd) => {
          order.push("save");
          state.chapters[0].body = String(cmd.body);
          state.chapters[0].revision++;
          return structuredClone(state);
        },
        backup: async () => null,
        restore: async () => null,
        exportBook: async () => null,
      }}
    />,
  );
  fireEvent.click(await screen.findByRole("button", { name: "打开 虚构书" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "章节正文" }), {
    target: { value: "待保存稿" },
  });
  fireEvent.click(screen.getByRole("button", { name: "全书伏笔" }));
  await select();
  fireEvent.click(await screen.findByRole("button", { name: "跳转来源章" }));
  await waitFor(() =>
    expect(screen.getByRole("textbox", { name: "章节正文" })).toHaveValue(
      "来源稿",
    ),
  );
  expect(order).toEqual(["load", "save", "overview", "load"]);
  expect(api.chapter).not.toHaveBeenCalled();
  expect(state.chapters[0].body).toBe("待保存稿");
});

async function openPrimary() {
  await select();
  fireEvent.click(
    await screen.findByRole("button", { name: "编辑主要埋设、回收与作者状态" }),
  );
  return screen.findByLabelText("资料标题");
}
async function editExcerpt(note: string) {
  await select();
  fireEvent.change(await screen.findByLabelText("选择出现来源章节"), {
    target: { value: "c" },
  });
  const body = (await screen.findByLabelText(
    "已保存来源正文",
  )) as HTMLTextAreaElement;
  body.setSelectionRange(1, 5);
  fireEvent.select(body);
  fireEvent.change(screen.getByLabelText("出现备注"), {
    target: { value: note },
  });
}

test("a mismatched successful occurrence acknowledgment requires lookup using the unchanged key", async () => {
  const api = makeAPI();
  vi.mocked(api.create).mockResolvedValueOnce({
    ...item,
    occurrence: { ...item.occurrence, note: "错误回执" },
  });
  display(api);
  await editExcerpt("备注");
  fireEvent.click(screen.getByRole("button", { name: "记录出现" }));
  fireEvent.click(await screen.findByRole("button", { name: "读取核对" }));
  const [[request]] = vi.mocked(api.create).mock.calls;
  await waitFor(() =>
    expect(api.lookup).toHaveBeenCalledWith(
      "b",
      "a",
      request.request_key,
      "epoch",
    ),
  );
});

test("a committed but lost primary save reconciles by exact readback without another save", async () => {
  const api = makeAPI();
  const before = {
    ...card("a", "铜钥匙", "planned"),
    fields: { setup: "原埋设", resolution: "原回收" },
  };
  let canonical = before;
  vi.mocked(api.material).mockImplementation(async () => canonical);
  vi.mocked(api.saveCard).mockImplementationOnce(async () => {
    canonical = { ...before, title: "已提交标题", revision: 1 };
    throw new Error("回执丢失");
  });
  display(api);
  fireEvent.change(await openPrimary(), { target: { value: "已提交标题" } });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  fireEvent.click(await screen.findByRole("button", { name: "读取核对资料" }));
  expect(
    await screen.findByText("已核对：草稿已保存，未再次写入。"),
  ).toBeVisible();
  expect(api.saveCard).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "返回全书伏笔" }));
  expect(await screen.findByRole("dialog", { name: "全书伏笔" })).toBeVisible();
});

test("primary CAS conflict retains local changes and rebases only on explicit author choice", async () => {
  const api = makeAPI();
  const before = {
    ...card("a", "铜钥匙", "planned"),
    fields: { setup: "旧埋设", resolution: "旧回收" },
  };
  let canonical = before;
  vi.mocked(api.material).mockImplementation(async () => canonical);
  vi.mocked(api.saveCard).mockImplementationOnce(async () => {
    canonical = {
      ...before,
      title: "并发标题",
      fields: { ...before.fields, resolution: "并发回收" },
      revision: 2,
    };
    throw { code: "revision_conflict", message: "版本改变" };
  });
  display(api);
  fireEvent.change(await openPrimary(), { target: { value: "我的标题" } });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  fireEvent.click(await screen.findByRole("button", { name: "读取核对资料" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "保留我的修改并换用当前版本" }),
  );
  expect(screen.getByLabelText("资料标题")).toHaveValue("我的标题");
  expect(screen.getByLabelText("回收说明")).toHaveValue("并发回收");
  expect(api.saveCard).toHaveBeenCalledTimes(1);
  vi.mocked(api.saveCard).mockResolvedValueOnce({
    ...canonical,
    title: "我的标题",
    revision: 3,
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  expect(api.saveCard).toHaveBeenLastCalledWith(
    "b",
    expect.objectContaining({
      expected_revision: 2,
      title: "我的标题",
      fields: { setup: "旧埋设", resolution: "并发回收" },
    }),
    "epoch",
  );
});

test("confirmed epoch replacement keeps a dirty primary draft copyable until explicit discard", async () => {
  const api = makeAPI();
  vi.mocked(api.saveCard).mockRejectedValueOnce({
    code: "epoch_changed",
    message: "书库变了",
  });
  display(api);
  fireEvent.change(await openPrimary(), {
    target: { value: "只属于旧书库的标题" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  expect(
    ((await screen.findByLabelText("旧书库资料草稿")) as HTMLTextAreaElement)
      .value,
  ).toContain("只属于旧书库");
  fireEvent.click(screen.getByRole("button", { name: "返回全书伏笔" }));
  expect(screen.getByRole("dialog", { name: "伏笔资料编辑" })).toBeVisible();
  fireEvent.click(
    screen.getByRole("button", { name: "放弃旧书库资料草稿并返回" }),
  );
  expect(await screen.findByRole("dialog", { name: "全书伏笔" })).toBeVisible();
  expect(api.saveCard).toHaveBeenCalledTimes(1);
});

test("unknown primary creation cannot be re-enabled before a successful read and explicit resolution", async () => {
  const api = makeAPI();
  vi.mocked(api.createCard).mockRejectedValueOnce(Error("回执丢失"));
  display(api);
  fireEvent.click(await screen.findByRole("button", { name: "新增伏笔卡" }));
  fireEvent.click(await screen.findByRole("button", { name: "新增伏笔" }));
  const read = await screen.findByRole("button", {
    name: "重新读取核对新增伏笔卡",
  });
  fireEvent.click(screen.getByRole("button", { name: "返回全书伏笔" }));
  expect(screen.getByRole("dialog", { name: "伏笔资料编辑" })).toBeVisible();
  vi.mocked(api.overview).mockRejectedValueOnce(Error("读取仍失败"));
  fireEvent.click(read);
  expect(await screen.findByText("读取仍失败")).toBeVisible();
  expect(screen.getByRole("button", { name: "新增伏笔" })).toBeDisabled();
  fireEvent.click(read);
  fireEvent.click(
    await screen.findByRole("button", { name: "已核对列表，解除新增锁定" }),
  );
  expect(screen.getByRole("button", { name: "新增伏笔" })).toBeEnabled();
  expect(api.createCard).toHaveBeenCalledTimes(1);
});

test("replacement does not discard the old occurrence draft or replay it", async () => {
  const api = makeAPI(),
    onClose = vi.fn();
  vi.mocked(api.create).mockRejectedValueOnce({
    code: "epoch_changed",
    message: "书库变了",
  });
  display(api, { onClose });
  await editExcerpt("仅旧书库备注");
  fireEvent.click(screen.getByRole("button", { name: "记录出现" }));
  expect(
    ((await screen.findByLabelText("旧书库片段草稿")) as HTMLTextAreaElement)
      .value,
  ).toContain("仅旧书库备注");
  fireEvent.click(screen.getByRole("button", { name: "关闭全书伏笔" }));
  expect(onClose).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "放弃旧书库片段草稿并关闭" }),
  );
  expect(onClose).toHaveBeenCalledTimes(1);
  expect(api.create).toHaveBeenCalledTimes(1);
});

test("reads only a selected card's primary prose and distinguishes duplicate chapter labels", async () => {
  const api = makeAPI();
  const twin = {
    ...position,
    chapter_id: "d",
    volume_id: "v2",
    volume_sort_order: 1,
  };
  vi.mocked(api.directory).mockResolvedValue([position, twin]);
  vi.mocked(api.material).mockResolvedValue({
    ...card("a", "铜钥匙", "planned"),
    fields: { setup: "保存的主要埋设", resolution: "保存的主要回收" },
  });
  display(api);
  expect(await cardButton("铜钥匙")).toBeVisible();
  expect(api.material).not.toHaveBeenCalled();
  await select();
  expect(await screen.findByText("保存的主要埋设")).toBeVisible();
  expect(screen.getByText("保存的主要回收")).toBeVisible();
  expect(
    await screen.findByRole("option", {
      name: /一卷 · 桥下（卷 1 \/ 章 1 · c）/,
    }),
  ).toBeVisible();
  expect(
    screen.getByRole("option", { name: /一卷 · 桥下（卷 2 \/ 章 1 · d）/ }),
  ).toBeVisible();
  fireEvent.click(
    screen.getByRole("button", { name: "编辑主要埋设、回收与作者状态" }),
  );
  expect(
    await screen.findAllByRole("option", {
      name: /一卷 · 桥下（卷 2 \/ 章 1 · d）/,
    }),
  ).toHaveLength(2);
});

test("failed jump reports its cause inside the modal and invalidates a confirmed changed epoch", async () => {
  const api = makeAPI(),
    onClose = vi.fn();
  const onJump = vi
    .fn()
    .mockRejectedValueOnce(Error("当前稿件不能落盘"))
    .mockRejectedValueOnce({ code: "epoch_changed", message: "替换已确认" });
  display(api, { onJump, onClose });
  await select();
  const button = await screen.findByRole("button", { name: "跳转来源章" });
  fireEvent.click(button);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "当前稿件不能落盘",
  );
  fireEvent.click(button);
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent("书库已更换"),
  );
  expect(button).toBeDisabled();
  expect(onClose).not.toHaveBeenCalled();
});

test("a rejected same-book card receipt cannot change selection after correct readback", async () => {
  const api = makeAPI();
  const before = {
    ...card("a", "铜钥匙", "planned"),
    fields: { setup: "原埋设", resolution: "原回收" },
  };
  let canonical = before;
  vi.mocked(api.material).mockImplementation(async (_book, id) =>
    id === "a"
      ? canonical
      : {
          ...card("b2", "纸伞", "planted"),
          fields: { setup: "其他卡", resolution: "" },
        },
  );
  vi.mocked(api.saveCard).mockImplementationOnce(async () => {
    canonical = { ...before, title: "正确提交", revision: 1 };
    return { ...canonical, id: "b2" };
  });
  display(api);
  fireEvent.change(await openPrimary(), { target: { value: "正确提交" } });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  fireEvent.click(await screen.findByRole("button", { name: "读取核对资料" }));
  await screen.findByText("已核对：草稿已保存，未再次写入。");
  vi.mocked(api.list).mockClear();
  fireEvent.click(screen.getByRole("button", { name: "返回全书伏笔" }));
  expect(await cardButton("铜钥匙")).toHaveAttribute("aria-pressed", "true");
  expect(await cardButton("纸伞")).toHaveAttribute("aria-pressed", "false");
  expect(api.list).toHaveBeenCalledExactlyOnceWith("b", "a", "epoch");
  expect(api.saveCard).toHaveBeenCalledTimes(1);
});

test("a definitively inactive primary card leaves a copyable draft and an explicit exit", async () => {
  const api = makeAPI();
  vi.mocked(api.saveCard).mockRejectedValueOnce({
    code: "not_found",
    message: "活动伏笔卡已不存在",
  });
  display(api);
  fireEvent.change(await openPrimary(), {
    target: { value: "尚未保存的标题" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  const draft = (await screen.findByLabelText(
    "无法保存的资料草稿",
  )) as HTMLTextAreaElement;
  expect(draft.readOnly).toBe(true);
  expect(draft.value).toContain("尚未保存的标题");
  expect(screen.getByRole("button", { name: "保存资料" })).toBeDisabled();
  expect(
    screen.queryByRole("button", { name: "读取核对资料" }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "返回全书伏笔" }));
  expect(screen.getByRole("dialog", { name: "伏笔资料编辑" })).toBeVisible();
  fireEvent.click(
    screen.getByRole("button", { name: "放弃无法保存的资料草稿并返回" }),
  );
  expect(await screen.findByRole("dialog", { name: "全书伏笔" })).toBeVisible();
  expect(api.saveCard).toHaveBeenCalledTimes(1);
});

test("directory label title inspection grows linearly across overview and focused selectors", async () => {
  const api = makeAPI();
  let inspections = 0;
  const count = 120;
  vi.mocked(api.directory).mockResolvedValue(
    Array.from({ length: count }, (_, n) => ({
      ...position,
      chapter_id: n === 0 ? "c" : `source-${n}`,
      chapter_sort_order: n,
      get chapter_title() {
        inspections++;
        return `虚构章 ${n}`;
      },
    })),
  );
  display(api);
  await select();
  expect(
    await screen.findByRole("option", { name: "一卷 · 虚构章 119" }),
  ).toBeVisible();
  fireEvent.click(
    screen.getByRole("button", { name: "编辑主要埋设、回收与作者状态" }),
  );
  expect(
    await screen.findAllByRole("option", { name: "一卷 · 虚构章 119" }),
  ).toHaveLength(2);
  expect(inspections).toBeLessThan(count * 20);
});

test("returning from a card's proposal history restores focus to that card's trigger", async () => {
  const api = makeAPI();
  api.proposals = {
    list:vi.fn(async()=>({items:[],pending_count:0,next_after_id:null})),
    get:vi.fn(async()=>{throw Error("not expected")}),
    decide:vi.fn(async()=>{throw Error("not expected")}),
  };
  display(api);
  await select();
  fireEvent.click(await screen.findByRole("button",{name:"查看此卡的建议历史"}));
  fireEvent.click(await screen.findByRole("button",{name:"返回全书伏笔"}));
  await waitFor(()=>expect(screen.getByRole("button",{name:"查看此卡的建议历史"})).toHaveFocus());
});
