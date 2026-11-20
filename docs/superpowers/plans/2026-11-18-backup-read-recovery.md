# Backup read recovery plan
> Use subagent-driven-development, independent task/final review.
Goal: Escape hung readonly backup status waits without treating unresolved work as failure/repeating writes.
Architecture: local cancellable current/status runs and generation fences, backend authority unchanged.
Spec: ../specs/2026-11-18-backup-read-recovery-design.md
Global: noWindows/userPC/manuscriptnetwork; stoppingreadnevercancelswrite; unresolvedblocksclose/mutation; releaseawaitstack; preserveevidence.
Reviewfocus: initialcurrent/manualstatus/fallbackcurrent; rapidstopretry; pendingwrite; nullcurrent resolution; late/unmount/reloadonce; actualsaveafterfailed/cancelledbackup.
- [x] Task1 reproduce, minimalreadstop/UIcopy and tests, realServicecontinuation/reopen; focusedchecks/taskreview.
- [ ] Finalreview/fullRust/UI/build/QA/sameLibrary.
