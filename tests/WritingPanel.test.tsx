import "./writingWebCrypto";
import "@testing-library/jest-dom/vitest";
import React from "react";
import {
  render,
  screen,
  fireEvent,
  act,
  waitFor,
  cleanup,
  within,
} from "@testing-library/react";
import { it, expect, vi, afterEach } from "vitest";
// @ts-expect-error Node filesystem is available in Vitest only (same as TasksPanel tests).
import { readFileSync } from "node:fs";
import outlineFixture from "../qa/acceptance/outline-conflict.json";
import WritingPanel from "../src/WritingPanel";
import FrozenOutlineComparison from "../src/FrozenOutlineComparison";
import type { AuthorReviewItem, WritingPack, WritingProposal } from "../src/writingTypes";
import { WritingDecisionSession } from "../src/writingDecision";
afterEach(cleanup);
const chapter = {
  id: "c",
  volume_id: "v",
  title: "章",
  body: "原稿",
  revision: 0,
  sort_order: 0,
  deleted_at: null,
};
const policy = {
  chapter_id: "c",
  book_id: "b",
  revision: 0,
  mode: "active",
  goal: "过河",
  required_material_ids: ["m"],
  updated_at: 1,
};
const card = {
  id: "m",
  book_id: "b",
  title: "规则",
  kind: "setting",
  fields: { synopsis: "不能游泳" },
  revision: 0,
  deleted_at: null,
};
const state = {
  epoch: "e",
  policy,
  packs: [],
  proposals: [
    {
      id: "q",
      pack_id: "p",
      decision: "pending",
      archived: false,
      created_at: 1,
    },
  ],
};
const proposal = {
  id: "q",
  book_id: "b",
  chapter_id: "c",
  client_id: "a",
  base_revision: 0,
  decision: "pending",
  live_epoch: "e",
  accepted_revision: null,
  input: {
    pack_id: "p",
    new_body: "坐船",
    idempotency_key: "k",
    citations: [
      {
        entry_id: "m",
        field_key: "synopsis",
        start: 0,
        end: 2,
        explanation: "遵循设定",
      },
    ],
    conflicts: [{ entry_ids: ["m"], explanation: "人工判定" }],
  },
};
function review(revision: number, items: unknown[]) {
  return {
    proposal_id: "q",
    pack_id: "p",
    book_id: "b",
    chapter_id: "c",
    originating_epoch: "e",
    policy_revision: 0,
    chapter_revision: 0,
    revision,
    items,
  };
}
const reviewed = {
  ...proposal,
  author_review: review(1, [
    { material_id: "m", verdict: "conforms", note: "核对" },
  ]),
};
function mount(
  overrides: any = {},
  reload: any = async () => chapter,
  decisionSession?: WritingDecisionSession,
  sourceReader?: any,
) {
  let stored: any = proposal;
  const api = {
    epoch: vi.fn(async () => "e"),
    state: vi.fn(async () => state),
    contextDirectory: vi.fn(async () => ({
      ...contextDirectory(),
      chapters: [],
      volumes: [],
    })),
    configure: vi.fn(async () => policy),
    prepare: vi.fn(async () => ({ id: "p" })),
    pack: vi.fn(async () => ({
      id: "p",
      originating_epoch: "e",
      expires_at: Date.now() / 1000 + 1800,
      book_id: "b",
      chapter_id: "c",
      chapter,
      policy,
      materials: [card],
    })),
    proposal: vi.fn(async () => stored),
    saveReview: vi.fn(async (_book, _id, expected, _epoch, items) => {
      const receipt = review((expected ?? 0) + 1, items);
      stored = { ...stored, author_review: receipt };
      return receipt;
    }),
    decide: vi.fn(async () => ({ ...stored, decision: "rejected" })),
    acceptReviewed: vi.fn(async () => ({
      ...proposal,
      decision: "accepted",
      accepted_revision: 1,
    })),
    ...overrides,
  };
  const close = vi.fn();
  render(
    <WritingPanel
      api={api as any}
      bookId="b"
      chapter={chapter}
      materials={{ list: async () => [card] } as any}
      onReload={reload}
      onReadDecisionChapter={async () => ({ chapter: await reload() })}
      decisionSession={decisionSession}
      {...{sourceReader}}
      onClose={close}
      onPendingChange={() => {}}
    />,
  );
  return { api, close };
}
it("requires explicit author conflict acknowledgement and shows frozen quotes", async () => {
  const { api } = mount();
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  expect(await screen.findByText("不能")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "采纳建议" })).toBeDisabled();
  fireEvent.change(screen.getByLabelText("作者判断：规则"), {
    target: { value: "conforms" },
  });
  fireEvent.change(screen.getByLabelText("作者备注：规则"), {
    target: { value: "核对" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存逐项审阅" }));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "采纳建议" })).toBeEnabled(),
  );
  expect(api.saveReview).toHaveBeenCalledWith("b", "q", null, "e", [
    { material_id: "m", verdict: "conforms", note: "核对" },
  ]);
  fireEvent.click(screen.getByRole("button", { name: "采纳建议" }));
  expect(screen.getByRole("button", { name: "确认采纳" })).toBeDisabled();
  fireEvent.click(screen.getByLabelText("已人工核对并接受上述未解决冲突"));
  fireEvent.click(screen.getByRole("button", { name: "确认采纳" }));
  await waitFor(() =>
    expect(api.acceptReviewed).toHaveBeenCalledWith(
      "b",
      "q",
      0,
      0,
      "e",
      1,
      true,
    ),
  );
  expect(screen.getByText(/不代表语义一致/)).toBeInTheDocument();
});
it("unknown write receipt reconciles only and blocks close if read fails", async () => {
  let fail = false;
  const read = vi.fn(async () => {
    if (fail) throw Error("读取暂不可用");
    return state;
  });
  const { api, close } = mount({
    state: read,
    configure: vi.fn(async () => {
      fail = true;
      throw Error("回执丢失");
    }),
  });
  await screen.findByDisplayValue("过河");
  fireEvent.click(screen.getByRole("button", { name: "保存约束配置" }));
  fireEvent.click(screen.getByRole("button", { name: "确认更新配置" }));
  await screen.findByRole("button", { name: "重新读取核对" });
  expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "重新读取核对" }));
  await waitFor(() => expect(read.mock.calls.length).toBeGreaterThan(2));
  expect(api.configure).toHaveBeenCalledTimes(1);
  expect(close).not.toHaveBeenCalled();
  fail = false;
  fireEvent.click(screen.getByRole("button", { name: "重新读取核对" }));
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "关闭写作约束" }),
    ).not.toBeDisabled(),
  );
  expect(api.configure).toHaveBeenCalledTimes(1);
});
it("ignores late review after unmount and never accepts a superseded selection", async () => {
  let resolve: (v: any) => void = () => {};
  const get = vi.fn(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  const { api } = mount({ proposal: get });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  cleanup();
  resolve(proposal);
  await Promise.resolve();
  expect(api.decide).not.toHaveBeenCalled();
});
it("shows an unreported semantic contradiction without certifying it", async () => {
  mount({
    proposal: async () => ({
      ...proposal,
      input: {
        ...proposal.input,
        new_body: "他毫不费力地游过了河。",
        conflicts: [],
      },
    }),
  });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  expect(await screen.findByText("他毫不费力地游过了河。")).toBeInTheDocument();
  expect(
    screen.getByText("未自报冲突，不等于系统验证无冲突。"),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "拒绝建议" }));
  fireEvent.click(screen.getByRole("button", { name: "确认拒绝" }));
});
it("preserves exact accepted receipt without settling from a pending full read", async () => {
  let advanced = false;
  const reload = vi.fn(async () =>
    advanced ? { ...chapter, revision: 1, body: "坐船" } : chapter,
  );
  const acceptReviewed = vi.fn(async () => acceptedDecision);
  mount({
    proposal: async () => advanced ? acceptedDecision : reviewed,
    // The summary stays pending even after the independently verified result.
    acceptReviewed,
  }, reload);
  await confirmStrictDecision();
  await screen.findByText(/仍待核对.*完整提案仍待审/);
  expect(screen.getByText(/写入回执候选.*未核实.*已采纳/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "采纳建议" })).toBeDisabled();
  advanced = true;
  fireEvent.click(screen.getByRole("button", { name: "只读核对建议 q" }));
  await screen.findByText(/当前正文已核对/);
  expect(screen.queryByRole("button", { name: "只读核对建议 q" })).not.toBeInTheDocument();
  expect(acceptReviewed).toHaveBeenCalledTimes(1);
});
it("changing configuration after opening acceptance invalidates final confirmation", async () => {
  const { api } = mount({ proposal: async () => reviewed });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  await screen.findByText("不能");
  fireEvent.click(screen.getByRole("button", { name: "采纳建议" }));
  fireEvent.click(screen.getByLabelText("已人工核对并接受上述未解决冲突"));
  fireEvent.change(screen.getByLabelText("写作目标"), {
    target: { value: "新目标尚未保存" },
  });
  const final = screen.queryByRole("button", { name: "确认采纳" });
  if (final) fireEvent.click(final);
  expect(api.acceptReviewed).not.toHaveBeenCalled();
  expect(screen.getByDisplayValue("新目标尚未保存")).toBeInTheDocument();
});
it("renders namespaced material quotes even when imported card id was reserved", async () => {
  const m = { ...card, id: "$chapter" };
  mount({
    pack: async () => ({
      id: "p",
      handle_version: 2,
      book_id: "b",
      chapter_id: "c",
      chapter,
      policy,
      expires_at: Date.now() / 1000 + 1800,
      materials: [m],
    }),
    proposal: async () => ({
      ...proposal,
      input: {
        ...proposal.input,
        citations: [
          {
            entry_id: "material:$chapter",
            field_key: "synopsis",
            start: 0,
            end: 2,
            explanation: "真实资料字段",
          },
        ],
      },
    }),
  });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  expect(await screen.findByText("不能")).toBeInTheDocument();
  expect(screen.queryByText("引用字段不存在")).not.toBeInTheDocument();
});
it("replaces a missing required card through confirmed configuration and freezes the replacement", async () => {
  let current: any = {
    ...state,
    policy: { ...policy, required_material_ids: ["missing-A"] },
    packs: [
      {
        id: "old-evidence",
        policy_revision: 0,
        chapter_revision: 0,
        expires_at: 1,
        archived: true,
      },
    ],
  };
  const configure = vi.fn(
    async (
      _book: string,
      _chapter: string,
      _revision: number,
      _mode: string,
      _goal: string,
      ids: string[],
    ) => {
      current = {
        ...current,
        policy: { ...current.policy, revision: 1, required_material_ids: ids },
      };
      return current.policy;
    },
  );
  const prepare = vi.fn(async () => {
    current = {
      ...current,
      packs: [
        ...current.packs,
        {
          id: "new-pack",
          policy_revision: 1,
          chapter_revision: 0,
          expires_at: Date.now() / 1000 + 1800,
          archived: false,
        },
      ],
    };
    return { id: "new-pack" };
  });
  const { api } = mount({ state: async () => current, configure, prepare });
  fireEvent.click(
    await screen.findByRole("button", { name: "移除不可用资料 missing-A" }),
  );
  expect(screen.getByRole("button", { name: "保存约束配置" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "冻结当前依据包" })).toBeDisabled();
  fireEvent.click(screen.getByLabelText("规则"));
  fireEvent.click(screen.getByRole("button", { name: "保存约束配置" }));
  expect(api.configure).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "确认更新配置" }));
  await waitFor(() =>
    expect(configure).toHaveBeenCalledWith(
      "b",
      "c",
      0,
      "active",
      "过河",
      ["m"],
      "e",
      [],
    ),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "冻结当前依据包" }),
    ).not.toBeDisabled(),
  );
  expect(screen.queryByText(/不可用资料：missing-A/)).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: /查看依据 old-evidence/ }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "冻结当前依据包" }));
  await waitFor(() =>
    expect(prepare).toHaveBeenCalledWith("b", "c", 1, 0, "e"),
  );
  await screen.findByRole("button", { name: /查看依据 new-pack/ });
  expect(
    screen.getByRole("button", { name: /查看依据 old-evidence/ }),
  ).toBeInTheDocument();
});
it("does not allow an unreviewed proposal to replace the manuscript", async () => {
  const { api } = mount();
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  await screen.findByText("不能");
  expect(screen.getByRole("button", { name: "采纳建议" })).toBeDisabled();
  expect(api.decide).not.toHaveBeenCalled();
});

