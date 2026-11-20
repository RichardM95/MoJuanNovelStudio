//! The pre-backup v2 story branches into author history rework. Trusted
//! operations use Service; new agent submissions use an in-process Broker.
use super::{write_chapter_txt, AcceptedChapter, Story};
use novel_core::{
    agent::AgentBroker, AuditEvent, ChapterVersion, ConstrainedEditInput, ConstrainedProposal,
    Service, WorkflowError, WritingPack,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, Mutex},
};

const STALE_PACK: &str = "章节、前文、必需资料或来源已改变，旧建议不可采纳";

fn assert_error(error: WorkflowError, code: &str, message: &str) {
    assert_eq!(error.code, code);
    assert!(error.message.contains(message), "{error}");
}

// Assert retained rows rather than only counts. In particular an old pending
// review must not be silently cleared or archived to make acceptance fail.
fn assert_evidence(
    store: &Service,
    original: &AcceptedChapter<'_>,
    epoch: &str,
    added_packs: &[&WritingPack],
    added_proposals: &[&ConstrainedProposal],
) {
    let book = &original.policy.book_id;
    let state = store.get_writing_state(book, &original.chapter.id).unwrap();
    assert_eq!(store.epoch().unwrap(), epoch);
    assert_eq!(state.epoch, epoch);
    assert_eq!(state.policy.as_ref(), Some(original.policy));
    assert_eq!(state.packs.len(), 1 + added_packs.len());
    for pack in [original.pack]
        .into_iter()
        .chain(added_packs.iter().copied())
    {
        let summary = state.packs.iter().find(|p| p.id == pack.id).unwrap();
        assert!(!summary.archived);
        assert_eq!(summary.chapter_revision, pack.chapter.revision);
        assert_eq!(&store.get_writing_pack(book, &pack.id).unwrap(), pack);
    }
    assert_eq!(state.proposals.len(), 2 + added_proposals.len());
    for proposal in original
        .proposals
        .into_iter()
        .chain(added_proposals.iter().copied())
    {
        let summary = state
            .proposals
            .iter()
            .find(|p| p.id == proposal.id)
            .unwrap();
        assert!(!summary.archived);
        assert_eq!(summary.decision, proposal.decision);
        assert_eq!(
            &store.get_constrained_proposal(book, &proposal.id).unwrap(),
            proposal
        );
    }
    assert_eq!(
        store.get_chapter(&original.prior.id).unwrap(),
        *original.prior
    );
}

#[derive(Debug, PartialEq, Eq)]
struct Timeline {
    history: Vec<ChapterVersion>,
    audit: Vec<AuditEvent>,
}

impl Timeline {
    fn assert_appends_one(&self, before: &Self) {
        assert_eq!(&self.history[..self.history.len() - 1], before.history);
        assert_eq!(&self.audit[1..], before.audit);
    }
}

// The saved Service view, scoped GUI history detail and audit are independent
// read paths; all must agree while retaining the complete v0..v2 prefix.
fn assert_timeline(
    store: &Service,
    original: &AcceptedChapter<'_>,
    epoch: &str,
    initial_audit: &[AuditEvent],
    additions: &[(&str, &str)],
) -> Timeline {
    let book = &original.policy.book_id;
    let chapter = &original.chapter.id;
    let history = store.chapter_history(chapter).unwrap();
    let audit = store.audit_log().unwrap();
    assert_eq!(history.len(), 3 + additions.len());
    assert_eq!(&history[..3], original.history);
    assert_eq!(audit.len(), initial_audit.len() + additions.len());
    assert_eq!(&audit[additions.len()..], initial_audit);

    for (i, (body, source)) in additions.iter().enumerate() {
        let revision = 3 + i as i64;
        let row = &history[revision as usize];
        assert_eq!(
            (row.revision, row.body.as_bytes(), row.source.as_str()),
            (revision, body.as_bytes(), *source)
        );
        let event = &audit[additions.len() - 1 - i];
        assert_eq!(
            (
                event.chapter_id.as_deref(),
                event.action.as_str(),
                event.revision,
                event.source.as_str()
            ),
            (Some(chapter.as_str()), "save", Some(revision), *source)
        );
    }
    let live = store.get_chapter(chapter).unwrap();
    let latest = history.last().unwrap();
    assert_eq!(
        (live.revision, live.body.as_bytes()),
        (latest.revision, latest.body.as_bytes())
    );
    assert_eq!(
        history.iter().map(|v| &v.id).collect::<BTreeSet<_>>().len(),
        history.len()
    );
    let page = store
        .list_chapter_versions(book, chapter, None, 50, false)
        .unwrap();
    assert_eq!(page.epoch, epoch);
    assert!(page.next_cursor.is_none());
    assert_eq!(page.items.len(), history.len());
    for (summary, row) in page.items.iter().zip(history.iter().rev()) {
        assert_eq!(
            (
                &summary.id,
                &summary.chapter_id,
                summary.revision,
                &summary.source
            ),
            (&row.id, chapter, row.revision, &row.source)
        );
        let detail = store
            .get_chapter_version(book, chapter, &summary.id)
            .unwrap();
        assert_eq!(detail.epoch, epoch);
        assert_eq!(&detail.version, row);
    }
    Timeline { history, audit }
}

