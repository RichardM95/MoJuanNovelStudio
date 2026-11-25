import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  FileText,
  FolderPlus,
  GripVertical,
  MoreHorizontal,
  Plus,
} from "lucide-react";
import type { DragEvent as ReactDragEvent, KeyboardEvent as ReactKeyboardEvent } from "react";
import type { Chapter, Command, LibraryState, Volume } from "./types";

type Target = { kind: "chapter" | "volume"; id: string };
type Rename = { kind: "chapter" | "volume"; id: string; value: string };
type DropHint =
  | { kind: "chapter"; id: string; edge: "before" | "after" }
  | { kind: "volume"; id: string; edge: "before" | "after" }
  | { kind: "volume-end"; id: string };

const MENU_WIDTH = 188;
const MENU_HEIGHT = 360;

export default function Directory({
  bookId,
  volumes,
  chapters,
  selected,
  busy,
  run,
  mutate,
  select,
  confirm,
  titleForm,
  onOpen,
}: {
  bookId: string;
  volumes: Volume[];
  chapters: Chapter[];
  selected: Chapter | null;
  busy: boolean;
  run: (fn: () => Promise<void>) => Promise<boolean>;
  mutate: (command: Command) => Promise<LibraryState>;
  select: (chapter: Chapter | null) => void;
  confirm: (text: string, run: () => Promise<void>) => void;
  titleForm: (
    heading: string,
    title: string,
    submit: (title: string, author: string) => Promise<void>,
  ) => void;
  onOpen: (chapter: Chapter) => void;
}) {
  const [menu, setMenu] = useState<{ target: Target; left: number; top: number } | null>(null);
  const [renaming, setRenaming] = useState<Rename | null>(null);
  const [dragging, setDragging] = useState<Target | null>(null);
  const [drop, setDrop] = useState<DropHint | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const scrollRef = useRef<HTMLElement | null>(null);
  const renameCommitted = useRef(false);

  const chaptersOf = (volumeId: string) =>
    chapters
      .filter((c) => c.volume_id === volumeId && c.deleted_at === null)
      .sort((a, b) => a.sort_order - b.sort_order);
  const orderedChapters = volumes.flatMap((v) => chaptersOf(v.id));
  const numberOf = (id: string) => orderedChapters.findIndex((c) => c.id === id) + 1;

  const openMenu = (target: Target, left: number, top: number, alignRight = false) => {
    const x = alignRight ? left - MENU_WIDTH : left;
    setMenu({
      target,
      left: Math.max(8, Math.min(x, window.innerWidth - MENU_WIDTH - 8)),
      top: Math.max(8, Math.min(top, window.innerHeight - MENU_HEIGHT - 12)),
    });
  };

  useEffect(() => {
    if (!menu) return;
    const first = menuRef.current?.querySelector<HTMLButtonElement>(
      '[role="menuitem"]:not(:disabled)',
    );
    first?.focus();
    const closeIfOutside = (event: Event) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenu(null);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        setMenu(null);
        return;
      }
      if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
      const items = Array.from(
        menuRef.current?.querySelectorAll<HTMLButtonElement>(
          '[role="menuitem"]:not(:disabled)',
        ) ?? [],
      );
      if (!items.length) return;
      event.preventDefault();
      const index = items.indexOf(document.activeElement as HTMLButtonElement);
      const next =
        event.key === "ArrowDown"
          ? (index + 1) % items.length
          : (index - 1 + items.length) % items.length;
      items[next].focus();
    };
    window.addEventListener("pointerdown", closeIfOutside, true);
    window.addEventListener("scroll", closeIfOutside, true);
    window.addEventListener("resize", closeIfOutside);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", closeIfOutside, true);
      window.removeEventListener("scroll", closeIfOutside, true);
      window.removeEventListener("resize", closeIfOutside);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [menu]);

  const startRename = (kind: "chapter" | "volume", id: string, value: string) => {
    renameCommitted.current = false;
    setMenu(null);
    setRenaming({ kind, id, value });
  };

  const commitRename = async () => {
    if (!renaming || renameCommitted.current) return;
    const next = renaming.value.trim();
    if (!next) {
      renameCommitted.current = true;
      setRenaming(null);
      return;
    }
    renameCommitted.current = true;
    const ok = await run(async () => {
      await mutate({
        type: renaming.kind === "volume" ? "rename_volume" : "rename_chapter",
        id: renaming.id,
        title: next,
      });
    });
    if (ok) setRenaming(null);
    else renameCommitted.current = false;
  };

  const cancelRename = () => {
    renameCommitted.current = true;
    setRenaming(null);
  };

  const placeChapter = (id: string, volumeId: string, beforeId: string | null) =>
    run(async () => {
      await mutate({
        type: "place_chapter",
        id,
        volume_id: volumeId,
        before_id: beforeId,
      });
    });

  const moveChapter = (chapter: Chapter, direction: -1 | 1) => {
    const index = orderedChapters.findIndex((c) => c.id === chapter.id);
    const target = orderedChapters[index + direction];
    if (!target) return;
    let beforeId: string | null;
    if (direction < 0) {
      beforeId = target.volume_id === chapter.volume_id ? target.id : null;
    } else {
      const siblings = chaptersOf(target.volume_id);
      const next = siblings[siblings.findIndex((c) => c.id === target.id) + 1];
      beforeId = next?.id ?? null;
    }
    void placeChapter(chapter.id, target.volume_id, beforeId);
  };

  const moveVolume = (volume: Volume, direction: -1 | 1) => {
    const index = volumes.findIndex((v) => v.id === volume.id);
    const target = volumes[index + direction];
    if (!target) return;
    const beforeId =
      direction < 0 ? target.id : volumes[index + 2]?.id ?? null;
    void run(async () => {
      await mutate({ type: "place_volume", id: volume.id, before_id: beforeId });
    });
  };

  const deleteChapter = (chapter: Chapter) => {
    setMenu(null);
    confirm(`将章节《${chapter.title}》移入回收站？可恢复。`, async () => {
      await mutate({ type: "trash_chapter", id: chapter.id });
      if (selected?.id === chapter.id) select(null);
    });
  };

  const deleteVolume = (volume: Volume) => {
    setMenu(null);
    confirm(`将卷《${volume.title}》及其中章节移入回收站？可恢复，正文不会丢失。`, async () => {
      await mutate({ type: "trash_volume", id: volume.id });
      if (selected?.volume_id === volume.id) select(null);
    });
  };

  const addChapter = (volume: Volume) =>
    titleForm("新增章节", "", async (title) => {
      const before = new Set(chapters.map((c) => c.id));
      const next = await mutate({ type: "create_chapter", volume_id: volume.id, title });
      select(next.chapters.find((c) => !before.has(c.id)) ?? null);
    });

  const addVolume = () =>
    titleForm("新增卷", "", async (title) => {
      await mutate({ type: "create_volume", book_id: bookId, title });
    });

  const chapterKeys = (event: ReactKeyboardEvent, chapter: Chapter) => {
    if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      event.preventDefault();
      moveChapter(chapter, event.key === "ArrowUp" ? -1 : 1);
    } else if (event.ctrlKey && event.key === "Delete") {
      event.preventDefault();
      deleteChapter(chapter);
    }
  };

  const volumeKeys = (event: ReactKeyboardEvent, volume: Volume) => {
    if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
    event.preventDefault();
    moveVolume(volume, event.key === "ArrowUp" ? -1 : 1);
  };

  const startDrag = (event: ReactDragEvent, target: Target) => {
    if (busy) {
      event.preventDefault();
      return;
    }
    setDragging(target);
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData("text/plain", target.id);
    }
  };

  const endDrag = () => {
    setDragging(null);
    setDrop(null);
  };

  const dragOverChapter = (event: ReactDragEvent, chapter: Chapter) => {
    if (!dragging || dragging.kind !== "chapter") return;
    event.preventDefault();
    if (dragging.id === chapter.id) {
      setDrop(null);
      return;
    }
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    const rect = event.currentTarget.getBoundingClientRect();
    const edge = event.clientY - rect.top < rect.height / 2 ? "before" : "after";
    setDrop((current) =>
      current?.kind === "chapter" && current.id === chapter.id && current.edge === edge
        ? current
        : { kind: "chapter", id: chapter.id, edge },
    );
  };

  const dropOnChapter = (event: ReactDragEvent, chapter: Chapter) => {
    event.preventDefault();
    const dragged = dragging;
    const hint = drop;
    endDrag();
    if (!dragged || dragged.kind !== "chapter" || dragged.id === chapter.id) return;
    let beforeId: string | null;
    if (hint?.kind === "chapter" && hint.id === chapter.id && hint.edge === "after") {
      const siblings = chaptersOf(chapter.volume_id);
      const next = siblings[siblings.findIndex((c) => c.id === chapter.id) + 1];
      beforeId = next?.id ?? null;
    } else {
      beforeId = chapter.id;
    }
    void placeChapter(dragged.id, chapter.volume_id, beforeId);
  };

  const dragOverVolume = (event: ReactDragEvent, volume: Volume) => {
    if (!dragging) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    if (dragging.kind === "chapter") {
      setDrop({ kind: "volume-end", id: volume.id });
      return;
    }
    if (dragging.id === volume.id) {
      setDrop(null);
      return;
    }
    const rect = event.currentTarget.getBoundingClientRect();
    const edge = event.clientY - rect.top < rect.height / 2 ? "before" : "after";
    setDrop((current) =>
      current?.kind === "volume" && current.id === volume.id && current.edge === edge
        ? current
        : { kind: "volume", id: volume.id, edge },
    );
  };

  const dropOnVolume = (event: ReactDragEvent, volume: Volume) => {
    event.preventDefault();
    const dragged = dragging;
    const hint = drop;
    endDrag();
    if (!dragged) return;
    if (dragged.kind === "chapter") {
      void placeChapter(dragged.id, volume.id, null);
      return;
    }
    if (dragged.id === volume.id) return;
    const beforeId =
      hint?.kind === "volume" && hint.id === volume.id && hint.edge === "after"
        ? volumes[volumes.findIndex((v) => v.id === volume.id) + 1]?.id ?? null
        : volume.id;
    void run(async () => {
      await mutate({ type: "place_volume", id: dragged.id, before_id: beforeId });
    });
  };

  const autoScroll = (event: ReactDragEvent) => {
    const element = scrollRef.current;
    if (!dragging || !element) return;
    const rect = element.getBoundingClientRect();
    if (event.clientY - rect.top < 36) element.scrollTop -= 12;
    else if (rect.bottom - event.clientY < 36) element.scrollTop += 12;
  };

  const menuChapter =
    menu && menu.target.kind === "chapter"
      ? chapters.find((c) => c.id === menu.target.id && c.deleted_at === null)
      : undefined;
  const menuVolume =
    menu && menu.target.kind === "volume"
      ? volumes.find((v) => v.id === menu.target.id)
      : undefined;
  const menuChapterIndex = menuChapter
    ? orderedChapters.findIndex((c) => c.id === menuChapter.id)
    : -1;
  const menuVolumeIndex = menuVolume
    ? volumes.findIndex((v) => v.id === menuVolume.id)
    : -1;

  return (
    <aside
      className="directory"
      ref={scrollRef}
      onDragOver={autoScroll}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node)) setDrop(null);
      }}
    >
      <div className="directory-title">
        <strong>目录</strong>
        <button aria-label="新增卷" disabled={busy} onClick={addVolume}>
          <FolderPlus size={18} />
        </button>
      </div>

      {volumes.map((volume, vi) => (
        <section key={volume.id} className="volume">
          <div
            className={`volume-title ${
              dragging?.kind === "volume" && dragging.id === volume.id ? "dragging" : ""
            } ${
              drop?.kind === "volume-end" && drop.id === volume.id
                ? "drop-into"
                : drop?.kind === "volume" && drop.id === volume.id
                  ? drop.edge === "before"
                    ? "drop-before"
                    : "drop-after"
                  : ""
            }`}
            draggable={!renaming && !busy}
            onDragStart={(event) => startDrag(event, { kind: "volume", id: volume.id })}
            onDragOver={(event) => dragOverVolume(event, volume)}
            onDrop={(event) => dropOnVolume(event, volume)}
            onDragEnd={endDrag}
            onContextMenu={(event) => {
              event.preventDefault();
              if (renaming) return;
              openMenu({ kind: "volume", id: volume.id }, event.clientX, event.clientY);
            }}
          >
            <span className="drag-handle" aria-hidden="true">
              <GripVertical size={13} />
            </span>
            {renaming?.kind === "volume" && renaming.id === volume.id ? (
              <input
                className="rename-input"
                aria-label="修改卷名"
                value={renaming.value}
                autoFocus
                onFocus={(event) => event.currentTarget.select()}
                onChange={(event) => setRenaming({ ...renaming, value: event.target.value })}
                onClick={(event) => event.stopPropagation()}
                onBlur={() => void commitRename()}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    void commitRename();
                  } else if (event.key === "Escape") {
                    event.preventDefault();
                    cancelRename();
                  }
                }}
              />
            ) : (
              <button
                className="volume-name"
                onDoubleClick={() => startRename("volume", volume.id, volume.title)}
                onKeyDown={(event) => volumeKeys(event, volume)}
              >
                第{vi + 1}卷 {volume.title}
              </button>
            )}
            <button
              className="row-menu"
              aria-label={`卷操作 ${volume.title}`}
              aria-haspopup="menu"
              aria-expanded={menu?.target.kind === "volume" && menu.target.id === volume.id}
              onClick={(event) => {
                event.stopPropagation();
                const rect = event.currentTarget.getBoundingClientRect();
                openMenu({ kind: "volume", id: volume.id }, rect.right, rect.bottom + 4, true);
              }}
            >
              <MoreHorizontal size={15} />
            </button>
          </div>

          {chaptersOf(volume.id).map((chapter) => (
            <div
              key={chapter.id}
              className={`chapter-row ${
                chapter.id === selected?.id ? "selected" : ""
              } ${
                dragging?.kind === "chapter" && dragging.id === chapter.id ? "dragging" : ""
              } ${
                drop?.kind === "chapter" && drop.id === chapter.id
                  ? drop.edge === "before"
                    ? "drop-before"
                    : "drop-after"
                  : ""
              }`}
              draggable={!renaming && !busy}
              onDragStart={(event) => startDrag(event, { kind: "chapter", id: chapter.id })}
              onDragOver={(event) => dragOverChapter(event, chapter)}
              onDrop={(event) => dropOnChapter(event, chapter)}
              onDragEnd={endDrag}
              onContextMenu={(event) => {
                event.preventDefault();
                if (renaming) return;
                openMenu({ kind: "chapter", id: chapter.id }, event.clientX, event.clientY);
              }}
            >
              <span className="drag-handle" aria-hidden="true">
                <GripVertical size={12} />
              </span>
              {renaming?.kind === "chapter" && renaming.id === chapter.id ? (
                <input
                  className="rename-input"
                  aria-label="修改章名"
                  value={renaming.value}
                  autoFocus
                  onFocus={(event) => event.currentTarget.select()}
                  onChange={(event) => setRenaming({ ...renaming, value: event.target.value })}
                  onClick={(event) => event.stopPropagation()}
                  onBlur={() => void commitRename()}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      void commitRename();
                    } else if (event.key === "Escape") {
                      event.preventDefault();
                      cancelRename();
                    }
                  }}
                />
              ) : (
                <button
                  className="chapter-open"
                  onClick={() => onOpen(chapter)}
                  onDoubleClick={() => startRename("chapter", chapter.id, chapter.title)}
                  onKeyDown={(event) => chapterKeys(event, chapter)}
                >
                  <FileText size={15} />
                  <span>
                    第{numberOf(chapter.id)}章 {chapter.title}
                  </span>
                </button>
              )}
              <button
                className="row-menu"
                aria-label={`章节操作 ${chapter.title}`}
                aria-haspopup="menu"
                aria-expanded={menu?.target.kind === "chapter" && menu.target.id === chapter.id}
                onClick={(event) => {
                  event.stopPropagation();
                  const rect = event.currentTarget.getBoundingClientRect();
                  openMenu({ kind: "chapter", id: chapter.id }, rect.right, rect.bottom + 4, true);
                }}
              >
                <MoreHorizontal size={15} />
              </button>
            </div>
          ))}

          <button className="add-chapter" disabled={busy} onClick={() => addChapter(volume)}>
            <Plus size={14} />
            新增章节
          </button>
        </section>
      ))}

      {menu &&
        (menuChapter || menuVolume) &&
        createPortal(
          <div
            ref={menuRef}
            className="dir-menu"
            role="menu"
            aria-label={menuChapter ? "章节操作" : "卷操作"}
            style={{ left: menu.left, top: menu.top }}
          >
            {menuChapter && (
              <>
                <button
                  role="menuitem"
                  onClick={() => startRename("chapter", menuChapter.id, menuChapter.title)}
                >
                  重命名
                </button>
                <button
                  role="menuitem"
                  disabled={menuChapterIndex <= 0 || busy}
                  onClick={() => {
                    setMenu(null);
                    moveChapter(menuChapter, -1);
                  }}
                >
                  上移
                </button>
                <button
                  role="menuitem"
                  disabled={
                    menuChapterIndex < 0 ||
                    menuChapterIndex >= orderedChapters.length - 1 ||
                    busy
                  }
                  onClick={() => {
                    setMenu(null);
                    moveChapter(menuChapter, 1);
                  }}
                >
                  下移
                </button>
                <div className="dir-menu-label">移动到卷</div>
                {volumes.map((volume, vi) => (
                  <button
                    key={volume.id}
                    role="menuitem"
                    aria-label={`移动到卷 ${volume.title}`}
                    disabled={volume.id === menuChapter.volume_id || busy}
                    onClick={() => {
                      setMenu(null);
                      void placeChapter(menuChapter.id, volume.id, null);
                    }}
                  >
                    第{vi + 1}卷 {volume.title}
                  </button>
                ))}
                <div className="dir-menu-sep" />
                <button
                  role="menuitem"
                  className="danger"
                  disabled={busy}
                  onClick={() => deleteChapter(menuChapter)}
                >
                  删除章节
                </button>
              </>
            )}
            {menuVolume && (
              <>
                <button
                  role="menuitem"
                  onClick={() => startRename("volume", menuVolume.id, menuVolume.title)}
                >
                  重命名
                </button>
                <button
                  role="menuitem"
                  disabled={busy}
                  onClick={() => {
                    setMenu(null);
                    addChapter(menuVolume);
                  }}
                >
                  新增章节
                </button>
                <button
                  role="menuitem"
                  disabled={menuVolumeIndex <= 0 || busy}
                  onClick={() => {
                    setMenu(null);
                    moveVolume(menuVolume, -1);
                  }}
                >
                  上移
                </button>
                <button
                  role="menuitem"
                  disabled={menuVolumeIndex < 0 || menuVolumeIndex >= volumes.length - 1 || busy}
                  onClick={() => {
                    setMenu(null);
                    moveVolume(menuVolume, 1);
                  }}
                >
                  下移
                </button>
                <div className="dir-menu-sep" />
                <button
                  role="menuitem"
                  className="danger"
                  disabled={busy}
                  onClick={() => deleteVolume(menuVolume)}
                >
                  删除卷
                </button>
              </>
            )}
          </div>,
          document.body,
        )}
    </aside>
  );
}