it("local review edits invalidate confirmation and block losing the draft", async () => {
  const { api, close } = mount({ proposal: async () => reviewed });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  await screen.findByText("不能");
  fireEvent.click(screen.getByRole("button", { name: "采纳建议" }));
  fireEvent.click(screen.getByLabelText("已人工核对并接受上述未解决冲突"));
  fireEvent.change(screen.getByLabelText("作者判断：规则"), {
    target: { value: "question" },
  });
  expect(
    screen.queryByRole("button", { name: "确认采纳" }),
  ).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "刷新依据与建议" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "查看建议 q" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "拒绝建议" })).toBeDisabled();
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(close).not.toHaveBeenCalled();
  expect(api.acceptReviewed).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "放弃未保存的审阅修改" }));
  expect(screen.getByRole("button", { name: "采纳建议" })).toBeEnabled();
});

it("does not repeat a review save if its receipt was lost or a reread failed", async () => {
  let stored: any = proposal;
  let readUnavailable = false;
  const saveReview = vi.fn(async (_b, _q, _expected, _e, items) => {
    stored = { ...proposal, author_review: review(1, items) };
    readUnavailable = true;
    throw Error("回执丢失");
  });
  const { api, close } = mount({
    saveReview,
    proposal: async () => {
      if (readUnavailable) throw Error("暂不能重读");
      return stored;
    },
  });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  await screen.findByText("不能");
  fireEvent.change(screen.getByLabelText("作者备注：规则"), {
    target: { value: "作者手记" },
  });
  const save = screen.getByRole("button", { name: "保存逐项审阅" });
  fireEvent.click(save);
  fireEvent.click(save);
  await screen.findByText("暂不能重读");
  expect(screen.getByDisplayValue("作者手记")).toBeInTheDocument();
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(close).not.toHaveBeenCalled();
  expect(saveReview).toHaveBeenCalledTimes(1);
  readUnavailable = false;
  fireEvent.click(screen.getByRole("button", { name: "重新读取核对" }));
  await screen.findByText(/已核对保存的逐项审阅版本 1/);
  expect(screen.getByDisplayValue("作者手记")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "采纳建议" })).toBeDisabled();
  expect(api.saveReview).toHaveBeenCalledTimes(1);
});

it("shows legacy accepted proposals without inventing author judgments", async () => {
  mount({
    proposal: async () => ({
      ...proposal,
      decision: "accepted",
      accepted_revision: 1,
    }),
  });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  expect(await screen.findByText("旧版未记录逐项审阅")).toBeInTheDocument();
  expect(screen.queryByLabelText("作者判断：规则")).not.toBeInTheDocument();
});

it("keeps an unsaved draft visible without silently rebasing over a newer author judgment", async () => {
  let stored: any = reviewed;
  const saveReview = vi.fn(async () => {
    stored = {
      ...proposal,
      author_review: review(2, [
        { material_id: "m", verdict: "conflict", note: "另一份已保存判断" },
      ]),
    };
    throw Error("审阅版本已经改变");
  });
  mount({ saveReview, proposal: async () => stored });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  await screen.findByText("不能");
  fireEvent.change(screen.getByLabelText("作者备注：规则"), {
    target: { value: "我的草稿" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存逐项审阅" }));
  await screen.findByText(/另一份已保存判断/);
  expect(screen.getByDisplayValue("我的草稿")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "保存逐项审阅" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "采纳建议" })).toBeDisabled();
  fireEvent.click(
    screen.getByRole("button", { name: "已核对较新版本，保留我的草稿" }),
  );
  expect(screen.getByRole("button", { name: "保存逐项审阅" })).toBeEnabled();
  expect(saveReview).toHaveBeenCalledTimes(1);
});

it.each([false, true])(
  "preserves a prior-epoch config draft without offering to carry it over (known receipt: %s)",
  async (known) => {
    let current = state;
    const configure = vi.fn(async () => {
      current = {
        ...state,
        epoch: "restored-epoch",
        policy: { ...policy, revision: 7, goal: "恢复后的约束" },
      };
      if (!known) throw Error("回执丢失");
      return { ...policy, revision: 1, goal: "旧书库草稿" };
    });
    const { close } = mount({
      state: async () => current,
      epoch: async () => current.epoch,
      configure,
      contextDirectory: async () => ({
        ...contextDirectory(current.epoch),
        chapters: [],
        volumes: [],
      }),
    });
    fireEvent.change(await screen.findByLabelText("写作目标"), {
      target: { value: "旧书库草稿" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存约束配置" }));
    fireEvent.click(screen.getByRole("button", { name: "确认更新配置" }));
    await screen.findByText("书库已恢复，旧草稿不可带入");
    expect(screen.getByDisplayValue("旧书库草稿")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "保存约束配置" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "已核对当前配置，保留我的草稿" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "重新读取核对" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "放弃旧书库草稿" }));
    fireEvent.click(screen.getByRole("button", { name: "刷新依据与建议" }));
    expect(await screen.findByLabelText("写作目标")).toHaveValue(
      "恢复后的约束",
    );
    fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
    expect(close).toHaveBeenCalledOnce();
    expect(configure).toHaveBeenCalledTimes(1);
    expect(configure).toHaveBeenCalledWith(
      "b",
      "c",
      0,
      "active",
      "旧书库草稿",
      ["m"],
      "e",
      [],
    );
  },
);

it("compares same-epoch configuration before explicit rebase, then requires separate save confirmation", async () => {
  let current = state;
  const configure = vi
    .fn()
    .mockImplementationOnce(async () => {
      current = {
        ...state,
        policy: {
          ...policy,
          revision: 7,
          goal: "另一份配置",
          mode: "paused",
          required_material_ids: ["different-card"],
        },
      };
      throw Error("回执丢失");
    })
    .mockImplementationOnce(async () => {
      current = {
        ...state,
        policy: { ...policy, revision: 8, goal: "我的目标草稿" },
      };
      return current.policy;
    });
  mount({ state: async () => current, configure });
  fireEvent.change(await screen.findByLabelText("写作目标"), {
    target: { value: "我的目标草稿" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存约束配置" }));
  fireEvent.click(screen.getByRole("button", { name: "确认更新配置" }));
  await screen.findByText(/另一份配置/);
  expect(screen.getByText(/different-card/)).toBeInTheDocument();
  expect(screen.getByDisplayValue("我的目标草稿")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "保存约束配置" })).toBeDisabled();
  fireEvent.click(
    screen.getByRole("button", { name: "已核对当前配置，保留我的草稿" }),
  );
  expect(configure).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "保存约束配置" }));
  expect(configure).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "确认更新配置" }));
  await waitFor(() =>
    expect(configure).toHaveBeenCalledWith(
      "b",
      "c",
      7,
      "active",
      "我的目标草稿",
      ["m"],
      "e",
      [],
    ),
  );
  expect(configure).toHaveBeenCalledTimes(2);
});

it("keeps the writing dialog itself vertically scrollable under a viewport height bound", async () => {
  const style = document.createElement("style");
  // The npm test script runs from the project root. Load the real sheet;
  // Vitest's default CSS transform stubs stylesheet module exports.
  style.textContent = readFileSync("src/style.css", "utf8");
  document.head.append(style);
  try {
    mount();
    await screen.findByDisplayValue("过河");
    const dialog = screen.getByRole("dialog", { name: "章节写作约束" });
    const dialogStyle = getComputedStyle(dialog);
    expect(dialogStyle.overflowY).toBe("auto");
    expect(dialogStyle.maxHeight).toMatch(/\d+(?:d?vh)\b/);
    expect(dialogStyle.overflowWrap).toBe("anywhere");
  } finally {
    style.remove();
  }
});

it("releases a known review receipt after restore kept only an older archived revision", async () => {
  let epoch = "e";
  const getProposal = vi.fn(async () =>
    epoch === "e" ? reviewed : { ...reviewed, live_epoch: null },
  );
  const saveReview = vi.fn(async (_b, _q, _expected, _e, items) => {
    epoch = "restored-epoch";
    return review(2, items);
  });
  const { api, close } = mount({
    epoch: async () => epoch,
    proposal: getProposal,
    saveReview,
  });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  await screen.findByText("不能");
  fireEvent.change(screen.getByLabelText("作者备注：规则"), {
    target: { value: "旧书库已确认的备注" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存逐项审阅" }));
  await screen.findByText("书库已恢复，旧审阅草稿不可带入");
  const note = screen.getByDisplayValue("旧书库已确认的备注");
  expect(note).toHaveAttribute("readonly");
  expect(note).toBeEnabled();
  expect(
    screen.queryByRole("button", { name: "重新读取核对" }),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "采纳建议" }),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "保存逐项审阅" }),
  ).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeDisabled();
  expect(getProposal).toHaveBeenCalledTimes(1);
  expect(saveReview).toHaveBeenCalledTimes(1);
  expect(api.acceptReviewed).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "放弃旧书库审阅草稿" }));
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  expect(close).toHaveBeenCalledOnce();
});

it("checks the epoch again when a lost review receipt races a restore that removes the proposal", async () => {
  let epoch = "e",
    attempted = false;
  const epochRead = vi.fn(async () => epoch);
  const getProposal = vi.fn(async () => {
    if (!attempted) return reviewed;
    epoch = "restored-without-chapter";
    throw Error("此建议已不在书库");
  });
  const saveReview = vi.fn(async () => {
    attempted = true;
    throw Error("回执丢失");
  });
  const scopedRead = vi.fn(async () => {
    if (attempted) throw Error("原章节不存在");
    return state;
  });
  const { api, close } = mount({
    state: scopedRead,
    epoch: epochRead,
    proposal: getProposal,
    saveReview,
  });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  await screen.findByText("不能");
  fireEvent.change(screen.getByLabelText("作者备注：规则"), {
    target: { value: "待核对的旧书库备注" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存逐项审阅" }));
  await screen.findByText("书库已恢复，旧审阅草稿不可带入");
  expect(screen.getByDisplayValue("待核对的旧书库备注")).toHaveAttribute(
    "readonly",
  );
  expect(
    screen.queryByRole("button", { name: "重新读取核对" }),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: /保留我的草稿/ }),
  ).not.toBeInTheDocument();
  expect(epochRead).toHaveBeenCalledTimes(2);
  expect(getProposal).toHaveBeenCalledTimes(2);
  expect(scopedRead).toHaveBeenCalledTimes(1);
  expect(saveReview).toHaveBeenCalledTimes(1);
  expect(api.acceptReviewed).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "放弃旧书库审阅草稿" }));
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  expect(close).toHaveBeenCalledOnce();
});

it("keeps the service diagnostic after successful readback of a rejected review save", async () => {
  const saveReview = vi.fn(async () => {
    throw Error("写作依据证据达到64MiB上限");
  });
  mount({ saveReview });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  await screen.findByText("不能");
  fireEvent.change(screen.getByLabelText("作者备注：规则"), {
    target: { value: "尚未存入的备注" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存逐项审阅" }));
  await screen.findByText(/草稿已保留.*64MiB上限/);
  expect(screen.getByDisplayValue("尚未存入的备注")).toBeInTheDocument();
  expect(saveReview).toHaveBeenCalledTimes(1);
});

const priorA = {
  id: "prior-a",
  volume_id: "early",
  title: "第九章 · 旧岸",
  revision: 3,
  sort_order: 0,
};
const priorB = {
  id: "prior-b",
  volume_id: "early",
  title: "第三章 · 船",
  revision: 1,
  sort_order: 1,
};
const earlyVolume = {
  id: "early",
  book_id: "b",
  title: "上卷",
  sort_order: 0,
  deleted_at: null,
};
const contextDirectory = (epoch = "e") => ({
  epoch,
  book_id: "b",
  chapter_id: "c",
  chapters: [priorA, priorB],
  volumes: [earlyVolume],
});

it("blocks generation for an unsaved prior selection and reconciles a lost configuration receipt by context", async () => {
  let current: any = state;
  const configure = vi.fn(
    async (
      _b: string,
      _c: string,
      _r: number,
      _m: string,
      _g: string,
      _i: string[],
      _e: string,
      contextIds: string[],
    ) => {
      current = {
        ...state,
        policy: { ...policy, revision: 1, context_chapter_ids: contextIds },
      };
      throw Error("回执丢失");
    },
  );
  mount({
    state: async () => current,
    configure,
    contextDirectory: async () => contextDirectory(),
  });
  const select = await screen.findByRole("checkbox", { name: /上卷.*旧岸/ });
  expect(select).not.toBeChecked();
  fireEvent.click(select);
  expect(screen.getByRole("button", { name: "冻结当前依据包" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "保存约束配置" }));
  fireEvent.click(screen.getByRole("button", { name: "确认更新配置" }));
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "冻结当前依据包" }),
    ).toBeEnabled(),
  );
  expect(configure).toHaveBeenCalledExactlyOnceWith(
    "b",
    "c",
    0,
    "active",
    "过河",
    ["m"],
    "e",
    ["prior-a"],
  );
  expect(screen.getByRole("checkbox", { name: /旧岸/ })).toBeChecked();
});

