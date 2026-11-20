# Provenance depth plan
> Use subagent-driven-development, independent task and final review.
Goal: Trace original story evidence through setting/source/location/deletion changes and explicit renewal.
Architecture: Isolated branches from verified v2 export, existing Service/Broker and independent actual-component mocks. No product APIs.
Spec: ../specs/2026-11-18-provenance-depth-design.md
Global: Cloud only, no Windows/real manuscripts/model/network, preserve exact frozen evidence/history, distinguish source and occurrence semantics, no silent status repair.
Review focus: unchanged quote but changed revision; movement metadata versus source revision; unavailable source still traceable; author explicit renewal; stale proposal cannot be revived by new evidence; actual modeled UI versus real persistence.
- [x] Task1 four-branch original matrix, explicit source/occurrence renewal positive, reopen persistence; component mocked source-state tests; focused verification and docs. Independent review.
- [ ] Final independent review/full Rust/UI/build/QA, same Library delivery.
