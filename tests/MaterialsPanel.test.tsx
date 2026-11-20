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
import MaterialsPanel from "../src/MaterialsPanel";
import type {
  Material,
  MaterialsAPI,
  MaterialProposal,
} from "../src/materialTypes";
afterEach(cleanup);
const card = (id: string, kind: Material["kind"] = "setting"): Material => ({
  id,
  book_id: "b",
  kind,
  title: id,
  fields:
    kind === "setting" ? { world: "世界" + id } : { content: "内容" + id },
  volume_id: null,
  chapter_id: null,
  revision: 1,
  deleted_at: null,
  updated_at: 1,
});
function setup(
  initialMaterialId?: string,
  cards = [card("甲"), card("乙"), card("纲", "outline")],
) {
  const api: MaterialsAPI = {
    list: vi.fn(async () => cards.map(({ fields, ...summary }) => summary)),
    get: vi.fn(async (id) => cards.find((c) => c.id === id)!),
    create: vi.fn(async (input) => ({
      ...input,
      id: "新",
      revision: 1,
      deleted_at: null,
      updated_at: 1,
    })),
    save: vi.fn(async (input) => ({
      ...cards.find((c) => c.id === input.id)!,
      ...input,
      revision: 2,
    })),
    trash: vi.fn(async (id) => ({
      ...cards.find((c) => c.id === id)!,
      deleted_at: 2,
      revision: 2,
    })),
    restore: vi.fn(async (id) => ({
      ...cards.find((c) => c.id === id)!,
      deleted_at: null,
      revision: 3,
    })),
  };
  const onClose = vi.fn(),
    onPendingChange = vi.fn();
  const view = render(
    <MaterialsPanel
      initialMaterialId={initialMaterialId}
      bookId="b"
      api={api}
      volumes={[
        {
          id: "v",
          book_id: "b",
          title: "第一卷",
          sort_order: 0,
          deleted_at: null,
        },
        {
          id: "foreign",
          book_id: "other",
          title: "外书卷",
          sort_order: 0,
          deleted_at: null,
        },
      ]}
      chapters={[
        {
          id: "c",
          volume_id: "v",
          title: "第一章",
          body: "",
          revision: 1,
          sort_order: 0,
          deleted_at: null,
        },
      ]}
      onClose={onClose}
      onPendingChange={onPendingChange}
    />,
  );
  return { api, onClose, view, onPendingChange };
}
it("opens one selected card and blocks leaving dirty fields until explicit save", async () => {
  const { api, onClose } = setup();
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "甲" }));
  await screen.findByDisplayValue("世界甲");
  expect(screen.queryByDisplayValue("世界乙")).toBeNull();
  fireEvent.change(screen.getByLabelText("世界观"), {
    target: { value: "修改世界" },
  });
  fireEvent.click(screen.getByRole("button", { name: "乙" }));
  fireEvent.click(screen.getByRole("button", { name: "关闭资料库" }));
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(onClose).not.toHaveBeenCalled();
  expect(screen.getByDisplayValue("修改世界")).toBeTruthy();
  expect(screen.getByRole("alert").textContent).toContain("保存");
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  await waitFor(() =>
    expect(api.save).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "甲",
        expected_revision: 1,
        fields: expect.objectContaining({ world: "修改世界" }),
      }),
    ),
  );
  await waitFor(() => expect(screen.getByText("已保存")).toBeTruthy());
  fireEvent.click(screen.getByRole("button", { name: "乙" }));
  await screen.findByDisplayValue("世界乙");
});
it("preserves draft on save conflict", async () => {
  const { api } = setup();
  vi.mocked(api.save).mockRejectedValue({ message: "版本冲突" });
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "甲" }));
  await screen.findByDisplayValue("世界甲");
  fireEvent.change(screen.getByLabelText("世界观"), {
    target: { value: "不能丢" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  await screen.findByText("版本冲突");
  expect(screen.getByDisplayValue("不能丢")).toBeTruthy();
});
it("filters outline targets and saves selected chapter association", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "大纲" }));
  fireEvent.click(screen.getByRole("button", { name: "纲" }));
  await screen.findByLabelText("关联范围");
  expect(screen.queryByRole("option", { name: "卷：外书卷" })).toBeNull();
  fireEvent.change(screen.getByLabelText("关联范围"), {
    target: { value: "chapter:c" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  await waitFor(() =>
    expect(api.save).toHaveBeenCalledWith(
      expect.objectContaining({ volume_id: null, chapter_id: "c" }),
    ),
  );
});
it("ignores late card responses after selecting another card", async () => {
  const { api } = setup();
  let finish!: (value: Material) => void;
  vi.mocked(api.get).mockImplementationOnce(
    () => new Promise((r) => (finish = r)),
  );
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "甲" }));
  fireEvent.click(screen.getByRole("button", { name: "乙" }));
  await screen.findByDisplayValue("世界乙");
  await act(async () => finish(card("甲")));
  expect(screen.queryByDisplayValue("世界甲")).toBeNull();
  expect(screen.getByDisplayValue("世界乙")).toBeTruthy();
});
it("shows recycled cards read-only and allows restore only", async () => {
  const { api } = setup();
  const deleted = { ...card("废"), deleted_at: 2 };
  vi.mocked(api.list).mockResolvedValue([deleted]);
  vi.mocked(api.get).mockResolvedValue(deleted);
  fireEvent.click(screen.getByRole("button", { name: "回收站" }));
  await screen.findByRole("button", { name: "废" });
  fireEvent.click(screen.getByRole("button", { name: "废" }));
  await screen.findByDisplayValue("世界废");
  expect(
    (screen.getByLabelText("世界观") as HTMLTextAreaElement).readOnly,
  ).toBe(true);
  expect(screen.queryByRole("button", { name: "永久删除" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "恢复资料" }));
  await waitFor(() => expect(api.restore).toHaveBeenCalledWith("废", 1));
});