it("keeps a lost prior selection separate from a changed same-epoch policy and compares both", async () => {
  let current: any = state;
  const configure = vi.fn(async () => {
    current = {
      ...state,
      policy: { ...policy, revision: 1, context_chapter_ids: ["prior-b"] },
    };
    throw Error("回执丢失");
  });
  mount({
    state: async () => current,
    configure,
    contextDirectory: async () => contextDirectory(),
  });
  fireEvent.click(await screen.findByRole("checkbox", { name: /旧岸/ }));
  fireEvent.click(screen.getByRole("button", { name: "保存约束配置" }));
  fireEvent.click(screen.getByRole("button", { name: "确认更新配置" }));
  await screen.findByText(/当前前文.*第三章/);
  expect(screen.getByRole("checkbox", { name: /旧岸/ })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: /第三章/ })).not.toBeChecked();
  expect(screen.getByRole("button", { name: "冻结当前依据包" })).toBeDisabled();
  fireEvent.click(
    screen.getByRole("button", { name: "已核对当前配置，保留我的草稿" }),
  );
  expect(configure).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "保存约束配置" })).toBeEnabled();
});

it("does not carry a prior selection across replacement and explicitly removes invalid restored references", async () => {
  let current: any = state;
  const configure = vi.fn(async () => {
    current = {
      ...state,
      epoch: "restored",
      policy: { ...policy, revision: 2, context_chapter_ids: ["removed"] },
    };
    throw Error("旧书库回执不明");
  });
  mount({
    state: async () => current,
    epoch: async () => current.epoch,
    configure,
    contextDirectory: async () => contextDirectory(current.epoch),
  });
  fireEvent.click(await screen.findByRole("checkbox", { name: /旧岸/ }));
  fireEvent.click(screen.getByRole("button", { name: "保存约束配置" }));
  fireEvent.click(screen.getByRole("button", { name: "确认更新配置" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "放弃旧书库草稿" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "刷新依据与建议" }));
  expect(
    await screen.findByRole("checkbox", { name: /旧岸/ }),
  ).not.toBeChecked();
  expect(screen.getByRole("button", { name: "冻结当前依据包" })).toBeDisabled();
  fireEvent.click(
    screen.getByRole("button", { name: "移除不可用前文 removed" }),
  );
  expect(screen.getByRole("button", { name: "保存约束配置" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "冻结当前依据包" })).toBeDisabled();
  expect(configure).toHaveBeenCalledTimes(1);
});

it("quotes Unicode prior text from the frozen pack without adding an author-review card", async () => {
  const packPolicy = { ...policy, context_chapter_ids: ["prior-a"] };
  mount({
    state: async () => ({ ...state, policy: packPolicy }),
    contextDirectory: async () => contextDirectory(),
    pack: async () => ({
      handle_version: 3,
      id: "p",
      originating_epoch: "e",
      expires_at: Date.now() / 1000 + 1800,
      book_id: "b",
      chapter_id: "c",
      chapter,
      policy: packPolicy,
      materials: [card],
      context_chapters: [
        {
          volume: earlyVolume,
          chapter: { ...priorA, body: "甲🚣\r\n乙", deleted_at: null },
        },
      ],
    }),
    proposal: async () => ({
      ...proposal,
      input: {
        ...proposal.input,
        citations: [
          { ...proposal.input.citations[0], entry_id: "material:m" },
          {
            entry_id: "context:prior-a",
            field_key: "body",
            start: 1,
            end: 2,
            explanation: "承接旧岸",
          },
        ],
      },
    }),
  });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  expect(await screen.findByText("🚣")).toBeInTheDocument();
  expect(screen.getByText(/前文衔接仍需作者检查/)).toBeInTheDocument();
  const summary = screen.getByText(/上卷.*旧岸.*版本 3/);
  expect(summary.closest("details")?.querySelector("pre")).toBeNull();
  const details = summary.closest("details")!;
  details.open = true;
  fireEvent(details, new Event("toggle"));
  await waitFor(() =>
    expect(details.querySelector("pre")?.textContent).toBe("甲🚣\r\n乙"),
  );
  expect(screen.getByLabelText("作者判断：规则")).toBeInTheDocument();
  expect(screen.queryByLabelText(/作者判断.*旧岸/)).not.toBeInTheDocument();
});

it("treats a configure acknowledgement with a different prior selection as unconfirmed", async () => {
  let current: any = state;
  const configure = vi.fn(async () => {
    current = {
      ...state,
      policy: { ...policy, revision: 1, context_chapter_ids: ["prior-b"] },
    };
    return current.policy;
  });
  mount({
    state: async () => current,
    configure,
    contextDirectory: async () => contextDirectory(),
  });
  fireEvent.click(await screen.findByRole("checkbox", { name: /旧岸/ }));
  fireEvent.click(screen.getByRole("button", { name: "保存约束配置" }));
  fireEvent.click(screen.getByRole("button", { name: "确认更新配置" }));
  await screen.findByText(/配置回执与本次提交不匹配/);
  expect(screen.getByRole("checkbox", { name: /旧岸/ })).toBeChecked();
  expect(screen.getByRole("button", { name: "冻结当前依据包" })).toBeDisabled();
  expect(configure).toHaveBeenCalledTimes(1);
});

it("does not show a proposal as reviewable if the pack is missing a selected frozen prior chapter", async () => {
  const packPolicy = { ...policy, context_chapter_ids: ["prior-a"] };
  mount({
    state: async () => ({ ...state, policy: packPolicy }),
    contextDirectory: async () => contextDirectory(),
    pack: async () => ({
      handle_version: 3,
      id: "p",
      originating_epoch: "e",
      expires_at: Date.now() / 1000 + 1800,
      book_id: "b",
      chapter_id: "c",
      chapter,
      policy: packPolicy,
      materials: [card],
      context_chapters: [],
    }),
  });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  await screen.findByText("依据或提案归属不匹配");
  expect(
    screen.queryByRole("button", { name: "采纳建议" }),
  ).not.toBeInTheDocument();
});

it("opens archive evidence and can reject a proposal when the optional prior directory fails", async () => {
  const current = {
    ...state,
    policy: { ...policy, context_chapter_ids: ["prior-a"] },
    packs: [
      {
        id: "p",
        policy_revision: 0,
        chapter_revision: 0,
        expires_at: 1,
        archived: true,
      },
    ],
  };
  const decide = vi.fn(async () => {
    current.proposals = [{ ...current.proposals[0], decision: "rejected" }];
    return { ...proposal, decision: "rejected" };
  });
  mount({
    state: async () => current,
    contextDirectory: async () => {
      throw Error("可选前文目录超过16MiB或节点上限");
    },
    decide,
  });
  await screen.findByText(/暂无法读取前文目录/);
  expect(screen.getByText(/已选前文.*prior-a/)).toBeInTheDocument();
  expect(
    screen.queryByText(/不可用前文：prior-a|没有可选的前文章节/),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /查看依据 p.*归档/ }));
  expect(await screen.findByText("冻结依据与目标")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "查看建议 q" }));
  fireEvent.click(await screen.findByRole("button", { name: "拒绝建议" }));
  fireEvent.click(screen.getByRole("button", { name: "确认拒绝" }));
  expect(await screen.findByText("rejected")).toBeInTheDocument();
  expect(decide).toHaveBeenCalledTimes(1);
});

it("reconciles a saved pause and explicit prior clear despite repeated directory failures, without trapping close", async () => {
  let current: any = {
    ...state,
    policy: { ...policy, context_chapter_ids: ["prior-a"] },
  };
  const configure = vi.fn(
    async (
      _b: string,
      _c: string,
      r: number,
      mode: string,
      _g: string,
      _i: string[],
      _e: string,
      contextIds: string[],
    ) => {
      current = {
        ...current,
        policy: {
          ...policy,
          mode,
          revision: r + 1,
          context_chapter_ids: contextIds,
        },
      };
      return current.policy;
    },
  );
  const failedDirectory = vi.fn(async () => {
    throw Error("可选前文目录超过16MiB或节点上限");
  });
  const { close } = mount({
    state: async () => current,
    configure,
    contextDirectory: failedDirectory,
  });
  await screen.findByText(/暂无法读取前文目录/);
  fireEvent.change(screen.getByLabelText("约束状态"), {
    target: { value: "paused" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存约束配置" }));
  fireEvent.click(screen.getByRole("button", { name: "确认更新配置" }));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeEnabled(),
  );
  expect(configure).toHaveBeenNthCalledWith(
    1,
    "b",
    "c",
    0,
    "paused",
    "过河",
    ["m"],
    "e",
    ["prior-a"],
  );
  expect(screen.getByText(/已选前文.*prior-a/)).toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", { name: "从选择移除前文 prior-a" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "保存约束配置" }));
  fireEvent.click(screen.getByRole("button", { name: "确认更新配置" }));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeEnabled(),
  );
  expect(configure).toHaveBeenNthCalledWith(
    2,
    "b",
    "c",
    1,
    "paused",
    "过河",
    ["m"],
    "e",
    [],
  );
  expect(failedDirectory).toHaveBeenCalledTimes(3);
  expect(
    screen.queryByRole("button", { name: "重新读取核对" }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  expect(close).toHaveBeenCalledOnce();
});

it("does not use a successful directory payload from another epoch or scope", async () => {
  mount({
    state: async () => ({
      ...state,
      policy: { ...policy, context_chapter_ids: ["prior-a"] },
    }),
    contextDirectory: async () => ({
      ...contextDirectory("other"),
      book_id: "other",
    }),
  });
  expect(
    await screen.findByText(/暂无法读取前文目录.*归属不匹配/),
  ).toBeInTheDocument();
  expect(screen.getByText(/已选前文.*prior-a/)).toBeInTheDocument();
  expect(
    screen.queryByRole("checkbox", { name: /旧岸/ }),
  ).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "查看建议 q" })).toBeEnabled();
});

it("does not wait for a stalled directory request before completing the required state read", async () => {
  const contextDirectoryRead = vi.fn(() => new Promise(() => {}));
  const { close } = mount({ contextDirectory: contextDirectoryRead });
  expect(await screen.findByText("正在读取前文目录。")).toBeInTheDocument();
  expect(contextDirectoryRead).toHaveBeenCalledOnce();
  expect(screen.getByRole("button", { name: "查看建议 q" })).toBeEnabled();
  fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
  expect(close).toHaveBeenCalledOnce();
});