impl AcceptedChapter<'_> {
    pub(super) fn rework_history(
        &self,
        database: &Path,
        dir: &Path,
        epoch: &str,
        story: &Story,
        initial_audit: &[AuditEvent],
    ) {
        let book = &self.policy.book_id;
        let target = &self.chapter.id;
        let accepted = self.proposals[0];
        let store = Arc::new(Mutex::new(Service::open(database).unwrap()));
        let (original, adopted, wrong_chapter, old_pack) = {
            let mut live = store.lock().unwrap();
            assert_eq!(live.get_chapter(target).unwrap(), *self.chapter);
            self.assert_writing_evidence(&live, epoch, false);
            let versions = assert_timeline(&live, self, epoch, initial_audit, &[]).history;
            let prior_page = live
                .list_chapter_versions(book, &self.prior.id, None, 50, false)
                .unwrap();
            let wrong_chapter = prior_page
                .items
                .iter()
                .find(|v| v.revision == 1)
                .unwrap()
                .id
                .clone();
            assert_ne!(wrong_chapter, versions[1].id);
            let manifest = live
                .prepare_writing_pack(book, target, 0, 2, epoch)
                .unwrap();
            let pack = live.get_writing_pack(book, &manifest.id).unwrap();
            assert_eq!(pack.chapter, *self.chapter);
            assert_ne!(pack.id, self.pack.id);
            (
                versions[1].clone(),
                versions[2].clone(),
                wrong_chapter,
                pack,
            )
        };

        let broker = Arc::new(AgentBroker::new(store.clone()));
        let session = broker.connect().unwrap();
        let request = session.call(
            json!({"method":"request_access","params":{"client_name":"chapter rework fixture"}}),
        );
        assert_eq!(request["result"]["status"], "pending");
        broker.grant_session(session.id(), book).unwrap();
        let submit = |input: &ConstrainedEditInput| {
            session.call(json!({"method":"propose_constrained_edit","params":input}))
        };
        let propose = |input: &ConstrainedEditInput, base| {
            let response: Value = submit(input);
            assert!(response.get("error").is_none(), "{response}");
            let receipt = &response["result"];
            assert_eq!(receipt["base_revision"], base);
            assert_eq!(receipt["live_epoch"], epoch);
            assert_eq!(receipt["decision"], "pending");
            assert!(receipt["accepted_revision"].is_null());
            assert_eq!(receipt["new_body_bytes"], input.new_body.len());
            receipt["id"].as_str().unwrap().to_owned()
        };
        let draft = |pack: &WritingPack, body: &str, key: &str| ConstrainedEditInput {
            pack_id: pack.id.clone(),
            new_body: body.into(),
            citations: accepted.input.citations.clone(),
            conflicts: vec![],
            idempotency_key: key.into(),
        };
        let old_input = draft(
            &old_pack,
            &story.rework.pending_body,
            "clock-rework-pending",
        );
        let old_id = propose(&old_input, 2);
        let author_items = &accepted.author_review.as_ref().unwrap().items;
        let (pending, fresh_pack, at_v4) = {
            let mut live = store.lock().unwrap();
            let review = live
                .save_constrained_review(book, &old_id, None, epoch, author_items.clone())
                .unwrap();
            assert_eq!(
                (
                    review.revision,
                    review.chapter_revision,
                    review.policy_revision
                ),
                (1, 2, 0)
            );
            assert_eq!(&review.items, author_items);
            let pending = live.get_constrained_proposal(book, &old_id).unwrap();
            assert_eq!(pending.input, old_input);
            assert_eq!(pending.author_review.as_ref(), Some(&review));
            assert_eq!(pending.decision, "pending");
            assert_eq!(
                (pending.accepted_revision, pending.decided_at),
                (None, None)
            );
            let initial = assert_timeline(&live, self, epoch, initial_audit, &[]);
            let prior_history = live.chapter_history(&self.prior.id).unwrap();
            let assert_v2 = |s: &Service| {
                assert_eq!(s.get_chapter(target).unwrap(), *self.chapter);
                assert_eq!(s.chapter_history(&self.prior.id).unwrap(), prior_history);
                assert_eq!(assert_timeline(s, self, epoch, initial_audit, &[]), initial);
                assert_evidence(s, self, epoch, &[&old_pack], &[&pending]);
            };
            assert_v2(&live);
            assert_error(
                live.restore_chapter_version_scoped(book, target, &original.id, 1, epoch)
                    .unwrap_err(),
                "revision_conflict",
                "未恢复旧版本",
            );
            assert_v2(&live);
            assert_error(
                live.restore_chapter_version_scoped(book, target, &wrong_chapter, 2, epoch)
                    .unwrap_err(),
                "not_found",
                "不属于该章节",
            );
            assert_v2(&live);

            // Force failure at the final audit INSERT, conditional on both
            // chapter and new history already containing the restored body.
            let sql = rusqlite::Connection::open(database).unwrap();
            sql.execute_batch(
                "CREATE TRIGGER fixture_fail_restore_audit BEFORE INSERT ON workflow_audit
                 WHEN NEW.action='save' AND NEW.revision=3 AND NEW.source LIKE 'history:%'
                   AND EXISTS (
                     SELECT 1 FROM chapters c
                     JOIN chapter_versions v ON v.chapter_id=c.id AND v.revision=c.revision
                     JOIN chapter_versions old ON old.chapter_id=c.id AND old.revision=1
                     WHERE c.id=NEW.chapter_id AND c.revision=3 AND v.source=NEW.source
                       AND c.body=v.body AND c.body=old.body
                   )
                 BEGIN SELECT RAISE(ABORT, 'isolated chapter restore audit fault'); END;",
            )
            .unwrap();
            assert_error(
                live.restore_chapter_version_scoped(book, target, &original.id, 2, epoch)
                    .unwrap_err(),
                "database_error",
                "isolated chapter restore audit fault",
            );
            assert_v2(&live);
            sql.execute_batch("DROP TRIGGER fixture_fail_restore_audit")
                .unwrap();
            drop(sql);
            eprintln!("chapter rework: wrong CAS/wrong chapter/audit fault preserve v2 chapter, history, audit, epoch and reviewed pending evidence");

            let v3 = live
                .restore_chapter_version_scoped(book, target, &original.id, 2, epoch)
                .unwrap();
            let v3_c = v3.chapters.iter().find(|c| c.id == *target).unwrap();
            assert_eq!(
                (v3_c.revision, v3_c.body.as_bytes()),
                (3, story.target.original_body.as_bytes())
            );
            let source_v3 = format!("history:{}", original.id);
            let at_v3 = assert_timeline(
                &live,
                self,
                epoch,
                initial_audit,
                &[(&story.target.original_body, &source_v3)],
            );
            at_v3.assert_appends_one(&initial);
            assert_evidence(&live, self, epoch, &[&old_pack], &[&pending]);
            let v4 = live
                .restore_chapter_version_scoped(book, target, &adopted.id, 3, epoch)
                .unwrap();
            let v4_c = v4.chapters.iter().find(|c| c.id == *target).unwrap();
            assert_eq!(
                (v4_c.revision, v4_c.body.as_bytes()),
                (4, story.accepted.body.as_bytes())
            );
            assert_eq!(v4_c.body.as_bytes(), old_pack.chapter.body.as_bytes());
            assert_ne!(v4_c.revision, old_pack.chapter.revision);
            let source_v4 = format!("history:{}", adopted.id);
            let additions = [
                (&*story.target.original_body, &*source_v3),
                (&*story.accepted.body, &*source_v4),
            ];
            let at_v4 = assert_timeline(&live, self, epoch, initial_audit, &additions);
            at_v4.assert_appends_one(&at_v3);
            assert_evidence(&live, self, epoch, &[&old_pack], &[&pending]);

            let old_review_revision = review.revision;
            for revision in [2, 4] {
                assert_error(
                    live.accept_reviewed_constrained_proposal(
                        book,
                        &old_id,
                        0,
                        revision,
                        epoch,
                        old_review_revision,
                        false,
                    )
                    .unwrap_err(),
                    "writing_constraint",
                    STALE_PACK,
                );
            }
            // Drop the mutex before the Broker uses the same live Service.
            drop(live);
            let stale = submit(&draft(
                &old_pack,
                &story.rework.pending_body,
                "clock-rework-stale-new-key",
            ));
            assert_eq!(stale["error"]["code"], "writing_constraint");
            assert_eq!(stale["error"]["message"], STALE_PACK);
            assert!(stale.get("result").is_none());

            let mut live = store.lock().unwrap();
            assert_eq!(
                assert_timeline(&live, self, epoch, initial_audit, &additions),
                at_v4
            );
            assert_evidence(&live, self, epoch, &[&old_pack], &[&pending]);
            eprintln!("chapter rework: v3/v4 append exact history and audit sources; byte-identical v4 cannot revive old pack or v2 author-reviewed suggestion");
            let manifest = live
                .prepare_writing_pack(book, target, 0, 4, epoch)
                .unwrap();
            let fresh = live.get_writing_pack(book, &manifest.id).unwrap();
            assert_ne!(old_pack.id, fresh.id);
            assert_eq!(
                (fresh.chapter.revision, fresh.chapter.body.as_bytes()),
                (4, story.accepted.body.as_bytes())
            );
            (pending, fresh, at_v4)
        };

        let input = draft(&fresh_pack, &story.rework.fresh_body, "clock-rework-fresh");
        assert_ne!(input.new_body, pending.input.new_body);
        assert_ne!(input.new_body.as_bytes(), story.accepted.body.as_bytes());
        let fresh_id = propose(&input, 4);
        let source_v3 = format!("history:{}", original.id);
        let source_v4 = format!("history:{}", adopted.id);
        let source_v5 = format!("strict:{fresh_id}");
        let additions = [
            (&*story.target.original_body, &*source_v3),
            (&*story.accepted.body, &*source_v4),
            (&*story.rework.fresh_body, &*source_v5),
        ];
        let (fresh_receipt, committed) = {
            let mut live = store.lock().unwrap();
            let candidate = live.get_constrained_proposal(book, &fresh_id).unwrap();
            assert_eq!(candidate.input, input);
            assert!(candidate.author_review.is_none());
            assert_eq!(
                assert_timeline(&live, self, epoch, initial_audit, &additions[..2]),
                at_v4
            );
            assert_error(
                live.accept_reviewed_constrained_proposal(book, &fresh_id, 0, 4, epoch, 1, false)
                    .unwrap_err(),
                "writing_constraint",
                "请先保存逐项作者审阅",
            );
            assert_eq!(
                assert_timeline(&live, self, epoch, initial_audit, &additions[..2]),
                at_v4
            );
            assert_evidence(
                &live,
                self,
                epoch,
                &[&old_pack, &fresh_pack],
                &[&pending, &candidate],
            );

            let review = live
                .save_constrained_review(book, &fresh_id, None, epoch, author_items.clone())
                .unwrap();
            assert_eq!(
                (
                    review.revision,
                    review.chapter_revision,
                    review.policy_revision
                ),
                (1, 4, 0)
            );
            assert_eq!(&review.items, author_items);
            assert_eq!(
                assert_timeline(&live, self, epoch, initial_audit, &additions[..2]),
                at_v4
            );
            let receipt = live
                .accept_reviewed_constrained_proposal(
                    book,
                    &fresh_id,
                    0,
                    4,
                    epoch,
                    review.revision,
                    false,
                )
                .unwrap();
            assert_eq!(
                (receipt.decision.as_str(), receipt.accepted_revision),
                ("accepted", Some(5))
            );
            assert_eq!(receipt.author_review.as_ref(), Some(&review));
            assert_evidence(
                &live,
                self,
                epoch,
                &[&old_pack, &fresh_pack],
                &[&pending, &receipt],
            );
            let committed = assert_timeline(&live, self, epoch, initial_audit, &additions);
            committed.assert_appends_one(&at_v4);
            (receipt, committed)
        };
        drop(session);
        drop(broker);
        drop(store);

        let mut reopened = Service::open(database).unwrap();
        assert_evidence(
            &reopened,
            self,
            epoch,
            &[&old_pack, &fresh_pack],
            &[&pending, &fresh_receipt],
        );
        assert_eq!(
            assert_timeline(&reopened, self, epoch, initial_audit, &additions),
            committed
        );
        write_chapter_txt(
            &mut reopened,
            book,
            target,
            story,
            &format!("{}\r\n\r\n", story.rework.fresh_body),
            &dir.join("chapter-rework-v5.txt"),
            184,
        );
        eprintln!("chapter rework: fresh v4 review reaches v5; isolated SQLite close/reopen and 184-byte real TXT (no overwrite) passed");
    }
}
