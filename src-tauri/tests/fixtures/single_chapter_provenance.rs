//! Independent branches of the original production v2 export. Only trusted
//! Service calls edit or review; the new suggestions use an in-process Broker.
use super::{AcceptedChapter, Story};
use novel_core::{
    agent::{AgentBroker, AgentConnection},
    AuditEvent, Chapter, ChapterVersion, Command, ConstrainedEditInput, ConstrainedProposal,
    CreateForeshadowOccurrence, ForeshadowOccurrenceView, ForeshadowPosition, ForeshadowSourceStatus,
    Material, SaveMaterialInput, Service, WorkflowError, WritingPack, WritingPolicy,
};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

const CHANGED: (&str, &str) = ("writing_constraint", "章节、前文、必需资料或来源已改变，旧建议不可采纳");
const MATERIAL: (&str, &str) = ("writing_constraint", "必需资料或来源待复核/不可用，请作者先处理");
const OCCURRENCE: (&str, &str) = ("revision_conflict", "必需伏笔的出现来源已变化或不可用，请作者先处理");

#[derive(Debug, Clone, Copy)]
enum Branch {
    Setting,
    Body,
    Move,
    Trash,
}

impl Branch {
    fn name(self) -> &'static str {
        match self {
            Self::Setting => "setting",
            Self::Body => "body",
            Self::Move => "move",
            Self::Trash => "trash",
        }
    }
    fn blocked_by(self) -> (&'static str, &'static str) {
        match self {
            Self::Setting => CHANGED,
            Self::Body | Self::Trash => MATERIAL,
            Self::Move => OCCURRENCE,
        }
    }
}

fn assert_error(error: WorkflowError, (code, message): (&str, &str)) {
    assert_eq!((error.code.as_str(), error.message.as_str()), (code, message));
}

fn submit(session: &AgentConnection, input: &ConstrainedEditInput) -> Value {
    session.call(json!({"method":"propose_constrained_edit","params":input}))
}

fn propose(session: &AgentConnection, input: &ConstrainedEditInput, epoch: &str) -> String {
    let response = submit(session, input);
    assert!(response.get("error").is_none(), "{}", response);
    let receipt = &response["result"];
    assert_eq!(receipt["base_revision"], 2);
    assert_eq!(receipt["live_epoch"], epoch);
    assert_eq!(receipt["decision"], "pending");
    assert!(receipt["accepted_revision"].is_null());
    receipt["id"].as_str().unwrap().into()
}

fn edit(material: &Material) -> SaveMaterialInput {
    SaveMaterialInput {
        id: material.id.clone(),
        title: material.title.clone(),
        fields: material.fields.clone(),
        volume_id: material.volume_id.clone(),
        chapter_id: material.chapter_id.clone(),
        expected_revision: material.revision,
        source_chapter_id: material.source_chapter_id.clone(),
        source_revision: material.source_revision,
        story_order: material.story_order,
        story_time: material.story_time.clone(),
        tracking_status: material.tracking_status.clone(),
        payoff_chapter_id: material.payoff_chapter_id.clone(),
        payoff_revision: material.payoff_revision,
    }
}

fn prepare(store: &mut Service, policy: &WritingPolicy, epoch: &str) -> WritingPack {
    let book = &policy.book_id;
    let manifest = store.prepare_writing_pack(book, &policy.chapter_id, policy.revision, 2, epoch).unwrap();
    store.get_writing_pack(book, &manifest.id).unwrap()
}

// Read the bytes committed by the real Service, not round-tripped JSON.
#[derive(Debug, PartialEq, Eq)]
struct RawWriting {
    payload: Vec<u8>,
    manifest: Vec<u8>,
    proposal: Vec<u8>,
}

impl RawWriting {
    fn read(path: &Path, pack: &str, proposal: &str) -> Self {
        let sql = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let (payload, manifest) = sql
            .query_row(
                "SELECT CAST(payload AS BLOB),CAST(manifest AS BLOB) FROM writing_packs WHERE id=?1",
                [pack],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let proposal = sql
            .query_row("SELECT CAST(payload AS BLOB) FROM writing_proposals WHERE id=?1", [proposal], |r| {
                r.get(0)
            })
            .unwrap();
        Self { payload, manifest, proposal }
    }
}

#[derive(Debug, PartialEq)]
struct Timeline {
    chapter: Chapter,
    history: Vec<ChapterVersion>,
    audit: Vec<AuditEvent>,
}

impl Timeline {
    fn read(store: &Service, chapter: &str) -> Self {
        Self {
            chapter: store.get_chapter(chapter).unwrap(),
            history: store.chapter_history(chapter).unwrap(),
            audit: store
                .audit_log()
                .unwrap()
                .into_iter()
                .filter(|event| event.chapter_id.as_deref() == Some(chapter))
                .collect(),
        }
    }

