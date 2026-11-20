// Actual React panels; the native bridge and persistence are distinct Rust coverage.
import React from "react";
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import story from "../qa/acceptance/single-chapter.json";
import MaterialsPanel from "../src/MaterialsPanel";
import ForeshadowPanel from "../src/ForeshadowPanel";
import type { Material, MaterialsAPI } from "../src/materialTypes";
import type {
  ForeshadowAPI,
  OccurrenceView,
  SourcePosition,
} from "../src/foreshadowTypes";

afterEach(cleanup);

const prior = {
  id: "prior",
  title: story.prior.title,
  body: story.prior.body,
  revision: 1,
  volume_id: "late-tide",
  sort_order: 0,
  deleted_at: null,
};
const currentPrior = {
  ...prior,
  revision: 2,
  body: prior.body + story.provenance.prior_append,
};
const volume = {
  id: prior.volume_id,
  title: story.volume.title,
  book_id: "clock",
  sort_order: 0,
  deleted_at: null,
};
const position: SourcePosition = {
  chapter_id: prior.id,
  chapter_title: prior.title,
  chapter_revision: prior.revision,
  chapter_sort_order: prior.sort_order,
  volume_id: volume.id,
  volume_title: volume.title,
  volume_sort_order: volume.sort_order,
};
const fixture = story.materials.find((card) => card.key === "foreshadow")!;
const material: Material = {
  id: "copper",
  book_id: volume.book_id,
  kind: "foreshadow",
  title: fixture.title,
  fields: {
    setup: fixture.fields.setup!,
    resolution: fixture.fields.resolution!,
  },
  volume_id: null,
  chapter_id: null,
  revision: 1,
  updated_at: 1,
  deleted_at: null,
  story_order: 0,
  story_time: null,
  tracking_status: "planted",
  source_chapter_id: prior.id,
  source_revision: 1,
  source_status: "current",
  payoff_chapter_id: null,
  payoff_revision: null,
  payoff_status: "none",
  content_review_required: false,
};
const unexpected = async () => {
  throw Error("Unexpected bridge operation in fixture");
};

test.each(["stale", "unavailable"] as const)(
  "original copper card shows its %s historical source without silently replacing that revision",
  async (status) => {
    const source = { ...material, source_status: status };
    const retitled = {
      ...source,
      title: story.provenance.ordinary_card_title,
      revision: 2,
      updated_at: 2,
    };
    const refreshed: Material = {
      ...retitled,
      revision: 3,
      updated_at: 3,
      source_revision: 2,
      source_status: "current",
    };
    const api: MaterialsAPI = {
      list: vi.fn(async () => {
        const { fields, ...summary } = source;
        return [summary];
      }),
      get: vi.fn(async () => source),
      save: vi
        .fn()
        .mockResolvedValueOnce(retitled)
        .mockResolvedValueOnce(refreshed),
      create: vi.fn(unexpected),
      trash: vi.fn(unexpected),
      restore: vi.fn(unexpected),
    };
    render(
      <MaterialsPanel
        bookId={volume.book_id}
        api={api}
        volumes={[volume]}
        chapters={
          status === "stale" ? [currentPrior] : [{ ...prior, deleted_at: 10 }]
        }
        foreshadowFocus
        initialMaterialId={source.id}
        onClose={vi.fn()}
      />,
    );
    const label = status === "stale" ? "来源正文已更新，待复核" : "来源不可用";
    await screen.findByText(label);
    expect(screen.getByLabelText("来源章节")).toHaveValue(prior.id);
    expect(screen.getByText(/记录版本 1$/)).toBeVisible();
    expect(screen.getByLabelText("埋设内容")).toHaveValue(
      material.fields.setup,
    );
    expect(screen.getByRole("button", { name: "保存资料" })).toBeDisabled();
    expect(api.save).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText("资料标题"), {
      target: { value: retitled.title },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
    await screen.findByText("已保存");
    expect(api.save).toHaveBeenCalledExactlyOnceWith({
      id: material.id,
      title: retitled.title,
      fields: material.fields,
      volume_id: null,
      chapter_id: null,
      expected_revision: 1,
      source_chapter_id: prior.id,
      source_revision: 1,
      story_order: 0,
      story_time: null,
      tracking_status: "planted",
      payoff_chapter_id: null,
      payoff_revision: null,
    });
    expect(screen.getByText(label)).toBeVisible();
    const verify = screen.queryByRole("button", { name: "按当前章节版本复核" });
    if (status === "unavailable") {
      expect(verify).not.toBeInTheDocument();
      expect(
        screen.getByRole("option", { name: "原来源不可用：prior" }),
      ).toBeInTheDocument();
      return;
    }
    fireEvent.click(verify!);
    expect(screen.getByText(/这不会自动生成或校正内容/)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "取消复核" }));
    expect(screen.getByText(/记录版本 1$/)).toBeVisible();
    fireEvent.click(verify!);
    fireEvent.click(
      screen.getByRole("button", { name: "已核对内容，更新来源版本" }),
    );
    expect(screen.getByText(/记录版本 2$/)).toBeVisible();
    expect(
      screen.getByText("来源修改尚未保存；状态将在保存后重新检查。"),
    ).toBeVisible();
    expect(api.save).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "保存资料" }));
    expect(await screen.findByText("来源版本未变化")).toBeVisible();
    expect(api.save).toHaveBeenLastCalledWith(
      expect.objectContaining({
        expected_revision: 2,
        source_chapter_id: prior.id,
        source_revision: 2,
        fields: material.fields,
        tracking_status: "planted",
      }),
    );
    expect(api.save).toHaveBeenCalledTimes(2);
  },
);