function startPriorConfigSave() {
  fireEvent.click(screen.getByRole("checkbox", { name: /旧岸/ }));
  fireEvent.change(screen.getByLabelText("写作目标"), {
    target: { value: "旧库渡河草稿" },
  });
  fireEvent.change(screen.getByLabelText("约束状态"), {
    target: { value: "paused" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存约束配置" }));
  fireEvent.click(screen.getByRole("button", { name: "确认更新配置" }));
}

it.each([false, true])(
  "retains only a discardable config draft if restoration removed the target before readback (known receipt: %s)",
  async (known) => {
    let epoch = "e";
    const scopedRead = vi.fn(async () => {
      if (epoch !== "e") throw Error("原章节已移除");
      return state;
    });
    const reload = vi.fn(async () => {
      if (epoch !== "e") throw Error("App 找不到原章节");
      return chapter;
    });
    const configure = vi.fn(async () => {
      epoch = "restored-without-target";
      if (!known) throw Error("回执丢失");
      return {
        ...policy,
        revision: 1,
        mode: "paused",
        goal: "旧库渡河草稿",
        context_chapter_ids: ["prior-a"],
      };
    });
    const { close } = mount(
      {
        epoch: async () => epoch,
        state: scopedRead,
        configure,
        contextDirectory: async () => contextDirectory(),
      },
      reload,
    );
    await screen.findByRole("checkbox", { name: /旧岸/ });
    startPriorConfigSave();
    expect(
      await screen.findByText("书库已恢复，旧草稿不可带入"),
    ).toBeInTheDocument();
    expect(screen.getByDisplayValue("旧库渡河草稿")).toHaveAttribute(
      "readonly",
    );
    expect(screen.getByText("旧约束状态：暂停")).toBeInTheDocument();
    expect(screen.getByText("旧必需资料 ID：m")).toBeInTheDocument();
    expect(screen.getByText("旧前文 ID：prior-a")).toBeInTheDocument();
    expect(scopedRead).toHaveBeenCalledOnce();
    expect(reload).toHaveBeenCalledOnce();
    expect(
      screen.queryByRole("button", {
        name: /重新读取核对|保留我的草稿|保存约束配置/,
      }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "放弃旧书库草稿" }));
    fireEvent.click(screen.getByRole("button", { name: "关闭写作约束" }));
    expect(close).toHaveBeenCalledOnce();
    expect(configure).toHaveBeenCalledExactlyOnceWith(
      "b",
      "c",
      0,
      "paused",
      "旧库渡河草稿",
      ["m"],
      "e",
      ["prior-a"],
    );
  },
);

it.each([false, true])(
  "checks the epoch again when restoration races configuration scope reads (scoped success: %s)",
  async (scopedSuccess) => {
    let epoch = "e",
      attempted = false;
    const epochRead = vi.fn(async () => epoch);
    const scopedRead = vi.fn(async () => {
      if (!attempted) return state;
      epoch = "restored-during-scoped-read";
      if (!scopedSuccess) throw Error("原章节在读取时被移除");
      return {
        ...state,
        policy: {
          ...policy,
          revision: 1,
          goal: "旧库渡河草稿",
          mode: "paused",
          context_chapter_ids: ["prior-a"],
        },
      };
    });
    const configure = vi.fn(async () => {
      attempted = true;
      throw Error("回执丢失");
    });
    mount({
      epoch: epochRead,
      state: scopedRead,
      configure,
      contextDirectory: async () => contextDirectory(),
    });
    await screen.findByRole("checkbox", { name: /旧岸/ });
    startPriorConfigSave();
    await screen.findByText("书库已恢复，旧草稿不可带入");
    expect(screen.getByText("旧前文 ID：prior-a")).toBeInTheDocument();
    expect(screen.getByDisplayValue("旧库渡河草稿")).toHaveAttribute(
      "readonly",
    );
    expect(epochRead).toHaveBeenCalledTimes(2);
    expect(scopedRead).toHaveBeenCalledTimes(2);
    expect(configure).toHaveBeenCalledOnce();
    expect(
      screen.queryByRole("button", { name: /重新读取核对|保留我的草稿/ }),
    ).not.toBeInTheDocument();
  },
);

it.each(["same", "unreadable", "after-unreadable"])(
  "keeps config readback locked without a verified epoch change (%s)",
  async (check) => {
    let attempted = false;
    let epochReads = 0;
    const scopedRead = vi.fn(async () => {
      if (attempted && check === "same") throw Error("原章节不可用");
      return state;
    });
    const configure = vi.fn(async () => {
      attempted = true;
      throw Error("回执丢失");
    });
    mount({
      epoch: async () => {
        epochReads++;
        if (
          check === "unreadable" ||
          (check === "after-unreadable" && epochReads === 2)
        )
          throw Error("书库时代暂不可读");
        return "e";
      },
      state: scopedRead,
      configure,
      contextDirectory: async () => contextDirectory(),
    });
    await screen.findByRole("checkbox", { name: /旧岸/ });
    startPriorConfigSave();
    await screen.findByText(
      check === "same" ? "原章节不可用" : "书库时代暂不可读",
    );
    expect(screen.getByDisplayValue("旧库渡河草稿")).toBeDisabled();
    expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "重新读取核对" })).toBeEnabled();
    expect(
      screen.queryByRole("button", { name: "放弃旧书库草稿" }),
    ).not.toBeInTheDocument();
    expect(scopedRead).toHaveBeenCalledTimes(check === "unreadable" ? 1 : 2);
    expect(configure).toHaveBeenCalledOnce();
  },
);

it("opens v4 prior context alongside a foreshadow quote and keeps the same author review card", async () => {
  const packPolicy={...policy,context_chapter_ids:["prior-a"]};
  mount({
    state:async()=>({...state,policy:packPolicy}),
    contextDirectory:async()=>contextDirectory(),
    pack:async()=>({
      handle_version:4,id:"p",originating_epoch:"e",expires_at:Date.now()/1000+1800,
      book_id:"b",chapter_id:"c",chapter,policy:packPolicy,
      materials:[{...card,kind:"foreshadow",fields:{setup:"旧钥匙"},title:"线索"}],
      context_chapters:[{volume:earlyVolume,chapter:{...priorA,body:"过🚣",deleted_at:null}}],
      foreshadow_occurrences:[{
        id:"occ",book_id:"b",material_id:"m",source:{chapter_id:"prior-a",chapter_title:"旧岸",
          chapter_revision:3,chapter_sort_order:0,volume_id:earlyVolume.id,volume_title:earlyVolume.title,volume_sort_order:0},
        start:0,end:2,quote:"过🚣",role:"hint",note:"虚构附注",revision:0,deleted_at:null,created_at:1
      }]
    }),
    proposal:async()=>({...proposal,input:{...proposal.input,citations:[
      {entry_id:"material:m",field_key:"occurrence:occ",start:0,end:1,explanation:"解释伏笔"},
      {entry_id:"context:prior-a",field_key:"body",start:1,end:2,explanation:"承接前文"},
    ]}})
  });
  fireEvent.click(await screen.findByRole("button",{name:"查看建议 q"}));
  expect(await screen.findByText("冻结伏笔出现 · 提示")).toBeVisible();
  expect(screen.getByText("解释伏笔")).toBeVisible();
  expect(screen.getByText("🚣")).toBeVisible();
  expect(screen.getByLabelText("作者判断：线索")).toBeVisible();
});

const acceptedDecision = {
  ...reviewed,
  decision: "accepted",
  accepted_revision: 1,
};
async function confirmStrictDecision(kind: "accept" | "reject" = "accept") {
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  fireEvent.click(await screen.findByRole("button", { name: kind === "accept" ? "采纳建议" : "拒绝建议" }));
  if (kind === "accept")
    fireEvent.click(screen.getByLabelText("已人工核对并接受上述未解决冲突"));
  fireEvent.click(screen.getByRole("button", { name: kind === "accept" ? "确认采纳" : "确认拒绝" }));
}
it("does not settle a lost receipt from a terminal summary when the full proposal is still pending", async () => {
  let submitted = false;
  const decide = vi.fn(async () => {
    submitted = true;
    throw Error("决定回执丢失");
  });
  const full = vi.fn(async () => reviewed);
  mount({
    decide,
    proposal: full,
    state: async () => ({
      ...state,
      proposals: [{ ...state.proposals[0], decision: submitted ? "rejected" : "pending" }],
    }),
  });
  await confirmStrictDecision("reject");
  await screen.findByText(/仍待核对.*仍待审/);
  expect(full).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "拒绝建议" })).toBeDisabled();
  expect(decide).toHaveBeenCalledOnce();
});

it.each([
  ["本文", { input: { ...reviewed.input, new_body: "变造稿" } }],
  ["引用", { input: { ...reviewed.input, citations: [] } }],
  ["冲突", { input: { ...reviewed.input, conflicts: [] } }],
  ["幂等", { input: { ...reviewed.input, idempotency_key: "其他键" } }],
  ["客户端", { client_id: "other" }],
  ["基础版本", { base_revision: 1 }],
  ["采纳版本", { accepted_revision: 2 }],
  ["审核绑定", { author_review: { ...reviewed.author_review, pack_id: "p-other" } }],
  ["审核版本", { author_review: { ...reviewed.author_review, revision: 2 } }],
  ["审核内容", { author_review: review(1, [{ material_id: "m", verdict: "conforms", note: "改过" }]) }],
  ["库时代", { live_epoch: "other" }],
])("does not trust structurally plausible accepted proof with different %s", async (_label, change) => {
  let submitted = false;
  const wrong = { ...acceptedDecision, ...change };
  const reload = vi.fn(async () => submitted ? { ...chapter, revision: 2, body: "后续稿" } : chapter);
  mount({
    state: async () => ({
      ...state,
      proposals: [{ ...state.proposals[0], decision: submitted ? "accepted" : "pending" }],
    }),
    proposal: async () => submitted ? wrong : reviewed,
    acceptReviewed: async () => {
      submitted = true;
      return wrong;
    },
  }, reload);
  await confirmStrictDecision();
  await screen.findByText(/仍待核对.*不匹配/);
  expect(screen.getByRole("button", { name: "采纳建议" })).toBeDisabled();
  expect(reload).toHaveBeenCalledOnce();
});

