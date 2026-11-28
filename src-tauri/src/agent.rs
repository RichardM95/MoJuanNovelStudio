//! Connection-scoped, deny-by-default agent access. Trusted GUI/host owns this broker.
use crate::{ProposalInput, Service, WorkflowError, WorkflowResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
const MAX_SESSIONS: usize = 16;
#[derive(Clone, Serialize, Debug)]
pub struct AgentSession {
    pub id: String,
    pub client_name: String,
    pub status: String,
    pub book_id: Option<String>,
    pub created_at: u64,
    pub browser_worker: bool,
    pub fanqie_send: bool,
}
struct Session {
    view: AgentSession,
    epoch: String,
    window: Instant,
    count: u32,
    created: Instant,
}
pub struct AgentBroker {
    store: Arc<Mutex<Service>>,
    sessions: Mutex<HashMap<String, Session>>,
}
pub struct AgentConnection {
    broker: Arc<AgentBroker>,
    id: String,
}
fn err(code: &str, message: &str) -> WorkflowError {
    WorkflowError {
        code: code.into(),
        message: message.into(),
    }
}
fn encode<T: Serialize>(v: T) -> WorkflowResult<Value> {
    serde_json::to_value(v).map_err(|_| err("internal_error", "无法编码响应"))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    method: String,
    #[serde(default = "empty")]
    params: Value,
}
fn empty() -> Value {
    json!({})
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Access {
    client_name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChapterArg {
    chapter_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalArg {
    proposal_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArg {
    chapter_id: String,
    base_revision: i64,
    new_body: String,
    reason: String,
    idempotency_key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterialListArg {
    kind: Option<String>,
    after_id: Option<String>,
    limit: Option<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterialArg {
    material_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ForeshadowPageArg {
    material_id: String,
    after_id: Option<String>,
    limit: Option<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArg {
    query: String,
    limit: Option<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterialEditArg {
    material_id: String,
    base_revision: i64,
    new_title: String,
    new_fields: BTreeMap<String, String>,
    new_source_revision: Option<i64>,
    reason: String,
    idempotency_key: String,
}
fn parse<T: serde::de::DeserializeOwned>(v: Value) -> WorkflowResult<T> {
    serde_json::from_value(v).map_err(|_| err("invalid_input", "参数格式无效或含不允许字段"))
}
impl AgentBroker {
    pub fn new(store: Arc<Mutex<Service>>) -> Self {
        Self {
            store,
            sessions: Mutex::new(HashMap::new()),
        }
    }
    pub fn connect(self: &Arc<Self>) -> WorkflowResult<AgentConnection> {
        let store = self
            .store
            .lock()
            .map_err(|_| err("unavailable", "编辑服务不可用"))?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| err("unavailable", "会话服务不可用"))?;
        if sessions.len() >= MAX_SESSIONS {
            return Err(err("limit_exceeded", "连接数已满"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = Instant::now();
        sessions.insert(
            id.clone(),
            Session {
                view: AgentSession {
                    id: id.clone(),
                    client_name: String::new(),
                    browser_worker: false,
                    fanqie_send: false,
                    status: "connected".into(),
                    book_id: None,
                    created_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                },
                epoch: store.epoch()?,
                window: now,
                count: 0,
                created: now,
            },
        );
        Ok(AgentConnection {
            broker: self.clone(),
            id,
        })
    }
    pub fn list_sessions(&self) -> Vec<AgentSession> {
        // Same lock order everywhere: Service, then session registry.
        let Ok(store) = self.store.lock() else {
            return vec![];
        };
        let Ok(mut sessions) = self.sessions.lock() else {
            return vec![];
        };
        let epoch = store.epoch().ok();
        for s in sessions.values_mut() {
            if epoch.as_deref() != Some(s.epoch.as_str()) || s.created.elapsed().as_secs() > 3600 {
                s.view.status = "expired".into();
                s.view.book_id = None;
            }
        }
        let mut out: Vec<_> = sessions.values().map(|s| s.view.clone()).collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }
    pub fn grant_session(&self, id: &str, book_id: &str) -> WorkflowResult<()> {
        self.grant_access(id, book_id, false)
    }
    pub fn grant_access(&self, id: &str, book_id: &str, fanqie_send: bool) -> WorkflowResult<()> {
        let store = self
            .store
            .lock()
            .map_err(|_| err("unavailable", "编辑服务不可用"))?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| err("unavailable", "会话服务不可用"))?;
        let epoch = store.epoch()?;
        let worker = sessions.get(id).is_some_and(|s| s.view.browser_worker);
        if worker
            && sessions.values().any(|s| {
                s.view.browser_worker
                    && s.view.status == "granted"
                    && s.created.elapsed().as_secs() <= 3600
                    && s.epoch == epoch
                    && s.view.book_id.as_deref() == Some(book_id)
            })
        {
            return Err(err("invalid_session", "请先撤销此作品的旧浏览器连接"));
        }
        let s = sessions
            .get_mut(id)
            .ok_or_else(|| err("not_found", "连接已断开"))?;
        if s.view.status != "pending"
            || s.epoch != store.epoch()?
            || s.created.elapsed().as_secs() > 3600
        {
            return Err(err("invalid_session", "会话不可授权，请重新连接"));
        }
        store.get_outline(book_id)?;
        s.view.fanqie_send = fanqie_send && !worker;
        s.view.book_id = Some(book_id.into());
        s.view.status = "granted".into();
        Ok(())
    }
    pub fn revoke_session(&self, id: &str) -> WorkflowResult<()> {
        let _store = self
            .store
            .lock()
            .map_err(|_| err("unavailable", "编辑服务不可用"))?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| err("unavailable", "会话服务不可用"))?;
        let s = sessions
            .get_mut(id)
            .ok_or_else(|| err("not_found", "连接已断开"))?;
        s.view.status = "revoked".into();
        s.view.book_id = None;
        Ok(())
    }
    fn dispatch(&self, id: &str, value: Value) -> WorkflowResult<Value> {
        let req: Request = parse(value)?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| err("unavailable", "编辑服务不可用"))?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| err("unavailable", "会话服务不可用"))?;
        let s = sessions
            .get_mut(id)
            .ok_or_else(|| err("invalid_session", "连接已失效"))?;
        if s.window.elapsed().as_secs() >= 60 {
            s.window = Instant::now();
            s.count = 0;
        }
        s.count += 1;
        if s.count > 120 {
            return Err(err("rate_limited", "请求过于频繁"));
        }
        if s.epoch != store.epoch()? || s.created.elapsed().as_secs() > 3600 {
            s.view.status = "expired".into();
            s.view.book_id = None;
        }
        if ["expired", "revoked"].contains(&s.view.status.as_str()) {
            return Err(err("invalid_session", "授权已失效，请重新连接"));
        }
        if matches!(req.method.as_str(), "request_access" | "request_browser_access") {
            let p: Access = parse(req.params)?;
            if s.view.status != "connected" {
                return Err(err("invalid_session", "连接已申请授权"));
            }
            if p.client_name.trim().is_empty()
                || p.client_name.len() > 128
                || p.client_name.chars().any(char::is_control)
            {
                return Err(err("invalid_input", "客户端名称无效"));
            }
            s.view.browser_worker = req.method == "request_browser_access";
            s.view.client_name = p.client_name;
            s.view.status = "pending".into();
            return encode(&s.view);
        }
        if req.method == "session_status" {
            if req.params != empty() {
                return Err(err("invalid_input", "此操作不接受参数"));
            }
            return encode(&s.view);
        }
        if s.view.status != "granted" {
            return Err(err("access_pending", "请由可信操作员授权一本作品"));
        }
        let book = s
            .view
            .book_id
            .as_deref()
            .ok_or_else(|| err("access_denied", "未授权"))?;
        if s.view.browser_worker {
            let book = book.to_owned();
            return match req.method.as_str() {
                "browser_volumes" => {
                    let dir = store.fanqie_directory(&book)?;
                    let mut volumes = BTreeMap::new();
                    for c in dir.chapters {
                        volumes.insert(c.volume_id, c.volume_title);
                    }
                    Ok(json!({"book":dir.book_title,"volumes":volumes}))
                }
                "browser_bind" => store.bind_fanqie(&book, parse(req.params)?),
                "browser_claim" | "browser_validate" | "browser_body_chunk" => {
                    let epoch = store.epoch()?;
                    let authorized = sessions
                        .values()
                        .filter(|s| {
                            s.view.fanqie_send
                                && s.view.status == "granted"
                                && s.view.book_id.as_ref() == Some(&book)
                                && s.epoch == epoch
                                && s.created.elapsed().as_secs() <= 3600
                        })
                        .map(|s| s.view.id.clone())
                        .collect();
                    if req.method != "browser_claim" {
                        #[derive(Deserialize)]
                        #[serde(deny_unknown_fields)]
                        struct Check {
                            job_id: String,
                            claim: String,
                            cursor: Option<usize>,
                        }
                        let check: Check = parse(req.params)?;
                        if (req.method == "browser_body_chunk") != check.cursor.is_some() {
                            return Err(err("invalid_input", "正文分段参数无效"));
                        }
                        store.validate_fanqie(
                            &book,
                            &check.job_id,
                            &check.claim,
                            &authorized,
                            check.cursor,
                        )
                    } else {
                        store.claim_fanqie(&book, &authorized)
                    }
                }
                "browser_finish" => store.finish_fanqie(&book, parse(req.params)?),
                _ => Err(err("access_denied", "浏览器连接仅用于番茄草稿任务")),
            };
        }
        if matches!(
            req.method.as_str(),
            "create_fanqie_draft_job" | "get_fanqie_draft_job"
        ) {
            if !s.view.fanqie_send {
                return Err(err(
                    "access_denied",
                    "请重新连接，在墨卷选择允许番茄存稿的授权",
                ));
            }
            if req.method == "create_fanqie_draft_job" {
                return store.create_fanqie_job(book, id, parse(req.params)?);
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Status {
                job_id: String,
            }
            let p: Status = parse(req.params)?;
            let mut report = store.fanqie_status(book, &p.job_id)?;
            let epoch = s.epoch.clone();
            let book = book.to_owned();
            report["browser_connected"] =
                json!(sessions.values().any(|other| other.view.browser_worker
                    && other.view.status == "granted"
                    && other.epoch == epoch
                    && other.view.book_id.as_deref() == Some(book.as_str())
                    && other.created.elapsed().as_secs() <= 3600));
            return Ok(report);
        }
        let outline = store
            .get_outline(book)
            .map_err(|_| err("access_denied", "授权作品当前不可用"))?;
        let chapter_allowed = |chapter: &str| outline.chapters.iter().any(|c| c.id == chapter);
        match req.method.as_str() {
            "list_books" => {
                if req.params != empty() {
                    return Err(err("invalid_input", "此操作不接受参数"));
                }
                encode(vec![outline.book])
            }
            "get_outline" => {
                if req.params != empty() {
                    return Err(err("invalid_input", "此操作不接受参数"));
                }
                encode(outline)
            }
            "get_writing_policy" => {let p:ChapterArg=parse(req.params)?;if !chapter_allowed(&p.chapter_id){return Err(err("not_found","章节不可访问"));}let state=store.get_writing_state(book,&p.chapter_id)?;encode(serde_json::json!({"epoch":state.epoch,"policy":state.policy,"packs":state.packs}))}
            "get_writing_pack_manifest" => {let p:WritingPackArg=parse(req.params)?;encode(store.get_writing_pack_manifest(book,&p.pack_id)?)}
            "read_writing_pack_entry" => {let p:WritingChunkArg=parse(req.params)?;encode(store.read_writing_pack_entry(book,&p.pack_id,&p.entry_id,&p.field_key,p.cursor)?)}
            "propose_constrained_edit" => {let p:crate::ConstrainedEditInput=parse(req.params)?;encode(store.propose_constrained_edit(book,id,p)?.receipt())}
            "get_constrained_proposal" | "cancel_constrained_proposal" => {let p:ProposalArg=parse(req.params)?;let proposal=store.get_constrained_proposal(book,&p.proposal_id)?;if proposal.client_id!=id{return Err(err("not_found","提案不可访问"));}if req.method=="cancel_constrained_proposal"{encode(store.cancel_constrained_proposal(book,&p.proposal_id,id)?.receipt())}else{encode(proposal.receipt())}}
            "search_book" => {
                let p: SearchArg = parse(req.params)?;
                let limit = p.limit.unwrap_or(20);
                if p.query.trim().is_empty() || p.query.len() > 256 || !(1..=50).contains(&limit) {
                    return Err(err("invalid_input", "搜索参数无效"));
                }
                encode(store.search_book(book, &p.query, limit)?)
            }
            "retrieve_writing_context" => {
                let p: crate::retrieval::RetrievalRequest = parse(req.params)?;
                if !chapter_allowed(&p.chapter_id) {
                    return Err(err("not_found", "章节不可访问"));
                }
                encode(store.retrieve_writing_context(
                    book, &p.chapter_id, &p.query, p.limit.unwrap_or(12), p.include_references,
                )?)
            }
            "propose_material_edit" => {
                let p: MaterialEditArg = parse(req.params)?;
                let materials = store.list_materials(book, false)?;
                let material = materials
                    .iter()
                    .find(|m| m.id == p.material_id)
                    .ok_or_else(|| err("not_found", "资料不可访问"))?;
                if ["timeline", "foreshadow", "document"].contains(&material.kind.as_str()) {
                    return Err(err(
                        "unsupported_operation",
                        "此类资料由作者管理；伏笔出现建议须单独提交待作者审阅",
                    ));
                }
                encode(store.propose_material_edit(crate::MaterialProposalInput {
                    client_id: id.into(),
                    material_id: p.material_id,
                    base_revision: p.base_revision,
                    new_title: p.new_title,
                    new_fields: p.new_fields,
                    new_source_revision: p.new_source_revision,
                    reason: p.reason,
                    idempotency_key: p.idempotency_key,
                })?)
            }
            "get_material_proposal" | "cancel_material_proposal" => {
                let p: ProposalArg = parse(req.params)?;
                let proposal = store
                    .get_material_proposal(&p.proposal_id, id)
                    .map_err(|_| err("not_found", "提案不可访问"))?;
                if !store
                    .list_materials(book, false)?
                    .iter()
                    .any(|m| m.id == proposal.material_id)
                {
                    return Err(err("not_found", "提案不可访问"));
                }
                if req.method == "cancel_material_proposal" {
                    encode(store.cancel_material_proposal(&proposal.id, id)?)
                } else {
                    encode(proposal)
                }
            }
            "list_materials" => {
                let p: MaterialListArg = parse(req.params)?;
                let limit = p.limit.unwrap_or(50) as usize;
                if !(1..=100).contains(&limit)
                    || p.kind.as_deref().is_some_and(|k| {
                        ![
                            "setting",
                            "character",
                            "outline",
                            "summary",
                            "timeline",
                            "foreshadow",
                            "document",
                        ]
                        .contains(&k)
                    })
                    || p.after_id
                        .as_ref()
                        .is_some_and(|id| id.is_empty() || id.len() > 128)
                {
                    return Err(err("invalid_input", "资料筛选或分页参数无效"));
                }
                let mut items = store.list_materials(book, false)?;
                items.retain(|m| {
                    p.kind.as_deref().is_none_or(|kind| m.kind == kind)
                        && p.after_id
                            .as_deref()
                            .is_none_or(|after| m.id.as_str() > after)
                });
                items.sort_by(|a, b| a.id.cmp(&b.id));
                let more = items.len() > limit;
                items.truncate(limit);
                let next_cursor = if more {
                    items.last().map(|m| m.id.clone())
                } else {
                    None
                };
                encode(json!({"items":items,"next_cursor":next_cursor}))
            }
            "propose_foreshadow_occurrence" => {
                let p: crate::ForeshadowProposalInput = parse(req.params)?;
                encode(store.propose_foreshadow_occurrence(book, id, &s.epoch, p)?)
            }
            "get_foreshadow_proposal" => {
                let p: ProposalArg = parse(req.params)?;
                encode(store.get_own_foreshadow_proposal(book, id, &s.epoch, &p.proposal_id)?)
            }
            "cancel_foreshadow_proposal" => {
                let p: ProposalArg = parse(req.params)?;
                encode(store.cancel_own_foreshadow_proposal(book, id, &s.epoch, &p.proposal_id)?)
            }
            "list_foreshadow_occurrences" => {
                let p: ForeshadowPageArg = parse(req.params)?;
                encode(store.foreshadow_agent_page(book, &p.material_id, p.after_id.as_deref(), p.limit)?)
            }
            "get_material" => {
                let p: MaterialArg = parse(req.params)?;
                if p.material_id.is_empty() || p.material_id.len() > 128 {
                    return Err(err("invalid_input", "资料标识无效"));
                }
                // Check authorized metadata BEFORE loading any content, under the same Service lock.
                if !store
                    .list_materials(book, false)?
                    .iter()
                    .any(|m| m.id == p.material_id)
                {
                    return Err(err("not_found", "资料不可访问"));
                }
                encode(
                    store
                        .get_material(&p.material_id, false)
                        .map_err(|_| err("not_found", "资料不可访问"))?,
                )
            }
            "get_chapter" => {
                let p: ChapterArg = parse(req.params)?;
                if !chapter_allowed(&p.chapter_id) {
                    return Err(err("not_found", "章节不可访问"));
                }
                encode(store.get_chapter(&p.chapter_id)?)
            }
            "propose_edit" => {
                let p: EditArg = parse(req.params)?;
                if !chapter_allowed(&p.chapter_id) {
                    return Err(err("not_found", "章节不可访问"));
                }
                if p.new_body.len() > 1024 * 1024 {
                    return Err(err("limit_exceeded", "建议正文超过 1 MiB"));
                }
                encode(store.propose_edit(ProposalInput {
                    client_id: id.into(),
                    idempotency_key: p.idempotency_key,
                    chapter_id: p.chapter_id,
                    base_revision: p.base_revision,
                    new_body: p.new_body,
                    reason: p.reason,
                })?)
            }
            "get_proposal" | "cancel_proposal" => {
                let p: ProposalArg = parse(req.params)?;
                let proposal = store
                    .get_own_proposal(&p.proposal_id, id)
                    .map_err(|_| err("not_found", "提案不可访问"))?;
                if !chapter_allowed(&proposal.chapter_id) {
                    return Err(err("not_found", "提案不可访问"));
                }
                if req.method == "cancel_proposal" {
                    encode(store.cancel_own_proposal(&proposal.id, id)?)
                } else {
                    encode(proposal)
                }
            }
            _ => Err(err("method_not_found", "操作不受支持")),
        }
    }
}
impl AgentConnection {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn call(&self, value: Value) -> Value {
        match self.broker.dispatch(&self.id, value) {
            Ok(result) => json!({"result":result}),
            Err(error) => json!({"error":error}),
        }
    }
}
impl Drop for AgentConnection {
    fn drop(&mut self) {
        if let Ok(mut s) = self.broker.sessions.lock() {
            s.remove(&self.id);
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WritingPackArg{pack_id:String}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WritingChunkArg{pack_id:String,entry_id:String,field_key:String,cursor:usize}