it("reports unsaved state and saves with Ctrl+S without bubbling to the editor", async () => {
  const { api, onPendingChange } = setup();
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "甲" }));
  await screen.findByDisplayValue("世界甲");
  fireEvent.change(screen.getByLabelText("世界观"), {
    target: { value: "快捷保存" },
  });
  expect(onPendingChange).toHaveBeenLastCalledWith(true);
  const outside = vi.fn();
  document.addEventListener("keydown", outside);
  fireEvent.keyDown(screen.getByLabelText("世界观"), {
    key: "s",
    ctrlKey: true,
  });
  await waitFor(() => expect(api.save).toHaveBeenCalled());
  await waitFor(() => expect(onPendingChange).toHaveBeenLastCalledWith(false));
  expect(outside).not.toHaveBeenCalled();
  document.removeEventListener("keydown", outside);
});
it("retains the existing card after a failed read and a failed new-card creation", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "甲" }));
  await screen.findByDisplayValue("世界甲");
  vi.mocked(api.get).mockRejectedValueOnce(new Error("读取失败"));
  fireEvent.click(screen.getByRole("button", { name: "乙" }));
  await screen.findByText("读取失败");
  expect(screen.getByDisplayValue("世界甲")).toBeTruthy();
  vi.mocked(api.create).mockRejectedValueOnce(new Error("创建失败"));
  fireEvent.click(screen.getByRole("button", { name: "新增设定" }));
  await screen.findByText("创建失败");
  expect(screen.getByDisplayValue("世界甲")).toBeTruthy();
});
it("creates an editable blank card and disables structural actions while dirty", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "新增设定" }));
  await screen.findByDisplayValue("未命名设定");
  expect(api.create).toHaveBeenCalledWith(
    expect.objectContaining({
      book_id: "b",
      kind: "setting",
      fields: expect.objectContaining({ world: "" }),
    }),
  );
  fireEvent.change(screen.getByLabelText("资料标题"), {
    target: { value: "航海世界" },
  });
  expect(
    (screen.getByRole("button", { name: "新增设定" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(
    (screen.getByRole("button", { name: "移入回收站" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "角色" }));
  expect(screen.getByDisplayValue("航海世界")).toBeTruthy();
});
it("contains keyboard focus and releases pending status on unmount", async () => {
  const outside = document.createElement("button");
  document.body.appendChild(outside);
  outside.focus();
  const { view, onPendingChange } = setup();
  await screen.findByRole("button", { name: "甲" });
  outside.focus();
  expect(screen.getByRole("dialog").contains(document.activeElement)).toBe(
    true,
  );
  fireEvent.click(screen.getByRole("button", { name: "甲" }));
  await screen.findByDisplayValue("世界甲");
  fireEvent.change(screen.getByLabelText("世界观"), {
    target: { value: "待保存" },
  });
  expect(onPendingChange).toHaveBeenLastCalledWith(true);
  view.unmount();
  expect(onPendingChange).toHaveBeenLastCalledWith(false);
  expect(document.activeElement).toBe(outside);
  outside.remove();
});
it("does not let an older list response erase a newly created card", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: "甲" });
  let finish!: (value: []) => void;
  vi.mocked(api.list).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  fireEvent.click(screen.getByRole("button", { name: "设定" }));
  fireEvent.click(screen.getByRole("button", { name: "新增设定" }));
  await screen.findByRole("button", { name: "未命名设定" });
  await act(async () => finish([]));
  expect(screen.getByRole("button", { name: "未命名设定" })).toBeTruthy();
});
it("creates a summary only after choosing its chapter source", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "摘要" }));
  expect(
    (screen.getByRole("button", { name: "新增摘要" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  fireEvent.change(screen.getByLabelText("新摘要来源章节"), {
    target: { value: "c" },
  });
  fireEvent.click(screen.getByRole("button", { name: "新增摘要" }));
  await waitFor(() =>
    expect(api.create).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: "summary",
        source_chapter_id: "c",
        source_revision: 1,
        fields: { content: "" },
      }),
    ),
  );
});
it("preserves stale source revision on ordinary saves until explicit verified refresh", async () => {
  const { api } = setup();
  vi.mocked(api.get).mockResolvedValue({
    ...card("甲"),
    source_chapter_id: "c",
    source_revision: 0,
    source_status: "stale",
  });
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "甲" }));
  await screen.findByText("来源正文已更新，待复核");
  fireEvent.change(screen.getByLabelText("世界观"), {
    target: { value: "修改" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  await waitFor(() =>
    expect(api.save).toHaveBeenCalledWith(
      expect.objectContaining({ source_revision: 0, source_chapter_id: "c" }),
    ),
  );
  await screen.findByText("已保存");
  fireEvent.click(screen.getByRole("button", { name: "按当前章节版本复核" }));
  expect(screen.getByText(/请确认已核对/)).toBeTruthy();
  fireEvent.click(
    screen.getByRole("button", { name: "已核对内容，更新来源版本" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  await waitFor(() =>
    expect(api.save).toHaveBeenLastCalledWith(
      expect.objectContaining({ source_revision: 1 }),
    ),
  );
});
it("reviews full fields and requires a bound confirmation before material acceptance", async () => {
  const { api } = setup();
  api.review = {
    list: vi.fn(async () => [
      {
        id: "p",
        client_id: "agent",
        material_id: "甲",
        base_revision: 1,
        old_snapshot: card("甲"),
        new_title: "建议标题",
        new_fields: { world: "建议世界" },
        new_source_revision: null,
        reason: "更新设定",
        status: "pending",
        created_at: 1,
      },
    ]),
    decide: vi.fn(async () => ({
      ...card("甲"),
      title: "建议标题",
      fields: { world: "建议世界" },
      revision: 2,
    })),
  };
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "甲" }));
  await screen.findByDisplayValue("世界甲");
  fireEvent.click(screen.getByRole("button", { name: "待审资料提案" }));
  await screen.findByText("建议世界");
  expect(screen.getByText("世界甲")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "采纳资料提案" }));
  expect(api.review.decide).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "确认采纳资料提案" }));
  await waitFor(() =>
    expect(api.review!.decide).toHaveBeenCalledWith("p", true, 1),
  );
  await screen.findByDisplayValue("建议世界");
});
it("blocks entering material review with dirty content", async () => {
  const { api } = setup();
  api.review = { list: vi.fn(async () => []), decide: vi.fn() };
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "甲" }));
  await screen.findByDisplayValue("世界甲");
  fireEvent.change(screen.getByLabelText("世界观"), {
    target: { value: "手稿" },
  });
  fireEvent.click(screen.getByRole("button", { name: "待审资料提案" }));
  expect(api.review.list).not.toHaveBeenCalled();
  expect(screen.getByDisplayValue("手稿")).toBeTruthy();
  expect(screen.getByRole("alert").textContent).toContain("保存");
});