    fn assert_appends(&self, before: &Self, body: &str, source: &str) {
        assert_eq!(self.history.len(), before.history.len() + 1);
        assert_eq!(&self.history[..before.history.len()], &before.history);
        assert_eq!(self.audit.len(), before.audit.len() + 1);
        assert_eq!(&self.audit[1..], &before.audit);
        let revision = before.chapter.revision + 1;
        let version = self.history.last().unwrap();
        assert_eq!(
            (version.revision, version.body.as_bytes(), version.source.as_str()),
            (revision, body.as_bytes(), source)
        );
        let event = &self.audit[0];
        assert_eq!(
            (event.action.as_str(), event.revision, event.source.as_str()),
            ("save", Some(revision), source)
        );
        let mut expected = before.chapter.clone();
        expected.revision = revision;
        expected.body = body.into();
        assert_eq!(self.chapter, expected);
    }
}

struct Pending {
    pack: WritingPack,
    proposal: ConstrainedProposal,
    raw: RawWriting,
}
impl Pending {
    fn assert_retained(&self, store: &Service, path: &Path, epoch: &str) {
        let book = &self.pack.book_id;
        let state = store.get_writing_state(book, &self.pack.chapter_id).unwrap();
        assert_eq!(state.epoch, epoch);
        assert_eq!(state.policy.as_ref(), Some(&self.pack.policy));
        let pack = state.packs.iter().find(|p| p.id == self.pack.id).unwrap();
        assert!(!pack.archived);
        let proposal = state.proposals.iter().find(|p| p.id == self.proposal.id).unwrap();
        assert_eq!(
            (&proposal.pack_id, proposal.decision.as_str(), proposal.archived),
            (&self.pack.id, "pending", false)
        );
        assert_eq!(store.get_writing_pack(book, &self.pack.id).unwrap(), self.pack);
        assert_eq!(store.get_constrained_proposal(book, &self.proposal.id).unwrap(), self.proposal);
        assert_eq!(RawWriting::read(path, &self.pack.id, &self.proposal.id), self.raw);
    }

    fn assert_author_blocked(&self, store: &mut Service, epoch: &str, error: (&str, &str)) {
        let book = &self.pack.book_id;
        let review = self.proposal.author_review.as_ref().unwrap();
        assert_error(
            store
                .save_constrained_review(
                    book,
                    &self.proposal.id,
                    Some(review.revision),
                    epoch,
                    review.items.clone(),
                )
                .unwrap_err(),
            error,
        );
        assert_error(
            store
                .accept_reviewed_constrained_proposal(
                    book,
                    &self.proposal.id,
                    self.pack.policy.revision,
                    self.pack.chapter.revision,
                    epoch,
                    review.revision,
                    false,
                )
                .unwrap_err(),
            error,
        );
    }

    fn assert_agent_blocked(&self, session: &AgentConnection, error: (&str, &str), key: &str) {
        // A previously used key is a receipt lookup, so use a distinct key.
        let mut input = self.proposal.input.clone();
        input.idempotency_key = key.into();
        let response = submit(session, &input);
        assert_eq!(response["error"]["code"], error.0, "{}", response);
        assert_eq!(response["error"]["message"], error.1, "{}", response);
        assert!(response.get("result").is_none());
    }
}

struct Baseline<'a> {
    story: &'a Story,
    policy: &'a WritingPolicy,
    epoch: &'a str,
    prior: Timeline,
    target: Timeline,
    setting: Material,
    card: Material,
    occurrence: ForeshadowOccurrenceView,
}