it.each([
  [1, "错稿", /同版本正文不匹配/],
  [2, "作者另存的正文", /曾采纳，正文后来已改变/],
])("reports terminal adoption independently of the current chapter (v%s)", async (revision, body, expected) => {
  let submitted = false;
  mount({
    state: async () => ({...state, proposals: submitted ? [{...state.proposals[0], decision:"accepted"}] : state.proposals}),
    proposal: async () => submitted ? acceptedDecision : reviewed,
    acceptReviewed: async () => {
      submitted = true;
      return acceptedDecision;
    },
  }, async () => submitted ? { ...chapter, revision, body } : chapter);
  await confirmStrictDecision();
  await screen.findByText(expected);
  expect(screen.queryByText(/当前正文已核对/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeEnabled();
});

function deferredDecision<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
it.each(["initial epoch", "proposal", "sandwich epoch", "chapter", "publish epoch"])(
  "stops an indefinitely blocked %s read and ignores its late completion in a newer read",
  async (stage) => {
    let submitted = false, epochReads = 0, proposalReads = 0, chapterReads = 0;
    const late = deferredDecision<any>();
    const retry = deferredDecision<string>();
    const madeWait = vi.fn();
    const wait = () => { madeWait(); return late.promise; };
    const body = { ...chapter, revision: 1, body: "坐船" };
    const acceptReviewed = vi.fn(async () => {
      submitted = true;
      return acceptedDecision;
    });
    mount({
      acceptReviewed,
      epoch: async () => {
        if (!submitted) return "e";
        const n = ++epochReads;
        if (
          (stage === "initial epoch" && n === 1) ||
          (stage === "sandwich epoch" && n === 2) ||
          (stage === "publish epoch" && n === 3)
        ) return wait();
        if (madeWait.mock.calls.length && (
          (stage === "initial epoch" && n === 2) ||
          ((stage === "proposal" || stage === "sandwich epoch") && n === (stage === "proposal" ? 2 : 3)) ||
          ((stage === "chapter" || stage === "publish epoch") && n === (stage === "chapter" ? 3 : 4))
        )) return retry.promise;
        return "e";
      },
      proposal: async () => !submitted ? reviewed
        : stage === "proposal" && ++proposalReads === 1 ? wait() : acceptedDecision,
    }, async () => !submitted ? chapter
      : stage === "chapter" && ++chapterReads === 1 ? wait() : body);
    await confirmStrictDecision();
    await waitFor(() => expect(madeWait).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("button", { name: "停止等待核对" }));
    expect(screen.getByText(/不表示原写入失败/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "只读核对建议 q" }));
    await screen.findByRole("button", { name: "停止等待核对" });
    const outcome = stage === "proposal" ? acceptedDecision : stage === "chapter" ? body : "different";
    await act(async () => {
      if (stage === "initial epoch" || stage === "chapter") late.reject(Error("旧读取失败"));
      else late.resolve(outcome);
    });
    expect(screen.getByRole("button", { name: "停止等待核对" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeDisabled();
    expect(screen.queryByText(/旧读取失败|书库已更换|当前正文已核对/)).not.toBeInTheDocument();
    await act(async () => retry.resolve("e"));
    await screen.findByText(/当前正文已核对/);
    expect(acceptReviewed).toHaveBeenCalledOnce();
  },
);

it.each(["rejected", "cancelled"])(
  "reports an independently read opposite terminal (%s), without any chapter reread",
  async (decision) => {
    let submitted = false;
    const reload = vi.fn(async () => chapter);
    mount({
      // Frozen pack is fetched once to inspect the suggestion, never required in reconciliation.
      state: async () => submitted ? { ...state, proposals: [] } : state,
      proposal: async () => submitted ? { ...reviewed, decision } : reviewed,
      acceptReviewed: async () => {
        submitted = true;
        throw Error("决定回执丢失");
      },
    }, reload);
    await confirmStrictDecision();
    // The provisional full read also says this was another decision. Wait for
    // the settled notice after the fingerprint and trailing epoch check.
    await screen.findByText(/其他决定处理.*并非本次采纳.*未按此决定刷新或更改正文/);
    expect(screen.queryByRole("button", { name: "只读核对建议 q" })).not.toBeInTheDocument();
    expect(reload).toHaveBeenCalledOnce();
  },
);

it("does not use a rejected payload that claims an accepted revision", async () => {
  let submitted = false;
  const bad = { ...proposal, decision: "rejected", accepted_revision: 1 };
  mount({
    proposal: async () => submitted ? bad : proposal,
    decide: async () => { submitted = true; return bad; },
  });
  await confirmStrictDecision("reject");
  await screen.findByText(/仍待核对.*不匹配/);
  expect(screen.queryByText(/精确终态证据/)).not.toBeInTheDocument();
});

it("keeps exact historical evidence when the chapter has been deleted", async () => {
  let submitted = false;
  const newBody = "过河\r\n🚣第二行";
  const local = { ...reviewed, input: { ...reviewed.input, new_body: newBody } };
  const committed = { ...local, decision: "accepted", accepted_revision: 1 };
  const acceptReviewed = vi.fn(async () => { submitted = true; return committed; });
  mount({
    proposal: async () => submitted ? committed : local,
    acceptReviewed,
  }, async () => {
    if (submitted) throw Error("原章节已删除");
    return chapter;
  });
  await confirmStrictDecision();
  await screen.findByText(/已采纳.*当前正文未核对.*原章节已删除/);
  expect(screen.getByText(/精确终态证据：已采纳/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeEnabled();
  expect(acceptReviewed).toHaveBeenCalledOnce();
});

it("preserves precise proof for the old epoch after independent epoch mismatch", async () => {
  let submitted = false;
  let reads = 0;
  const reload = vi.fn(async () => chapter);
  mount({
    epoch: async () => ++reads === 1 ? "e" : "replacement",
    proposal: async () => submitted ? acceptedDecision : reviewed,
    acceptReviewed: async () => { submitted = true; return acceptedDecision; },
  }, reload);
  await confirmStrictDecision();
  await screen.findByText(/书库已更换.*证据仅供查看/);
  expect(screen.getByText(/完整提案候选.*尚未完成时代核对.*已采纳/)).toBeInTheDocument();
  expect(screen.queryByText(/当前正文已核对/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "只读核对建议 q" })).not.toBeInTheDocument();
  expect(reload).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "清理旧库证据 q" }));
  expect(screen.queryByText(/完整提案候选/)).not.toBeInTheDocument();
});

it("refuses the real bridge write when the recovery store cannot reserve another target", async () => {
  const session = new WritingDecisionSession();
  for (let i = 0; i < 32; i++) session.admit({
    id: `other-${i}`, epoch: "e", bookId: "other-book", chapterId: "other-chapter",
    clientId: "agent", action: "reject", policyRevision: 0, frozenPolicyRevision: 0,
    chapterRevision: 0, baseRevision: 0, confirmedReview: null, conflictsAck: false,
    requiredMaterialIds: ["m"], input: { ...proposal.input, idempotency_key: `key-${i}` },
  });
  const { api } = mount({ proposal: async () => reviewed }, undefined, session);
  await confirmStrictDecision();
  await screen.findByText(/32 MiB.*未提交/);
  expect(api.acceptReviewed).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "关闭写作约束" })).toBeEnabled();
  expect(session.entries()).toHaveLength(32);
});

it("does not use details changed after capturing the original author confirmation", async () => {
  const source = structuredClone(reviewed);
  mount({
    proposal: async () => source,
    acceptReviewed: async () => {
      source.input.citations[0].explanation = "提交后改写";
      source.author_review.items[0] = { material_id: "m", verdict: "conforms", note: "也改写" };
      Object.assign(source, { decision: "accepted", accepted_revision: 1 });
      return source;
    },
  });
  await confirmStrictDecision();
  await screen.findByText(/仍待核对.*不匹配/);
  expect(screen.queryByText(/精确终态证据/)).not.toBeInTheDocument();
});

function farSource(overrides: any = {}, session?: WritingDecisionSession) {
  const chapters = readFileSync(
    "qa/acceptance/longrange-foreshadow.txt", "utf8",
  ).trimEnd().split("\n").map((line: string) => line.split("\t"));
  const [sourceTitle, saved] = chapters[1];
  const body = "🌊\r\n" + saved + "末".repeat(1000);
  const quote = "两短一长，走背潮。";
  const start = [...body.slice(0, body.indexOf(quote))].length;
  const end = start + [...quote].length;
  const position = {
    chapter_id: "far-ch2", chapter_title: sourceTitle, chapter_revision: 1,
    chapter_sort_order: 1, volume_id: "first-vol", volume_title: "上卷", volume_sort_order: 0,
  };
  const frozen = {
    id: "plant", book_id: "b", material_id: "m", source: position, quote,
    start, end, role: "plant", note: "", revision: 0, deleted_at: null, created_at: 1,
  };
  const window = (current = position, text = body) => {
    const scalars = [...text];
    return {position:current, before:scalars.slice(Math.max(0,start-80),start).join(""),
      at_range:scalars.slice(start,end).join(""), after:scalars.slice(end,end+80).join("")};
  };
  const sourceReader = {
    epoch: vi.fn(async () => "e"),
    sourceWindow: vi.fn(async () => window()),
  };
  const mounted = mount({
    pack: vi.fn(async () => ({
      id: "p", originating_epoch: "e", expires_at: Date.now() / 1000 + 1800,
      book_id: "b", chapter_id: "c", chapter, policy, materials: [card],
      foreshadow_occurrences: [frozen],
    })),
    proposal: vi.fn(async () => ({
      ...proposal, input: {...proposal.input, new_body: chapters[20][1]},
    })),
    ...overrides,
  }, undefined, session, sourceReader);
  return {...mounted, sourceReader, position, body, quote, start, end, window};
}

it("checks the original long-range source in-place without saving review or replacing either draft", async () => {
  const {sourceReader, api, close, quote, start, end} = farSource();
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  const note = await screen.findByLabelText("作者备注：规则");
  fireEvent.change(note, {target:{value:"第十二章只是同节奏"}});
  expect(sourceReader.sourceWindow).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", {name:"核对已保存来源原文"}));
  expect(await screen.findByText("已保存来源位置与版本匹配")).toBeInTheDocument();
  expect(sourceReader.sourceWindow).toHaveBeenCalledWith("b","far-ch2","e",start,end);
  expect(sourceReader.epoch).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("mark")).toHaveTextContent(quote);
  expect(screen.getByLabelText("当前已保存正文周边")).not.toHaveTextContent("末".repeat(81));
  fireEvent.click(screen.getByRole("button", {name:"返回审阅"}));
  expect(note).toHaveValue("第十二章只是同节奏");
  expect(screen.getByRole("button", {name:"核对已保存来源原文"})).toHaveFocus();
  expect(close).not.toHaveBeenCalled();
  expect(api.saveReview).not.toHaveBeenCalled();
  expect(api.acceptReviewed).not.toHaveBeenCalled();
});

it("explains changed version and position without treating an identical quote as a refreshed basis", async () => {
  const {sourceReader, position, body, quote, window} = farSource();
  sourceReader.sourceWindow.mockResolvedValue(window(
    {...position, chapter_revision:2, volume_title:"下卷", volume_id:"second-vol"}, body+"补记",
  ));
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  await screen.findByLabelText("作者备注：规则");
  fireEvent.click(screen.getByRole("button", {name:"核对已保存来源原文"}));
  expect(await screen.findByText("正文版本已变化：冻结 v1，当前 v2。")).toBeInTheDocument();
  expect(screen.getByText("来源卷章位置或名称已变化。")).toBeInTheDocument();
  expect(screen.getByRole("mark")).toHaveTextContent(quote);
  expect(screen.getByText("只读对照不更新冻结依据或作者审阅。")).toBeInTheDocument();
});

it("can leave an unresponsive read and does not publish its late result or a changed library result", async () => {
  const {sourceReader, window} = farSource();
  let release!: (r: ReturnType<typeof window>) => void;
  sourceReader.sourceWindow.mockImplementationOnce(() => new Promise(r=>{ release=r; }));
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  await screen.findByLabelText("作者备注：规则");
  fireEvent.click(screen.getByRole("button", {name:"核对已保存来源原文"}));
  await waitFor(()=>expect(sourceReader.sourceWindow).toHaveBeenCalledTimes(1));
  fireEvent.click(screen.getByRole("button", {name:"返回审阅"}));
  expect(screen.queryByLabelText("来源原文核对")).not.toBeInTheDocument();
  sourceReader.epoch.mockResolvedValueOnce("e").mockResolvedValueOnce("new-library");
  fireEvent.click(screen.getByRole("button", {name:"核对已保存来源原文"}));
  expect(await screen.findByText("书库或审阅范围已变化，未显示原文。")).toBeInTheDocument();
  await act(async()=>release(window()));
  expect(screen.queryByLabelText("当前已保存正文周边")).not.toBeInTheDocument();
});

it("retains frozen evidence when the saved source is unavailable", async () => {
  const {sourceReader,quote} = farSource();
  sourceReader.sourceWindow.mockRejectedValue({code:"not_found"});
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  await screen.findByLabelText("作者备注：规则");
  fireEvent.click(screen.getByRole("button", {name:"核对已保存来源原文"}));
  expect(await screen.findByText("当前已保存来源不可用；冻结引文仍保留。")).toBeInTheDocument();
  expect(screen.getByText(quote)).toBeInTheDocument();
  expect(screen.queryByRole("mark")).not.toBeInTheDocument();
});

it("never labels a shifted quote as a match at the frozen coordinate", async () => {
  const {sourceReader, position, body, quote, window} = farSource();
  sourceReader.sourceWindow.mockResolvedValue(window(
    {...position, chapter_revision:2}, "新记：" + body,
  ));
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  await screen.findByLabelText("作者备注：规则");
  fireEvent.click(screen.getByRole("button", {name:"核对已保存来源原文"}));
  expect(await screen.findByText("当前原坐标不再匹配冻结引文，未自动寻找替代位置。")).toBeInTheDocument();
  expect(screen.queryByRole("mark")).not.toBeInTheDocument();
  expect(screen.getByText(quote)).toBeInTheDocument();
});

it("rejects a mismatched returned chapter before publishing any text", async () => {
  const {sourceReader, position, window} = farSource();
  sourceReader.sourceWindow.mockResolvedValue(window({...position, chapter_id:"another-chapter"}));
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  await screen.findByLabelText("作者备注：规则");
  fireEvent.click(screen.getByRole("button", {name:"核对已保存来源原文"}));
  expect(await screen.findByText("返回的来源章节不匹配，未显示原文。")).toBeInTheDocument();
  expect(screen.queryByLabelText("当前已保存正文周边")).not.toBeInTheDocument();
});

it("returns to review after a busy read and allows a fresh read without saving", async () => {
  const {sourceReader, api} = farSource();
  sourceReader.epoch.mockRejectedValueOnce({code:"busy"});
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  await screen.findByLabelText("作者备注：规则");
  fireEvent.click(screen.getByRole("button", {name:"核对已保存来源原文"}));
  expect(await screen.findByText("书库正忙，返回审阅后可再试。")).toBeInTheDocument();
  expect(sourceReader.sourceWindow).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", {name:"返回审阅"}));
  fireEvent.click(screen.getByRole("button", {name:"核对已保存来源原文"}));
  expect(await screen.findByText("已保存来源位置与版本匹配")).toBeInTheDocument();
  expect(api.saveReview).not.toHaveBeenCalled();
});

