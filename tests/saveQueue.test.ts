import { describe, it, expect, vi } from "vitest";
import { SaveQueue } from "../src/saveQueue";
describe("SaveQueue", () => {
  it("serializes edits and uses returned revision, never drops newer draft", async () => {
    const calls: any[] = [];
    let finish!: (n: number) => void;
    const q = new SaveQueue("a", 0, async (id, body, revision) => {
      calls.push([id, body, revision]);
      if (calls.length === 1) return new Promise<number>((r) => (finish = r));
      return revision + 1;
    });
    q.edit("first");
    const p = q.flush();
    q.edit("latest");
    expect(calls).toEqual([["a", "first", 0]]);
    finish(1);
    await p;
    expect(calls).toEqual([
      ["a", "first", 0],
      ["a", "latest", 1],
    ]);
    expect(q.dirty).toBe(false);
  });
  it("keeps both base readback and a generic rejection locked against replay", async () => {
    const save = vi.fn(async () => {
      throw Error("disk full");
    });
    const q = new SaveQueue("b", 4, save, "旧稿");
    q.observeEpoch("epoch");
    q.edit("稿件");
    await expect(q.flush()).rejects.toThrow("disk full");
    expect(q.dirty).toBe(true);
    expect(q.reconcile(q.unknownAttempt!, { revision: 4, body: "旧稿" })).toBe(
      "base",
    );
    await expect(q.flush(true)).rejects.toThrow("保存结果未确认");
    expect(save).toHaveBeenCalledTimes(1);
    expect(q.dirty).toBe(true);
  });
  it("flushes immediately after debounce and ignores already saved calls", async () => {
    const save = vi.fn(async () => 1);
    const q = new SaveQueue("c", 0, save);
    q.edit("text");
    await q.flush();
    await q.flush();
    expect(save).toHaveBeenCalledTimes(1);
  });
});
