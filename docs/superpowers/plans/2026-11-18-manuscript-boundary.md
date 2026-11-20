# Manuscript boundary implementation plan
> Use subagent-driven-development with independent task and final reviews.
Goal: Prove literal manuscript content cannot grant server authority.
Architecture: One named real MCP pipe fixture using existing child, Broker and isolated Service; no model or product changes.
Spec: ../specs/2026-11-18-manuscript-boundary-design.md
Global constraints: cloud only, no Windows/user data/model/transmission; preserve raw manuscript, single-writer semantics, no new APIs.
Review focus: exact source bytes; denied error category not generic failures; foreign secret absent; legitimate pending proposal remains unaccepted; persisted histories unchanged; no model-behavior claims.
- [x] Task1 new fixture module and harness registration: setup/save, raw readback, explicit attacks, legitimate pending positive control, reopen invariants. Docs and focused tests; independent review.
- [ ] Final independent review, full Rust/UI/build/QA; replace same Library.
