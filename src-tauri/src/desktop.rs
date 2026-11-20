use crate::{files::write_new_file, Command, LibraryState, Service};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
type Store = Arc<Mutex<Service>>;

async fn draft_recovery_io<T: Send + 'static>(
    store: Store,
    action: impl FnOnce(&mut Service) -> crate::WorkflowResult<T> + Send + 'static,
) -> crate::WorkflowResult<T> {
    fn unavailable() -> crate::WorkflowError {
        crate::WorkflowError {
            code: "draft_storage".into(),
            message: "草稿副本存储暂不可用".into(),
        }
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut service = store.lock().map_err(|_| unavailable())?;
        action(&mut service)
    })
    .await
    .map_err(|_| unavailable())?
}

#[tauri::command]
async fn start_draft_recovery_session(
    state: State<'_, Store>,
) -> crate::WorkflowResult<crate::DraftRecoverySession> {
    draft_recovery_io(Arc::clone(state.inner()), Service::start_draft_recovery_session).await
}

#[tauri::command]
async fn read_recovery_draft(
    state: State<'_, Store>,
    scope: crate::DraftRecoveryScope,
) -> crate::WorkflowResult<crate::RecoveryDraftRead> {
    draft_recovery_io(Arc::clone(state.inner()), move |s| s.read_recovery_draft(&scope)).await
}

#[tauri::command]
async fn checkpoint_recovery_draft(
    state: State<'_, Store>,
    input: crate::CheckpointRecoveryDraft,
) -> crate::WorkflowResult<crate::RecoveryDraftReceipt> {
    draft_recovery_io(Arc::clone(state.inner()), move |s| s.checkpoint_recovery_draft(input)).await
}

#[tauri::command]
async fn dismiss_recovery_draft(
    state: State<'_, Store>,
    input: crate::DismissRecoveryDraft,
) -> crate::WorkflowResult<()> {
    draft_recovery_io(Arc::clone(state.inner()), move |s| s.dismiss_recovery_draft(input)).await
}

fn with_foreshadow<T>(
    state: State<'_, Store>,
    action: impl FnOnce(&mut Service) -> crate::WorkflowResult<T>,
) -> crate::WorkflowResult<T> {
    let mut service = state.lock().map_err(|_| crate::WorkflowError {
        code: "unavailable".into(), message: "书库暂不可用".into(),
    })?;
    action(&mut service)
}
#[tauri::command]
fn list_foreshadow_proposals(state: State<'_, Store>, book_id: String, material_id: Option<String>, pending_only: bool, after_id: Option<String>, limit: Option<u32>, epoch: String) -> crate::WorkflowResult<crate::ForeshadowProposalPage> {
    with_foreshadow(state, |s| s.list_foreshadow_proposals(&book_id, material_id.as_deref(), pending_only, after_id.as_deref(), limit, &epoch))
}
#[tauri::command]
fn get_foreshadow_proposal_trusted(state: State<'_, Store>, book_id: String, material_id: String, proposal_id: String, epoch: String) -> crate::WorkflowResult<crate::ForeshadowProposal> {
    with_foreshadow(state, |s| s.get_foreshadow_proposal(&book_id, &material_id, &proposal_id, &epoch))
}
#[tauri::command]
fn decide_foreshadow_proposal(state: State<'_, Store>, input: crate::DecideForeshadowProposal) -> crate::WorkflowResult<crate::ForeshadowProposal> {
    with_foreshadow(state, |s| s.decide_foreshadow_proposal(input))
}
#[tauri::command]
fn list_book_foreshadow(state: State<'_, Store>, book_id: String, epoch: String) -> crate::WorkflowResult<Vec<crate::ForeshadowSummary>> {
    with_foreshadow(state, |s| s.list_book_foreshadow(&book_id, &epoch))
}
#[tauri::command]
fn get_foreshadow_directory(state: State<'_, Store>, book_id: String, epoch: String) -> crate::WorkflowResult<Vec<crate::ForeshadowPosition>> {
    with_foreshadow(state, |s| s.get_foreshadow_directory(&book_id, &epoch))
}
#[tauri::command]
fn get_foreshadow_chapter(state: State<'_, Store>, book_id: String, chapter_id: String, epoch: String) -> crate::WorkflowResult<crate::ForeshadowChapterExcerpt> {
    with_foreshadow(state, |s| s.get_foreshadow_chapter(&book_id, &chapter_id, &epoch))
}
// Only the two optional review-source reads use this worker path. Do not
// wait for a busy Store or change the scheduling of existing writes.
async fn review_source_read<T: Send + 'static>(
    store: Store,
    read: impl FnOnce(&Service) -> crate::WorkflowResult<T> + Send + 'static,
) -> crate::WorkflowResult<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = store.try_lock().map_err(|e| match e {
            std::sync::TryLockError::WouldBlock => crate::WorkflowError {
                code: "busy".into(), message: "书库正忙".into(),
            },
            std::sync::TryLockError::Poisoned(_) => crate::WorkflowError {
                code: "unavailable".into(), message: "书库暂不可用".into(),
            },
        })?;
        read(&service)
    }).await.map_err(|_| crate::WorkflowError {
        code: "unavailable".into(), message: "来源核对工作线程不可用".into(),
    })?
}

