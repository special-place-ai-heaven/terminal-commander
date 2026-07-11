# Terminal Commander — Independent Adversarial Review of the Audit-Remediation Batch

> **Report type:** report-only, adversarial. No application source was modified. No commit / push / PR /
> stash / reset. The only file written is this report.
> **Reviewer:** Claude Code / Opus 4.8 (1M), Windows 11 (10.0.26200, x86_64), ultracode orchestration
> (16-agent verify→challenge workflow + independent hand-verification of every load-bearing fact).
> **Independence:** every conclusion below was reproduced from the live source at HEAD `ac15f8b` and the
> uncommitted working tree. The implementing agent's code comments, its `specs/002-dogfood-remediation/`
> evidence, and the original audit's conclusions were treated as claims to verify, not as evidence.

---

## 1. Executive verdict

**READY WITH FOLLOW-UPS.**

The 23-path remediation is well-engineered, compiles and lints clean on **both** OS gates, passes every
new and pre-existing test on **both** platforms, and correctly closes the three narrowest defects it
targets (cancel-metrics preservation, the explicit-stop file-watch liveness, and the ADS/case/separator
read bypass on the default-deny suffix list). The path-policy hardening is genuinely good: I could not
find a single **read** bypass that exfiltrates one of the documented sensitive files, and the ordering
(canonicalize → gate → open) is correct and TOCTOU-safe on the read side.

It is **not** a clean "READY" because five real gaps survived adversarial challenge, two of which
(**P1**) mean a claimed goal is only *partially* achieved, and because two of the six reclassifications
are shakier than stated:

- **P1 — Claim 3 is incomplete:** the file-watch liveness fix reaps `self.live` **only on explicit
  `stop()`**. A watch whose probe dies involuntarily is never reaped, so `is_live()` reports a dead
  watch as `Running` — the *same* DEF-05 bug, re-triggered by natural death.
- **P1 — Claim 4 is incomplete:** synthetic lifecycle events (`command_exited`/`command_failed`/cancel)
  are built by `build_exit_event` which **hardcodes `source_type = Process`** and never flows through
  the newly-stamped `process_frame`, so a **PTY (terminal) command's exit event is emitted mislabeled
  `process`** — the identical attribution class DEF-05 is about, for lifecycle events instead of match
  events.
- **P1 (pre-existing, doc-backed) — Claim 6 / DEF-04:** the reclassification "unrestricted DeveloperLocal
  reads are intentional/documented" **contradicts POLICY.md §2.1**, which documents `developer_local`
  reads as confined to `$HOME/projects/**`, `$REPO_ROOT/**`, and an operator allow-set, and explicitly
  **denies** "file reads outside the allow-set." The code's `read_allow`-empty-means-allow default is
  *more permissive than the binding spec*. The two paths the original audit flagged (`hosts`,
  `~/.claude/CLAUDE.md`) are outside `$HOME/projects`/`$REPO_ROOT` and per §2.1 *should* be denied. The
  remediation didn't introduce this divergence, but the reclassification wrongly dismisses it.

None of these blocks *this diff* from landing — the diff itself is sound and additive — but they should
be tracked as fast-follows before the remediation is called "audit-complete," and the DEF-04
reclassification needs either a code fix or a POLICY.md reconciliation.

**No P0.** No crash, no data loss, no read-exfiltration of a documented sensitive file, no compile/lint
failure, no test regression on either platform.

### Severity counts (post-adversarial-challenge, de-duplicated)

| Severity | Count | Items |
|---|---:|---|
| **P0** | 0 | — |
| **P1** | 3 | watch involuntary-death not reaped (C3); lifecycle-event `source_type` mislabel (C4); DEF-04 reclass contradicts POLICY.md §2.1 (C6, pre-existing) |
| **P2** | 3 | default-deny suffix list ⊊ SECURITY.md §5 documented set (C1); deny_extra/allow-list not normalized like the suffix check (C1); write-path 8.3 / Unicode-homoglyph normalizer gap (C1, deferred-Windows-only) |
| **P3** | 6 | cancel-metrics off-by-one on redundant-stop-after-exit (C2); `audit_since` ungated global trail (C5); `cursor` non-lenient vs `limit` lenient (C5); fixture examples not asserted by a live test (C5); DEF-03 `-32603 [Internal]` taxonomy wart (C6); source-attribution test coverage gaps (C4) |

---

## 2. Scope and methodology

### 2.1 Exact changed-file scope reviewed (23 paths vs HEAD `ac15f8b`)

```
crates/daemon/src/command.rs                 crates/mcp/src/lib.rs
crates/daemon/src/file_watch.rs              crates/mcp/src/main.rs
crates/daemon/src/ipc/handlers/common.rs     crates/mcp/src/surface.rs
crates/daemon/src/policy.rs                  crates/mcp/src/surface_list.rs
crates/daemon/src/subscriptions/pull.rs      crates/mcp/src/tools.rs
crates/daemon/tests/command_status_lifecycle.rs
crates/mcp/src/facades.rs                    crates/mcp/tests/daemon_unavailable_envelope.rs
crates/probes/src/file.rs                    crates/mcp/tests/mcp_live_daemon.rs
crates/probes/src/noise_pipeline.rs          crates/mcp/tests/mcp_stdio.rs
crates/probes/src/process.rs                 scripts/smoke/test-all-mcp-tools.py
crates/probes/src/pty.rs                     tests/fixtures/contracts/mcp-tool-fixture-map.v1.json
tests/fixtures/contracts/mcp-tools/system_discover.v1.json
tests/fixtures/contracts/mcp-tools/audit_since.v1.json  (new, untracked)
```

