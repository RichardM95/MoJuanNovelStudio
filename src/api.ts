import { invoke } from "@tauri-apps/api/core";
import type { API, LibraryState } from "./types";
export const isDesktop =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export const api: API = {
  draftRecovery: {
    start: () => invoke("start_draft_recovery_session"),
    read: (scope) => invoke("read_recovery_draft", {scope}),
    checkpoint: (input) => invoke("checkpoint_recovery_draft", {input}),
    dismiss: (input) => invoke("dismiss_recovery_draft", {input}),
  },
  foreshadow: {
    proposals: {
      list: (bookId, materialId, pendingOnly, afterId, epoch) => invoke("list_foreshadow_proposals", {bookId, materialId, pendingOnly, afterId, epoch}),
      get: (bookId, materialId, proposalId, epoch) => invoke("get_foreshadow_proposal_trusted", {bookId, materialId, proposalId, epoch}),
      decide: input => invoke("decide_foreshadow_proposal", {input}),
    },
    epoch: () => invoke("get_writing_epoch"),
    overview: (bookId, epoch) => invoke("list_book_foreshadow", {bookId, epoch}),
    directory: (bookId, epoch) => invoke("get_foreshadow_directory", {bookId, epoch}),
    chapter: (bookId, chapterId, epoch) => invoke("get_foreshadow_chapter", {bookId, chapterId, epoch}),
    sourceEpoch: () => invoke("get_foreshadow_source_epoch"),
    sourceWindow: (bookId, chapterId, epoch, start, end) => invoke("get_foreshadow_source_window", {bookId, chapterId, epoch, start, end}),
    list: (bookId, materialId, epoch) => invoke("list_foreshadow_occurrences_trusted", {bookId, materialId, epoch}),
    get: (bookId, materialId, id, epoch) => invoke("get_foreshadow_occurrence", {bookId, materialId, id, epoch}),
    lookup: (bookId, materialId, key, epoch) => invoke("lookup_foreshadow_request", {bookId, materialId, key, epoch}),
    create: (input) => invoke("create_foreshadow_occurrence", {input}),
    setDeleted: (bookId, materialId, id, expectedRevision, epoch, deleted) =>
      invoke("set_foreshadow_occurrence_deleted", {bookId, materialId, id, expectedRevision, epoch, deleted}),
    material: (bookId, id, epoch) => invoke("get_book_foreshadow_material", {bookId, id, epoch}),
    createCard: (input, epoch) => invoke("create_book_foreshadow_material", {input, epoch}),
    saveCard: (bookId, input, epoch) => invoke("save_book_foreshadow_material", {bookId, input, epoch}),
  },
  writing: {
    epoch: () => invoke("get_writing_epoch"),
    state:(bookId,chapterId)=>invoke("get_writing_state",{bookId,chapterId}),
    contextDirectory: (bookId, chapterId) => invoke("get_writing_context_directory", {bookId, chapterId}),
    predecessorDiagnosis: {
      epoch: () => invoke("get_foreshadow_source_epoch"),
      read: (bookId, chapterId, packId, epoch) =>
        invoke("diagnose_writing_predecessors", {bookId, chapterId, packId, epoch}),
    },
    configure: (bookId, chapterId, expected, mode, goal, ids, epoch, contextChapterIds) =>
      invoke("configure_writing_policy_with_context", {bookId, chapterId, expected, mode, goal, ids, epoch, contextChapterIds}),
    prepare:(bookId,chapterId,policyRevision,chapterRevision,epoch)=>invoke("prepare_writing_pack",{bookId,chapterId,policyRevision,chapterRevision,epoch}),
    pack:(bookId,id)=>invoke("get_writing_pack",{bookId,id}),
    proposal:(bookId,id)=>invoke("get_constrained_proposal",{bookId,id}),
    saveReview: (bookId, id, expectedRevision, epoch, items) =>
      invoke("save_constrained_review", { bookId, id, expectedRevision, epoch, items }),
    acceptReviewed: (bookId, id, policyRevision, chapterRevision, epoch, expectedReviewRevision, conflictsAck) =>
      invoke("accept_reviewed_constrained_proposal", { bookId, id, policyRevision, chapterRevision, epoch, expectedReviewRevision, conflictsAck }),
    decide:(bookId,id,approve,policyRevision,chapterRevision,epoch,conflictsAck)=>invoke("decide_constrained_proposal",{bookId,id,approve,policyRevision,chapterRevision,epoch,conflictsAck}),
  },
  split: {prepare:(bookId,chapterId,expectedRevision,utf16Offset,newTitle)=>invoke("prepare_chapter_split",{bookId,chapterId,expectedRevision,utf16Offset,newTitle}),detail:(id,side)=>invoke("get_split_detail",{id,side}),commit:(id)=>invoke("commit_chapter_split",{id}),discard:(id)=>invoke("discard_chapter_split",{id})},
  txtExport: {prepare: (bookId, options) => invoke("prepare_txt_export", {bookId, options}), save: (id) => invoke("save_txt_export", {id}), discard: (id) => invoke("discard_txt_export", {id})},
  txtImport: {
    choose: (mode) => invoke("choose_txt_import", {mode}),
    revise: (id, title, author, mode) => invoke("revise_txt_import", {id, title, author, mode}),
    detail: (id, index) => invoke("get_txt_import_chapter", {id, index}),
    commit: (id) => invoke("commit_txt_import", {id}),
    discard: (id) => invoke("discard_txt_import", {id}),
  },
  projectImport: {
    automatic: true,
    choose: () => invoke("choose_project_import"),
    prepare: (id, options) => invoke("prepare_project_import", {id, options}),
    detail: (id, kind, index) => invoke("get_project_import_detail", {id, kind, index}),
    commit: (id, links, excluded) => invoke("commit_project_import", {id, links, excluded}),
    discard: (id) => invoke("discard_project_import", {id}),
  },
  agentAccess: {
    status: () => invoke("desktop_agent_status"),
    grant: (sessionId, bookId, fanqieSend) => invoke("grant_desktop_agent", {sessionId, bookId, fanqieSend}),
    connectCodex: () => invoke("connect_codex"),
    registerBrowser: () => invoke("register_fanqie_browser"),
    revoke: (sessionId) => invoke("revoke_desktop_agent", {sessionId}),
  },
  fanqie: {
    directory: (bookId) => invoke("get_fanqie_directory", {bookId}),
    packet: (bookId, epoch, expected) => invoke("get_fanqie_packet", {bookId, epoch, expected}),
  },
  replace: {
    preview: (bookId, query, replacement) => invoke("preview_book_replace", {bookId, query, replacement}),
    detail: (id, chapterId) => invoke("get_replace_detail", {id, chapterId}),
    commit: (id) => invoke("commit_book_replace", {id}),
    discard: (id) => invoke("discard_book_replace", {id}),
  },
  backups: {
    startExport: () => invoke("start_backup_export"),
    startPrepare: () => invoke("start_backup_prepare"),
    current: () => invoke("current_backup_job"),
    status: (id) => invoke("backup_job_status", {id}),
    cancel: (id) => invoke("cancel_backup_job", {id}),
    confirm: (id, token, writingAck) => invoke("confirm_backup_restore", {id, token, writingAck:writingAck??false}),
    exportJson: () => invoke("backup_library"),
  },
  tasks: {
    start: (bookId, key) =>
      invoke("start_context_task", { bookId, requestKey: key }),
    list: (bookId) => invoke("list_context_tasks", { bookId }),
    get: (bookId, id) => invoke("get_context_task", { bookId, id }),
    cancel: (bookId, id) => invoke("cancel_context_task", { bookId, id }),
    resume: (bookId, id) => invoke("resume_context_task", { bookId, id }),
    retry: (bookId, id, key) =>
      invoke("retry_context_task", { bookId, id, requestKey: key }),
  },
  searchBook: (bookId, query, limit) =>
    invoke("search_book", { bookId, query, limit }),
  materials: {
    organize: {
      preview: bookId => invoke("preview_material_organization", {bookId}),
      apply: (id, accepted) => invoke("apply_material_organization", {id, accepted}),
    },
    history: {
      list: (bookId,id,before,limit) => invoke("list_material_content_versions",{bookId,id,before,limit}),
      get: (bookId,id,versionId) => invoke("get_material_content_version",{bookId,id,versionId}),
      restore: (bookId,id,versionId,revision,epoch) => invoke("restore_material_content",{bookId,id,versionId,revision,epoch}),
      acknowledge: (bookId,id,revision,epoch) => invoke("acknowledge_material_content",{bookId,id,revision,epoch}),
    },
    review: {
      list: (bookId) => invoke("list_material_proposals", { bookId }),
      decide: (id, approve, expectedRevision) =>
        invoke("decide_material_proposal", { id, approve, expectedRevision }),
    },
    list: (bookId, includeDeleted) =>
      invoke("list_materials", { bookId, includeDeleted }),
    get: (id, includeDeleted) => invoke("get_material", { id, includeDeleted }),
    create: (input) => invoke("create_material", { input }),
    save: (input) => invoke("save_material", { input }),
    trash: (id, expectedRevision) =>
      invoke("trash_material", { id, expectedRevision }),
    restore: (id, expectedRevision) =>
      invoke("restore_material", { id, expectedRevision }),
  },
  workflow: {
    versionBrowser: {
      list: (bookId, chapterId, cursor, limit, markedOnly) => invoke("list_chapter_versions", {bookId, chapterId, cursor, limit, markedOnly}),
      get: (bookId, chapterId, versionId) => invoke("get_chapter_version", {bookId, chapterId, versionId}),
      label: (bookId, chapterId, versionId, label, expectedLabelRevision, expectedEpoch) => invoke("set_chapter_version_label", {bookId, chapterId, versionId, label, expectedLabelRevision, expectedEpoch}),
      restore: (bookId, chapterId, versionId, expectedRevision, expectedEpoch) => invoke("restore_chapter_version_scoped", {bookId, chapterId, versionId, expectedRevision, expectedEpoch}),
    },
    listProposals: (bookId) => invoke("list_proposals", { bookId }),
    decideProposal: (id, approve, expectedRevision) =>
      invoke("decide_proposal", { id, approve, expectedRevision }),
  },
  load: () => invoke<LibraryState>("load_library"),
  apply: (command) => invoke<LibraryState>("apply_command", { command }),
  exportBook: (id) => invoke("export_book", { id }),
  backup: () => invoke("backup_library"),
  restore: async () => { throw new Error("请通过备份面板校验并确认恢复。"); },
};