impl Baseline<'_> {
    fn occurrence(&self, store: &Service) -> ForeshadowOccurrenceView {
        store
            .get_foreshadow_occurrence(
                &self.policy.book_id,
                &self.card.id,
                &self.occurrence.occurrence.id,
                self.epoch,
            )
            .unwrap()
    }

    fn assert_frozen_source(
        &self,
        pack: &WritingPack,
        expected_prior: &Chapter,
        expected: &ForeshadowOccurrenceView,
    ) {
        let occurrence = &expected.occurrence;
        assert_eq!(pack.chapter, self.target.chapter);
        assert_eq!(pack.context_chapters.len(), 1);
        let context = &pack.context_chapters[0];
        assert_eq!(&context.chapter, expected_prior);
        let source = pack.sources.iter().find(|s| s.id == expected_prior.id).unwrap();
        let position = ForeshadowPosition {
            chapter_id: expected_prior.id.clone(),
            chapter_title: expected_prior.title.clone(),
            chapter_revision: expected_prior.revision,
            chapter_sort_order: expected_prior.sort_order,
            volume_id: expected_prior.volume_id.clone(),
            volume_title: self.story.volume.title.clone(),
            volume_sort_order: 0,
        };
        assert_eq!(
            (&source.title, source.revision, source.sort_order, &source.volume),
            (
                &position.chapter_title,
                position.chapter_revision,
                position.chapter_sort_order,
                &context.volume
            ),
        );
        assert_eq!(
            (&context.volume.id, &context.volume.title, context.volume.sort_order),
            (&position.volume_id, &position.volume_title, position.volume_sort_order),
        );
        assert_eq!(pack.foreshadow_occurrences.as_slice(), std::slice::from_ref(occurrence));
        assert_eq!(occurrence.source, position);
        let sample = &self.story.occurrence;
        assert_eq!(
            (
                occurrence.start,
                occurrence.end,
                occurrence.quote.as_bytes(),
                occurrence.role,
                &occurrence.note
            ),
            (sample.start, sample.end, sample.quote.as_bytes(), sample.role, &sample.note)
        );
        assert_eq!(
            expected_prior
                .body
                .chars()
                .skip(sample.start as usize)
                .take((sample.end - sample.start) as usize)
                .collect::<String>()
                .as_bytes(),
            sample.quote.as_bytes()
        );
        let frozen_card = pack.materials.iter().find(|m| m.id == self.card.id).unwrap();
        assert_eq!(
            (
                frozen_card.source_chapter_id.as_deref(),
                frozen_card.source_revision,
                frozen_card.source_status.as_str(),
                frozen_card.tracking_status.as_deref()
            ),
            (Some(expected_prior.id.as_str()), Some(expected_prior.revision), "current", Some("planted"))
        );
    }

    fn assert_live_source(&self, store: &Service, branch: Branch, later_volume: Option<&str>) {
        use ForeshadowSourceStatus::{Current, Stale, Unavailable};
        let card = store.get_material(&self.card.id, false).unwrap();
        let occurrence = self.occurrence(store);
        let prior = if matches!(branch, Branch::Trash) {
            // Live chapter/history reads are denied in the recycle bin, but a
            // local backup still preserves that complete author history.
            assert_eq!(store.get_chapter(&self.prior.chapter.id).unwrap_err().code, "deleted");
            assert_eq!(store.chapter_history(&self.prior.chapter.id).unwrap_err().code, "deleted");
            let chapter =
                store.load().unwrap().chapters.into_iter().find(|c| c.id == self.prior.chapter.id).unwrap();
            let backup: Value = serde_json::from_str(&store.backup().unwrap()).unwrap();
            let history: Vec<ChapterVersion> = serde_json::from_value(backup["history"].clone()).unwrap();
            Timeline {
                chapter,
                history: history.into_iter().filter(|v| v.chapter_id == self.prior.chapter.id).collect(),
                audit: store
                    .audit_log()
                    .unwrap()
                    .into_iter()
                    .filter(|event| event.chapter_id.as_deref() == Some(&self.prior.chapter.id))
                    .collect(),
            }
        } else {
            Timeline::read(store, &self.prior.chapter.id)
        };
        let target = Timeline::read(store, &self.target.chapter.id);
        let mut expected_target = self.target.chapter.clone();
        if matches!(branch, Branch::Move) {
            // Transfer reindexes the old volume; this isn't a manuscript write.
            expected_target.sort_order = 0;
        }
        assert_eq!(target.chapter, expected_target);
        assert_eq!(target.history, self.target.history);
        assert_eq!(target.audit, self.target.audit);
        assert_eq!(occurrence.occurrence, self.occurrence.occurrence);
        let (material_status, occurrence_status) = match branch {
            Branch::Setting => ("current", Current),
            Branch::Body => ("stale", Stale),
            Branch::Move => ("current", Stale),
            Branch::Trash => ("unavailable", Unavailable),
        };
        let mut expected_card = self.card.clone();
        expected_card.source_status = material_status.into();
        assert_eq!(card, expected_card, "{}", branch.name());
        assert_eq!(occurrence.source_status, occurrence_status, "{}", branch.name());
        let mut position = self.occurrence.occurrence.source.clone();
        let current = match branch {
            Branch::Setting => {
                assert_eq!(prior, self.prior);
                Some(position)
            }
            Branch::Body => {
                let body = format!("{}{}", self.story.prior.body, self.story.provenance.prior_append);
                prior.assert_appends(&self.prior, &body, "gui");
                position.chapter_revision += 1;
                Some(position)
            }
            Branch::Move => {
                let mut expected = self.prior.chapter.clone();
                expected.volume_id = later_volume.unwrap().into();
                // The destination is a newly created empty second volume.
                expected.sort_order = 0;
                assert_eq!(prior.chapter, expected);
                position.volume_id = expected.volume_id;
                position.volume_title = self.story.provenance.later_volume.clone();
                position.volume_sort_order = 1;
                position.chapter_sort_order = 0;
                Some(position)
            }
            Branch::Trash => {
                let mut expected = self.prior.chapter.clone();
                assert!(prior.chapter.deleted_at.is_some());
                expected.deleted_at = prior.chapter.deleted_at;
                assert_eq!(prior.chapter, expected);
                None
            }
        };
        assert_eq!(occurrence.current, current);
        if !matches!(branch, Branch::Body) {
            assert_eq!(prior.history, self.prior.history);
            assert_eq!(prior.audit, self.prior.audit);
        }
    }
}

