# Disclosure predecessor move acceptance plan

> **For agentic workers:** Follow the existing depth-first acceptance plan; review independently before packaging.

**Goal:** Reproduce the effect of changing directory order without rewriting the disclosure or target.

**Architecture:** Reuse the original `setting-disclosure.json` in the existing Rust integration suite. Run the same saved review in two isolated SQLite libraries, moving only the order in one.

**Constraints:** Do not infer in-world chronology from directory order, change product behavior, or add models or native verification.

- [x] Prove both branches froze and can read the disclosure, and recorded the same author approval before any move.
- [x] Use the unchanged branch as a positive accept control; in the moved branch verify unchanged bytes, revisions, histories and epoch, then require the precise predecessor error and a non-mutating denial.
- [x] Verify the earlier frozen text remains readable as history. The changed directory and old selection must disagree; explicitly update the policy, freeze a new pack without the disclosure and verify its new suggestion has no review and cannot be accepted without one.
- [x] Reopen SQLite, verify durable observations, record focused/full Rust results and caveats in the public acceptance index.