test.each(["body", "move", "trash"] as const)(
  "original recorded occurrence keeps its exact frozen quote and position after %s",
  async (change) => {
    const current =
      change === "body"
        ? { ...position, chapter_revision: 2 }
        : change === "move"
          ? {
              ...position,
              volume_id: "ebb",
              volume_title: story.provenance.later_volume,
              volume_sort_order: 1,
            }
          : null;
    const card = {
      ...material,
      source_status:
        change === "move"
          ? "current"
          : change === "body"
            ? "stale"
            : "unavailable",
    } satisfies Material;
    const { fields, ...summary } = card;
    const { start, end, quote, note } = story.occurrence;
    const old: OccurrenceView = {
      occurrence: {
        id: "frozen-plant",
        book_id: card.book_id,
        material_id: card.id,
        source: position,
        start,
        end,
        quote,
        note,
        role: "plant",
        revision: 0,
        created_at: 1,
        deleted_at: null,
      },
      source_status: current ? "stale" : "unavailable",
      current,
    };
    const api: ForeshadowAPI = {
      epoch: vi.fn(async () => "epoch"),
      overview: vi.fn(async () => [
        { material: summary, occurrence_count: 1, review_count: 1 },
      ]),
      directory: vi.fn(async () => (current ? [current] : [])),
      chapter: vi.fn(async () => {
        if (!current) return unexpected();
        return {
          position: current,
          body: change === "body" ? currentPrior.body : prior.body,
        };
      }),
      list: vi.fn(async () => [old]),
      material: vi.fn(async () => card),
      get: vi.fn(unexpected),
      lookup: vi.fn(unexpected),
      create: vi.fn(unexpected),
      setDeleted: vi.fn(unexpected),
      createCard: vi.fn(unexpected),
      saveCard: vi.fn(unexpected),
    };
    render(
      <ForeshadowPanel
        bookId={card.book_id}
        api={api}
        onClose={vi.fn()}
        onJump={vi.fn()}
      />,
    );
    const selectedCard = await screen.findByRole("button", {
      name: /无名铜牌/,
    });
    expect(selectedCard).toHaveTextContent("已铺设");
    if (change === "move")
      expect(selectedCard).toHaveTextContent("来源版本未变化");
    fireEvent.click(selectedCard);
    const oldPosition = await screen.findByText(
      `冻结来源：${volume.title} · ${prior.title} · 正文 v1 · 标量 [${start}, ${end})`,
    );
    const frozenRow = oldPosition.closest("li")!;
    expect(frozenRow.querySelector("blockquote")?.textContent).toBe(quote);
    expect(frozenRow).toHaveTextContent(
      current ? "铺设 · 来源已变化，待复核" : "铺设 · 来源不可用",
    );
    if (current) {
      expect(
        screen.getByText(
          `当前位置：${current.volume_title} · ${prior.title} · 正文 v${current.chapter_revision}`,
        ),
      ).toBeVisible();
    } else {
      expect(
        screen.getByRole("heading", { name: "来源不可用（冻结位置）" }),
      ).toBeVisible();
      expect(screen.queryByText(/当前位置：/)).not.toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "跳转来源章" }),
      ).not.toBeInTheDocument();
    }
    expect(api.chapter).not.toHaveBeenCalled();
    if (change === "body") {
      fireEvent.change(screen.getByLabelText("选择出现来源章节"), {
        target: { value: prior.id },
      });
      const excerpt =
        await screen.findByLabelText<HTMLTextAreaElement>("已保存来源正文");
      expect(excerpt.value).toBe(currentPrior.body.replaceAll("\r\n", "\n"));
      expect(screen.getByText("请框选片段。当前读取版本 v2。")).toBeVisible();
      expect(screen.getByRole("button", { name: "记录出现" })).toBeDisabled();
      fireEvent.click(screen.getByRole("button", { name: "重读来源正文" }));
      await screen.findByLabelText("已保存来源正文");
      expect(api.chapter).toHaveBeenCalledTimes(2);
      expect(api.chapter).toHaveBeenLastCalledWith(
        card.book_id,
        prior.id,
        "epoch",
      );
      expect(frozenRow.querySelector("blockquote")?.textContent).toBe(quote);
      expect(oldPosition).toBeVisible();
    }
    expect(api.create).not.toHaveBeenCalled();
    expect(api.setDeleted).not.toHaveBeenCalled();
    expect(api.saveCard).not.toHaveBeenCalled();
  },
);
