# R1-beta Research Summary

Agent: R1-beta. Topics covered: D1 (file watcher), D2 (WSL boundary),
E1/E2/E3 (SQLite + FTS5 + alternatives), F1/F2 (policy prior art).

ASCII only. All sub-docs are in `docs/research/`. Source URLs cited
inline in each sub-doc.

## One-line recommendation per topic

| Topic | File | Recommendation | Confidence |
| --- | --- | --- | --- |
| D1 file watcher | `file-watcher.md` | `notify` 8.2 + `notify-debouncer-full` 0.7 with explicit per-target transport (native inotify on Linux/WSL-native, `PollWatcher` for 9P, `ReadDirectoryChangesW` on Windows). Caveat: must watch parent dir for create-after-start and rotation; FSEvents has a 32-subpath coalescing limit and is deferred to non-MVP. | High |
| D2 WSL boundary | `wsl-boundary.md` | Treat native WSL FS (`/home/...`) as normal Linux; force polling on `/mnt/c/...` (9P). inotify on 9P is a known Microsoft issue open since 2019 (microsoft/WSL#4739). Detect via `/proc/self/mountinfo` at probe construction, NOT via runtime back-off. | High |
| E1/E2/E3 storage | `sqlite-fts5.md` | `rusqlite` 0.39 with the `bundled` feature (ships FTS5 by default) + `refinery` 0.9 for migrations. Use WAL + batched single-writer transactions for the event-store lane. Defer libsql, sled, redb, fjall. | High |
| F1/F2 policy | `policy-prior-art.md` | Advisory enforcement in MVP: declarative policy file, cap-std `Dir` handles for FS access, audit log as source of truth. Roadmap kernel-enforcement via Landlock (already enabled in WSL2 since kernel 5.15.57.1) and seccomp-bpf (`seccompiler`). | High |

## Top 3 findings (across all topics)

1. **inotify on WSL2's `/mnt/c` does not work at all** - it silently
   accepts the watch and never delivers events. Microsoft has tracked
   this as microsoft/WSL#4739 since 2019 with no fix. Terminal Commander
   MUST detect 9P mounts at probe-construction time and force the
   polling backend. Silent-fail mode is the worst possible failure for
   a security-adjacent product. See `wsl-boundary.md` section 2.2.

2. **Landlock IS available on WSL2** as of kernel 5.15.57.1 (2022) and
   in every WSL2 kernel since. The current WSL2 kernel branch is
   linux-msft-wsl-6.18.y. This means TC's future kernel-enforcement
   roadmap covers WSL2 alongside bare-metal Linux. See
   `policy-prior-art.md` section 2.4 and `wsl-boundary.md` section 6.

3. **FTS5 ships in rusqlite's `bundled` feature without any extra flag**.
   The bundled SQLite is compiled with `SQLITE_ENABLE_FTS5` enabled, so
   the search story for probe output is a free pickup with the
   recommended database choice. No need to install a separate FTS engine
   or hunt for the right system SQLite. See `sqlite-fts5.md` section 1.1
   and section 2.

## HALT-WORTHY findings

None of the four research topics surfaced a HALT-level blocker for the
locked TC02 scope. All four have clear MVP-shippable recommendations.

Two findings deserve emphasis (not halt, but "do not silently accept"):

- **WSL 9P inotify**: this is a behavioral cliff, not a performance
  cliff. Any TC build that defaults to inotify-everywhere on WSL2 ships
  broken. Must be designed in, not patched in. ARCH should treat this
  as a TC18 acceptance criterion.

- **Policy is advisory in MVP**: the term "policy enforcement" must not
  be marketed as kernel-level for the MVP. If the product copy or spec
  says "TC enforces X," it should be qualified as "advisory enforcement
  in the daemon; kernel enforcement is a documented future hardening
  goal." See `policy-prior-art.md` section 7.3 for the recommended
  framing.

## Reclassifications for SOURCE_MAP

Pre-confirmed context entries that R1-beta has moved from "inference"
to evidence-backed:

- **Rust + notify + SQLite** as the embedded stack: now backed by
  cited crate versions and feature flags. Evidence: docs.rs and
  GitHub repos for each crate.
- **WSL2 file probes must use polling for /mnt/c**: previously a
  reasonable assumption; now backed by microsoft/WSL#4739 and the
  notify crate's own documentation.
- **FTS5 viability for embedded search**: previously a candidate; now
  confirmed shipped in rusqlite bundled.

New entries R1-beta would like SOURCE_MAP to capture as evidence-backed:

- **Linux Landlock minimum kernel** is 5.13 (June 2021); ABI v4 (network
  controls) needs newer kernels but the rust-landlock binding handles
  ABI-version fallback. Evidence:
  https://docs.kernel.org/userspace-api/landlock.html
- **WSL2 kernel landlock baseline**: 5.15.57.1 (mid-2022). Evidence:
  Microsoft's WSL kernel release notes.
- **macOS FSEvents 32-subpath coalescing limit**: a real behavioral
  constraint that any future macOS probe design must accommodate.
  Evidence: rb-fsevent README (long-stable description of upstream
  semantics). Apple's own developer docs corroborate but I could only
  fetch the TOC page, so confidence on the qualitative behavior is
  high, confidence on the specific "32" number is medium pending a
  primary-source citation. Flagged for follow-up.
- **sled is functionally abandoned for production**: last release
  2021-09-12, repo's own README redirects reliability-critical
  workloads to SQLite. Not archived but not safe. Evidence:
  https://github.com/spacejam/sled

## Files written

All under `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\`:

- `file-watcher.md` - D1, file watcher landscape.
- `wsl-boundary.md` - D2, WSL2 native-vs-9P semantics.
- `sqlite-fts5.md` - E1/E2/E3, embedded store choice.
- `policy-prior-art.md` - F1/F2, policy enforcement primitives.
- `_R1-beta-summary.md` - this file.

## Open items deferred to other agents or future research waves

- **Quantitative performance baseline** for rusqlite WAL on TC's
  target hardware: not measured. Tag for whoever owns benchmarking
  (R1-gamma?).
- **macOS native probe design**: deferred per pre-confirmed scope.
  FSEvents details captured in `file-watcher.md` for that future
  work but not designed-against.
- **Cross-host federated audit**: not in MVP. If it ever enters
  scope, revisit libSQL.
- **Encryption-at-rest** for the audit store: deferred until policy
  explicitly demands it; rusqlite has `sqlcipher` available behind a
  feature flag.
- **Apple's primary-source confirmation** of the FSEvents 32-subpath
  coalescing limit: corroborated across multiple bindings but the
  Apple dev docs I tried only returned the introduction TOC. Low
  priority.
