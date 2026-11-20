import { useEffect, useRef, useState } from "react";
import type {
  DraftRecord,
  DraftRecoveryAPI,
  DraftScope,
  DraftSession,
  DraftView,
} from "./draftRecoveryTypes";
import type { SaveQueue } from "./saveQueue";

export type DraftAnchor = DraftScope & {
  queue: SaveQueue;
  valid: () => boolean;
};
type Input = { base_revision: number; base_body: string; draft_body: string };
type Runtime = {
  anchor: DraftAnchor;
  session: DraftSession;
  scope: DraftScope;
  stream: string;
  timer?: ReturnType<typeof setTimeout>;
  input?: Input;
  serial: number;
  running: boolean;
  loaded: boolean;
  readFailed?: boolean;
  token: string | null;
  protected: boolean;
};
const BODY_MAX_BYTES = 10 * 1024 * 1024;
let streamSequence = 0;
type StartGate = { queued?: { start: () => void; cancel: () => void } };
const starting = new WeakMap<DraftRecoveryAPI, StartGate>();
function startInOrder(api: DraftRecoveryAPI) {
  return new Promise<DraftSession>((resolve, reject) => {
    const occupied = starting.get(api);
    const gate = occupied ?? {};
    const start = () => {
      starting.set(api, gate);
      void Promise.resolve()
        .then(() => api.start())
        .then(resolve, reject)
        .finally(() => {
          const queued = gate.queued;
          gate.queued = undefined;
          if (queued) queued.start();
          else starting.delete(api);
        });
    };
    if (occupied) {
      occupied.queued?.cancel();
      occupied.queued = {
        start,
        cancel: () => reject(Error("恢复会话启动已被替代")),
      };
    } else start();
  });
}
const validId = (value: unknown) =>
  typeof value === "string" && value.length > 0;
const validRev = (value: unknown) =>
  Number.isSafeInteger(value) && Number(value) >= 0;
function canLoad(view: DraftView, record: DraftRecord) {
  return (
    record.state === "matches_base" &&
    record.base_revision === view.current_revision &&
    record.base_body === view.current_body &&
    record.draft_body !== view.current_body
  );
}
function checkedView(value: DraftView, scope: DraftScope) {
  if (
    !value ||
    value.epoch !== scope.epoch ||
    value.book_id !== scope.book_id ||
    value.chapter_id !== scope.chapter_id ||
    !validRev(value.current_revision) ||
    typeof value.current_body !== "string"
  )
    throw Error("恢复副本的范围或存档版本未能核对");
  const record = value.record;
  if (
    record !== null &&
    (!record ||
      !validId(record.token) ||
      !validRev(record.base_revision) ||
      typeof record.base_body !== "string" ||
      typeof record.draft_body !== "string" ||
      !["matches_base", "stale", "already_saved"].includes(record.state) ||
      (record.state === "already_saved" &&
        record.draft_body !== value.current_body) ||
      (record.state === "matches_base" && !canLoad(value, record)))
  )
    throw Error("恢复副本内容不匹配");
  return value;
}

