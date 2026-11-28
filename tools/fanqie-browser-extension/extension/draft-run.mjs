const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// Only observations are retried. Each navigation or save click is issued once.
export async function observe(read, accept = () => true, tries = 24) {
  let reason = "页面尚未就绪";
  for (let n = 0; n < tries; n++) {
    try {
      const value = await read();
      if (accept(value)) return value;
    } catch (e) {
      reason = e.message;
    }
    if (n + 1 < tries) await pause(250);
  }
  throw Error(reason);
}

async function locate(page, number) {
  let listing = await observe(() => page.step("list"));
  const total = listing.total;
  if (total > 20_000) throw Error("草稿箱超过可核对范围");
  const seen = new Set();
  let found = -1;
  for (let index = 0; ; index++) {
    if (listing.total !== total) throw Error("草稿箱总数发生变化");
    for (const n of listing.numbers) {
      if (seen.has(n)) throw Error("草稿箱内出现重复章号");
      seen.add(n);
      if (n === number) found = index;
    }
    if (seen.size === total) return { found, last: index };
    if (!listing.numbers.length || seen.size > total)
      throw Error("无法完整读取草稿列表");
    const signature = listing.numbers.join(",");
    await page.step("next");
    listing = await observe(
      () => page.step("list"),
      (v) => v.numbers.join(",") !== signature,
    );
  }
}

export async function deliverDraft(job, browser) {
  // Re-open the list in a fresh tab for each reconciliation, including after disconnect.
  async function scan() {
    const page = await browser.listPage(job.target.list_url);
    const { found, last } = await locate(page, job.number);
    if (found === -1) return { page, exists: false };
    let target = page;
    if (found !== last) {
      target = await browser.listPage(job.target.list_url);
      for (let i = 0; i < found; i++) {
        const before = await observe(() => target.step("list"));
        await target.step("next");
        await observe(
          () => target.step("list"),
          (v) => v.numbers.join(",") !== before.numbers.join(","),
        );
      }
    }
    await target.step("open");
    const saved = await observe(
      () => target.step("inspect"),
      (v) => v.matches || !v.empty,
    );
    if (!saved.matches) throw Error("同号草稿内容或标题不同，已保留原稿");
    return { page: target, exists: true };
  }
  const existing = await scan();
  if (existing.exists)
    return { state: "exists", detail: "草稿箱中已有相同章稿" };
  if (job.verify_only)
    throw Error("上次存稿结果不明，草稿箱未找到该章，请人工核对");
  await browser.validate();
  await existing.page.step("new");
  await observe(
    () => existing.page.step("inspect"),
    (v) => v.empty,
  );
  await browser.validate();
  await existing.page.step("fill");
  await browser.validate();
  // A navigation may interrupt the reply to a successful save. Reconcile independently.
  let saveError = "";
  try {
    await existing.page.step("save");
  } catch (e) {
    saveError = e.message;
  }
  for (let i = 0; i < 3; i++) {
    if (i) await pause(750);
    if ((await scan()).exists)
      return {
        state: "verified",
        detail: "已从草稿箱重新打开并核对章号、标题和正文",
      };
  }
  throw Error(saveError || "已尝试存稿，草稿箱尚未确认，请人工核对");
}