it("opens a verified initial search result only once and lets the user choose another card", async () => {
  setup("甲");
  await screen.findByDisplayValue("世界甲");
  fireEvent.click(screen.getByRole("button", { name: "乙" }));
  await screen.findByDisplayValue("世界乙");
  fireEvent.click(screen.getByRole("button", { name: "设定" }));
  await screen.findByText("点击一条资料开始查看或编辑。");
  expect(screen.queryByDisplayValue("世界甲")).toBeNull();
});
it("does not retrieve an unknown initial search result", async () => {
  const { api } = setup("foreign-id");
  await screen.findByRole("button", { name: "甲" });
  expect(api.get).not.toHaveBeenCalled();
});
it("preserves a proposal and original card after a conflicting acceptance", async () => {
  const { api } = setup();
  api.review = {
    list: vi.fn(async () => [
      {
        id: "p",
        client_id: "agent",
        material_id: "甲",
        base_revision: 1,
        old_snapshot: card("甲"),
        new_title: "提案标题",
        new_fields: { world: "提案内容" },
        new_source_revision: null,
        reason: "理由",
        status: "pending",
        created_at: 1,
      },
    ]),
    decide: vi.fn(async () => {
      throw new Error("资料版本冲突");
    }),
  };
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "甲" }));
  await screen.findByDisplayValue("世界甲");
  fireEvent.click(screen.getByRole("button", { name: "待审资料提案" }));
  await screen.findByText("提案内容");
  fireEvent.click(screen.getByRole("button", { name: "采纳资料提案" }));
  expect(
    (screen.getByRole("button", { name: "刷新资料提案" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "确认采纳资料提案" }));
  await screen.findByText("资料版本冲突");
  expect(screen.getByText("提案内容")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "返回资料编辑" }));
  await screen.findByDisplayValue("世界甲");
});
it("allows a human to attach a chapter source without confusing the outline association", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "甲" }));
  await screen.findByDisplayValue("世界甲");
  fireEvent.change(screen.getByLabelText("来源章节"), {
    target: { value: "c" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  await waitFor(() =>
    expect(api.save).toHaveBeenCalledWith(
      expect.objectContaining({
        source_chapter_id: "c",
        source_revision: 1,
        chapter_id: null,
        volume_id: null,
      }),
    ),
  );
});
it("keeps an unavailable source identity on ordinary edits", async () => {
  const { api } = setup();
  vi.mocked(api.get).mockResolvedValue({
    ...card("甲"),
    source_chapter_id: "removed-source",
    source_revision: 7,
    source_status: "unavailable",
  });
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "甲" }));
  await screen.findByText("来源不可用");
  expect((screen.getByLabelText("来源章节") as HTMLSelectElement).value).toBe(
    "removed-source",
  );
  expect(
    screen.queryByRole("button", { name: "按当前章节版本复核" }),
  ).toBeNull();
  fireEvent.change(screen.getByLabelText("世界观"), {
    target: { value: "保留历史来源" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  await waitFor(() =>
    expect(api.save).toHaveBeenCalledWith(
      expect.objectContaining({
        source_chapter_id: "removed-source",
        source_revision: 7,
      }),
    ),
  );
});
it("ignores a late proposal list after switching to another material", async () => {
  const { api } = setup();
  let finish!: (value: MaterialProposal[]) => void;
  api.review = {
    list: vi.fn(
      () =>
        new Promise<MaterialProposal[]>((resolve) => {
          finish = resolve;
        }),
    ),
    decide: vi.fn(),
  };
  await screen.findByRole("button", { name: "甲" });
  fireEvent.click(screen.getByRole("button", { name: "甲" }));
  await screen.findByDisplayValue("世界甲");
  fireEvent.click(screen.getByRole("button", { name: "待审资料提案" }));
  await screen.findByText("读取提案中…");
  fireEvent.click(screen.getByRole("button", { name: "乙" }));
  await screen.findByDisplayValue("世界乙");
  await act(async () =>
    finish([
      {
        id: "late",
        client_id: "a",
        material_id: "甲",
        base_revision: 1,
        old_snapshot: card("甲"),
        new_title: "迟到标题",
        new_fields: { world: "迟到内容" },
        new_source_revision: null,
        reason: "晚到",
        status: "pending",
        created_at: 1,
      },
    ]),
  );
  expect(screen.queryByText("迟到内容")).toBeNull();
  expect(screen.getByDisplayValue("世界乙")).toBeTruthy();
});
it.each([
  ["current", 0, false],
  ["current", 1, true],
  ["unavailable", 1, false],
] as const)(
  "checks proposal source against visible current chapter (%s, revision %s)",
  async (status, newRevision, allowed) => {
    const { api } = setup();
    const sourceId = status === "unavailable" ? "missing" : "c";
    const original = {
      ...card("甲"),
      source_chapter_id: sourceId,
      source_revision: 0,
      source_status: "stale" as const,
    };
    vi.mocked(api.get).mockResolvedValue(original);
    api.review = {
      list: vi.fn(async () => [
        {
          id: "source-p",
          client_id: "agent",
          material_id: "甲",
          base_revision: 1,
          old_snapshot: original,
          new_title: "带来源建议",
          new_fields: { world: "新内容" },
          new_source_revision: newRevision,
          reason: "复核",
          status: "pending",
          created_at: 1,
        },
      ]),
      decide: vi.fn(),
    };
    await screen.findByRole("button", { name: "甲" });
    fireEvent.click(screen.getByRole("button", { name: "甲" }));
    await screen.findByDisplayValue("世界甲");
    fireEvent.click(screen.getByRole("button", { name: "待审资料提案" }));
    await screen.findByText("新内容");
    expect(
      (
        screen.getByRole("button", {
          name: "采纳资料提案",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(!allowed);
    if (status !== "unavailable") {
      expect(screen.getByText(/来源章节：第一章/)).toBeTruthy();
      expect(screen.getByText(/当前来源版本：1/)).toBeTruthy();
    }
    if (!allowed) expect(screen.getByText(/请重新提交提案/)).toBeTruthy();
  },
);

const tracked = (
  id: string,
  kind: "timeline" | "foreshadow",
  order = 0,
  status = "planned",
): Material => ({
  ...card(id, kind),
  story_order: order,
  story_time: null,
  tracking_status: status,
  payoff_chapter_id: null,
  payoff_revision: null,
  payoff_status: "none",
  fields:
    kind === "timeline"
      ? { content: "事件正文" + id }
      : { setup: "埋设正文" + id, resolution: "" },
});
it("sorts timeline metadata by numeric story order without loading all contents", async () => {
  const { api } = setup(undefined, [
    tracked("后", "timeline", 20),
    tracked("前", "timeline", 2),
  ]);
  fireEvent.click(screen.getByRole("button", { name: "时间线" }));
  await screen.findByRole("button", { name: /前/ });
  const buttons = screen
    .getAllByRole("button")
    .filter((b) => /前|后/.test(b.textContent ?? ""));
  expect(buttons.map((b) => b.textContent)).toEqual([
    "2 · 计划 · 前",
    "20 · 计划 · 后",
  ]);
  expect(api.get).not.toHaveBeenCalled();
  fireEvent.click(buttons[0]);
  await screen.findByDisplayValue("事件正文前");
  expect(screen.queryByDisplayValue("事件正文后")).toBeNull();
});
it("creates timeline and foreshadow with constrained default status and no proposal actions", async () => {
  const { api } = setup();
  api.review = { list: vi.fn(async () => []), decide: vi.fn() };
  fireEvent.click(screen.getByRole("button", { name: "时间线" }));
  fireEvent.click(screen.getByRole("button", { name: "新增时间线" }));
  await screen.findByDisplayValue("未命名时间线");
  expect(api.create).toHaveBeenCalledWith(
    expect.objectContaining({
      kind: "timeline",
      story_order: 0,
      tracking_status: "planned",
      fields: { content: "" },
    }),
  );
  expect(screen.queryByRole("button", { name: "待审资料提案" })).toBeNull();
  expect(
    screen.getAllByRole("option").some((o) => o.textContent === "已发生"),
  ).toBe(true);
});
it("blocks status filters while dirty and keeps stale tracking sources on ordinary save", async () => {
  const original = {
    ...tracked("线索", "foreshadow", 3, "planted"),
    source_chapter_id: "c",
    source_revision: 0,
    source_status: "stale" as const,
    payoff_chapter_id: "missing",
    payoff_revision: 8,
    payoff_status: "unavailable" as const,
  };
  const { api } = setup("线索", [original]);
  await screen.findByDisplayValue("埋设正文线索");
  fireEvent.change(screen.getByLabelText("埋设内容"), {
    target: { value: "改写线索" },
  });
  fireEvent.change(screen.getByLabelText("伏笔状态筛选"), {
    target: { value: "resolved" },
  });
  expect(
    (screen.getByLabelText("伏笔状态筛选") as HTMLSelectElement).value,
  ).toBe("all");
  expect(screen.getByDisplayValue("改写线索")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  await waitFor(() =>
    expect(api.save).toHaveBeenCalledWith(
      expect.objectContaining({
        source_revision: 0,
        payoff_chapter_id: "missing",
        payoff_revision: 8,
        tracking_status: "planted",
      }),
    ),
  );
});
it("requires both current chapter sources and resolution before marking a foreshadow resolved", async () => {
  const { api } = setup("线索", [tracked("线索", "foreshadow")]);
  await screen.findByDisplayValue("埋设正文线索");
  fireEvent.change(screen.getByLabelText("跟踪状态"), {
    target: { value: "resolved" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  expect(api.save).not.toHaveBeenCalled();
  expect(screen.getByRole("alert").textContent).toContain("回收");
  fireEvent.change(screen.getByLabelText("来源章节"), {
    target: { value: "c" },
  });
  fireEvent.change(screen.getByLabelText("回收章节"), {
    target: { value: "c" },
  });
  fireEvent.change(screen.getByLabelText("回收说明"), {
    target: { value: "身份揭晓" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  await waitFor(() =>
    expect(api.save).toHaveBeenCalledWith(
      expect.objectContaining({
        tracking_status: "resolved",
        source_revision: 1,
        payoff_revision: 1,
        fields: { setup: "埋设正文线索", resolution: "身份揭晓" },
      }),
    ),
  );
});
it("retains the entered draft when invalid story order or backend conflict prevents saving", async () => {
  const { api } = setup("事件", [tracked("事件", "timeline")]);
  await screen.findByDisplayValue("事件正文事件");
  fireEvent.change(screen.getByLabelText("故事位置"), {
    target: { value: "-2" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  expect(api.save).not.toHaveBeenCalled();
  expect(screen.getByRole("alert").textContent).toContain("故事位置");
  fireEvent.change(screen.getByLabelText("故事位置"), {
    target: { value: "12" },
  });
  vi.mocked(api.save).mockRejectedValue(new Error("版本冲突"));
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  await screen.findByText("版本冲突");
  expect((screen.getByLabelText("故事位置") as HTMLInputElement).value).toBe(
    "12",
  );
});
it("refuses an occurred event without current source and saves after manual source selection", async () => {
  const { api } = setup("事件", [tracked("事件", "timeline")]);
  await screen.findByDisplayValue("事件正文事件");
  fireEvent.change(screen.getByLabelText("跟踪状态"), {
    target: { value: "written" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  expect(api.save).not.toHaveBeenCalled();
  expect(screen.getByRole("alert").textContent).toContain("来源");
  fireEvent.change(screen.getByLabelText("来源章节"), {
    target: { value: "c" },
  });
  fireEvent.change(screen.getByLabelText("故事时间"), {
    target: { value: "第三日夜" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  await waitFor(() =>
    expect(api.save).toHaveBeenCalledWith(
      expect.objectContaining({
        tracking_status: "written",
        source_chapter_id: "c",
        source_revision: 1,
        story_time: "第三日夜",
      }),
    ),
  );
});
it("refreshes only the explicitly verified payoff version and retains the stale planted source", async () => {
  const original = {
    ...tracked("伏笔", "foreshadow", 2, "planted"),
    source_chapter_id: "c",
    source_revision: 0,
    source_status: "stale" as const,
    payoff_chapter_id: "c",
    payoff_revision: 0,
    payoff_status: "stale" as const,
  };
  const { api } = setup("伏笔", [original]);
  await screen.findByDisplayValue("埋设正文伏笔");
  fireEvent.click(
    screen.getByRole("button", { name: "按当前回收章节版本复核" }),
  );
  expect(api.save).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "已核对回收内容，更新版本" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  await waitFor(() =>
    expect(api.save).toHaveBeenCalledWith(
      expect.objectContaining({
        source_revision: 0,
        payoff_revision: 1,
        tracking_status: "planted",
      }),
    ),
  );
});
it("rejects overly long UTF-8 story labels without discarding the label", async () => {
  const { api } = setup("事件", [tracked("事件", "timeline")]);
  await screen.findByDisplayValue("事件正文事件");
  const text = "中".repeat(86);
  fireEvent.change(screen.getByLabelText("故事时间"), {
    target: { value: text },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
  expect(api.save).not.toHaveBeenCalled();
  expect(screen.getByRole("alert").textContent).toContain("256");
  expect(screen.getByDisplayValue(text)).toBeTruthy();
});
it("filters resolved foreshadow metadata and restores a recycled tracking card without status changes", async () => {
  const done = {
    ...tracked("完结伏笔", "foreshadow", 1, "resolved"),
    fields: { setup: "埋设", resolution: "已解释" },
    source_chapter_id: "c",
    source_revision: 1,
    payoff_chapter_id: "c",
    payoff_revision: 1,
  };
  const deleted = {
    ...tracked("已删事件", "timeline", 9, "written"),
    deleted_at: 2,
  };
  const { api } = setup(undefined, [
    done,
    tracked("待办伏笔", "foreshadow", 3),
    deleted,
  ]);
  fireEvent.click(screen.getByRole("button", { name: "伏笔" }));
  await screen.findByRole("button", { name: /完结伏笔/ });
  fireEvent.change(screen.getByLabelText("伏笔状态筛选"), {
    target: { value: "resolved" },
  });
  expect(screen.queryByRole("button", { name: /待办伏笔/ })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "回收站" }));
  await screen.findByRole("button", { name: /已删事件/ });
  fireEvent.click(screen.getByRole("button", { name: /已删事件/ }));
  await screen.findByDisplayValue("事件正文已删事件");
  expect(
    (screen.getByLabelText("跟踪状态") as HTMLSelectElement).disabled,
  ).toBe(true);
  expect((screen.getByLabelText("故事位置") as HTMLInputElement).readOnly).toBe(
    true,
  );
  fireEvent.click(screen.getByRole("button", { name: "恢复资料" }));
  await waitFor(() => expect(api.restore).toHaveBeenCalledWith("已删事件", 1));
  expect(api.save).not.toHaveBeenCalled();
});
it.each(["source", "payoff"] as const)(
  "allows independently rechecking the %s of a resolved clue while retaining the other unavailable source",
  async (which) => {
    const original = {
      ...tracked("已解伏笔", "foreshadow", 4, "resolved"),
      fields: { setup: "身份线索", resolution: "已揭晓身份" },
      source_chapter_id: which === "source" ? "c" : "lost-source",
      source_revision: 0,
      source_status:
        which === "source" ? ("stale" as const) : ("unavailable" as const),
      payoff_chapter_id: which === "payoff" ? "c" : "lost-payoff",
      payoff_revision: 0,
      payoff_status:
        which === "payoff" ? ("stale" as const) : ("unavailable" as const),
    };
    const { api } = setup("已解伏笔", [original]);
    await screen.findByDisplayValue("已揭晓身份");
    fireEvent.click(
      screen.getByRole("button", {
        name:
          which === "source" ? "按当前章节版本复核" : "按当前回收章节版本复核",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name:
          which === "source"
            ? "已核对内容，更新来源版本"
            : "已核对回收内容，更新版本",
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
    await waitFor(() =>
      expect(api.save).toHaveBeenCalledWith(
        expect.objectContaining({
          tracking_status: "resolved",
          source_chapter_id: original.source_chapter_id,
          source_revision: which === "source" ? 1 : 0,
          payoff_chapter_id: original.payoff_chapter_id,
          payoff_revision: which === "payoff" ? 1 : 0,
        }),
      ),
    );
    await screen.findByText("已保存");
  },
);
