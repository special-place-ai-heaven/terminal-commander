# File Watcher Research (Topic D1)

Research agent: R1-beta. All claims tied to a cited URL. ASCII only.

## Scope

Document the Rust filesystem watcher landscape, OS limits, and the recommended
pattern for Terminal Commander file probes (TC18, TC20). This research is
input to ARCH; it does not commit to any design decision.

## 1. The notify crate family

The crate set is maintained by the `notify-rs` organization. There are three
relevant crates that ship together and version somewhat independently.

### 1.1 notify

- Current published version: **8.2.0** on docs.rs as of the research date.
  Source: https://docs.rs/notify/latest/notify/
- License: MIT / Apache-2.0 (per crates listing on docs.rs).
- Single-process abstraction over OS-native FS events.
- Per-platform backend selection (see section 2).
- Emits raw events: rename pairs are split into separate `from`/`to` events,
  the caller must reconcile them.

### 1.2 notify-debouncer-mini

- Current published version: **0.7.0**.
  Source: https://docs.rs/notify-debouncer-mini/latest/notify_debouncer_mini/
- License: MIT OR Apache-2.0.
- Per its docs.rs description: "a debouncer for notify. Filters incoming
  events and emits only one event per timeframe per file."
- Does NOT stitch rename `from`/`to` pairs.
- Lightweight, suitable when the consumer just wants
  one-event-per-file-per-window.

### 1.3 notify-debouncer-full

- Current published version: **0.7.0**.
  Source: https://docs.rs/notify-debouncer-full/latest/notify_debouncer_full/
- License: MIT OR Apache-2.0.
- Per its docs.rs page, it adds:
  - "Only emits a single `Rename` event if the rename `From` and `To` events
    can be matched."
  - File-ID tracking via `FileIdMap` (uses macOS FSEvents file IDs and
    Windows file IDs) so renames stitch correctly.
  - "Updates paths for events that occurred before the rename event."
  - De-duplicates create + modify storms.
- Emits `DebouncedEvent` instead of raw `Event`.
- The cost is more state in memory per watched path.

### 1.4 notify-types

- Current published version: **2.1.0** (Jan 25, 2026).
  Source: https://github.com/notify-rs/notify
- Shared event-type crate used by the watchers above; relevant only as a
  dependency, not directly consumed by application code in most cases.

## 2. Per-platform backends

Sourced from the docs.rs notify overview and the notify-rs README:

- **Linux / Android**: inotify backend.
  https://docs.rs/notify/latest/notify/
- **macOS**: FSEvents by default (`macos_fsevent` feature). Optional kqueue
  alternative (`macos_kqueue` feature).
  https://docs.rs/notify/latest/notify/
- **Windows**: ReadDirectoryChangesW via `windows-sys`.
  https://docs.rs/notify/latest/notify/
- **iOS / FreeBSD / NetBSD / OpenBSD / DragonflyBSD**: kqueue.
  https://github.com/notify-rs/notify
- **All platforms**: a `PollWatcher` polling fallback.

The notify docs explicitly call out when polling is needed:

- "Network mounted filesystems like NFS may not emit any events" and
  "WSL programs watching Windows paths" fall back to polling.
- Docker-on-macOS-M1 throws `Function not implemented` for FSEvents.
- macOS FSEvents has limits on monitoring unowned files (security restrictions).
- `/proc` and `/sys` "don't emit proper change events."
- "Resource limits exceeded" - hitting inotify max-watches limits.

Source for all five bullets: https://docs.rs/notify/latest/notify/

`PollWatcher` walks the tree on a tick and diffs metadata. It is the only
reliable mode where native watchers don't deliver events; it is also the
expensive mode.

## 3. Linux inotify limits

### 3.1 Defaults

- `fs.inotify.max_user_watches`: limits the number of inotify watches per
  user (UID). Historically hardcoded to **8192** since 2005.
  Source: https://watchexec.github.io/docs/inotify-limits.html
- `fs.inotify.max_user_instances`: per-user inotify instance count. Often
  **128** on servers and **1024** on workstations.
  Source: https://watchexec.github.io/docs/inotify-limits.html
- `fs.inotify.max_queued_events`: per-instance event queue cap. Default
  **16384** historically.
  Source: cited in watchexec docs above and in general kernel notes.

### 3.2 Per-watch memory

From the watchexec inotify-limits doc:

> "One inotify watch consumes 540 bytes on 32-bit systems and 1080 bytes on
> 64-bit architectures."

The same doc notes that notify-based applications add roughly 10 bytes
plus the path length per watch.

Source: https://watchexec.github.io/docs/inotify-limits.html

### 3.3 Recent kernel default change

Linux mainline now scales the default `max_user_watches` instead of pinning
it at 8192. The torvalds/linux commit `9289012` documents the formula:

> "Allow up to 1% of addressable memory to be allocated for inotify watches
> (per user) limited to the range [8192, 1048576]."

A 64 GB+ machine on a recent kernel will land at the 1,048,576 ceiling
without user intervention. Older / minimal kernels still ship 8192.

Source:
https://github.com/torvalds/linux/commit/92890123749bafc317bbfacbe0a62ce08d78efb7

### 3.4 Raising limits at runtime

Sysctl path: `/proc/sys/fs/inotify/max_user_watches`.

Temporary: `sysctl fs.inotify.max_user_watches=65536`.
Permanent: drop a file into `/etc/sysctl.d/`, e.g. `inotify.conf`:

```
fs.inotify.max_user_watches=65536
```

Source: https://watchexec.github.io/docs/inotify-limits.html

### 3.5 Behavior at exhaustion

If `inotify_add_watch` cannot allocate a slot, it returns `ENOSPC` and the
notify backend propagates an error. The notify docs list "Linux inotify
max-files watched limits" as one of the conditions where falling back to
`PollWatcher` is appropriate.

Source: https://docs.rs/notify/latest/notify/

## 4. macOS FSEvents quirks

The official Apple FSEvents Programming Guide intro page is still online
but the detailed chapters need separate fetches; I sourced behavioral
details from rb-fsevent docs (a long-standing Ruby binding whose README
documents the same upstream semantics) and the fsnotify/fsevents Go
binding. These match the canonical Apple semantics that have been stable
for years.

- **Latency parameter**: the API takes a `latency` (seconds, float). FSEvents
  buffers events for `latency` seconds before delivering, which coalesces
  rapid storms into one callback. Higher latency means more coalescing and
  less overhead but worse responsiveness.
  Source: https://www.rubydoc.info/gems/rb-fsevent/0.11.2
- **NoDefer flag** (`kFSEventStreamCreateFlagNoDefer`): delivers the first
  event on the leading edge of activity rather than after the latency window;
  used by interactive editors.
  Source: https://www.rubydoc.info/gems/rb-fsevent/0.11.2
- **Coalescing limit**: FSEvents coalesces at most **32 distinct subpaths**.
  Beyond that, multiple callbacks fire even within one latency window.
  Source: https://www.rubydoc.info/gems/rb-fsevent/0.11.2
- **Directory-level granularity (historical)**: FSEvents originally reported
  events at directory granularity, requiring the consumer to scan the
  directory to find which file changed. Per-file granularity is now standard
  on macOS 10.7+ via `kFSEventStreamCreateFlagFileEvents`, used by notify by
  default.
  Source: https://docs.rs/notify/latest/notify/
- **Ownership restriction**: macOS FSEvents can decline to monitor files the
  process does not own, in which case notify recommends fallback to polling.
  Source: https://docs.rs/notify/latest/notify/

Note: I could not access the Apple developer page's detailed chapters
through WebFetch (the intro page returned the TOC only). The behaviors
above are corroborated across multiple long-standing FSEvents bindings.
Confidence: medium-high on the numbers, high on the qualitative behavior.

## 5. Windows ReadDirectoryChangesW behavior

Source for this whole section:
https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw

- Reports changes within a directory (file rename/create/delete, dir
  rename/create/delete, size, last write, last access, creation, security).
- Optionally recursive (`bWatchSubtree = TRUE`).
- Buffer associated with the directory handle; size is fixed when first
  allocated.
- **Buffer overflow**: if events fill the buffer between calls, the function
  still returns success but discards all buffered changes and sets
  `lpBytesReturned = 0`. Caller must rescan the tree to recover state.
- **Async failure**: returns `ERROR_NOTIFY_ENUM_DIR` when the system can't
  record all changes; same recovery (enumerate the directory).
- **Network limit**: fails with `ERROR_INVALID_PARAMETER` if the buffer
  exceeds 64 KB AND the directory is a network share (SMB packet limit).
- Reads from the network redirector may fail with `ERROR_INVALID_FUNCTION`
  if the target FS doesn't support the operation.