impl AcceptedChapter<'_> {
    pub(super) fn rebuild_provenance(&self, export: &Path, dir: &Path, epoch: &str, story: &Story) {
        let policy = self.policy;
        let book = &policy.book_id;
        let target = &self.chapter.id;
        let prior = &self.prior.id;
        let original = self.proposals[0];
        let review_items = &original.author_review.as_ref().unwrap().items;
        for branch in [Branch::Setting, Branch::Body, Branch::Move, Branch::Trash] {
            let path = dir.join(format!("provenance-{}.sqlite", branch.name()));
            std::fs::copy(export, &path).unwrap();
            let store = Arc::new(Mutex::new(Service::open(&path).unwrap()));
            let broker = Arc::new(AgentBroker::new(store.clone()));
            let session = broker.connect().unwrap();
            let request = session.call(
                json!({"method":"request_access","params":{"client_name":"original provenance fixture"}}),
            );
            assert_eq!(request["result"]["status"], "pending");
            broker.grant_session(session.id(), book).unwrap();
            let (before, pack, draft) = {
                let mut live = store.lock().unwrap();
                self.assert_writing_evidence(&live, epoch, false);
                let card_id = &self.pack.foreshadow_occurrences[0].material_id;
                let occurrence = live
                    .get_foreshadow_occurrence(book, card_id, &self.pack.foreshadow_occurrences[0].id, epoch)
                    .unwrap();
                assert_eq!(occurrence.source_status, ForeshadowSourceStatus::Current);
                assert_eq!(occurrence.current.as_ref(), Some(&occurrence.occurrence.source));
                let setting = self.pack.materials.iter().find(|m| m.kind == "setting").unwrap();
                let before = Baseline {
                    story,
                    policy,
                    epoch,
                    prior: Timeline::read(&live, prior),
                    target: Timeline::read(&live, target),
                    setting: live.get_material(&setting.id, false).unwrap(),
                    card: live.get_material(card_id, false).unwrap(),
                    occurrence,
                };
                assert_eq!(&before.target.chapter, self.chapter);
                assert_eq!(before.target.history, self.history);
                assert_eq!(&before.prior.chapter, self.prior);
                assert_eq!(before.prior.chapter.body.as_bytes(), story.prior.body.as_bytes());
                assert_eq!(
                    before.setting.fields,
                    story.materials.iter().find(|card| card.key == "setting").unwrap().fields
                );
                assert_eq!(
                    before.card.fields,
                    story.materials.iter().find(|card| card.key == "foreshadow").unwrap().fields
                );

                let pack = prepare(&mut live, policy, epoch);
                assert_ne!(pack.id, self.pack.id);
                before.assert_frozen_source(&pack, self.prior, &before.occurrence);
                assert!(pack.materials.contains(&before.setting));
                let draft = ConstrainedEditInput {
                    pack_id: pack.id.clone(),
                    new_body: story.rework.pending_body.clone(),
                    citations: original.input.citations.clone(),
                    conflicts: vec![],
                    idempotency_key: "provenance-before-change".into(),
                };
                (before, pack, draft)
            };
            let pending_id = propose(&session, &draft, epoch);
            let (pending, later_volume, writing_state) = {
                let mut live = store.lock().unwrap();
                let review = live
                    .save_constrained_review(book, &pending_id, None, epoch, review_items.clone())
                    .unwrap();
                let proposal = live.get_constrained_proposal(book, &pending_id).unwrap();
                assert_eq!(proposal.input, draft);
                assert_eq!(
                    (proposal.decision.as_str(), proposal.accepted_revision, proposal.decided_at),
                    ("pending", None, None)
                );
                assert_eq!(proposal.author_review.as_ref(), Some(&review));
                assert_eq!(
                    (review.revision, review.chapter_revision, review.policy_revision, &review.items),
                    (1, 2, policy.revision, review_items)
                );
                let raw = RawWriting::read(&path, &pack.id, &proposal.id);
                let pending = Pending { pack, proposal, raw };
                let writing_state = json!(live.get_writing_state(book, target).unwrap());
                let later_volume = match branch {
                    Branch::Setting => {
                        let mut input = edit(&before.setting);
                        input.fields.insert("rules".into(), story.provenance.revised_rule.clone());
                        let saved = live.save_material(input).unwrap();
                        assert_eq!(saved.revision, before.setting.revision + 1);
                        assert_eq!(saved.fields["rules"], story.provenance.revised_rule);
                        None
                    }
                    Branch::Body => {
                        let body = format!("{}{}", story.prior.body, story.provenance.prior_append);
                        assert_ne!(body.as_bytes(), story.prior.body.as_bytes());
                        assert!(body.as_bytes().starts_with(story.occurrence.quote.as_bytes()));
                        live.apply(Command::SaveChapter {
                            id: prior.clone(),
                            body,
                            expected_revision: self.prior.revision,
                        })
                        .unwrap();
                        None
                    }
                    Branch::Move => {
                        let tree = live
                            .apply(Command::CreateVolume {
                                book_id: book.clone(),
                                title: story.provenance.later_volume.clone(),
                            })
                            .unwrap();
                        let volume = tree
                            .volumes
                            .into_iter()
                            .find(|v| v.book_id == *book && v.title == story.provenance.later_volume)
                            .unwrap();
                        assert!(volume.sort_order > pending.pack.volume.sort_order);
                        live.apply(Command::TransferChapter {
                            id: prior.clone(),
                            volume_id: volume.id.clone(),
                        })
                        .unwrap();
                        Some(volume.id)
                    }
                    Branch::Trash => {
                        live.apply(Command::TrashChapter { id: prior.clone() }).unwrap();
                        None
                    }
                };
                before.assert_live_source(&live, branch, later_volume.as_deref());
                if let Branch::Move | Branch::Trash = branch {
                    let directory = live.get_writing_context_directory(book, target).unwrap();
                    assert!(!directory.chapters.iter().any(|c| c.id == *prior));
                    let message = match branch {
                        Branch::Move => "所选前文章节必须在目标章之前，请调整配置",
                        Branch::Trash => "所选前文章节不可用，请调整配置",
                        _ => unreachable!(),
                    };
                    // Independently exercise the prior-context gate: occurrence
                    // validation is the earlier check when freezing a pack.
                    assert_error(
                        live.configure_writing_policy_with_context(
                            book,
                            target,
                            Some(policy.revision),
                            "active",
                            &policy.goal,
                            policy.required_material_ids.clone(),
                            vec![prior.clone()],
                            epoch,
                        )
                        .unwrap_err(),
                        ("writing_constraint", message),
                    );
                }
                (pending, later_volume, writing_state)
            };

            pending.assert_agent_blocked(&session, branch.blocked_by(), "provenance-old-pack");
            let mut live = store.lock().unwrap();
            pending.assert_author_blocked(&mut live, epoch, branch.blocked_by());
            assert_eq!(json!(live.get_writing_state(book, target).unwrap()), writing_state);
            pending.assert_retained(&live, &path, epoch);
            before.assert_live_source(&live, branch, later_volume.as_deref());

            // The settings-only branch needs a new pack but doesn't need a
            // refreshed chapter/occurrence. The other live blockers prevent it.
            let setting_pack = if matches!(branch, Branch::Setting) {
                let fresh = prepare(&mut live, policy, epoch);
                assert_ne!(fresh.id, pending.pack.id);
                before.assert_frozen_source(&fresh, self.prior, &before.occurrence);
                let setting_entry = format!("material:{}", before.setting.id);
                for (id, rules) in [
                    (&pending.pack.id, &before.setting.fields["rules"]),
                    (&fresh.id, &story.provenance.revised_rule),
                ] {
                    let chunk = live.read_writing_pack_entry(book, id, &setting_entry, "rules", 0).unwrap();
                    assert_eq!(chunk.text.as_bytes(), rules.as_bytes());
                    assert_eq!(chunk.total_bytes, rules.len());
                    assert!(chunk.next_cursor.is_none());
                }
                Some(fresh)
            } else {
                assert_error(
                    live.prepare_writing_pack(book, target, policy.revision, 2, epoch).unwrap_err(),
                    branch.blocked_by(),
                );
                None
            };

            let mut renewed = None;
            if matches!(branch, Branch::Body) {
                let provenance = renew_body_sources(&mut live, &before, &pending);
                let old_field = format!("occurrence:{}", before.occurrence.occurrence.id);
                let new_field = format!("occurrence:{}", provenance.new_occurrence.occurrence.id);
                let fresh_draft = ConstrainedEditInput {
                    pack_id: provenance.pack.id.clone(),
                    new_body: story.rework.fresh_body.clone(),
                    citations: draft
                        .citations
                        .iter()
                        .cloned()
                        .map(|mut citation| {
                            if citation.field_key == old_field {
                                citation.field_key = new_field.clone();
                            }
                            citation
                        })
                        .collect(),
                    conflicts: vec![],
                    idempotency_key: "provenance-new-candidate".into(),
                };
                assert!(fresh_draft.citations.iter().any(|c| c.field_key == new_field));
                assert!(!fresh_draft.citations.iter().any(|c| c.field_key == old_field));
                assert_ne!(fresh_draft.new_body, draft.new_body);
                assert_ne!(fresh_draft.new_body, self.chapter.body);
                assert_eq!(Timeline::read(&live, target), before.target);
                pending.assert_retained(&live, &path, epoch);
                pending.assert_author_blocked(&mut live, epoch, CHANGED);
                drop(live);
                pending.assert_agent_blocked(&session, CHANGED, "provenance-old-after-renewal");
                let candidate = propose(&session, &fresh_draft, epoch);

                live = store.lock().unwrap();
                let fresh = live.get_constrained_proposal(book, &candidate).unwrap();
                assert_eq!(fresh.input, fresh_draft);
                assert!(fresh.author_review.is_none());
                assert_error(
                    live.accept_reviewed_constrained_proposal(
                        book,
                        &candidate,
                        policy.revision,
                        2,
                        epoch,
                        1,
                        false,
                    )
                    .unwrap_err(),
                    ("writing_constraint", "请先保存逐项作者审阅"),
                );
                let review = live
                    .save_constrained_review(book, &candidate, None, epoch, review_items.clone())
                    .unwrap();
                assert_eq!(
                    (&review.pack_id, review.revision, review.chapter_revision, &review.items),
                    (&provenance.pack.id, 1, 2, review_items)
                );
                assert_eq!(Timeline::read(&live, target), before.target);
                let accepted = live
                    .accept_reviewed_constrained_proposal(
                        book,
                        &candidate,
                        policy.revision,
                        2,
                        epoch,
                        review.revision,
                        false,
                    )
                    .unwrap();
                assert_eq!((accepted.decision.as_str(), accepted.accepted_revision), ("accepted", Some(3)));
                assert_eq!(accepted.input, fresh_draft);
                assert_eq!(accepted.author_review.as_ref(), Some(&review));
                let source = format!("strict:{candidate}");
                let timeline = Timeline::read(&live, target);
                timeline.assert_appends(&before.target, &story.rework.fresh_body, &source);
                assert_eq!(Timeline::read(&live, prior), provenance.prior);
                let raw = RawWriting::read(&path, &provenance.pack.id, &candidate);
                renewed = Some((provenance, accepted, timeline, raw));
            }

            let final_state = json!(live.get_writing_state(book, target).unwrap());
            assert_eq!(
                final_state["packs"].as_array().unwrap().len(),
                if setting_pack.is_some() || renewed.is_some() { 3 } else { 2 }
            );
            assert_eq!(
                final_state["proposals"].as_array().unwrap().len(),
                if renewed.is_some() { 4 } else { 3 }
            );
            drop(live);
            drop(session);
            drop(broker);
            drop(store);
            let mut reopened = Service::open(&path).unwrap();
            assert_eq!(reopened.epoch().unwrap(), epoch);
            assert_eq!(json!(reopened.get_writing_state(book, target).unwrap()), final_state);
            assert_eq!(reopened.get_writing_pack(book, &self.pack.id).unwrap(), *self.pack);
            for original in self.proposals {
                assert_eq!(reopened.get_constrained_proposal(book, &original.id).unwrap(), *original);
            }
            if let Some((provenance, accepted, timeline, raw)) = renewed {
                assert_eq!(Timeline::read(&reopened, target), timeline);
                assert_eq!(Timeline::read(&reopened, prior), provenance.prior);
                assert_eq!(reopened.get_writing_pack(book, &provenance.pack.id).unwrap(), provenance.pack);
                assert_eq!(reopened.get_constrained_proposal(book, &accepted.id).unwrap(), accepted);
                assert_eq!(RawWriting::read(&path, &provenance.pack.id, &accepted.id), raw);
                provenance.assert_current(&reopened, &before);
                pending.assert_author_blocked(&mut reopened, epoch, CHANGED);
                assert_eq!(Timeline::read(&reopened, target), timeline);
            } else {
                pending.assert_author_blocked(&mut reopened, epoch, branch.blocked_by());
                if let Some(fresh) = setting_pack {
                    assert_eq!(reopened.get_writing_pack(book, &fresh.id).unwrap(), fresh);
                    assert_eq!(
                        reopened.get_material(&before.setting.id, false).unwrap().fields["rules"],
                        story.provenance.revised_rule
                    );
                }
                before.assert_live_source(&reopened, branch, later_volume.as_deref());
            }
            pending.assert_retained(&reopened, &path, epoch);
            assert_eq!(json!(reopened.get_writing_state(book, target).unwrap()), final_state);
            eprintln!(
                "original provenance {}: exact frozen v2, blocked old writes and SQLite reopen passed",
                branch.name()
            );
        }
    }
}