**Scope note (important):** `specs/002-dogfood-remediation/` is **committed at HEAD** (`git status specs/`
is clean) and describes a much broader US1–US9 remediation (facade strictness, argv-smuggle closure,
directory listing, append, compact/delta). That bulk is already merged. The 23 uncommitted paths are a
**distinct, later slice** — precisely the five audit fixes (claims 1–5) plus the reclassifications
(claim 6). The daemon-side `audit_since` handler, its store, and IPC DTOs **pre-exist at HEAD and are
NOT in this diff**; the remediation only *surfaces* them via the MCP adapter.

**Deferred-verification note (implementer's own honesty, corroborated):** `evidence-report.md` states the
running MCP is the installed v0.1.72, which does **not** contain these changes, so a live
agent-perspective pass through the *installed* adapter was not performed. My review is likewise unable to
drive the new behavior through the live MCP tools; both rest on real-daemon + real-IPC automated tests,
not installed-adapter dogfood.

### 2.2 Method

SymForge (Ready, 643 files / 16,122 symbols indexed) was the primary code-intel path; `git diff HEAD`,
raw `Read`, and `rg` were used where exact text/git evidence was needed. A 16-agent adversarial workflow
ran one high-effort verifier per claim dimension + a dedicated Windows bypass-matrix agent, then
adversarially *challenged/refuted* every non-cosmetic finding and every "clean" verdict. **Every P1/P2
conclusion below I also re-derived by hand from the source** — the workflow findings are corroborated,
never taken on faith. Where the workflow's first-pass reviewer and its refuter disagreed (DEF-04, the 8.3
write bypass, the cancel-metrics "clean" verdict), I read the decisive file myself and adjudicated.

---

## 3. Validation results (commands, exit codes, both OS gates)

Per CONTRIBUTING.md §6.1 this change touches `cfg(windows)`/`cfg(unix)` code **and** tests, so both gates
were run. All commands run against **current-source builds**, never the stale installed binary.

| # | Command | Platform | Exit | Result |
|---|---|---|---:|---|
| V1 | `cargo fmt --all -- --check` | Windows | 0 | clean |
| V2 | `cargo clippy --workspace --all-targets -- -D warnings` | Windows | 0 | clean (gate flags: no `--all-features`) |
| V3 | `cargo nextest run -p terminal-commanderd -p terminal-commander-mcp -p terminal-commander-probes` | Windows | 0 | 526 passed, 1 skipped (1 leaky) |
| V4 | `cargo nextest run -p terminal-commander-mcp` (full) | Windows | 0 | 132 passed, 0 skipped |
| V5 | `cargo clippy -p terminal-commanderd -p terminal-commander-probes -p terminal-commander-mcp --all-targets -- -D warnings` | **WSL/Linux** (1.95.0) | 0 | clean — **exercises the `cfg(unix)` `path_default_denied` branch** |
| V6 | `cargo nextest run … -E '<affected filter>'` | **WSL/Linux** | 0 | 31 passed, 758 skipped |

**New/affected tests verified by name (seen, not inferred from an aggregate green):**

- `policy::tests::default_deny_path_normalizes_windows_aliases` — PASS (Windows; `#[cfg(windows)]`,
  correctly absent on Linux)
- `ipc::server::handlers::common::tests::native_sensitive_path_is_denied_before_file_access` — PASS
  (both platforms)
- `subscriptions::pull::tests::stopped_file_watch_reports_stopped_liveness` — PASS (both platforms)
- `tools::tests::catalogue_lists_fifty_one_live_tools`, `fixture_map_matches_live_tool_catalogue`,
  `discover_advertises_exactly_the_catalogue_no_drift` — PASS (the catalogue/fixture no-drift trio)
- `noise_pipeline::tests::password_prompt_bypasses_dedupe` + `file::tests::file_probe_scan_once_existing_file`
  (source-type asserts) — PASS
- `tools::tests::*daemon_unavailable_envelope*` — PASS
- Additionally surfaced on Linux and passing: `sensitive_path_default_deny_paths_all_variants`,
  `file_read_window_denies_symlink_to_default_deny_target`, `file_watch_denies_symlink_to_default_deny_target`,
  and the **live-MCP e2e** `file_write_denies_sensitive_path_through_mcp` /
  `file_read_window_denies_sensitive_path_through_mcp` — these prove claim-1 denial is wired through the
  full MCP boundary, not just unit-tested.

**Environmental limitation:** the one nextest "skipped" is the live-daemon integration case (needs a
running daemon/installed adapter); consistent with the deferred-verification note. `mcp_live_daemon`
self-gates on daemon availability. No test *failed* on either platform.

---

## 4. Findings, ordered by severity

### P1-1 — Claim 3: file-watch liveness reaps `self.live` only on explicit `stop()`; involuntary probe death re-triggers the exact DEF-05 bug

- **Location:** `crates/daemon/src/file_watch.rs:236` (`is_live`) + `crates/probes/src/file.rs:~450`
  (probe `run()` returns `Err(Io)` on any non-`NotFound` IO error) + `crates/daemon/src/subscriptions/pull.rs:436`.
- **Evidence:** `is_live` is `self.live.read().contains_key(&watch_id)` — pure map presence. `self.live`
  is mutated at exactly two sites: insert in `start()` and remove in `stop()`. There is **no waiter,
  reaper, or `JoinHandle` await** on the `FileProbe` task (confirmed by grep for
  `tokio::spawn|join|is_finished|reap|waiter` in `file_watch.rs`: none tied to liveness cleanup).
