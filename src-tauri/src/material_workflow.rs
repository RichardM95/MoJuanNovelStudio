//! Immutable, review-only updates of existing materials. No creation, deletion, or undo API.
use crate::{Material, SaveMaterialInput, Service, WorkflowError, WorkflowResult};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialProposalInput {
    pub client_id: String,
    pub idempotency_key: String,
    pub material_id: String,
    pub base_revision: i64,
    pub new_title: String,
    pub new_fields: BTreeMap<String, String>,
    pub new_source_revision: Option<i64>,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialProposal {
    pub id: String,
    pub client_id: String,
    pub material_id: String,
    pub base_revision: i64,
    pub old_snapshot: Material,
    pub new_title: String,
    pub new_fields: BTreeMap<String, String>,
    pub new_source_revision: Option<i64>,
    pub reason: String,
    pub status: String,
    pub created_at: i64,
}
fn error(code: &str, msg: &str) -> WorkflowError {
    WorkflowError {
        code: code.into(),
        message: msg.into(),
    }
}
fn now() -> WorkflowResult<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|n| n.as_secs() as i64)
        .map_err(|_| error("clock_error", "系统时间无效"))
}
fn serial<T: Serialize>(v: &T) -> WorkflowResult<String> {
    serde_json::to_string(v).map_err(|_| error("invalid_input", "提案不能序列化"))
}
pub(crate) fn migrate(conn: &Connection) -> WorkflowResult<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS material_proposals(id TEXT PRIMARY KEY,client_id TEXT NOT NULL,material_id TEXT NOT NULL REFERENCES materials(id) ON DELETE CASCADE,base_revision INTEGER NOT NULL,status TEXT NOT NULL CHECK(status IN ('pending','accepted','rejected','cancelled')),record TEXT NOT NULL,payload TEXT NOT NULL,idempotency_key TEXT NOT NULL,created_at INTEGER NOT NULL,UNIQUE(client_id,idempotency_key));CREATE INDEX IF NOT EXISTS material_proposals_card ON material_proposals(material_id);CREATE TABLE IF NOT EXISTS material_proposal_audit(id TEXT PRIMARY KEY,proposal_id TEXT NOT NULL REFERENCES material_proposals(id) ON DELETE CASCADE,material_id TEXT NOT NULL,action TEXT NOT NULL,actor TEXT NOT NULL,revision INTEGER NOT NULL,created_at INTEGER NOT NULL);")?;
    Ok(())
}
pub(crate) fn clear_for_restore(tx: &Transaction) -> WorkflowResult<()> {
    tx.execute_batch("DELETE FROM material_proposal_audit;DELETE FROM material_proposals;")?;
    Ok(())
}
fn read(conn: &Connection, id: &str) -> WorkflowResult<MaterialProposal> {
    let row = conn
        .query_row(
            "SELECT record,status FROM material_proposals WHERE id=?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| error("not_found", "资料提案不存在或已失效"))?;
    let mut p: MaterialProposal =
        serde_json::from_str(&row.0).map_err(|_| error("database_error", "资料提案记录损坏"))?;
    p.status = row.1;
    Ok(p)
}
fn own(conn: &Connection, id: &str, client: &str) -> WorkflowResult<MaterialProposal> {
    let owner = conn
        .query_row(
            "SELECT client_id FROM material_proposals WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    if owner.as_deref() != Some(client) {
        return Err(error("not_found", "资料提案不可访问"));
    }
    read(conn, id)
}
fn audit(
    tx: &Transaction,
    p: &MaterialProposal,
    action: &str,
    actor: &str,
    revision: i64,
) -> WorkflowResult<()> {
    tx.execute(
        "INSERT INTO material_proposal_audit VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            uuid::Uuid::new_v4().to_string(),
            p.id,
            p.material_id,
            action,
            actor,
            revision,
            now()?
        ],
    )?;
    Ok(())
}
fn draft(
    current: &Material,
    title: String,
    fields: BTreeMap<String, String>,
    source_revision: Option<i64>,
    expected_revision: i64,
) -> SaveMaterialInput {
    SaveMaterialInput {
        id: current.id.clone(),
        title,
        fields,
        volume_id: current.volume_id.clone(),
        chapter_id: current.chapter_id.clone(),
        source_chapter_id: current.source_chapter_id.clone(),
        source_revision,
        story_order: current.story_order,
        story_time: current.story_time.clone(),
        tracking_status: current.tracking_status.clone(),
        payoff_chapter_id: current.payoff_chapter_id.clone(),
        payoff_revision: current.payoff_revision,
        expected_revision,
    }
}
fn source(conn: &Connection, current: &Material, new_revision: Option<i64>) -> WorkflowResult<()> {
    crate::materials::require_current_source(
        conn,
        &current.book_id,
        current.source_chapter_id.as_deref(),
        new_revision,
    )
}
impl Service {
    pub fn propose_material_edit(
        &mut self,
        input: MaterialProposalInput,
    ) -> WorkflowResult<MaterialProposal> {
        if input.client_id.trim().is_empty()
            || input.client_id.len() > 128
            || input.idempotency_key.trim().is_empty()
            || input.idempotency_key.len() > 128
            || input.material_id.is_empty()
            || input.material_id.len() > 128
            || input.base_revision < 0
            || input.base_revision == i64::MAX
            || input.reason.len() > 4096
            || input.new_title.len() > 4096
            || input.new_fields.len() > 16
            || input.new_fields.keys().any(|k| k.len() > 128)
            || input.new_fields.values().map(|v| v.len()).sum::<usize>() > 256 * 1024
        {
            return Err(error("invalid_input", "资料提案参数无效"));
        }
        let payload = serial(&input)?;
        let tx = self.conn.transaction()?;
        let current = crate::materials::get_material_in_tx(&tx, &input.material_id, false)?;
        if matches!(current.kind.as_str(), "timeline" | "foreshadow") {
            return Err(error(
                "unsupported_operation",
                "追踪资料本阶段仅供智能体读取",
            ));
        }
        if let Some((id,prior))=tx.query_row("SELECT id,payload FROM material_proposals WHERE client_id=?1 AND idempotency_key=?2",params![input.client_id,input.idempotency_key],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional()?{if prior!=payload{return Err(error("idempotency_conflict","同一幂等键不能用于不同资料提案"))}return read(&tx,&id)}
        if current.revision != input.base_revision {
            return Err(error("revision_conflict", "资料已更新，请重新读取"));
        }
        source(&tx, &current, input.new_source_revision)?;
        let edit = draft(
            &current,
            input.new_title.clone(),
            input.new_fields.clone(),
            input.new_source_revision,
            input.base_revision,
        );
        crate::materials::validate_material_edit_in_tx(&tx, &edit)?;
        let p = MaterialProposal {
            id: uuid::Uuid::new_v4().to_string(),
            client_id: input.client_id,
            material_id: input.material_id,
            base_revision: input.base_revision,
            old_snapshot: current,
            new_title: input.new_title,
            new_fields: input.new_fields,
            new_source_revision: input.new_source_revision,
            reason: input.reason,
            status: "pending".into(),
            created_at: now()?,
        };
        let record = serial(&p)?;
        // JSON-RPC tools wrap the JSON response as text and escape it again.
        // Bound before any INSERT, leaving ample room below the 2 MiB IPC frame.
        if record.len() > 512 * 1024 {
            return Err(error(
                "limit_exceeded",
                "资料提案JSON编码超过512KiB；未保存提案，请缩小建议内容",
            ));
        }
        let(total,pending,per_card,bytes):(i64,i64,i64,i64)=tx.query_row("SELECT count(*),coalesce(sum(status='pending'),0),coalesce(sum(status='pending' AND material_id=?1),0),coalesce(sum(length(CAST(record AS BLOB))+length(CAST(payload AS BLOB))),0) FROM material_proposals",[&p.material_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        if total >= 1000
            || pending >= 100
            || per_card >= 20
            || bytes + record.len() as i64 + payload.len() as i64 > 32 * 1024 * 1024
        {
            return Err(error(
                "limit_exceeded",
                "资料提案达到配额；不会自动清理审核记录",
            ));
        }
        tx.execute(
            "INSERT INTO material_proposals VALUES(?1,?2,?3,?4,'pending',?5,?6,?7,?8)",
            params![
                p.id,
                p.client_id,
                p.material_id,
                p.base_revision,
                record,
                payload,
                input.idempotency_key,
                p.created_at
            ],
        )?;
        audit(&tx, &p, "propose", &p.client_id, p.base_revision)?;
        tx.commit()?;
        Ok(p)
    }
    pub fn get_material_proposal(
        &self,
        id: &str,
        client_id: &str,
    ) -> WorkflowResult<MaterialProposal> {
        own(&self.conn, id, client_id)
    }
    pub fn list_material_proposals(&self, book_id: &str) -> WorkflowResult<Vec<MaterialProposal>> {
        // Only the current review queue, not a full-history/body dump.
        let active: std::collections::HashSet<_> = self
            .list_materials(book_id, false)?
            .into_iter()
            .map(|m| m.id)
            .collect();
        let ids=self.conn.prepare("SELECT p.id,p.material_id FROM material_proposals p JOIN materials m ON m.id=p.material_id WHERE m.book_id=?1 AND p.status='pending' ORDER BY p.created_at,p.id")?.query_map([book_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        ids.into_iter()
            .filter(|(_, material)| active.contains(material))
            .map(|(id, _)| read(&self.conn, &id))
            .collect()
    }
    pub fn cancel_material_proposal(
        &mut self,
        id: &str,
        client_id: &str,
    ) -> WorkflowResult<MaterialProposal> {
        let tx = self.conn.transaction()?;
        let mut p = own(&tx, id, client_id)?;
        if p.status != "pending" {
            return Err(error("proposal_decided", "提案已处理"));
        }
        let current = crate::materials::get_material_in_tx(&tx, &p.material_id, false)?;
        tx.execute(
            "UPDATE material_proposals SET status='cancelled' WHERE id=?1 AND status='pending'",
            [id],
        )?;
        audit(&tx, &p, "cancel", client_id, current.revision)?;
        tx.commit()?;
        p.status = "cancelled".into();
        Ok(p)
    }
    pub fn decide_material_proposal(
        &mut self,
        id: &str,
        approve: bool,
        expected_revision: i64,
    ) -> WorkflowResult<Material> {
        let tx = self.conn.transaction()?;
        let p = read(&tx, id)?;
        if p.status != "pending" {
            return Err(error("proposal_decided", "提案已处理"));
        }
        let current = crate::materials::get_material_in_tx(&tx, &p.material_id, !approve)?;
        let result = if approve {
            if matches!(current.kind.as_str(), "timeline" | "foreshadow") {
                return Err(error(
                    "unsupported_operation",
                    "追踪资料本阶段不支持采纳提案",
                ));
            }
            if current.revision != expected_revision || current.revision != p.base_revision {
                return Err(error("revision_conflict", "资料已更新，请重新审核"));
            }
            if current.source_chapter_id != p.old_snapshot.source_chapter_id {
                return Err(error("revision_conflict", "资料来源已改变"));
            }
            source(&tx, &current, p.new_source_revision)?;
            crate::materials::save_material_in_tx(
                &tx,
                draft(
                    &current,
                    p.new_title.clone(),
                    p.new_fields.clone(),
                    p.new_source_revision,
                    expected_revision,
                ),
            )?
        } else {
            current
        };
        tx.execute(
            "UPDATE material_proposals SET status=?1 WHERE id=?2 AND status='pending'",
            params![if approve { "accepted" } else { "rejected" }, id],
        )?;
        audit(
            &tx,
            &p,
            if approve { "accept" } else { "reject" },
            "human",
            result.revision,
        )?;
        tx.commit()?;
        Ok(result)
    }
}