export function useDraftRecovery(
  api: DraftRecoveryAPI | undefined,
  generation: number,
  anchor: DraftAnchor | null,
) {
  const [sessionState, setSession] = useState<{
    api: DraftRecoveryAPI;
    generation: number;
    session: DraftSession;
  } | null>(null);
  const [view, setView] = useState<{
    runtime: Runtime;
    data: DraftView;
  } | null>(null);
  const [notice, setNotice] = useState<{
    runtime: Runtime;
    text: string;
  } | null>(null);
  const [choosing, setChoosing] = useState<Runtime | null>(null);
  const owner = useRef<Runtime | null>(null);
  const order = useRef(0);
  const lastStart = useRef<{
    api: DraftRecoveryAPI;
    generation: number;
    result: Promise<DraftSession>;
  } | null>(null);
  const current = useRef({ api, generation, anchor });
  current.current = { api, generation, anchor };
  const session =
    sessionState?.api === api && sessionState?.generation === generation
      ? sessionState.session
      : null;
  const isCurrent = (state: Runtime) =>
    owner.current === state &&
    state.anchor.valid() &&
    current.current.anchor === state.anchor &&
    current.current.api === api &&
    current.current.generation === generation;
  const tell = (state: Runtime, text: string) => {
    if (isCurrent(state)) setNotice({ runtime: state, text });
  };

  useEffect(() => {
    if (!api) return;
    let active = true;
    if (
      lastStart.current?.api !== api ||
      lastStart.current?.generation !== generation
    )
      lastStart.current = { api, generation, result: startInOrder(api) };
    void lastStart.current.result
      .then((next) => {
        if (!active) return;
        if (next && validId(next.session_id) && validId(next.epoch)) {
          order.current = 0;
          setSession({ api, generation, session: next });
        }
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [api, generation]);

  const pump = async (state: Runtime) => {
    if (
      !api ||
      !isCurrent(state) ||
      !state.loaded ||
      state.protected ||
      state.running ||
      state.timer ||
      !state.input ||
      !state.anchor.queue.dirty
    )
      return;
    const input = state.input;
    state.input = undefined;
    if (
      new Blob([input.draft_body]).size > BODY_MAX_BYTES ||
      new Blob([input.base_body]).size > BODY_MAX_BYTES
    ) {
      tell(state, "正文超过单章恢复副本 10 MiB 上限；未写入最新副本。");
      return;
    }
    const serial = state.serial;
    state.running = true;
    try {
      const result = await api.checkpoint({
        ...state.scope,
        ...input,
        session_id: state.session.session_id,
        stream_id: state.stream,
        sequence: ++order.current,
        expected_token: state.token,
      });
      if (!isCurrent(state)) return;
      if (
        !result ||
        !validId(result.token) ||
        !["matches_base", "stale", "already_saved"].includes(result.state)
      )
        throw Error("恢复回执未确认");
      // Record the exact result for the next CAS, but don't claim an older
      // in-flight reply covers a later draft generation.
      state.token = result.token;
      if (serial === state.serial && state.anchor.queue.dirty)
        tell(
          state,
          result.state === "already_saved"
            ? "本次核对时存档已包含这份输入，无需再载入恢复副本。此提示不确认正文保存回执。"
            : "恢复副本已保存；正文仍未保存。",
        );
    } catch {
      if (serial === state.serial)
        tell(state, "未能确认最新恢复副本；请保留编辑器中的稿件。");
    } finally {
      state.running = false;
      if (isCurrent(state) && state.input) void pump(state);
    }
  };

  useEffect(() => {
    if (!api || !anchor || !session || anchor.epoch !== session.epoch) return;
    const scope = {
      epoch: anchor.epoch,
      book_id: anchor.book_id,
      chapter_id: anchor.chapter_id,
    };
    const state: Runtime = {
      anchor,
      scope,
      session,
      stream: `chapter-draft-${++streamSequence}`,
      serial: 0,
      running: false,
      loaded: false,
      token: null,
      protected: true,
    };
    owner.current = state;
    void api
      .read(scope)
      .then((raw) => {
        if (!isCurrent(state)) return;
        const observed = checkedView(raw, scope);
        state.loaded = true;
        state.protected =
          !!observed.record && observed.record.state !== "already_saved";
        state.token = observed.record?.token ?? null;
        setView({ runtime: state, data: observed });
        if (state.protected && state.input)
          tell(state, "保留已有恢复副本，未覆盖。");
        else void pump(state);
      })
      .catch(() => {
        state.readFailed = true;
        tell(state, "无法读取本地恢复副本；未覆盖任何副本。");
      });
    return () => {
      if (state.timer) clearTimeout(state.timer);
      if (owner.current === state) owner.current = null;
    };
  }, [api, anchor, session]);

  function input(queue: SaveQueue, text: string) {
    const state = owner.current;
    if (!state || state.anchor.queue !== queue || !isCurrent(state)) return;
    const base = queue.baseline;
    state.serial++;
    if (state.timer) clearTimeout(state.timer);
    state.timer = undefined;
    if (state.readFailed) {
      tell(state, "无法读取本地恢复副本；未覆盖任何副本。");
      return;
    }
    if (state.loaded && state.protected) {
      tell(state, "保留已有恢复副本，未覆盖。");
      return;
    }
    state.input = {
      base_revision: base.revision,
      base_body: base.body,
      draft_body: text,
    };
    tell(state, "恢复副本待写入；尚未确认。");
    state.timer = setTimeout(() => {
      state.timer = undefined;
      void pump(state);
    }, 450);
  }

  const visible =
    view?.runtime === owner.current &&
    isCurrent(view.runtime) &&
    view.data.record?.state !== "already_saved"
      ? view.data
      : null;
  const record = visible?.record ?? null;
  const active = owner.current;
  const canChoose =
    !!active &&
    !!visible &&
    !!record &&
    !active.anchor.queue.dirty &&
    active.protected &&
    canLoad(visible, record) &&
    choosing !== active;
  const load = async (apply: (queue: SaveQueue, text: string) => void) => {
    if (!api || !active || !record || !canChoose) return;
    const serial = active.serial;
    setChoosing(active);
    try {
      const fresh = checkedView(await api.read(active.scope), active.scope);
      if (
        !isCurrent(active) ||
        active.anchor.queue.dirty ||
        active.serial !== serial
      )
        return;
      const reread = fresh.record;
      const base = active.anchor.queue.baseline;
      if (
        !reread ||
        reread.token !== record.token ||
        reread.draft_body !== record.draft_body ||
        reread.base_revision !== record.base_revision ||
        reread.base_body !== record.base_body ||
        !canLoad(fresh, reread) ||
        base.revision !== fresh.current_revision ||
        base.body !== fresh.current_body
      ) {
        setView({ runtime: active, data: fresh });
        tell(active, "副本或存档已变化，未载入。");
        return;
      }
      active.token = reread.token;
      active.protected = false;
      setView(null);
      apply(active.anchor.queue, reread.draft_body);
      tell(active, "已载入恢复副本；正文未保存，请手动保存。");
    } catch {
      tell(active, "无法再次核对副本；未载入。");
    } finally {
      if (isCurrent(active))
        setChoosing((value) => (value === active ? null : value));
    }
  };
  return {
    input,
    load,
    canChoose,
    checking: choosing === active && active !== null,
    record,
    saved: visible
      ? { revision: visible.current_revision, body: visible.current_body }
      : null,
    message:
      notice?.runtime === active &&
      active &&
      isCurrent(active) &&
      active.anchor.queue.dirty
        ? notice.text
        : "",
    available: !!api,
  };
}