#[tauri::command]
async fn get_foreshadow_source_epoch(state: State<'_, Store>) -> crate::WorkflowResult<String> {
    review_source_read(Arc::clone(state.inner()), Service::epoch).await
}

#[tauri::command]
async fn get_foreshadow_source_window(
    state: State<'_, Store>,
    book_id: String,
    chapter_id: String,
    epoch: String,
    start: u32,
    end: u32,
) -> crate::WorkflowResult<crate::ForeshadowSourceWindow> {
    review_source_read(Arc::clone(state.inner()), move |s| {
        s.get_foreshadow_source_window(&book_id, &chapter_id, &epoch, start, end)
    }).await
}
#[tauri::command]
async fn diagnose_writing_predecessors(
    state: State<'_, Store>, book_id: String, chapter_id: String, pack_id: String, epoch: String,
) -> crate::WorkflowResult<crate::WritingPredecessorDiagnosis> {
    review_source_read(Arc::clone(state.inner()), move |s| {
        s.diagnose_writing_predecessors(&book_id, &chapter_id, &pack_id, &epoch)
    }).await
}
#[tauri::command]
fn list_foreshadow_occurrences_trusted(state: State<'_, Store>, book_id: String, material_id: String, epoch: String) -> crate::WorkflowResult<Vec<crate::ForeshadowOccurrenceView>> {
    with_foreshadow(state, |s| s.list_foreshadow_occurrences(&book_id, &material_id, &epoch))
}
#[tauri::command]
fn get_foreshadow_occurrence(state: State<'_, Store>, book_id: String, material_id: String, id: String, epoch: String) -> crate::WorkflowResult<crate::ForeshadowOccurrenceView> {
    with_foreshadow(state, |s| s.get_foreshadow_occurrence(&book_id, &material_id, &id, &epoch))
}
#[tauri::command]
fn lookup_foreshadow_request(state: State<'_, Store>, book_id: String, material_id: String, key: String, epoch: String) -> crate::WorkflowResult<Option<crate::ForeshadowOccurrenceView>> {
    with_foreshadow(state, |s| s.lookup_foreshadow_request(&book_id, &material_id, &key, &epoch))
}
#[tauri::command]
fn create_foreshadow_occurrence(state: State<'_, Store>, input: crate::CreateForeshadowOccurrence) -> crate::WorkflowResult<crate::ForeshadowOccurrenceView> {
    with_foreshadow(state, |s| s.create_foreshadow_occurrence(input))
}
#[tauri::command]
fn set_foreshadow_occurrence_deleted(state: State<'_, Store>, book_id: String, material_id: String, id: String, expected_revision: i64, epoch: String, deleted: bool) -> crate::WorkflowResult<crate::ForeshadowOccurrenceView> {
    with_foreshadow(state, |s| s.set_foreshadow_occurrence_deleted(&book_id, &material_id, &id, expected_revision, &epoch, deleted))
}
#[tauri::command]
fn get_book_foreshadow_material(state: State<'_, Store>, book_id: String, id: String, epoch: String) -> crate::WorkflowResult<crate::Material> {
    with_foreshadow(state, |s| s.get_book_foreshadow_material(&book_id, &id, &epoch))
}
#[tauri::command]
fn create_book_foreshadow_material(state: State<'_, Store>, input: crate::CreateMaterialInput, epoch: String) -> crate::WorkflowResult<crate::Material> {
    with_foreshadow(state, |s| s.create_book_foreshadow_material(input, &epoch))
}
#[tauri::command]
fn save_book_foreshadow_material(state: State<'_, Store>, book_id: String, input: crate::SaveMaterialInput, epoch: String) -> crate::WorkflowResult<crate::Material> {
    with_foreshadow(state, |s| s.save_book_foreshadow_material(&book_id, input, &epoch))
}
#[tauri::command]
fn get_writing_epoch(state: State<'_, Store>) -> Result<String, String> {
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .epoch()
        .map_err(|e| e.message)
}
#[tauri::command]
fn get_writing_state(state:State<'_,Store>,book_id:String,chapter_id:String)->Result<crate::WritingState,String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.get_writing_state(&book_id,&chapter_id).map_err(|e|e.message)}
#[tauri::command]
fn configure_writing_policy(state:State<'_,Store>,book_id:String,chapter_id:String,expected:Option<i64>,mode:String,goal:String,ids:Vec<String>,epoch:String)->Result<crate::WritingPolicy,String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.configure_writing_policy(&book_id,&chapter_id,expected,&mode,&goal,ids,&epoch).map_err(|e|e.message)}
#[tauri::command]
fn get_writing_context_directory(
    state: State<'_, Store>,
    book_id: String,
    chapter_id: String,
) -> Result<crate::WritingContextDirectory, String> {
    state.lock().map_err(|_| "书库暂不可用".to_string())?
        .get_writing_context_directory(&book_id, &chapter_id)
        .map_err(|e| e.message)
}
#[tauri::command]
fn configure_writing_policy_with_context(
    state: State<'_, Store>,
    book_id: String,
    chapter_id: String,
    expected: Option<i64>,
    mode: String,
    goal: String,
    ids: Vec<String>,
    epoch: String,
    context_chapter_ids: Vec<String>,
) -> Result<crate::WritingPolicy, String> {
    state.lock().map_err(|_| "书库暂不可用".to_string())?
        .configure_writing_policy_with_context(
            &book_id, &chapter_id, expected, &mode, &goal, ids, context_chapter_ids, &epoch,
        )
        .map_err(|e| e.message)
}
#[tauri::command]
fn prepare_writing_pack(state:State<'_,Store>,book_id:String,chapter_id:String,policy_revision:i64,chapter_revision:i64,epoch:String)->Result<crate::WritingPackManifest,String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.prepare_writing_pack(&book_id,&chapter_id,policy_revision,chapter_revision,&epoch).map_err(|e|e.message)}
#[tauri::command]
fn get_writing_pack(state:State<'_,Store>,book_id:String,id:String)->Result<crate::WritingPack,String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.get_writing_pack(&book_id,&id).map_err(|e|e.message)}
#[tauri::command]
fn get_constrained_proposal(state:State<'_,Store>,book_id:String,id:String)->Result<crate::ConstrainedProposal,String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.get_constrained_proposal(&book_id,&id).map_err(|e|e.message)}
#[tauri::command]
fn decide_constrained_proposal(state:State<'_,Store>,book_id:String,id:String,approve:bool,policy_revision:i64,chapter_revision:i64,epoch:String,conflicts_ack:bool)->Result<crate::ConstrainedProposal,String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.decide_constrained_proposal(&book_id,&id,approve,policy_revision,chapter_revision,&epoch,conflicts_ack).map_err(|e|e.message)}
#[tauri::command]
fn save_constrained_review(
    state: State<'_, Store>,
    book_id: String,
    id: String,
    expected_revision: Option<i64>,
    epoch: String,
    items: Vec<crate::AuthorReviewItem>,
) -> Result<crate::AuthorReview, String> {
    state.lock().map_err(|_| "书库暂不可用".to_string())?
        .save_constrained_review(&book_id, &id, expected_revision, &epoch, items)
        .map_err(|e| e.message)
}
#[tauri::command]
fn accept_reviewed_constrained_proposal(
    state: State<'_, Store>,
    book_id: String,
    id: String,
    policy_revision: i64,
    chapter_revision: i64,
    epoch: String,
    expected_review_revision: i64,
    conflicts_ack: bool,
) -> Result<crate::ConstrainedProposal, String> {
    state.lock().map_err(|_| "书库暂不可用".to_string())?
        .accept_reviewed_constrained_proposal(&book_id, &id, policy_revision, chapter_revision,
            &epoch, expected_review_revision, conflicts_ack)
        .map_err(|e| e.message)
}
#[tauri::command]
fn load_library(state: State<Store>) -> Result<LibraryState, String> {
    state.lock().map_err(|e| e.to_string())?.load()
}
#[tauri::command]
fn apply_command(state: State<Store>, command: Command) -> Result<LibraryState, String> {
    state.lock().map_err(|e| e.to_string())?.apply(command)
}
#[tauri::command]
fn search_book(
    state: State<Store>,
    book_id: String,
    query: String,
    limit: u32,
) -> Result<crate::SearchResults, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .search_book(&book_id, &query, limit)
        .map_err(|e| e.message)
}
#[tauri::command]
fn list_material_proposals(
    state: State<Store>,
    book_id: String,
) -> Result<Vec<crate::MaterialProposal>, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .list_material_proposals(&book_id)
        .map_err(|e| e.message)
}
#[tauri::command]
fn decide_material_proposal(
    state: State<Store>,
    id: String,
    approve: bool,
    expected_revision: i64,
) -> Result<crate::Material, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .decide_material_proposal(&id, approve, expected_revision)
        .map_err(|e| e.message)
}
// Task runner shares the only writer Service. No task commands are agent tools.
#[tauri::command]
fn start_context_task(
    state: State<Store>,
    runner: State<crate::task_runner::TaskRunner>,
    book_id: String,
    request_key: String,
) -> Result<crate::Task, String> {
    runner.check_health()?;
    let result = state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .start_context_task(&book_id, &request_key)
        .map_err(|e| e.message)?;
    runner.notify();
    Ok(result)
}
#[tauri::command]
fn list_context_tasks(
    state: State<Store>,
    runner: State<crate::task_runner::TaskRunner>,
    book_id: String,
) -> Result<Vec<crate::Task>, String> {
    runner.check_health()?;
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .list_context_tasks(&book_id)
        .map_err(|e| e.message)
}
#[tauri::command]
fn get_context_task(
    state: State<Store>,
    runner: State<crate::task_runner::TaskRunner>,
    book_id: String,
    id: String,
) -> Result<crate::TaskDetail, String> {
    runner.check_health()?;
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .get_context_task(&book_id, &id)
        .map_err(|e| e.message)
}
#[tauri::command]
fn cancel_context_task(
    state: State<Store>,
    runner: State<crate::task_runner::TaskRunner>,
    book_id: String,
    id: String,
) -> Result<crate::Task, String> {
    runner.check_health()?;
    let result = state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .cancel_context_task(&book_id, &id)
        .map_err(|e| e.message)?;
    runner.notify();
    Ok(result)
}
#[tauri::command]
fn resume_context_task(
    state: State<Store>,
    runner: State<crate::task_runner::TaskRunner>,
    book_id: String,
    id: String,
) -> Result<crate::Task, String> {
    runner.check_health()?;
    let result = state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .resume_context_task(&book_id, &id)
        .map_err(|e| e.message)?;
    runner.notify();
    Ok(result)
}
#[tauri::command]
fn retry_context_task(
    state: State<Store>,
    runner: State<crate::task_runner::TaskRunner>,
    book_id: String,
    id: String,
    request_key: String,
) -> Result<crate::Task, String> {
    runner.check_health()?;
    let result = state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .retry_context_task(&book_id, &id, &request_key)
        .map_err(|e| e.message)?;
    runner.notify();
    Ok(result)
}
// Material mutations are trusted desktop-only commands, never agent tools.
#[tauri::command]
fn list_materials(
    state: State<Store>,
    book_id: String,
    include_deleted: bool,
) -> Result<Vec<crate::MaterialSummary>, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .list_materials(&book_id, include_deleted)
        .map_err(|e| e.message)
}
#[tauri::command]
fn list_material_content_versions(state: State<'_,Store>,book_id:String,id:String,before:Option<i64>,limit:u32)->Result<crate::MaterialVersionPage,String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.list_material_content_versions(&book_id,&id,before,limit).map_err(|e|e.message)}
#[tauri::command]
fn get_material_content_version(state: State<'_,Store>,book_id:String,id:String,version_id:String)->Result<crate::MaterialContentVersion,String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.get_material_content_version(&book_id,&id,&version_id).map_err(|e|e.message)}
#[tauri::command]
fn restore_material_content(state: State<'_,Store>,book_id:String,id:String,version_id:String,revision:i64,epoch:String)->Result<crate::Material,String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.restore_material_content(&book_id,&id,&version_id,revision,&epoch).map_err(|e|e.message)}
#[tauri::command]
fn acknowledge_material_content(state: State<'_,Store>,book_id:String,id:String,revision:i64,epoch:String)->Result<crate::Material,String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.acknowledge_material_content(&book_id,&id,revision,&epoch).map_err(|e|e.message)}
#[tauri::command]
fn get_material(
    state: State<Store>,
    id: String,
    include_deleted: bool,
) -> Result<crate::Material, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .get_material(&id, include_deleted)
        .map_err(|e| e.message)
}
#[tauri::command]
fn create_material(
    state: State<Store>,
    input: crate::CreateMaterialInput,
) -> Result<crate::Material, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .create_material(input)
        .map_err(|e| e.message)
}
#[tauri::command]
fn save_material(
    state: State<Store>,
    input: crate::SaveMaterialInput,
) -> Result<crate::Material, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .save_material(input)
        .map_err(|e| e.message)
}
#[tauri::command]
fn trash_material(
    state: State<Store>,
    id: String,
    expected_revision: i64,
) -> Result<crate::Material, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .trash_material(&id, expected_revision)
        .map_err(|e| e.message)
}
#[tauri::command]
fn restore_material(
    state: State<Store>,
    id: String,
    expected_revision: i64,
) -> Result<crate::Material, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .restore_material(&id, expected_revision)
        .map_err(|e| e.message)
}
// These commands are registered only on the trusted desktop webview, never IPC.
#[tauri::command]
fn list_proposals(state: State<Store>, book_id: String) -> Result<Vec<crate::Proposal>, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .list_proposals(&book_id)
        .map_err(|e| e.message)
}
#[tauri::command]
fn decide_proposal(
    state: State<Store>,
    id: String,
    approve: bool,
    expected_revision: i64,
) -> Result<LibraryState, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .decide_proposal(&id, approve, expected_revision)
        .map_err(|e| e.message)
}
// Trusted history browser: scope and epoch are checked again in Service.
#[tauri::command]
fn list_chapter_versions(
    state: State<Store>,
    book_id: String,
    chapter_id: String,
    cursor: Option<String>,
    limit: u32,
    marked_only: bool,
) -> Result<crate::VersionPage, String> {
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .list_chapter_versions(&book_id, &chapter_id, cursor.as_deref(), limit, marked_only)
        .map_err(|e| e.message)
}
#[tauri::command]
fn get_chapter_version(
    state: State<Store>,
    book_id: String,
    chapter_id: String,
    version_id: String,
) -> Result<crate::VersionDetail, String> {
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .get_chapter_version(&book_id, &chapter_id, &version_id)
        .map_err(|e| e.message)
}
#[tauri::command]
fn set_chapter_version_label(
    state: State<Store>,
    book_id: String,
    chapter_id: String,
    version_id: String,
    label: Option<String>,
    expected_label_revision: i64,
    expected_epoch: String,
) -> Result<crate::VersionSummary, String> {
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .set_chapter_version_label(
            &book_id,
            &chapter_id,
            &version_id,
            label,
            expected_label_revision,
            &expected_epoch,
        )
        .map_err(|e| e.message)
}
#[tauri::command]
fn restore_chapter_version_scoped(
    state: State<Store>,
    book_id: String,
    chapter_id: String,
    version_id: String,
    expected_revision: i64,
    expected_epoch: String,
) -> Result<LibraryState, String> {
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .restore_chapter_version_scoped(
            &book_id,
            &chapter_id,
            &version_id,
            expected_revision,
            &expected_epoch,
        )
        .map_err(|e| e.message)
}
#[tauri::command]
async fn prepare_chapter_split(state: State<'_, Store>, book_id:String, chapter_id:String, expected_revision:i64, utf16_offset:u32, new_title:String)->Result<crate::SplitPreview,String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.prepare_chapter_split(&book_id,&chapter_id,expected_revision,utf16_offset,&new_title).map_err(|e|e.message)}
#[tauri::command]
async fn get_split_detail(state:State<'_,Store>,id:String,side:String)->Result<String,String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.get_split_detail(&id,&side).map_err(|e|e.message)}
#[tauri::command]
async fn commit_chapter_split(state:State<'_,Store>,id:String)->Result<crate::SplitOutcome,String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.commit_chapter_split(&id).map_err(|e|e.message)}
#[tauri::command]
async fn discard_chapter_split(state:State<'_,Store>,id:String)->Result<(),String>{state.lock().map_err(|_|"书库暂不可用".to_string())?.discard_chapter_split(&id);Ok(())}
#[tauri::command]
async fn prepare_txt_export(state: State<'_, Store>, book_id: String, options: crate::ExportOptions) -> Result<crate::ExportPreview,String> {
 state.lock().map_err(|_| "书库暂不可用".to_string())?.prepare_txt_export(&book_id,options).map_err(|e|e.message)
}
#[tauri::command]
async fn discard_txt_export(state: State<'_, Store>, id:String) -> Result<(),String> {
 state.lock().map_err(|_| "书库暂不可用".to_string())?.discard_txt_export(&id); Ok(())
}
#[tauri::command]
async fn save_txt_export(app:tauri::AppHandle,state:State<'_,Store>,id:String)->Result<Option<String>,String>{
 let Some(path)=app.dialog().file().add_filter("TXT 文本", &["txt"]).set_file_name("小说.txt").blocking_save_file() else {return Ok(None);};
 let path=path.into_path().map_err(|_|"无法使用此保存位置".to_string())?;
 let bytes=state.lock().map_err(|_|"书库暂不可用".to_string())?.take_txt_export(&id).map_err(|e|e.message)?;
 write_new_file(&path,&bytes)?;
 Ok(Some(path.display().to_string()))
}
#[tauri::command]
async fn export_book(
    app: tauri::AppHandle,
    state: State<'_, Store>,
    id: String,
) -> Result<Option<String>, String> {
    let text = state.lock().map_err(|e| e.to_string())?.export_book(&id)?;
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("TXT 文本", &["txt"])
        .set_file_name("小说.txt")
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    write_new_file(&path, text.as_bytes())?;
    Ok(Some(path.display().to_string()))
}
#[tauri::command]
async fn backup_library(
    app: tauri::AppHandle,
    state: State<'_, Store>,
) -> Result<Option<String>, String> {
    let text = state.lock().map_err(|e| e.to_string())?.backup()?;
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("墨卷备份", &["json"])
        .set_file_name("墨卷备份.json")
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    write_new_file(&path, text.as_bytes())?;
    Ok(Some(path.display().to_string()))
}
#[tauri::command]
async fn start_backup_export(
    app: tauri::AppHandle,
    jobs: State<'_, crate::backup_jobs::BackupJobManager>,
) -> Result<Option<crate::backup_jobs::BackupJob>, String> {
    if jobs.has_pending() {
        return Err("已有备份或恢复任务，请先完成或取消".into());
    }
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("墨卷SQLite备份", &["sqlite"])
        .set_file_name("墨卷书库.sqlite")
        .blocking_save_file()
    else {
        return Ok(None);
    };
    jobs.start_export(path.into_path().map_err(|e| e.to_string())?)
        .map(Some)
}
#[tauri::command]
async fn start_backup_prepare(
    app: tauri::AppHandle,
    jobs: State<'_, crate::backup_jobs::BackupJobManager>,
) -> Result<Option<crate::backup_jobs::BackupJob>, String> {
    if jobs.has_pending() {
        return Err("已有备份或恢复任务，请先完成或取消".into());
    }
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("墨卷备份", &["sqlite", "db", "json"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    jobs.start_prepare_restore(path.into_path().map_err(|e| e.to_string())?)
        .map(Some)
}
#[tauri::command]
fn current_backup_job(
    jobs: State<crate::backup_jobs::BackupJobManager>,
) -> Result<Option<crate::backup_jobs::BackupJob>, String> {
    jobs.current()
}
#[tauri::command]
fn backup_job_status(
    jobs: State<crate::backup_jobs::BackupJobManager>,
    id: String,
) -> Result<crate::backup_jobs::BackupJob, String> {
    jobs.status(&id)
}
#[tauri::command]
fn cancel_backup_job(
    jobs: State<crate::backup_jobs::BackupJobManager>,
    id: String,
) -> Result<crate::backup_jobs::BackupJob, String> {
    jobs.cancel(&id)
}
#[tauri::command]
fn confirm_backup_restore(
    jobs: State<crate::backup_jobs::BackupJobManager>,
    id: String,
    token: String,
    writing_ack: Option<bool>,
) -> Result<crate::backup_jobs::BackupJob, String> {
    jobs.confirm_restore_with_writing_ack(&id, &token,writing_ack.unwrap_or(false))
}
#[tauri::command]
async fn preview_book_replace(
    state: State<'_, Store>,
    book_id: String,
    query: String,
    replacement: String,
) -> Result<crate::ReplacePreview, String> {
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .preview_book_replace(&book_id, &query, &replacement)
        .map_err(|e| e.message)
}
#[tauri::command]
async fn get_replace_detail(
    state: State<'_, Store>,
    id: String,
    chapter_id: String,
) -> Result<crate::ReplaceDetail, String> {
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .get_replace_detail(&id, &chapter_id)
        .map_err(|e| e.message)
}
#[tauri::command]
async fn commit_book_replace(
    state: State<'_, Store>,
    id: String,
) -> Result<crate::ReplaceOutcome, String> {
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .commit_book_replace(&id)
        .map_err(|e| e.message)
}
#[tauri::command]
async fn discard_book_replace(state: State<'_, Store>, id: String) -> Result<(), String> {
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .discard_book_replace(&id)
        .map_err(|e| e.message)
}
#[tauri::command]
async fn choose_txt_import(
    app: tauri::AppHandle,
    state: State<'_, Store>,
    mode: String,
) -> Result<Option<crate::ImportPreview>, String> {
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("UTF-8 TXT小说", &["txt"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|_| "文件路径不可用".to_string())?;
    let bytes = crate::files::read_txt_file(&path)?;
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "导入作品.txt".into());
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .prepare_txt_import_mode(&bytes, &name, &mode)
        .map(Some)
        .map_err(|e| e.message)
}
#[tauri::command]
async fn revise_txt_import(
    state: State<'_, Store>,
    id: String,
    title: String,
    author: String,
    mode: String,
) -> Result<crate::ImportPreview, String> {
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .revise_txt_import(&id, &title, &author, &mode)
        .map_err(|e| e.message)
}
#[tauri::command]
async fn get_txt_import_chapter(
    state: State<'_, Store>,
    id: String,
    index: u32,
) -> Result<crate::ImportDetail, String> {
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .get_txt_import_chapter(&id, index)
        .map_err(|e| e.message)
}
#[tauri::command]
async fn commit_txt_import(
    state: State<'_, Store>,
    id: String,
) -> Result<crate::ImportOutcome, String> {
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .commit_txt_import(&id)
        .map_err(|e| e.message)
}
#[tauri::command]
async fn discard_txt_import(state: State<'_, Store>, id: String) -> Result<(), String> {
    state
        .lock()
        .map_err(|_| "书库暂不可用".to_string())?
        .discard_txt_import(&id)
        .map_err(|e| e.message)
}

async fn project_import_io<T: Send + 'static>(
    store: Store,
    action: impl FnOnce(&mut Service) -> crate::WorkflowResult<T> + Send + 'static,
) -> crate::WorkflowResult<T> {
    fn unavailable() -> crate::WorkflowError {
        crate::WorkflowError {
            code: "unavailable".into(),
            message: "书库暂不可用，请重新核对导入结果".into(),
        }
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut service = store.lock().map_err(|_| unavailable())?;
        action(&mut service)
    })
    .await
    .map_err(|_| unavailable())?
}

#[tauri::command]
async fn choose_project_import(
    app: tauri::AppHandle,
    state: State<'_, Store>,
) -> crate::WorkflowResult<Option<crate::ProjectImportSelection>> {
    let Some(folder) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let folder = folder.into_path().map_err(|_| crate::WorkflowError {
        code: "file_error".into(),
        message: "目录路径不可用".into(),
    })?;
    project_import_io(Arc::clone(state.inner()), move |service| {
        service.choose_project_import(&folder).map(Some)
    }).await
}

#[tauri::command]
async fn prepare_project_import(
    state: State<'_, Store>,
    id: String,
    options: crate::ProjectImportOptions,
) -> crate::WorkflowResult<crate::ProjectImportPreview> {
    project_import_io(Arc::clone(state.inner()), move |service| {
        service.prepare_project_import(&id, options)
    }).await
}

#[tauri::command]
async fn get_project_import_detail(
    state: State<'_, Store>,
    id: String,
    kind: String,
    index: u32,
) -> crate::WorkflowResult<crate::ImportDetail> {
    project_import_io(Arc::clone(state.inner()), move |service| {
        service.get_project_import_detail(&id, &kind, index)
    }).await
}

#[tauri::command]
async fn commit_project_import(
    state: State<'_, Store>,
    id: String,
    links: Vec<crate::ProjectLink>,
    excluded: Option<Vec<usize>>,
) -> crate::WorkflowResult<crate::ProjectImportOutcome> {
    project_import_io(Arc::clone(state.inner()), move |service| {
        service.commit_reviewed_project_import(&id, links, excluded.unwrap_or_default())
    }).await
}

#[tauri::command]
async fn discard_project_import(state: State<'_, Store>, id: String) -> crate::WorkflowResult<()> {
    project_import_io(Arc::clone(state.inner()), move |service| {
        service.discard_project_import(&id)
    }).await
}

#[tauri::command]
async fn preview_material_organization(state: State<'_, Store>, book_id: String) -> crate::WorkflowResult<crate::project_import::OrganizationPreview> {
    project_import_io(Arc::clone(state.inner()), move |service| service.preview_material_organization(&book_id)).await
}

#[tauri::command]
async fn apply_material_organization(state: State<'_, Store>, id: String, accepted: Vec<usize>) -> crate::WorkflowResult<usize> {
    project_import_io(Arc::clone(state.inner()), move |service| service.apply_material_organization(&id, accepted)).await
}

struct DesktopAgents {
    broker: Arc<crate::agent::AgentBroker>,
    endpoint: Option<String>,
    _server: Option<crate::ipc::Server>,
}

#[derive(serde::Serialize)]
struct DesktopAgentStatus {
    endpoint: Option<String>,
    sessions: Vec<crate::agent::AgentSession>,
}

#[tauri::command]
fn desktop_agent_status(state: State<'_, DesktopAgents>) -> DesktopAgentStatus {
    DesktopAgentStatus {
        endpoint: state.endpoint.clone(),
        sessions: state.broker.list_sessions(),
    }
}

#[tauri::command]
fn grant_desktop_agent(
    state: State<'_, DesktopAgents>, session_id: String, book_id: String,
) -> crate::WorkflowResult<()> {
    state.broker.grant_session(&session_id, &book_id)
}

#[tauri::command]
fn revoke_desktop_agent(state: State<'_, DesktopAgents>, session_id: String) -> crate::WorkflowResult<()> {
    state.broker.revoke_session(&session_id)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let service = Service::open(dir.join("library.db")).map_err(std::io::Error::other)?;
            let store = Arc::new(Mutex::new(service));
            let broker = Arc::new(crate::agent::AgentBroker::new(Arc::clone(&store)));
            let mut agents = DesktopAgents { broker, endpoint: None, _server: None };
            if let Ok(path) = crate::ipc::desktop_endpoint(&app.config().identifier) {
                if let Ok(server) = crate::ipc::start_server(Arc::clone(&agents.broker), &path) {
                    agents.endpoint = path.to_str().map(str::to_owned);
                    agents._server = Some(server);
                }
            }
            app.manage(agents);
            let runner = crate::task_runner::TaskRunner::start(Arc::clone(&store));
            let backups =
                crate::backup_jobs::BackupJobManager::new(Arc::clone(&store), dir.join("recovery"));
            app.manage(backups);
            app.manage(store);
            app.manage(runner);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_draft_recovery_session,
            read_recovery_draft,
            checkpoint_recovery_draft,
            dismiss_recovery_draft,
            get_writing_epoch,get_writing_state,get_writing_context_directory,configure_writing_policy,configure_writing_policy_with_context,prepare_writing_pack,get_writing_pack,get_constrained_proposal,decide_constrained_proposal,
            save_constrained_review,accept_reviewed_constrained_proposal,
            choose_txt_import,
            revise_txt_import,
            get_txt_import_chapter,
            commit_txt_import,
            discard_txt_import,
            choose_project_import,
            prepare_project_import,
            get_project_import_detail,
            commit_project_import,
            discard_project_import,
            preview_material_organization,
            apply_material_organization,
            desktop_agent_status,
            grant_desktop_agent,
            revoke_desktop_agent,
            preview_book_replace,
            get_replace_detail,
            commit_book_replace,
            discard_book_replace,
            start_context_task,
            list_context_tasks,
            get_context_task,
            cancel_context_task,
            resume_context_task,
            retry_context_task,
            search_book,
            list_material_proposals,
            decide_material_proposal,
            list_materials,
            get_material,
            list_material_content_versions,
            get_material_content_version,
            restore_material_content,
            acknowledge_material_content,
            create_material,
            save_material,
            trash_material,
            restore_material,
            load_library,
            list_foreshadow_proposals,
            get_foreshadow_proposal_trusted,
            decide_foreshadow_proposal,
            list_book_foreshadow,
            get_foreshadow_directory,
            get_foreshadow_chapter,
            get_foreshadow_source_epoch,
            get_foreshadow_source_window,
            diagnose_writing_predecessors,
            list_foreshadow_occurrences_trusted,
            get_foreshadow_occurrence,
            lookup_foreshadow_request,
            create_foreshadow_occurrence,
            set_foreshadow_occurrence_deleted,
            get_book_foreshadow_material,
            create_book_foreshadow_material,
            save_book_foreshadow_material,
            apply_command,
            export_book,
            prepare_txt_export,
            prepare_chapter_split,
            get_split_detail,
            commit_chapter_split,
            discard_chapter_split,
            save_txt_export,
            discard_txt_export,
            backup_library,
            start_backup_export,
            start_backup_prepare,
            current_backup_job,
            backup_job_status,
            cancel_backup_job,
            confirm_backup_restore,
            list_proposals,
            decide_proposal,
            list_chapter_versions,
            get_chapter_version,
            set_chapter_version_label,
            restore_chapter_version_scoped,
        ])
        .build(tauri::generate_context!())
        .expect("无法启动墨卷")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                // Never hold the Service lock while joining the worker.
                app.state::<crate::backup_jobs::BackupJobManager>()
                    .shutdown();
                app.state::<crate::task_runner::TaskRunner>().shutdown();
            }
        });
}