- **Failure scenario:** a live file-watch's probe dies involuntarily — the watched file is replaced by a
  directory, an `EACCES`/`EIO` occurs, the volume unmounts — so the probe task exits `Err(Io)`. The
  `WatchBinding` stays in `self.live` forever. `is_live()` returns `true`; `liveness_for` reports
  `Running`. This is DEF-05 (a not-actually-live watch reported `Running`) exactly, just reached by
  natural death instead of an explicit `watch_stop`.
- **Adversarial status:** the refuter tried to neutralize this and **could not** — confirmed real, held
  at P1. The remediation's stated goal ("a stopped file watch reports Stopped, not Running") is achieved
  only for the *explicit-stop* path.
- **Recommended correction:** reap `self.live` when the probe task terminates for any reason — either
  spawn a small awaiter that removes the entry on `JoinHandle` completion, or have `is_live()` consult
  the probe's terminal state (as the PTY/command arms do via the job ledger) rather than trusting map
  presence. Add a regression test that lets a watch die involuntarily and asserts `Stopped`.
- **Also (P3, latent):** `pull.rs:436` defaults a `None` `job_id` to `Liveness::Running` — re-encoding
  the immortal-`Running` default the fix set out to kill. Currently unreachable, but it should fail safe
  to `Stopped`/`Unknown`, not `Running`.

### P1-2 — Claim 4: synthetic lifecycle events hardcode `source_type = Process`, bypassing the new probe-boundary stamp; a PTY command's exit event is emitted mislabeled `process`

- **Location:** `crates/core/src/job.rs:267` (`build_exit_event` hardcodes `source_type: SourceType::Process`,
  `stream: Meta`); reached via PTY finish/cancel at `crates/daemon/src/pty_command.rs:578/584` →
  `bucket_append` at `:592-593`; and via command exit at `crates/daemon/src/command.rs:1469-1471`.
- **Evidence:** the remediation stamps `source_type` inside `process_frame`
  (`noise_pipeline.rs:144-149`), and all four production `process_frame` call sites pass the correct
  type (file→File, process→Process, pty→Terminal). But `build_exit_event` builds `command_exited` /
  `command_failed` / cancel drafts **directly**, never through `process_frame`, and hardcodes
  `Process`. These drafts are appended to the **same bucket/subscription stream** consumers read. Because
  the PTY path calls the same `finish`/`cancel`, a terminal PTY command's exit event reaches subscription
  consumers labeled `source_type = process`. I searched every plausible re-stamp layer (`bucket_append`,
  `into_signal_event`, `pull.rs`, the file handler) and found none.
- **Failure scenario:** an MCP client runs a PTY command, subscribes, and reads the stream. The
  match/output events are correctly `Terminal` (the fix works), but the synthetic `command_exited` meta
  event for the same PTY job carries `source_type: "process"` — the identical mis-attribution class
  DEF-05 documents for match events, unresolved for lifecycle events.
- **Adversarial status:** confirmed real, held at P1 for the PTY exit-event variant (reaches consumers).
  The file-watch-stop variant (`file_watch.rs:444` → `jobs.finish` → `build_exit_event`) is **P2/latent**:
  the returned draft is discarded (`let _ =`) today, so it is not emitted — but the mislabel is a live
  landmine if that return value is ever routed to `bucket_append` like the PTY/command paths.
- **Recommended correction:** thread the owning probe's `SourceType` into `build_exit_event` (or carry it
  on the job record) so lifecycle events are stamped with the real source kind, closing the attribution
  gap the fix set out to close. Add an end-to-end assertion that a PTY job's `command_exited` event is
  `Terminal`, not `process`.

### P1-3 (pre-existing, doc-backed) — Claim 6 / DEF-04 reclassification contradicts POLICY.md §2.1 and masks a still-open default-profile read-containment gap

- **Location:** `POLICY.md §2.1` (developer_local) vs `crates/daemon/src/policy.rs:1281-1286`
  (`GlobList::allows` returns `true` when `!is_configured()`) + the empty-by-default `read_allow`.
- **The reclassification claims:** "unrestricted general DeveloperLocal reads are intentional… but
  documented sensitive paths must still be denied."
- **What the binding spec actually says (POLICY.md §2.1, verbatim):**
  - *permits:* "read+watch under `$HOME/projects/**`, `$REPO_ROOT/**`, and an operator-listed allow-set."
  - *denies (in addition to default-deny):* "**file reads outside the allow-set without explicit profile
    edit.**"
- **The divergence:** the code's default `developer_local` has `read_allow` empty, and empty means
  *allow-all-except-the-14-suffixes*. That is **strictly more permissive** than POLICY.md §2.1, which
  confines reads to `$HOME/projects` + `$REPO_ROOT` + operator allow-set and **denies** everything else.
  The two paths the original audit flagged (`C:\Windows\System32\…\hosts`, `~/.claude/CLAUDE.md`) are
  outside `$HOME/projects`/`$REPO_ROOT`, so **per §2.1 they should be denied** — yet the code reads them.
- **Failure scenario:** an LLM harness on the default `developer_local` profile reads `~/.claude/CLAUDE.md`,
  a project-adjacent `.env`, a Chrome "Login Data" store, `~/.gnupg/secring.gpg`, `~/.config/gcloud/…`,
  or `/etc/ssh/ssh_host_rsa_key` — none is in the 14 suffixes, none is inside the §2.1 allow-set, and the
  code returns the content. The reclassification labels this "intentional," but the spec labels it denied.