it("keeps only old notes after a stale long-range review cannot be saved, and starts a clean next round", async () => {
  const session = new WritingDecisionSession();
  const failure = "必需伏笔的出现来源已变化或不可用，请作者先处理";
  const {api, close, sourceReader, position, window} = farSource({
    saveReview: vi.fn(async () => {throw new Error(failure);}),
  }, session);
  sourceReader.sourceWindow.mockResolvedValue(window({...position, chapter_revision:2}));
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  fireEvent.change(await screen.findByLabelText("作者备注：规则"), {target:{value:"第12章只是同节奏，未回收。"}});
  fireEvent.change(screen.getByLabelText("作者判断：规则"), {target:{value:"conforms"}});
  fireEvent.click(screen.getByRole("button",{name:"保存逐项审阅"}));
  expect(await screen.findByText(/本次逐项审阅未核对为已保存/)).toHaveTextContent(failure);
  expect(screen.getByRole("button",{name:"关闭写作约束"})).toBeDisabled();
  fireEvent.click(screen.getByRole("button",{name:"核对已保存来源原文"}));
  expect(await screen.findByText("正文版本已变化：冻结 v1，当前 v2。")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button",{name:"返回审阅"}));
  fireEvent.click(screen.getByRole("button",{name:"保留备注副本并退出本次编辑"}));
  expect(screen.getByRole("button",{name:"关闭写作约束"})).toBeEnabled();
  fireEvent.click(screen.getByRole("button",{name:"关闭写作约束"}));
  expect(close).toHaveBeenCalledOnce();
  expect(api.saveReview).toHaveBeenCalledTimes(1);
  expect(api.acceptReviewed).not.toHaveBeenCalled();
  expect(api.decide).not.toHaveBeenCalled();

  cleanup();
  const fresh = {...proposal, id:"fresh", input:{...proposal.input, pack_id:"fresh-pack"}};
  let prepared = false;
  const next = farSource({
    prepare: vi.fn(async () => { prepared = true; return {id:"fresh-pack"}; }),
    state: vi.fn(async () => ({
      ...state,
      packs: prepared ? [{id:"fresh-pack", archived:false, expires_at:Date.now()/1000+1800}] : [],
      proposals:[{id:fresh.id, pack_id:"fresh-pack", decision:"pending", archived:false, created_at:2}],
    })),
    proposal: vi.fn(async () => fresh),
    pack: vi.fn(async () => ({
      id:"fresh-pack", originating_epoch:"e", expires_at: Date.now()/1000+1800,
      book_id:"b", chapter_id:"c", chapter, policy, materials:[card],
    })),
  }, session);
  expect(await screen.findByLabelText("旧建议备注副本")).toHaveValue("规则：第12章只是同节奏，未回收。");
  fireEvent.click(screen.getByRole("button",{name:"冻结当前依据包"}));
  await waitFor(() => expect(next.api.prepare).toHaveBeenCalledTimes(1));
  const nextButton = await screen.findByRole("button",{name:"查看建议 fresh"});
  await waitFor(() => expect(nextButton).toBeEnabled());
  fireEvent.click(nextButton);
  expect(await screen.findByLabelText("作者判断：规则")).toHaveValue("unreviewed");
  expect(screen.getByLabelText("作者备注：规则")).toHaveValue("");
  expect(screen.getByRole("button",{name:"采纳建议"})).toBeDisabled();
  expect(screen.getByText("原稿")).toBeInTheDocument();
  expect(next.api.saveReview).not.toHaveBeenCalled();
});

it("refuses to overwrite or exceed the session slot and hides notes across book, chapter and epoch", async () => {
  const session = new WritingDecisionSession();
  const old = {epoch:"old", bookId:"b", chapterId:"c"};
  const here = {...old, epoch:"e"};
  expect(session.retainReviewNotes(here, "q", "p", "🌊".repeat(16_384))).toBe("too_large");
  expect(session.hasReviewNoteCopy()).toBe(false);
  expect(session.retainReviewNotes(old, "q", "p", "不能出现在新库")).toBe("retained");
  expect(session.readReviewNotes(here)).toBeNull();
  expect(session.readReviewNotes({...old, bookId:"other"})).toBeNull();
  expect(session.readReviewNotes({...old, chapterId:"other"})).toBeNull();
  expect(session.retainReviewNotes(here, "q", "p", "不得覆盖")).toBe("occupied");
  expect(session.readReviewNotes(old)?.text).toBe("不能出现在新库");
  session.invalidateLibrary();
  expect(session.readReviewNotes(old)).toBeNull();
  mount({}, undefined, session);
  expect(await screen.findByText(/备注槽已被其他范围的副本占用；此处不显示其内容/)).toBeInTheDocument();
  expect(screen.queryByLabelText("旧建议备注副本")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", {name:"移除备注副本"}));
  fireEvent.click(screen.getByRole("button", {name:"取消移除"}));
  expect(session.hasReviewNoteCopy()).toBe(true);
  fireEvent.click(screen.getByRole("button", {name:"移除备注副本"}));
  fireEvent.click(screen.getByRole("button", {name:"确认移除备注副本"}));
  expect(session.hasReviewNoteCopy()).toBe(false);
  expect(session.retainReviewNotes(here, "fresh", "fresh-pack", "可以重新保留")).toBe("retained");
});

it("reports clipboard failure without losing or marking the copy as written, then reports explicit success", async () => {
  const session = new WritingDecisionSession();
  session.retainReviewNotes({epoch:"e", bookId:"b", chapterId:"c"}, "q", "p", "只读旧备注");
  const writeText = vi.fn().mockRejectedValueOnce(new DOMException("cancelled","AbortError")).mockResolvedValueOnce(undefined);
  const old = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  Object.defineProperty(navigator, "clipboard", {configurable:true, value:{writeText}});
  try {
    const persisted = structuredClone(reviewed);
    const {api} = mount({proposal: vi.fn(async () => persisted)}, undefined, session);
    fireEvent.click(await screen.findByRole("button",{name:"查看建议 q"}));
    await screen.findByText("已保存逐项作者审阅 · 版本 1");
    const copyButton = await screen.findByRole("button", {name:"复制备注副本"});
    fireEvent.click(copyButton);
    expect(await screen.findByText(/未能复制.*手动选择/)).toBeInTheDocument();
    expect(screen.queryByText("已复制纯文本备注。")).not.toBeInTheDocument();
    expect(screen.getByLabelText("旧建议备注副本")).toHaveValue("只读旧备注");
    fireEvent.click(copyButton);
    expect(await screen.findByText("已复制纯文本备注。")).toBeInTheDocument();
    expect(writeText.mock.calls).toEqual([["只读旧备注"], ["只读旧备注"]]);
    fireEvent.click(screen.getByRole("button",{name:"移除备注副本"}));
    fireEvent.click(screen.getByRole("button",{name:"取消移除"}));
    expect(screen.getByLabelText("旧建议备注副本")).toHaveValue("只读旧备注");
    expect(screen.getByText("已保存逐项作者审阅 · 版本 1")).toBeInTheDocument();
    expect(screen.getByLabelText("作者备注：规则")).toHaveValue("核对");
    expect(screen.getByLabelText("作者判断：规则")).toHaveValue("conforms");
    expect(persisted.author_review).toEqual(reviewed.author_review);
    expect(api.saveReview).not.toHaveBeenCalled();
    expect(api.acceptReviewed).not.toHaveBeenCalled();
    expect(api.decide).not.toHaveBeenCalled();
  } finally {
    if (old) Object.defineProperty(navigator, "clipboard", old);
    else Reflect.deleteProperty(navigator, "clipboard");
  }
});

it.each([
  {occupied:true, note:"这份备注不能覆盖旧副本", error:/已有另一份备注副本/},
  {occupied:false, note:"🌊".repeat(16_384), error:/备注副本超过 64 KiB 上限/},
])("protects the dirty note when the session copy cannot be retained ($occupied)", async ({occupied,note,error}) => {
  const session = new WritingDecisionSession();
  const scope = {epoch:"e", bookId:"b", chapterId:"c"};
  if (occupied) session.retainReviewNotes(scope, "older", "older-pack", "旧副本");
  const persisted = structuredClone(reviewed);
  const {api, sourceReader, position, window} = farSource({proposal: vi.fn(async () => persisted)}, session);
  sourceReader.sourceWindow.mockResolvedValue(window({...position, chapter_revision:2}));
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  const editor = await screen.findByLabelText("作者备注：规则");
  fireEvent.change(editor, {target:{value:note}});
  fireEvent.click(screen.getByRole("button",{name:"核对已保存来源原文"}));
  await screen.findByText("正文版本已变化：冻结 v1，当前 v2。");
  fireEvent.click(screen.getByRole("button",{name:"返回审阅"}));
  fireEvent.click(screen.getByRole("button",{name:"保留备注副本并退出本次编辑"}));
  expect(screen.getByText(error)).toBeInTheDocument();
  expect(editor).toHaveValue(note);
  expect(screen.getByRole("button",{name:"关闭写作约束"})).toBeDisabled();
  expect(session.readReviewNotes(scope)?.text ?? null).toBe(occupied ? "旧副本" : null);
  expect(screen.getByText("已保存逐项作者审阅 · 版本 1")).toBeInTheDocument();
  expect(persisted.author_review).toEqual(reviewed.author_review);
  expect(api.saveReview).not.toHaveBeenCalled();
  expect(api.acceptReviewed).not.toHaveBeenCalled();
  expect(api.decide).not.toHaveBeenCalled();
});

it("offers retention after a confirmed unsaved note but does not diagnose a generic failure as a stale source", async () => {
  const {sourceReader} = farSource({saveReview:vi.fn(async () => {throw Error("返回失败");})});
  sourceReader.sourceWindow.mockRejectedValue({code:"busy"});
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  fireEvent.change(await screen.findByLabelText("作者备注：规则"),{target:{value:"草稿"}});
  fireEvent.click(screen.getByRole("button",{name:"保存逐项审阅"}));
  await screen.findByText(/本次逐项审阅未核对为已保存/);
  fireEvent.click(screen.getByRole("button",{name:"核对已保存来源原文"}));
  await screen.findByText("书库正忙，返回审阅后可再试。");
  expect(screen.queryByText(/已核对到此旧建议的来源与冻结依据不一致/)).not.toBeInTheDocument();
  expect(screen.getByRole("button",{name:"保存逐项审阅"})).toBeEnabled();
  expect(screen.getByRole("button",{name:"保留备注副本并退出本次编辑"})).toBeEnabled();
});

it("prevents repeated saves and adoption after verified source drift, without blocking rejection or notes", async () => {
  const {api, sourceReader, position, window} = farSource({proposal: vi.fn(async () => reviewed)});
  sourceReader.sourceWindow.mockResolvedValue(window({...position, chapter_revision:2}));
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  expect(await screen.findByRole("button", {name:"采纳建议"})).toBeEnabled();
  fireEvent.click(screen.getByRole("button",{name:"核对已保存来源原文"}));
  await screen.findByText("正文版本已变化：冻结 v1，当前 v2。");
  fireEvent.click(screen.getByRole("button",{name:"返回审阅"}));
  expect(screen.getByRole("button",{name:"采纳建议"})).toBeDisabled();
  expect(screen.getByRole("button",{name:"拒绝建议"})).toBeEnabled();
  const editor = screen.getByLabelText("作者备注：规则");
  expect(editor).toBeEnabled();
  fireEvent.change(editor,{target:{value:"保留：来源已变"}});
  expect(screen.getByRole("button",{name:"保存逐项审阅"})).toBeDisabled();
  fireEvent.click(screen.getByRole("button",{name:"保存逐项审阅"}));
  expect(api.saveReview).not.toHaveBeenCalled();
  expect(api.acceptReviewed).not.toHaveBeenCalled();
  expect(screen.getByRole("button",{name:"保留备注副本并退出本次编辑"})).toBeEnabled();
});

const disclosureText = JSON.parse(readFileSync("qa/acceptance/setting-disclosure.json", "utf8"));
const disclosurePrior = {
  ...priorA,
  title: "当面开匣",
  body: disclosureText.disclosure,
  deleted_at: null,
};
const disclosureTarget = { ...chapter, title: "次晨作证", body: disclosureText.late_draft };
const disclosureTargetPosition = {id:"c",title:"次晨作证",revision:0,
  sort_order:0,volume:{...earlyVolume,id:"v",title:"下卷",sort_order:1}};
const disclosurePosition = {id:disclosurePrior.id,title:disclosurePrior.title,
  revision:disclosurePrior.revision,sort_order:disclosurePrior.sort_order,volume:earlyVolume};
const disclosureResult = (before: boolean) => ({
  book_id:"b",chapter_id:"c",pack_id:"p",epoch:"e",target:disclosureTargetPosition,
  entries:[{chapter_id:disclosurePrior.id,position:before?"before":"after",
    current:before?disclosurePosition:{...disclosurePosition,volume:disclosureTargetPosition.volume,sort_order:1}}],
});
const disclosurePolicy = {...policy, context_chapter_ids:[disclosurePrior.id]};

function disclosureReview(before = false, read?: () => Promise<unknown>, overrides: object = {}) {
  return mount({
    state: async () => ({...state,policy:disclosurePolicy}),
    contextDirectory: async () => ({...contextDirectory(),chapters:before?[disclosurePrior]:[],volumes:before?[earlyVolume]:[]}),
    pack: async () => ({id:"p",handle_version:4,originating_epoch:"e",expires_at:Date.now()/1000+1800,
      book_id:"b",chapter_id:"c",chapter:disclosureTarget,policy:disclosurePolicy,materials:[card],
      context_chapters:[{chapter:disclosurePrior,volume:earlyVolume}]}),
    proposal: async () => reviewed,
    predecessorDiagnosis: {epoch:async() => "e",read:read ?? (async () => disclosureResult(before))},
    ...overrides,
  }, async () => disclosureTarget);
}

it("names a moved frozen disclosure and routes to the existing prior picker without any write", async () => {
  const {api} = disclosureReview();
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  fireEvent.click(await screen.findByRole("button", {name:"核对冻结前文的目录位置"}));
  expect(await screen.findByText(/当面开匣.*已在目标章之后/)).toBeVisible();
  expect(screen.getByRole("button",{name:"采纳建议"})).toBeDisabled();
  fireEvent.click(screen.getByRole("button",{name:"前往前文选择"}));
  expect(screen.getByText("前文全文（可选，最多 8 章）")).toHaveFocus();
  expect(api.configure).not.toHaveBeenCalled();
  expect(api.prepare).not.toHaveBeenCalled();
  expect(api.acceptReviewed).not.toHaveBeenCalled();
});


it("keeps unchanged frozen disclosure reviewable and does not treat read failure or missing visibility as after", async () => {
  const read = vi.fn().mockResolvedValueOnce(disclosureResult(true))
    .mockResolvedValueOnce({...disclosureResult(true),entries:[{chapter_id:disclosurePrior.id,position:"not_visible",current:null}]})
    .mockRejectedValueOnce(new Error("书库正忙"));
  const {api} = disclosureReview(true, read);
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  const check = await screen.findByRole("button", {name:"核对冻结前文的目录位置"});
  expect(read).not.toHaveBeenCalled();
  fireEvent.click(check);
  expect(await screen.findByText(/当面开匣.*仍在目标章之前/)).toBeVisible();
  expect(screen.getByRole("button",{name:"采纳建议"})).toBeEnabled();
  fireEvent.click(check);
  expect(await screen.findByText(/当面开匣.*不可见；原因未确定/)).toBeVisible();
  expect(screen.getByRole("button",{name:"采纳建议"})).toBeEnabled();
  fireEvent.click(check);
  expect(await screen.findByText(/无法核对冻结前文.*不能据此判断/)).toBeVisible();
  expect(screen.queryByText(/当面开匣.*已在目标章之后/)).not.toBeInTheDocument();
  expect(api.saveReview).not.toHaveBeenCalled();
  expect(api.acceptReviewed).not.toHaveBeenCalled();
});

it("blocks stale review writes discovered after an older picker read and preserves dirty notes", async () => {
  const {api} = disclosureReview(true, async () => disclosureResult(false));
  fireEvent.click(await screen.findByRole("button", {name:"查看建议 q"}));
  const note = await screen.findByLabelText("作者备注：规则");
  fireEvent.change(note, {target:{value:"保持这条暂未保存的核对记录"}});
  expect(screen.getByRole("button",{name:"保存逐项审阅"})).toBeEnabled();
  fireEvent.click(screen.getByRole("button",{name:"核对冻结前文的目录位置"}));
  expect(await screen.findByText(/当面开匣.*已在目标章之后/)).toBeVisible();
  expect(note).toHaveValue("保持这条暂未保存的核对记录");
  fireEvent.click(screen.getByRole("button",{name:"前往前文选择"}));
  expect(screen.getByText("前文全文（可选，最多 8 章）")).toHaveFocus();
  expect(screen.getByRole("button",{name:"刷新依据与建议"})).toBeDisabled();
  expect(screen.getByRole("button",{name:"保存逐项审阅"})).toBeDisabled();
  expect(screen.getByRole("button",{name:"采纳建议"})).toBeDisabled();
  expect(screen.getByRole("button",{name:"拒绝建议"})).toBeDisabled(); // Existing dirty-note guard.
  fireEvent.click(screen.getByRole("button",{name:"保留备注副本并退出本次编辑"}));
  expect(screen.getByRole("button",{name:"拒绝建议"})).toBeEnabled();
  expect(screen.getByRole("button",{name:"采纳建议"})).toBeDisabled();
  expect(screen.getByText("保持这条暂未保存的核对记录",{exact:false})).toBeVisible();
  expect(api.saveReview).not.toHaveBeenCalled();
  expect(api.configure).not.toHaveBeenCalled();
  expect(api.prepare).not.toHaveBeenCalled();
});

it("does not diagnose from malformed scope, mismatched ordering or a late stopped result", async () => {
  let late!: (value: unknown) => void;
  const read = vi.fn()
    .mockResolvedValueOnce({...disclosureResult(false),book_id:"other"})
    .mockResolvedValueOnce({...disclosureResult(true),entries:[
      {...disclosureResult(true).entries[0],position:"after"},
    ]})
    .mockImplementationOnce(() => new Promise(resolve => {late=resolve;}))
    .mockResolvedValueOnce(disclosureResult(true));
  disclosureReview(true, read);
  fireEvent.click(await screen.findByRole("button",{name:"查看建议 q"}));
  const check = await screen.findByRole("button",{name:"核对冻结前文的目录位置"});
  for (let i=0; i<2; i++) {
    fireEvent.click(check);
    expect(await screen.findByText(/无法核对冻结前文/)).toBeVisible();
    expect(screen.queryByText(/已在目标章之后/)).not.toBeInTheDocument();
  }
  fireEvent.click(check);
  await waitFor(() => expect(read).toHaveBeenCalledTimes(3));
  fireEvent.click(screen.getByRole("button",{name:"停止核对前文"}));
  fireEvent.click(check);
  expect(await screen.findByText(/当面开匣.*仍在目标章之前/)).toBeVisible();
  await act(async () => {late(disclosureResult(false));});
  expect(screen.queryByText(/已在目标章之后/)).not.toBeInTheDocument();
  expect(screen.getByRole("button",{name:"采纳建议"})).toBeEnabled();
});

it("does not publish predecessor observations if the epoch changes after the snapshot", async () => {
  const read = vi.fn(async () => disclosureResult(false));
  const epoch = vi.fn().mockResolvedValueOnce("e").mockResolvedValueOnce("replacement");
  disclosureReview(true, undefined, {predecessorDiagnosis:{epoch,read}});
  fireEvent.click(await screen.findByRole("button",{name:"查看建议 q"}));
  fireEvent.click(await screen.findByRole("button",{name:"核对冻结前文的目录位置"}));
  expect(await screen.findByText(/无法核对冻结前文/)).toBeVisible();
  expect(screen.queryByText(/已在目标章之后/)).not.toBeInTheDocument();
  expect(screen.getByRole("button",{name:"采纳建议"})).toBeEnabled();
});


it("drops an old predecessor result after switching suggestions and starts the new review unreviewed", async () => {
  let release!: (value: unknown) => void;
  const read = vi.fn(() => new Promise(resolve => { release=resolve; }));
  const next = {...proposal,id:"q-next"};
  disclosureReview(true, read, {
    state:async()=>({...state,policy:disclosurePolicy,proposals:[
      ...state.proposals,{...state.proposals[0],id:next.id},
    ]}),
    proposal:async(_:string,id:string)=>id==="q"?reviewed:next,
  });
  fireEvent.click(await screen.findByRole("button",{name:"查看建议 q"}));
  fireEvent.click(await screen.findByRole("button",{name:"核对冻结前文的目录位置"}));
  await waitFor(() => expect(read).toHaveBeenCalledOnce());
  fireEvent.click(screen.getByRole("button",{name:"查看建议 q-next"}));
  expect(await screen.findByText("尚未保存逐项作者审阅")).toBeVisible();
  await act(async () => {release(disclosureResult(false));});
  expect(screen.queryByText(/已在目标章之后/)).not.toBeInTheDocument();
  expect(screen.getByLabelText("作者判断：规则")).toHaveValue("unreviewed");
  expect(screen.getByLabelText("作者备注：规则")).toHaveValue("");
});

it("does not gate review on contradictory volume order for two frozen predecessors", async () => {
  const second = {...priorB,body:disclosureText.prior,deleted_at:null};
  const pairPolicy = {...policy,context_chapter_ids:[disclosurePrior.id,second.id]};
  // Equal numeric volume ranks are valid. The canonical ID tie-break puts
  // both chapters in "early" before the target in "v".
  const sharedVolume = {...earlyVolume,sort_order:disclosureTargetPosition.volume.sort_order};
  const before = {
    ...disclosureResult(true),
    entries:[
      {chapter_id:disclosurePrior.id,position:"before",current:{...disclosurePosition,volume:sharedVolume}},
      {chapter_id:second.id,position:"before",current:{...second,volume:sharedVolume}},
    ],
  };
  const contradictory = {
    ...before,
    entries:[before.entries[0],{
      chapter_id:second.id,position:"after",current:{...second,volume:{...sharedVolume,sort_order:2}},
    }],
  };
  const read = vi.fn().mockResolvedValueOnce(contradictory).mockResolvedValueOnce(before);
  const {api} = disclosureReview(true, read, {
    state:async()=>({...state,policy:pairPolicy}),
    contextDirectory:async()=>({...contextDirectory(),chapters:[disclosurePrior,second],volumes:[earlyVolume]}),
    pack:async()=>({id:"p",handle_version:4,originating_epoch:"e",expires_at:Date.now()/1000+1800,
      book_id:"b",chapter_id:"c",chapter:disclosureTarget,policy:pairPolicy,materials:[card],
      context_chapters:[{chapter:disclosurePrior,volume:earlyVolume},{chapter:second,volume:earlyVolume}]}),
  });
  fireEvent.click(await screen.findByRole("button",{name:"查看建议 q"}));
  const check = await screen.findByRole("button",{name:"核对冻结前文的目录位置"});
  fireEvent.click(check);
  expect(await screen.findByText(/无法核对冻结前文/)).toBeVisible();
  expect(screen.queryByText(/仍在目标章之前|已在目标章之后/)).not.toBeInTheDocument();
  expect(screen.getByRole("button",{name:"采纳建议"})).toBeEnabled();
  fireEvent.click(check);
  expect(await screen.findAllByText(/仍在目标章之前（只核对位置）/)).toHaveLength(2);
  expect(screen.getByRole("button",{name:"采纳建议"})).toBeEnabled();
  expect(api.acceptReviewed).not.toHaveBeenCalled();
});

it("diagnoses the exact moved frozen ID among same-named chapters and keeps the other selection", async () => {
  const earlier = { ...earlyVolume, title:"雾港卷" };
  const sourceVolume = { ...earlyVolume, id:"disclosure-volume", title:"开匣卷", sort_order:1 };
  const laterVolume = { ...earlyVolume, id:"v", title:"次晨卷", sort_order:2 };
  const movedVolume = { ...earlyVolume, id:"moved-volume", title:"后置卷", sort_order:3 };
  const retained = { ...disclosurePrior, title:"第一章", volume_id:earlier.id, body:disclosureText.prior };
  const moved = { ...priorB, title:"第一章", volume_id:sourceVolume.id, body:disclosureText.disclosure, deleted_at:null };
  const selected = {...policy,context_chapter_ids:[retained.id,moved.id]};
  const read = vi.fn(async () => ({
    ...disclosureResult(true),target:{...disclosureTargetPosition,volume:laterVolume},
    entries:[
      {chapter_id:retained.id,position:"before",current:{...retained,volume:earlier}},
      {chapter_id:moved.id,position:"after",current:{...moved,volume:movedVolume}},
    ],
  }));
  const {api} = disclosureReview(true, read, {
    state:async()=>({...state,policy:selected}),
    contextDirectory:async()=>({...contextDirectory(),chapters:[retained],volumes:[earlier]}),
    pack:async()=>({id:"p",handle_version:4,originating_epoch:"e",expires_at:Date.now()/1000+1800,
      book_id:"b",chapter_id:"c",chapter:disclosureTarget,policy:selected,materials:[card],
      context_chapters:[{chapter:retained,volume:earlier},{chapter:moved,volume:sourceVolume}]}),
  });
  fireEvent.click(await screen.findByRole("button",{name:"查看建议 q"}));
  fireEvent.click(await screen.findByRole("button",{name:"核对冻结前文的目录位置"}));
  expect(await screen.findByText(/冻结时：开匣卷.*第一章.*当前：后置卷.*第一章.*已在目标章之后/)).toBeVisible();
  expect(screen.getByText(/雾港卷.*第一章.*仍在目标章之前/)).toBeVisible();
  fireEvent.click(screen.getByRole("button",{name:"前往前文选择"}));
  fireEvent.click(screen.getByRole("button",{name:`移除不可用前文 ${moved.id}`}));
  expect(screen.getByRole("checkbox",{name:/雾港卷.*第一章/})).toBeChecked();
  expect(screen.queryByRole("button",{name:`移除不可用前文 ${moved.id}`})).not.toBeInTheDocument();
  expect(api.configure).not.toHaveBeenCalled();
  expect(api.prepare).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button",{name:"保存约束配置"}));
  fireEvent.click(screen.getByRole("button",{name:"确认更新配置"}));
  await waitFor(()=>expect(api.configure).toHaveBeenCalledExactlyOnceWith(
    "b","c",0,"active","过河",["m"],"e",[retained.id],
  ));
  expect(api.prepare).not.toHaveBeenCalled();
  expect(api.acceptReviewed).not.toHaveBeenCalled();
});

it("compares only manually selected frozen outline and predecessor citations without changing author review", async () => {
  const outline = {
    ...card,
    title: outlineFixture.outline_title,
    kind: "outline" as const,
    volume_id: null,
    chapter_id: null,
    updated_at: 1,
    fields: { content: outlineFixture.outline_body },
  };
  const prior = {
    ...chapter,
    id: "outline-prior",
    title: outlineFixture.prior_title,
    body: outlineFixture.prior_body,
    revision: 2,
  };
  const outlinePolicy = {
    ...policy,
    mode: "active" as const,
    context_chapter_ids: [prior.id],
  };
  const volume = {
    id: "v",
    book_id: "b",
    title: outlineFixture.volume,
    sort_order: 0,
    deleted_at: null,
  };
  const outlinePack = {
    id: "p",
    originating_epoch: "e",
    created_at: 1,
    book: {
      id: "b",
      title: outlineFixture.title,
      author: "原创验收样本",
      deleted_at: null,
    },
    volume,
    expires_at: Date.now() / 1000 + 1800,
    book_id: "b",
    chapter_id: "c",
    chapter,
    policy: outlinePolicy,
    materials: [outline],
    handle_version: 4,
    context_chapters: [
      {
        chapter: prior,
        volume,
      },
    ],
  } satisfies WritingPack;
  const cite = (
    entry_id: string,
    field_key: string,
    body: string,
    quote: string,
  ) => {
    const start = [...body.slice(0, body.indexOf(quote))].length;
    return {
      entry_id,
      field_key,
      start,
      end: start + [...quote].length,
      explanation: "机械引用",
    };
  };
  const outlineCitation = cite(
    "material:m",
    "content",
    outlineFixture.outline_body,
    outlineFixture.outline_quote,
  );
  const priorCitation = cite(
    "context:outline-prior",
    "body",
    outlineFixture.prior_body,
    outlineFixture.prior_quote,
  );
  const first: WritingProposal = {
    ...proposal,
    input: {
      ...proposal.input,
      new_body: outlineFixture.flagged,
      citations: [outlineCitation, priorCitation],
      conflicts: [{ entry_ids: ["m"], explanation: outlineFixture.conflict }],
    },
  };
  const second = {
    ...first,
    id: "two",
    input: { ...first.input, conflicts: [] },
  };
  const foil = {
    ...second,
    id: "foil",
    input: {
      ...second.input,
      new_body: outlineFixture.foil,
      citations: [outlineCitation],
    },
  };
  const invalid = {
    ...second,
    id: "invalid",
    input: {
      ...second.input,
      citations: [
        outlineCitation,
        { ...priorCitation, end: priorCitation.end + 1 },
      ],
    },
  };
  const wrongBinding = {
    ...first,
    id: "wrong",
    input: { ...first.input, pack_id: "other" },
  };
  const malformed = [
    { id: "epoch", proposal: { live_epoch: "wrong-original-epoch" }, pack: {} },
    {
      id: "policy-book",
      proposal: {},
      pack: { policy: { ...outlinePolicy, book_id: "wrong-book" } },
    },
    {
      id: "policy-chapter",
      proposal: {},
      pack: { policy: { ...outlinePolicy, chapter_id: "wrong-chapter" } },
    },
    {
      id: "target",
      proposal: {},
      pack: { chapter: { ...chapter, id: "wrong-chapter" } },
    },
    { id: "base", proposal: { base_revision: chapter.revision + 1 }, pack: {} },
  ];
  const malformedPacks = new Map(
    malformed.map((x) => [x.id, { ...outlinePack, ...x.pack, id: x.id }]),
  );
  const malformedProposals = malformed.map((x) => ({
    ...second,
    ...x.proposal,
    id: x.id,
    input: { ...second.input, pack_id: x.id, new_body: `候选 ${x.id}` },
  }));
  const archive = {
    ...second,
    id: "archive",
    live_epoch: null,
    decision: "rejected",
    input: { ...second.input, pack_id: "archive", new_body: "历史候选" },
  };
  const stored = new Map<string, WritingProposal>(
    [
      first,
      second,
      foil,
      invalid,
      wrongBinding,
      ...malformedProposals,
      archive,
    ].map((q) => [q.id, q]),
  );
  const read = () => ({
    ...state,
    policy: outlinePolicy,
    proposals: [...stored.values()].map((q) => ({
      ...state.proposals[0],
      id: q.id,
      pack_id: q.input.pack_id,
      decision: q.decision,
      archived: q.live_epoch === null,
    })),
  });
  const saveReview = vi.fn(
    async (
      _b: string,
      id: string,
      _expected: unknown,
      _e: string,
      items: AuthorReviewItem[],
    ) => {
      const receipt = { ...review(1, items), proposal_id: id, items };
      stored.set(id, {
        ...stored.get(id)!,
        author_review: receipt,
      });
      return receipt;
    },
  );
  const { api } = mount({
    state: vi.fn(async () => read()),
    contextDirectory: vi.fn(async () => ({
      ...contextDirectory(),
      chapters: [prior],
      volumes: [outlinePack.context_chapters[0].volume],
    })),
    pack: vi.fn(async (_b: string, id: string) =>
      id === "archive"
        ? { ...outlinePack, id, originating_epoch: "past-epoch" }
        : (malformedPacks.get(id) ?? outlinePack),
    ),
    proposal: vi.fn(async (_b: string, id: string) => stored.get(id)),
    saveReview,
  });
  fireEvent.click(await screen.findByRole("button", { name: "查看建议 q" }));
  await screen.findByText(outlineFixture.conflict, { exact: false });
  const summary = await screen.findByText("对照大纲与前文引文");
  fireEvent.click(summary);
  const compare = within(summary.closest("details")!);
  const outlineField = compare.getByLabelText("选择大纲引文");
  const priorField = compare.getByLabelText("选择前文引文");
  expect(outlineField).toHaveValue("");
  expect(priorField).toHaveValue("");
  expect(
    compare.queryByText(outlineFixture.outline_quote),
  ).not.toBeInTheDocument();
  fireEvent.change(outlineField, { target: { value: "0" } });
  expect(
    compare.queryByText(outlineFixture.outline_quote),
  ).not.toBeInTheDocument();
  const note = screen.getByLabelText(`作者备注：${outline.title}`);
  fireEvent.change(screen.getByLabelText(`作者判断：${outline.title}`), {
    target: { value: "question" },
  });
  fireEvent.change(note, { target: { value: outlineFixture.note } });
  expect(screen.getByRole("button", { name: "查看建议 two" })).toBeDisabled();
  fireEvent.change(priorField, { target: { value: "1" } });
  expect(compare.getByText(outlineFixture.outline_quote)).toBeInTheDocument();
  expect(compare.getByText(outlineFixture.prior_quote)).toBeInTheDocument();
  expect(compare.getByText("material:m / content")).toBeInTheDocument();
  expect(compare.getByText("context:outline-prior / body")).toBeInTheDocument();
  expect(
    compare.getByRole("heading", { name: /昨夜封门.*版本 2.*\[7,/ }),
  ).toBeInTheDocument();
  expect(note).toHaveValue(outlineFixture.note);
  expect(api.pack).toHaveBeenCalledTimes(1);
  expect(api.proposal).toHaveBeenCalledTimes(1);
  expect(saveReview).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "保存逐项审阅" }));
  await screen.findByText("已保存逐项作者审阅 · 版本 1");
  expect(saveReview).toHaveBeenCalledExactlyOnceWith("b", "q", null, "e", [
    { material_id: "m", verdict: "question", note: outlineFixture.note },
  ]);
  expect(screen.getByRole("button", { name: "采纳建议" })).toBeDisabled();
  expect(api.acceptReviewed).not.toHaveBeenCalled();
  expect(api.decide).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "查看建议 two" }));
  await screen.findByText("未自报冲突，不等于系统验证无冲突。");
  const next = within(
    (await screen.findByText("对照大纲与前文引文")).closest("details")!,
  );
  expect(next.getByLabelText("选择大纲引文")).toHaveValue("");
  expect(next.getByLabelText("选择前文引文")).toHaveValue("");
  expect(next.queryByText(outlineFixture.prior_quote)).not.toBeInTheDocument();
  for (const [id, text] of [
    ["foil", outlineFixture.foil],
    ["invalid", outlineFixture.flagged],
  ] as const) {
    fireEvent.click(screen.getByRole("button", { name: `查看建议 ${id}` }));
    await screen.findByText(text);
    expect(screen.queryByText("对照大纲与前文引文")).not.toBeInTheDocument();
  }
  fireEvent.click(screen.getByRole("button", { name: "查看建议 wrong" }));
  await screen.findByText("依据或提案归属不匹配");
  expect(screen.queryByText("对照大纲与前文引文")).not.toBeInTheDocument();
  for (const q of malformedProposals) {
    fireEvent.click(screen.getByRole("button", { name: `查看建议 ${q.id}` }));
    await screen.findByText(q.input.new_body);
    expect(screen.queryByText("对照大纲与前文引文")).not.toBeInTheDocument();
  }
  fireEvent.click(screen.getByRole("button", { name: "查看建议 archive" }));
  await screen.findByText("历史候选");
  const archived = within(
    (await screen.findByText("对照大纲与前文引文")).closest("details")!,
  );
  fireEvent.click(archived.getByText("对照大纲与前文引文"));
  fireEvent.change(archived.getByLabelText("选择大纲引文"), {
    target: { value: "0" },
  });
  fireEvent.change(archived.getByLabelText("选择前文引文"), {
    target: { value: "1" },
  });
  expect(archived.getByText(outlineFixture.outline_quote)).toBeInTheDocument();
  expect(archived.getByText(outlineFixture.prior_quote)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "采纳建议" })).toBeDisabled();
  expect(api.acceptReviewed).not.toHaveBeenCalled();
  // Test the comparison directly with malformed records to avoid relying on
  // existing material-card rendering behavior for intentionally duplicate keys.
  cleanup();
  for (const ambiguous of [
    {
      ...outlinePack,
      materials: [outline, { ...outline, title: "同 ID 的第二张卡" }],
    },
    {
      ...outlinePack,
      context_chapters: [
        ...outlinePack.context_chapters,
        ...outlinePack.context_chapters,
      ],
    },
  ]) {
    const view = render(
      <FrozenOutlineComparison pack={ambiguous} proposal={first} />,
    );
    expect(screen.queryByText("对照大纲与前文引文")).not.toBeInTheDocument();
    view.unmount();
  }
});
