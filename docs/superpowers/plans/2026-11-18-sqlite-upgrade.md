# SQLite upgrade plan
Goal: defensively link fixed official SQLite while preserving existing data/backup behavior.
Spec: ../specs/2026-11-18-sqlite-upgrade-design.md
- [x] Preserve old-engine fixture and prove version guard RED.
- [x] Upgrade only compatible rusqlite/sys dependencies and necessary compile changes, both lockfiles.
- [x] Runtime/source id, WAL backup/restore, same-path/concurrent Service lock, old-engine fixture compatibility; full default tests, standalone scale smoke.
- [ ] Independent task/final review, report limitations and deliver same Library via controller.
