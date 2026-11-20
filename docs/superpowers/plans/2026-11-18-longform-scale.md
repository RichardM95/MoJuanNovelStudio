# Longform scale plan
> Use subagent-driven-development, independent task/final review.
Goal: Measure representative longform revision growth and validate data integrity across JSON limit and SQLite recovery.
Architecture: opt-in Rust existingAPI harness + Python stdlib resource runner, isolated workspace fixtures; no productchanges.
Spec: ../specs/2026-11-18-longform-scale-design.md
Global: cloud only/noWindows/userdata/network; deterministicgeneratedfiction not literaryproof; explicit resourcecaps; fullcontentvalidation; defaulttestsnotburdened.
Review focus: RSS per child vs parent cumulative; profile/compilation separation; snapshots expected growth; no hidden SQL bypass; untouched recoveryfile validatedbefore open; timeout/failure honestly incomplete.
- [x] Task1 tool/harness with bounded deterministic24chapter workload, checkpoints1/32/128, operation timing/RSS, 3backuprestore rounds if budget, exactdata/CAS/stale/JSON controls. Record actual device/results/limits; independent review.
- [ ] Final independent review and standardfullregression, sameLibrary source delivery excluding datasets.