Operational implication for Terminal Commander: when running on Windows,
the watcher must (a) size the buffer carefully, (b) handle overflow by
re-enumerating the tree, and (c) treat any network-share probe as a
"likely needs polling" case.

## 6. When does notify fall back to polling?

The notify crate does NOT silently fall back. Construction of an
auto-selected `RecommendedWatcher` returns the native backend; if the
caller wants a guarantee of working coverage on network drives or WSL
mounted Windows paths, they must construct a `PollWatcher` explicitly
(or use `Config::with_poll_interval`).

Source: https://docs.rs/notify/latest/notify/

Performance cost of polling: it is a periodic `walkdir` plus stat per
file. Cost scales with tree size, not with churn. For a TC file probe
that watches a single log file, polling cost is negligible. For a probe
that watches a 10k-file source tree on `/mnt/c`, polling is expensive
and likely the dominant cost; this should be documented as a limit, not
hidden.

## 7. TC18 / TC20 requirements mapping

The locked downstream constraints say file probes must handle:

- **File creation after watch start**: native backends require watching
  the parent directory, not the target file, since the target may not
  exist yet. The probe must watch the parent dir and filter for the
  target name, then attach further watches on the actual file when
  it appears. notify-debouncer-full's FileIdMap helps stitch the events.

- **Truncation**: on Linux, truncation arrives as a `Modify` event with
  size dropping; the consumer must detect this and reset its read offset.
  This is application logic - the watcher just reports modification.

- **Rotation**: file rotation is the watcher's hardest case. Pattern:
  watch parent dir; when target is renamed away, the old inode is gone;
  when a new file with the target name appears, re-stat it and reset
  read state. `notify-debouncer-full` provides rename stitching via
  `FileIdMap` which is the right primitive here.
  Source: https://docs.rs/notify-debouncer-full/latest/notify_debouncer_full/

## 8. Recommendation for Terminal Commander

**Primary recommendation**: use `notify` + `notify-debouncer-full` for
file probes on Linux (native) and Windows (native). Document explicit
caveats:

1. The watcher must watch the parent directory of any single-file target,
   not just the file itself, to survive create-after-start and rotation.
2. Each probe must declare a bounded watch set; do not recursively watch
   unbounded subtrees without an explicit policy budget (TC02
   `developer_local` may permit it; `read_only_observer` must not).
3. The probe's policy must include a per-user inotify watch budget check
   against `/proc/sys/fs/inotify/max_user_watches` on Linux, with a clear
   error when exceeded.
4. Windows: cap the `ReadDirectoryChangesW` buffer; on overflow, the
   probe must trigger a rescan + state-reset event rather than silently
   dropping changes.
5. macOS native (deferred per scope): document the 32-subpath coalescing
   limit and the directory-granularity history. Not a blocker for MVP
   since macOS is deferred.

**Secondary recommendation for WSL-mounted Windows paths**: see
`wsl-boundary.md` - those paths require `PollWatcher` because inotify is
known broken on the 9P mount. Make the probe explicitly opt into
polling mode for those paths rather than fail open.

**Unverified / open question for downstream**: should TC ship a custom
debouncer or accept notify-debouncer-full's behavior as-is? The full
debouncer adds latency in exchange for cleaner semantics; some probe
classes (e.g. low-latency security alert tailing) may want raw notify
with a custom debouncer.

## 9. Source map

- notify crate, docs.rs: https://docs.rs/notify/latest/notify/
- notify-debouncer-mini, docs.rs:
  https://docs.rs/notify-debouncer-mini/latest/notify_debouncer_mini/
- notify-debouncer-full, docs.rs:
  https://docs.rs/notify-debouncer-full/latest/notify_debouncer_full/
- notify-rs GitHub: https://github.com/notify-rs/notify
- watchexec inotify-limits docs:
  https://watchexec.github.io/docs/inotify-limits.html
- Linux kernel commit raising inotify default:
  https://github.com/torvalds/linux/commit/92890123749bafc317bbfacbe0a62ce08d78efb7
- Microsoft ReadDirectoryChangesW reference:
  https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw
- rb-fsevent README (FSEvents semantics):
  https://www.rubydoc.info/gems/rb-fsevent/0.11.2
- WSL 9P inotify limitation, GitHub issue (cross-ref to wsl-boundary.md):
  https://github.com/microsoft/WSL/issues/4739
