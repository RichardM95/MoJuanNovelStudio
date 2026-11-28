# Bundled SQLite maintenance (2026-11-18)

## Decision and scope

Pin `rusqlite = 0.41.0`, with registry-resolved `libsqlite3-sys = 0.39.1`
in both `src-tauri/Cargo.lock` and `qa/longform-scale/Cargo.lock`.
This is a compatible maintenance-line upgrade, **not the smallest version-number
change**: rusqlite 0.39.0 bundled SQLite 3.51.3, the first mainline WAL-reset fix;
we selected the later official SQLite 3.54.1 maintenance release including later
fixes. Registry entries were non-yanked when checked. No vendored C replacement,
checksum bypass or dependency-source substitution was used.

- [Official WAL-reset advisory](https://sqlite.org/wal.html#the_wal_reset_bug)
- [Official online backup API](https://www.sqlite.org/backup.html)
- [rusqlite 0.41.0 release](https://github.com/rusqlite/rusqlite/releases/tag/v0.41.0)
- [SQLite 3.54.1 release](https://sqlite.org/releaselog/3_54_1.html)

This is defensive maintenance, not evidence that this application suffered data
corruption. Managed Services retain the single-writer mutex/sidecar design.
External programs and hard-link aliases remain outside that complete guarantee.
Windows build/runtime validation remains paused.

## Verified identity

Linux build: Rust 1.91.1; crate minimum Rust versions are 1.89.0 (rusqlite)
and 1.88.0 (sys). Runtime `sqlite_version()` and `rusqlite::version()` agree:

```
3.54.1
2026-10-16 14:45:20 555e07ef33f69a0957d24d8e555b702464a131d6f1607f4c3c1e4551e8e6ca76
```

Registry lock checksums:
- rusqlite: `9c026b4444579e8d754556c6c75d040d69c4a44283cdb648ab8a8d5965c2cfa8`
- libsqlite3-sys: `14a5f01b87b92705d07b433c06f96cbc764637d81a0bffb5734ce17cf0f9a2b3`

Existing explicit `bundled`, `backup`, `hooks`, `limits` stay enabled. The new
`fallible_uint` Rust feature restores the previously default checked u64/usize SQL
conversions; tests reject negative reads and values above signed SQLite INTEGER.
No new SQLite extension feature was requested. Upstream default cache and
wasm-target backend features remain defaults; wasm-only lock entries do not mean
that the native Linux executable links a WASM engine. Verify target resolution
with `cargo tree -e features -i rusqlite` and `-i libsqlite3-sys`.

## Compatibility changes

Upstream hook setters now require mutable connections and return Results. Hook
registration and limit setup errors propagate, rather than silently continuing.
A restore transaction borrows through a cleanup guard which is created before
registration. Early returns drop the transaction before clearing its progress
hook. Cleanup installs a null callback, including on failure; it does not add
unsafe code or weaken authorization. Export authorizer tests likewise explicitly
check successful hook installation.

## Reproducible checks

```
cargo test --locked --manifest-path src-tauri/Cargo.toml
cargo test --locked --manifest-path src-tauri/Cargo.toml --test sqlite_runtime -- --nocapture
cargo build --locked --manifest-path qa/longform-scale/Cargo.toml
python3 scripts/longform_scale.py --binary <built-mojuan-longform-scale> --profile dev --smoke
yarn test
yarn build
```

`sqlite_runtime` verifies the maintained engine floor (it failed on old 3.46.0),
checked unsigned conversions, same-path Service exclusion in two threads, and a
real WAL-only committed revision. A read snapshot holds the old main-file state;
a bare copy is proven stale while the production online backup restores the new
body/history and reopens. This is **not** a probabilistic reproduction of the
upstream WAL-reset race.

`qa/sqlite_compatibility.rs` is a standalone cross-engine helper. In an isolated
checkout of pre-upgrade `da2f1a0`, build `novel_core` and compile the helper with
`rustc --edition=2021 ... -L dependency=<target/debug/deps>` and explicit
`--extern novel_core=<rlib> --extern rusqlite=<rlib>` from Cargo JSON artifacts.
Run `create <new-empty-fixture-directory>`. Build the upgraded checkout and
compile it against those exact upgraded artifacts; run `check <fixture-copy>`.
Never guess artifact hashes when multiple versions share a target directory.

The observed old engine was SQLite 3.46.0 source ID
`2024-05-23 13:25:27 96c92aba00c8375bc32fafcdf12429c58bd8aabfcadab6683e35bbb9cdebf19e`.
The JSON equality check covers exported library data and IDs, not the live
Service epoch (which is not independently captured by this fixture).
The helper checks exact exported old library data, retained pending legacy
proposal, history labels, Unicode/CRLF, continued writing and reopen, then old
SQLite backup restore. Canonical restore intentionally omits legacy
`edit_proposals` (existing behavior), whereas opening the original library retains
them. No schema migration or user database is needed for this dependency upgrade.

Scale smoke is only 24 chapters, two saves and one restore round. It verifies the
standalone lock/build path, not a new long-book benchmark. No native GUI or new
Windows evidence is claimed.
