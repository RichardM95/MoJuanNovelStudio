# Chapter rework implementation plan
> Use superpowers:subagent-driven-development, separate task/final review.
Goal: Prove original accepted chapter can safely be rolled back and rewritten without reviving obsolete suggestions.
Architecture: Extend original Rust acceptance with existing scoped history and writing APIs, plus independent actual-App mocked UI flow.
Tech: Rust/SQLite/real stdio, React/JSDOM. Spec: ../specs/2026-11-18-chapter-rework-design.md
## Global constraints
Cloud only; no Windows/user devices/model/transmission. No product APIs or schema growth. Preserve original raw text, revisions and unknown-result protections. Single writer; evidence levels separate.
## Review focus
ABA raw-body equality cannot satisfy old revision; chapter restore does not change epoch. Failed restore must not partially write history. Old accepted receipt remains historical. Dirty author text must not vanish on history navigation. Exact-byte export not inferred from textarea.
## Task 1: original rework acceptance
Files: src-tauri/tests/fixtures/single_chapter_acceptance.rs or adjacent fixture, tests/SingleChapterAcceptance.test.tsx, docs/ACCEPTANCE.md, docs/VERIFICATION.md, qa/acceptance/README.md.
- [x] Add v2 pending reviewed suggestion; wrong CAS/wrong historical chapter/injected restore failure preserve exact state.
- [x] Restore v1 at v3 then accepted body at v4; verify source/audit/epoch and rejection of old pack/proposal despite equal body.
- [x] Fresh v4 pack/review accepts v5, exact unique history, close/reopen and actual TXT.
- [x] Actual App mocked original-story history confirmation and unsaved-draft protection; preserve existing tests.
- [x] Focused tests and sensitivity controls; independent task review.
## Finish
- [x] Independent final review, full Rust/UI/build/QA regressions.
- [ ] Evidence docs and same Library replacement, no private artifacts.