- **Adjudication (my own reading, against the workflow's internal disagreement):** the claim-6 reviewer
  rated this **P1 DISAGREE**; its refuter downgraded to P3 ("DeveloperLocal-default + empty-allow is
  documented intentional"). The refuter did **not** cite POLICY.md §2.1's explicit allow-set + "denies
  file reads outside the allow-set" language. I read §2.1 directly: **the reviewer is correct.** The
  reclassification is not fully doc-backed; it recharacterizes a code-vs-spec divergence as intent.
- **Important caveat:** this gap is **pre-existing** — both the empty-allow=allow default and the §2.1
  confined-read language exist at HEAD; the 23-path diff neither introduced nor widened it. So it is **not
  a regression in this diff**, and it does not block the diff from landing. But the DEF-04 reclassification
  should not be accepted as-is.
- **Recommended correction:** pick one and make code and doc agree. Either (a) make the default
  `developer_local` `read_allow` non-empty (`$HOME/projects/**`, `$REPO_ROOT/**`) so reads are confined
  as §2.1 documents, denying out-of-set reads with a typed `[PathDenied]`; or (b) amend POLICY.md §2.1 to
  state that `developer_local` reads are unrestricted except for the default-deny suffix list, and expand
  that list to the full SECURITY.md §5 set (see P2-1). Option (a) matches the least-privilege intent the
  original audit expected; option (b) is a documented, eyes-open loosening.

### P2-1 — Claim 1: `DEFAULT_DENY_PATH_SUFFIXES` is a strict subset of the SECURITY.md §5 documented default-deny set

- **Location:** `crates/daemon/src/policy.rs:405-420` vs `SECURITY.md:130-166`.
- **Evidence:** the code has 14 flat `ends_with` suffixes: `.ssh/id_rsa`, `.ssh/id_ed25519`,
  `.ssh/id_ecdsa`, `/etc/shadow`, `/etc/sudoers`, `.pgpass`, `.netrc`, `.aws/credentials`, `.aws/config`,
  `.kube/config`, `.docker/config.json`, `.npmrc`, `.pypirc`, `.vault-token`. SECURITY.md §5 mandates a
  **superset** including `~/.ssh/**` (ALL of `.ssh` — the code misses `.ssh/config`, `.ssh/known_hosts`,
  `id_dsa`, and any non-standard key name), `~/.gnupg/**`, `~/.config/gcloud/**`, `/etc/sudoers.d/**`,
  `/etc/ssh/ssh_host_*`, `/etc/ssl/private/**`, `~/.mozilla/**`, `~/.config/google-chrome/**`,
  `~/.config/chromium/**`, `~/.config/op/**` (1Password), `~/.config/bw/**` (Bitwarden).
- **No second layer:** SECURITY.md §5–6 *claims* the daemon "uses `cap-std` `Dir` handles rooted at
  allowed paths." I grepped `crates/daemon/src/` for `cap-std`/`cap_std`/`Dir::open`: **zero usages.**
  There is no cap-std rooting; the 14-suffix default-deny (plus opt-in operator lists) is the *only*
  mandatory read-side containment. (The stale cap-std doc claim is a pre-existing doc bug, noted, out of
  primary scope.)
- **Failure scenario:** on a default `developer_local` engine (empty allow-lists), `file_read` of
  `~/.ssh/id_dsa`, `~/.ssh/config`, `~/.gnupg/secring.gpg`, `~/.config/gcloud/credentials.db`,
  `/etc/ssh/ssh_host_rsa_key`, or a browser credential DB is **not denied** and returns the secret. A
  flat `ends_with` list also cannot express the `**`/`ssh_host_*` wildcards §5 requires.
- **Adversarial status:** confirmed real; the refuter "could not refute it" and traced that no other
  layer neutralizes it in zero-config.
- **Recommended correction:** replace the flat suffix list with the SECURITY.md §5 glob set, compiled via
  the existing `glob_to_regex_src`/`GlobList` and wired as an always-on default deny, matched on the
  normalized canonical form. As shipped, the code and the spec disagree; the DEF-04 reclassification's
  premise ("documented sensitive paths must still be denied") is only ~60% delivered.

### P2-2 — Claim 1: `deny_extra` and the allow-lists do **not** share `path_default_denied`'s normalization, so operator globs are evadable by Windows aliasing

- **Location:** `crates/daemon/src/policy.rs:786` (`deny_extra.matches(&canonical)`) and `:801`
  (`allow.allows(&canonical)`) vs `path_default_denied` at `:777`; `GlobList::matches` at `:1268-1271`
  does `path.to_string_lossy()` with **no** case-fold, separator, verbatim-prefix, or ADS handling.
- **Failure scenario:** operator (developer_local) sets `deny_extra = ["C:/secrets/**"]`. A client
  `file_read` of `C:\SECRETS\data`. `evaluate()` canonicalizes to `\\?\C:\SECRETS\data`.
  `path_default_denied` does not fire (C:/secrets isn't one of the 14). The `deny_extra` glob compiles to
  `^C:/secrets/.*$` and is matched against `\\?\C:\SECRETS\data` — backslashes, the `\\?\` verbatim
  prefix, and case all defeat the regex → **the hard deny is bypassed and the read is allowed.** The same
  raw matching lets an allow-list spuriously miss a legitimately-allowed aliased path (over-deny) or match
  an unintended one.
- **Adversarial status:** confirmed real; "reproduces exactly."
- **Severity nuance:** these lists are **opt-in** (empty by default → allow), so the *default* posture is
  unaffected — this bites operators who configure `deny_extra`/allow-lists and reasonably expect the same
  Windows-alias robustness the remediation just gave the 14 suffixes. The remediation hardened one matcher
  and left the sibling matcher with the exact weakness the audit complained about. Pre-existing at
  baseline, but the remediation's own hardening makes the inconsistency conspicuous.
- **Recommended correction:** normalize the subject once (the `path_default_denied` transform) and match
  `deny_extra`/allow-lists against that normalized form, or normalize inside `GlobList::matches` and
  compile operator globs against the same alphabet (lowercase + `/` on Windows).

### P2-3 — Claim 1: write-path 8.3 short-name and Unicode-homoglyph normalizer gaps (deferred-Windows-only)

- **Location:** `crates/daemon/src/ipc/handlers/common.rs:405` (write joins the **raw** `file_name` tail
  to the canonical parent) + `policy.rs:1090` (`to_ascii_lowercase` folds only A–Z).
- **Evidence:** the READ gate is robust for every vector (see the matrix in §6) because
  `std::fs::canonicalize` requires existence and resolves symlinks, 8.3 short names, and Unicode names to
  the real long NTFS name **before** the suffix check. The WRITE gate canonicalizes only the *parent* and
  appends the raw tail; `evaluate` then re-runs `canonicalize_lexical`, which fails on the non-existent
  target and re-appends the raw tail. So a write target whose sensitive filename is expressed as an 8.3
  short name (`IDRSA~1`) or a Unicode homoglyph that NTFS upcases-equal to an ASCII suffix letter is **not**
  `ends_with`-matched and slips the suffix check, permitting a write/overwrite of an existing sensitive
  file.
- **Severity modifier (verified):** `policy.rs:1340-1346` and ARCHITECTURE.md §10 state the daemon's
  **production target is Linux/WSL; the Windows-native daemon is deferred.** The entire `#[cfg(windows)]`
  normalizer and the write gate's Windows behavior are explicitly a **deferred, non-prod path today.**
  This does not make the gap imaginary (the original audit ran on a Windows daemon), but it bounds it to a
  not-yet-production surface, and the write also requires the `write_allow` list to permit the parent.
  Hence P2, not P1.
- **Recommended correction:** when the Windows-native daemon is productized, resolve the write target's
  real name before the suffix check (or reject 8.3/`~N` tails and normalize via full Unicode case-fold),
  and account for the `\\?\` canonical prefix in policy-glob compilation (already flagged as deferred in
  the code comment).

### P3 findings (brief)

- **P3-1 — Claim 2 cancel-metrics off-by-one on redundant-stop-after-natural-exit.** `command.rs:1575`
  (`b.metrics = metrics.clone()`) runs **unconditionally before** the already-terminal early-return at
  `:1579`. The waiter had set `b.metrics = final_metrics` at `:1482` including a `+1 events_emitted`
  lifecycle bump (`:1472`) that lives only in the local `final_metrics`, not the shared `metrics_live`
  Arc. A redundant `stop()` after natural exit re-snapshots `metrics_live` (which never got the `+1`) and
  clobbers `b.metrics`, so `command_status.events_emitted` drops by **exactly 1**. Frames/bytes
  unaffected; true count still re-readable via `bucket_summary`. Ironic (the cancel-metrics fix introduces
  a tiny counter loss on the idempotent-stop edge) but narrow. *This was missed by the first-pass "CORRECT,
  no findings" review and caught by the adversarial challenge — I confirmed it by hand at
  `command.rs:1563-1592`.* **Fix:** move the `b.metrics = …` publish *inside* the not-yet-terminal branch
  (after the terminal early-return), or take `max` of the existing `b.metrics` and the live snapshot.
- **P3-2 — Claim 5 `audit_since` is ungated.** `handle_audit_since` (`audit.rs:47-77`) applies **no
  policy gate** — it clamps `limit` to `MAX_AUDIT_READ_LIMIT` and returns the full global audit trail:
  every action, decision (including denials), profile, actor, and `subject` **path** for operations the
  calling client never performed. The handler pre-exists at HEAD, but the remediation makes a previously
  MCP-unreachable ungated trail reachable. `subject` is a path (not file content) and `metadata_json`
  carries argv/byte-count metadata (no secrets observed), so it is enumeration/info-disclosure, not
  content leak. **Fix:** gate `audit_since` behind a capability or restrict rows to the caller's own
  actions; at minimum document that the audit surface exposes global operation metadata.
- **P3-3 — Claim 5 param-lenience asymmetry.** `McpAuditSinceParams.cursor` is a strict `u64` while
  `limit` uses `de_opt_usize_lenient` (accepts numeric strings). A client sending `"cursor":"0"` fails
  deserialization while `"limit":"50"` succeeds — inconsistent ergonomics. Cursor behavior is otherwise
  correct (exclusive; `next_cursor` = last row id, or echoes input on empty — matches the fixture). **Fix:**
  make `cursor` lenient too, or document the asymmetry.
- **P3-4 — Claim 5 fixture examples are documentation-only.** `audit_since.v1.json`'s `response_examples`
  (empty / with_rows) are not asserted by any automated test; only the catalogue/name-drift tests
  consume the tool listing. The fixture *shape* is right (`AuditRowWire` fields match), but no test proves
  the live response matches the example. **Fix:** add a live/stdio contract assertion against the fixture
  shape.
- **P3-5 — Claim 6 / DEF-03 taxonomy wart.** DEF-03's reclassification is **correct and consistent** with
  claim-1 (System32 matches none of the 14 suffixes; `deny_extra`/`write_allow` empty → policy Allows →
  write hits the OS ACL → `os error 5` → `-32603 [Internal]`, audited as `"error"`, no byte written). But
  surfacing an OS-ACL denial as `-32603 [Internal]` — the JSON-RPC class reserved for *server bugs* — is a
  mild error-taxonomy wart. Not a defect; a polish item.
- **P3-6 — Claim 4 test coverage gaps.** The new tests prove file-probe match events are all `File` and
  one Terminal match case, but **no test** asserts process/pty match-event attribution end-to-end, and
  **none** covers the lifecycle-event `source_type` (the P1-2 mislabel). The green tests would pass even
  with P1-2 unfixed.

---

## 5. Claim-by-claim verdict

| # | Claim | Verdict | Basis |
|---|---|---|---|
| **1** | Sensitive-path policy denies after robust lexical normalization (case, separators, trailing dot/space, verbatim/device, ADS `::$DATA`); reads+writes rejected before FS touch | **CORRECT WITH GAPS** | The suffix normalizer is correct and the L1363 test passes for the right reason; **no READ bypass of a sensitive file exists** (ADS/verbatim/UNC/8.3/Unicode all denied or FileNotFound-safe on read). Gate runs before open/create/truncate on all of FileRead/FileWatch/FileWrite; read is TOCTOU-safe (gate returns the opened path). Gaps: suffix list ⊊ SECURITY.md §5 (P2-1), operator globs unnormalized (P2-2), write-path 8.3/Unicode (P2-3, deferred-Windows), Unix over-denial (P3 in §7). |
| **2** | Cancelled-command status preserves last live counters instead of resetting to 0 | **CORRECT (with a P3 wart)** | The core DEF-05 counter-reset is fixed: `stop()` publishes `metrics_live` into `b.metrics` before the terminal transition, and `command_status` reads `b.metrics`. Verified consistent for cancel. One introduced off-by-one on `events_emitted` for redundant-stop-after-natural-exit (P3-1). |
| **3** | A stopped file watch reports Stopped, not Running; liveness authority is the live-handle registry | **CORRECT FOR EXPLICIT STOP; INCOMPLETE** | `self.live` is the single authority (start inserts, stop removes synchronously; `is_live`/`list`/`rebind` all read it; `JobId` is `Uuid::now_v7`, never reused → no id-reuse flip; leaf-lock, no inversion). But involuntary probe death is never reaped → DEF-05 re-triggered (P1-1). |
| **4** | Process events → Process, file → File, PTY → Terminal, attributed at the probe boundary without leaking probe knowledge into the sifter | **CORRECT FOR MATCH EVENTS; INCOMPLETE FOR LIFECYCLE EVENTS** | All 4 production `process_frame` sites pass the right type; dedupe key excludes `source_type` (no dedupe change); the one synthetic draft on the path (`password_prompt_draft`, Terminal) only enters via PTY so is never mislabeled. But `build_exit_event` hardcodes `Process` and bypasses the stamp → PTY exit events mislabeled `process` (P1-2). |
| **5** | `audit_since` added as granular tool + compact `status(action="audit_since")` with validation, cursor, filters, limits, schema, daemon-unavailable behavior, catalogue counts, fixtures, tests, discoverability; no sensitive fields exposed | **CORRECT WITH GAPS** | Adapter forwarding is right; `ensure_daemon_available()` first → typed `daemon_unavailable` envelope; 50→51 changed consistently everywhere (catalogue, sorted-names, fixture-map, discover, docs — all no-drift tests green); limit clamps at both wire and store; cursor exclusive and correct; response shape matches the wire type; audit_since present in both full and compact surfaces. Gaps: ungated global trail (P3-2), cursor non-lenient (P3-3), fixture examples unasserted (P3-4). **Practically usable by an LLM harness: yes** (cursor pagination + exact action/decision filters + bounded); minor ergonomic gap — no timestamp filter and no "latest-N" without paging from 0. |
| **6** | Reclassifications DEF-01/02/03/04/07 | **MIXED — 3 AGREE, 1 PARTIAL, 1 DISAGREE** | DEF-01 (cap-field naming = UX preference): **AGREE** (P3) — distinct fields on a flat facade struct, typed self-correcting errors. DEF-02 (grace_ms on start feeds the shutdown ladder): **PARTIAL** — not touched by this diff; I could not confirm a stop-time consumer in the diff, and the original ergonomic point (grace-before-force-kill is the natural param of *stop*) is legitimate. DEF-03 (`-32603 [Internal]` for OS-ACL write is correct, not a fake policy denial): **AGREE** and consistent with claim-1, with a taxonomy wart (P3-5). DEF-04 (unrestricted DeveloperLocal reads intentional/documented): **DISAGREE** — contradicts POLICY.md §2.1 (P1-3). DEF-07 (`remote_denied` already structured typed invalid_params): **AGREE** — `remote_denied_error` (tools.rs:3557) returns `McpError::invalid_params` = JSON-RPC `-32602` with a structured `data` payload `{code, message, details}`; only the human-readable prefix lacks the `[PolicyDenied]` bracket (cosmetic). |

---

## 6. Windows path-policy bypass matrix

Legend: **DENY** = sensitive target correctly denied. **FNF-safe** = returns `FileNotFound`, secret never
opened. **BYPASS** = sensitive file read/written. All rows target a sensitive suffix (e.g. `.ssh/id_rsa`).
READ path = `resolve_and_authorize_file` (`std::fs::canonicalize` before gate). WRITE path =
`resolve_and_authorize_file_write` (parent canonicalized, raw tail joined). Every step traced through the
real normalizer.

| # | Vector | READ | WRITE | Mechanism |
|---|---|---|---|---|
| 1 | lowercase `C:\…\.ssh\id_rsa` | DENY | DENY | `ascii_lowercase` + `\`→`/`; `ends_with .ssh/id_rsa` |
| 2 | uppercase `.SSH\ID_RSA` | DENY | DENY | `to_ascii_lowercase` folds A–Z |
| 3 | forward slashes | DENY | DENY | already `/` |
| 4 | mixed `C:\Users/me\.ssh/id_rsa` | DENY | DENY | `\`→`/` unifies |
| 5 | trailing dot `id_rsa.` | DENY | DENY | `trim_end_matches(['.',' '])` |
| 6 | trailing space `id_rsa ` | DENY | DENY | `trim_end_matches` |
| 7 | dot+space `id_rsa. .` / `id_rsa .` | DENY | DENY | `trim_end_matches` strips a run of both |
| 8 | ADS `id_rsa::$DATA` | DENY | DENY | `split_once(':')` → base `id_rsa` (read: canonicalize also strips) |
| 9 | ADS named `id_rsa:hidden` | DENY | DENY | `split_once(':')` → base `id_rsa` |
| 10 | verbatim `\\?\C:\…\.ssh\id_rsa` | DENY | DENY | `//?/` components inert, `c:` kept by drive guard, suffix matches |
| 11 | device `\\.\C:\…` | DENY | DENY | same shape as 10; `\\.\` write is not a real file-create anyway |
| 12 | UNC `\\localhost\C$\…\.ssh\id_rsa` | DENY | DENY | `ends_with .ssh/id_rsa` regardless of `//localhost/c$` prefix |
| 13 | UNC forward `//localhost/C$/…` | DENY | DENY | same |
| 14 | drive-relative `C:.ssh\id_rsa` | DENY | DENY | read: canonicalize resolves vs drive-CWD then suffix matches; write: raw tail still `ends_with` |
| 15 | rooted no-drive `\Users\me\.ssh\id_rsa` | DENY | DENY | `ends_with` matches after `\`→`/` |
| 16 | 8.3 short `…\IDRSA~1` | DENY | **BYPASS** (write) | read: canonicalize resolves `~1` to real long name → suffix matches; **write: raw `IDRSA~1` tail is not `ends_with .ssh/id_rsa`** → slips suffix check (P2-3; deferred-Windows) |
| 17 | dot segments `.\.ssh\id_rsa`, `..\me\.ssh\id_rsa` | DENY | DENY | read: canonicalize collapses; write: `..` rejected up front (common.rs:325) |
| 18 | Unicode homoglyph / non-ASCII upcase | DENY | **BYPASS** (write) | read: canonicalize resolves to real NTFS name → suffix matches; **write: `to_ascii_lowercase` folds only A–Z, raw homoglyph tail not `ends_with`** → slips (P2-3; deferred-Windows) |
| 19 | reserved device `CON` / `NUL` / `\\.\PhysicalDrive0` | FNF-safe / N-A | N-A | not a sensitive suffix; read canonicalize fails or is a device (no file content); no crash observed |

**Matrix conclusion:** **no sensitive-file READ bypass exists** — the read path's mandatory
`std::fs::canonicalize` resolves every alias (ADS, verbatim, UNC, 8.3, Unicode, dot-segments) to the real
name before the suffix check, or fails `FileNotFound`. The only BYPASS rows are **write-path 8.3 and
Unicode-homoglyph** (rows 16, 18), both on the **deferred, non-production Windows-native write gate**
(P2-3), and both additionally gated by the `write_allow` list. The `default_deny_path_normalizes_windows_aliases`
test genuinely exercises rows 1,2,3,5,6,8,10 and passes for the right reason.

---

## 7. Unix behavior assessment (regression check)

The remediation moved `to_ascii_lowercase()` and `replace('\\', "/")` to run **unconditionally** on all
platforms (baseline did neither — it was `path.to_string_lossy()` then a case-sensitive `ends_with`). The
`#[cfg(windows)]` block only *adds* ADS/trailing-trim on top. Consequences on Unix (case-sensitive fs):

- **Over-denial (P3, fail-safe):** `/home/u/.ssh/ID_RSA` — a *distinct* file from `id_rsa` on Unix —
  lowercases to `.ssh/id_rsa` and is now **denied** for read/write, though it is not the private key.
- **Literal-backslash rewrite (P3, fail-safe):** a legitimate Unix filename containing `\` is
  slash-rewritten and could false-match.

Both are **over-denial** (deny too much — the safe direction), not a security hole, but they are new
behavior the original code did not have, and they can block legitimate access. The Unix gate (V5/V6)
compiles and passes the `cfg(unix)` `default_deny_path_denied` test, so nothing is broken — but consider
gating the lowercase + `\`→`/` transforms behind `#[cfg(windows)]`, since POSIX paths need neither and the
transform trades a non-existent Unix threat for real over-denials. **Recommended correction:** wrap the two
transforms in `#[cfg(windows)]`.

---

## 8. Test-quality assessment and missing coverage

**Strengths.** The catalogue/fixture no-drift trio (`fixture_map_matches_live_tool_catalogue`,
`discover_advertises_exactly_the_catalogue_no_drift`, `catalogue_lists_fifty_one_live_tools`) is exactly
the right guard for the 50→51 surface change and genuinely prevents drift. The `#[cfg(windows)]`
`default_deny_path_normalizes_windows_aliases` test executes real alias inputs and passes for the right
reason. `native_sensitive_path_is_denied_before_file_access` proves gate-before-FS on both platforms. The
Linux-surfaced live-MCP e2e (`file_write_denies_sensitive_path_through_mcp`,
`file_read_window_denies_sensitive_path_through_mcp`) proves claim-1 through the full boundary.

**Missing / false-confidence coverage:**

1. **No involuntary-probe-death liveness test** (P1-1) — the new `stopped_file_watch_reports_stopped_liveness`
   covers only the explicit-`stop()` transition, so it stays green while the natural-death bug remains.
2. **No lifecycle-event `source_type` assertion** (P1-2) — no test asserts a PTY job's `command_exited`
   event is `Terminal`; the P1-2 mislabel is invisible to the suite.
3. **No process/pty match-event attribution test** (P3-6) — only File (all==File) and one Terminal match
   case are asserted end-to-end.
4. **`audit_since` fixture examples are unasserted** (P3-4) — the response shape is documented but not
   verified against a live response.
5. **No `deny_extra`/allow-list Windows-alias test** (P2-2) — the asymmetry has no negative test that
   would have caught it.
6. **No Unix over-denial guard** (§7) — the case-sensitivity change has no test pinning the intended
   (or flagging the unintended) Unix behavior.

---

## 9. False positives / disagreements (with both the original audit and the remediation agent)

- **Disagree with the original audit — 8.3 short-name *read* bypass is NOT real.** A first-pass reviewer
  posited an 8.3 read bypass; the refuter and my own trace show `std::fs::canonicalize` resolves a real
  8.3 alias to its long name before the suffix check, so the read is denied. Only the *write* path (8.3
  tail, deferred-Windows) is affected. Rated P2-3, not a read hole.
- **Disagree with the remediation agent — Claim 2 is not defect-free.** The first-pass claim-2 review
  returned "CORRECT, no findings." The adversarial challenge (and my hand-check at `command.rs:1563-1592`)
  found a real, introduced off-by-one on `events_emitted` (P3-1). Minor, but not "no findings."
- **Disagree with the remediation agent — DEF-04 reclassification.** The reclassification says
  unrestricted DeveloperLocal reads are "intentional/documented." POLICY.md §2.1 documents the opposite
  (confined to an allow-set; out-of-set reads denied). Rated P1-3. *Note:* the workflow's own internal
  refuter tried to downgrade this to P3 without citing §2.1; I read §2.1 directly and side with the
  original reviewer.
- **Agree with the remediation agent — DEF-03, DEF-07, DEF-01 reclassifications are correct** (with the
  minor `-32603` taxonomy wart on DEF-03).
- **Partial on DEF-02** — not touched by this diff; the stop-time-grace ergonomic point remains arguable,
  but it's a P3 preference either way.
- **Agree with the remediation agent — the ADS read bypass is genuinely closed** (three independent
  layers), and the core path-policy ordering is correct.

---

## 10. Ship recommendation and remaining risks

**Ship this diff: YES, as READY WITH FOLLOW-UPS.** The 23-path change is additive, sound, green on both OS
gates, and closes real defects. It introduces no P0 and no read-exfiltration of a documented sensitive
file. Land it, then open follow-ups for:

**Before "audit-complete" (fast-follows):**
1. **P1-1** reap `self.live` on involuntary probe death (+ test).
2. **P1-2** stamp `source_type` on lifecycle exit/cancel events (+ PTY-exit test).
3. **P1-3 / P2-1** reconcile DeveloperLocal read posture with POLICY.md §2.1 **and** expand the
   default-deny list to the full SECURITY.md §5 set (decide code-fix vs doc-amend; these two are one
   decision).

**Polish (P2/P3):**
4. **P2-2** normalize `deny_extra`/allow-lists like the suffix check.
5. **§7** gate the Unix lowercase/`\`→`/` behind `#[cfg(windows)]`.
6. **P3-1** move the cancel-metrics publish inside the not-yet-terminal branch (or `max`).
7. **P3-2** gate or scope `audit_since`; **P3-3/P3-4** cursor lenience + a live fixture assertion.
8. **P2-3** deferred until the Windows-native daemon is productized (already flagged in-code).
9. Correct the stale SECURITY.md cap-std enforcement claim (no cap-std in the daemon today).

**Remaining risks if shipped as-is:** (a) a naturally-dying file-watch appears `Running` to a subscriber
(observability, not data); (b) PTY exit events read as `process` (attribution, not data); (c) under the
**default** MCP profile, an LLM harness can still read credential stores documented as default-deny
(`~/.gnupg`, gcloud, SSH host keys, browser DBs, `~/.ssh/config`) and any path outside `$HOME/projects`/
`$REPO_ROOT` — the highest-impact residual, bounded today by the daemon's Linux/WSL prod target and the
operator's run-as user, but contrary to the project's own §2.1/§5 doctrine. None is a crash or a
guaranteed secret leak; all are containment/observability gaps the remediation's own goals imply should be
closed.

---

## Appendix — the strongest attacks that FAILED (and why)

1. **ADS read of a private key** (`…\.ssh\id_rsa::$DATA`): fails in all three reachable states — canonicalize
   strips the stream (suffix catches base), or the manual `split_once(':')` catches it, or the open fails
   `FileNotFound`. The gate also runs on the exact canonical path the caller then opens (no re-resolution
   window).
2. **8.3 / Unicode / UNC / verbatim *read* of a sensitive file**: `std::fs::canonicalize` resolves every
   alias to the real NTFS long name before the suffix check → DENY.
3. **`watch_id` reuse to flip a stale liveness record to Running**: impossible — `JobId::new()` is
   `Uuid::now_v7()`, monotonic, never reused.
4. **Dedupe corruption from stamping `source_type` early**: the dedupe key excludes `source_type`
   (composed of rule id/version, kind, captures), so stamping before dedupe changes no grouping.
5. **`create_dirs` traversal to build a directory outside the allow-list**: `..` is rejected up front
   (common.rs:325) before `create_dir_all`, and the parent is gated before creation.