struct Renewal {
    prior: Timeline,
    card: Material,
    retired: ForeshadowOccurrenceView,
    new_occurrence: ForeshadowOccurrenceView,
    pack: WritingPack,
}
impl Renewal {
    fn assert_current(&self, store: &Service, before: &Baseline<'_>) {
        let book = &before.policy.book_id;
        let card = &before.card.id;
        let epoch = before.epoch;
        let current = store.get_material(card, false).unwrap();
        assert_eq!(current, self.card);
        assert_eq!(current.tracking_status.as_deref(), Some("planted"));
        assert_eq!(
            (current.payoff_chapter_id, current.payoff_revision, current.payoff_status.as_str()),
            (None, None, "none")
        );
        assert_eq!(before.occurrence(store), self.retired);
        let rows = store.list_foreshadow_occurrences(book, card, epoch).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.contains(&self.new_occurrence));
        assert!(rows.contains(&self.retired));
        let overview = store.list_book_foreshadow(book, epoch).unwrap();
        let summary = overview.iter().find(|row| row.material.id == *card).unwrap();
        assert_eq!((summary.occurrence_count, summary.review_count), (1, 0));
        assert_eq!(summary.material.tracking_status.as_deref(), Some("planted"));
    }
}

fn renew_body_sources(store: &mut Service, before: &Baseline<'_>, pending: &Pending) -> Renewal {
    let book = &before.policy.book_id;
    let card = &before.card.id;
    let target = &before.target.chapter.id;
    let prior = &before.prior.chapter.id;
    let epoch = before.epoch;
    let old = &before.occurrence.occurrence;
    let mut ordinary = edit(&store.get_material(card, false).unwrap());
    ordinary.title = before.story.provenance.ordinary_card_title.clone();
    let saved = store.save_material(ordinary).unwrap();
    assert_eq!(saved.title, before.story.provenance.ordinary_card_title);
    assert_eq!(saved.fields, before.card.fields);
    assert_eq!(saved.revision, before.card.revision + 1);
    assert_eq!(
        (
            saved.source_chapter_id.as_deref(),
            saved.source_revision,
            saved.source_status.as_str(),
            saved.tracking_status.as_deref()
        ),
        (Some(prior.as_str()), Some(old.source.chapter_revision), "stale", Some("planted"))
    );
    let unchanged = before.occurrence(store);
    assert_eq!(unchanged.occurrence, *old);
    assert_eq!(unchanged.source_status, ForeshadowSourceStatus::Stale);
    assert_error(
        store.prepare_writing_pack(book, target, before.policy.revision, 2, epoch).unwrap_err(),
        MATERIAL,
    );

    let reviewed = store.get_foreshadow_chapter(book, prior, epoch).unwrap();
    assert_eq!(reviewed.position.chapter_revision, old.source.chapter_revision + 1);
    let actual_quote: String =
        reviewed.body.chars().skip(old.start as usize).take((old.end - old.start) as usize).collect();
    assert_eq!(actual_quote.as_bytes(), old.quote.as_bytes());
    let mut explicit = edit(&saved);
    explicit.source_revision = Some(reviewed.position.chapter_revision);
    let confirmed_card = store.save_material(explicit).unwrap();
    assert_eq!(confirmed_card.revision, saved.revision + 1);
    assert_eq!(
        (confirmed_card.source_revision, confirmed_card.source_status.as_str()),
        (Some(reviewed.position.chapter_revision), "current")
    );
    assert_eq!(before.occurrence(store), unchanged);
    assert_error(
        store.prepare_writing_pack(book, target, before.policy.revision, 2, epoch).unwrap_err(),
        OCCURRENCE,
    );

    let retired =
        store.set_foreshadow_occurrence_deleted(book, card, &old.id, old.revision, epoch, true).unwrap();
    let mut expected = old.clone();
    expected.revision += 1;
    expected.deleted_at = retired.occurrence.deleted_at;
    assert!(expected.deleted_at.is_some());
    assert_eq!(retired.occurrence, expected);
    assert_eq!(retired.source_status, ForeshadowSourceStatus::Stale);
    assert_eq!(retired.current.as_ref(), Some(&reviewed.position));

    let replacement = CreateForeshadowOccurrence {
        book_id: book.clone(),
        material_id: card.clone(),
        chapter_id: prior.clone(),
        chapter_revision: reviewed.position.chapter_revision,
        start: old.start,
        end: old.end,
        quote: actual_quote,
        role: old.role,
        note: old.note.clone(),
        epoch: epoch.into(),
        request_key: "provenance-explicit-new-occurrence".into(),
    };
    let new_occurrence = store.create_foreshadow_occurrence(replacement).unwrap();
    assert_ne!(new_occurrence.occurrence.id, old.id);
    assert_eq!(new_occurrence.occurrence.material_id, *card);
    assert!(new_occurrence.occurrence.deleted_at.is_none());
    assert_eq!(new_occurrence.source_status, ForeshadowSourceStatus::Current);
    assert_eq!(new_occurrence.current.as_ref(), Some(&reviewed.position));
    let current_card = store.get_material(card, false).unwrap();
    assert_eq!(current_card.source_revision, confirmed_card.source_revision);
    assert_eq!(current_card.source_status, "current");
    assert_eq!(current_card.fields, before.card.fields);
    let fresh = prepare(store, before.policy, epoch);
    let prior = Timeline::read(store, prior);
    let current_body = format!("{}{}", before.story.prior.body, before.story.provenance.prior_append);
    prior.assert_appends(&before.prior, &current_body, "gui");
    assert_eq!(reviewed.body.as_bytes(), current_body.as_bytes());
    assert_ne!(fresh.id, pending.pack.id);
    before.assert_frozen_source(&fresh, &prior.chapter, &new_occurrence);
    before.assert_frozen_source(&pending.pack, &before.prior.chapter, &before.occurrence);
    let renewal = Renewal { prior, card: current_card, retired, new_occurrence, pack: fresh };
    renewal.assert_current(store, before);
    renewal
}
