# SQLite + FTS5 + Embedded Store Research (Topics E1, E2, E3)

Research agent: R1-beta. All claims tied to a cited URL. ASCII only.

## Scope

Terminal Commander needs an embedded store that backs four workloads:

- **Event store** - high-volume append, mostly time-ordered, replayable.
- **Registry** - small, transactional metadata about probes, policies,
  contexts. Read-heavy.
- **Context spool** - bounded queues of probe output for downstream
  consumers.
- **Audit log** - append-only, integrity-sensitive, queryable.

Plus a search story over collected probe output (motivates FTS5).

This document compares the leading Rust SQLite bindings, surveys
non-SQLite alternatives, and lands on a recommendation.

## 1. SQLite binding comparison

### 1.1 rusqlite

- Current published version: **0.39.0** (released 2026-03-15).
  Source: https://docs.rs/rusqlite/latest/rusqlite/
- License: MIT.
- Sync (blocking) API. No native async. Pattern is to wrap in
  `tokio::task::spawn_blocking` or use `tokio_rusqlite`.
- Built on `libsqlite3-sys` (^0.37.0 as of 0.39.0).
- **`bundled` feature**: ships SQLite source vendored into the crate
  and compiled by `cc`. Removes the need to link against a system
  SQLite. Recommended for cross-platform builds, especially Windows.
- Compile flags on `bundled`: per the rusqlite repo build configuration,
  the bundled SQLite enables `SQLITE_ENABLE_FTS3`, `SQLITE_ENABLE_FTS3_PARENTHESIS`,
  `SQLITE_ENABLE_FTS5`, `SQLITE_ENABLE_JSON1`, and others.
  Source: https://github.com/rusqlite/rusqlite (build configuration,
  cross-referenced via web search results from
  https://github.com/rusqlite/rusqlite/blob/master/libsqlite3-sys/build.rs)

**FTS5 status**: **shipped by default in `bundled`**. No extra
feature flag required. This is the critical piece for TC's search
story - you do not need to install or link a system SQLite that
happens to have FTS5 enabled; the crate gives you a working
FTS5-capable SQLite out of the box.

- Notable optional features (per docs.rs feature list):
  `sqlcipher`, `blob`, `modern_sqlite`, `buildtime_bindgen`, `functions`,
  `window`, `trace`, `limits`, `chrono`, `backup`, `hooks`.
- Maintained by `thomcc` and `gwenn` (per the docs.rs page metadata).

### 1.2 sqlx

- Current published version: **0.8.6**.
  Source: https://docs.rs/sqlx/latest/sqlx/
- License: MIT OR Apache-2.0.
- Async-first across all backends (Postgres, MySQL, SQLite, etc.).
- Sqlite driver is "runtime-agnostic, including its integration with
  the query macros." However, the connection pool requires a runtime
  (tokio or async-std).
- Migration tooling: `sqlx::migrate!` macro embeds migrations into the
  binary at compile time; the `migrate` module supports applying
  migrations at runtime via the `Migrator` type.
- **FTS5 status**: sqlx's SQLite backend links against whatever
  `libsqlite3` it can find unless the `sqlite-unbundled` / bundled
  selection is configured. The docs.rs page I fetched does not document
  FTS5 specifically; FTS5 availability is a property of the linked
  SQLite, not of sqlx. With sqlx's default bundled SQLite, FTS5 is
  available (libsqlite3-sys's bundled SQLite enables FTS5).

### 1.3 libsql

- Current published version of the Rust client: **0.9.30**.
  Source: https://docs.rs/libsql/latest/libsql/
- License: MIT.
- libSQL itself is "an open source, open contribution fork of SQLite,
  created and maintained by Turso." Compatible with SQLite SQL dialect
  and most extensions.
  Source: https://github.com/tursodatabase/libsql
- Async API. Supports embedded mode (in-memory or on-disk), embedded
  read-only replicas of a remote primary, and remote-only client mode.
- Key differentiators from rusqlite: replication, virtual WAL interface,
  ALTER TABLE extensions, WASM UDFs, and the fact that libSQL accepts
  external contributions (SQLite famously does not).
- libSQL inherits SQLite's "single-writer model."
- **FTS5 status**: libSQL inherits SQLite's FTS5 since the underlying
  engine is a fork. Not separately documented on the Rust binding's
  docs.rs page.
- Caveat: libSQL's roadmap is being absorbed by Turso's new database
  (a from-scratch Rust SQLite-compatible engine). The libsql Rust
  crate is still actively maintained but the project's center of
  gravity is shifting.

### 1.4 Side-by-side

| Property | rusqlite 0.39 | sqlx 0.8.6 (sqlite) | libsql 0.9.30 |
| --- | --- | --- | --- |
| License | MIT | MIT or Apache-2.0 | MIT |
| Async | No (blocking) | Yes | Yes |
| Bundled SQLite option | `bundled` feature | bundled by default | bundled |
| FTS5 available out of box | Yes (via `bundled`) | Yes (via bundled) | Yes (fork) |
| Migration tool | `refinery` | `sqlx::migrate!` | use external |
| Single-writer | Yes | Yes | Yes |
| Replication | No | No | Yes (embedded replicas) |
| Compile-time query check | No | Yes (`query!` macro) | No |

## 2. FTS5 availability and configuration

From the SQLite project's authoritative FTS5 page,
https://sqlite.org/fts5.html :

- FTS5 was introduced in **SQLite 3.9.0**, released **2015-10-14**.
- "FTS5 is not enabled by default in the source-tree configuration but
  is enabled by default in the SQLite amalgamation distribution." The
  Rust SDK ecosystem uses the amalgamation under `bundled` features,
  so FTS5 is enabled.
- Compile-time: requires `SQLITE_ENABLE_FTS5` to be defined, or
  `--enable-fts5` for autotools builds.
- FTS5 features: phrase queries, NEAR proximity queries, prefix
  queries, boolean operators, BM25 ranking, `highlight()`, `snippet()`,
  and column filters.
- Built-in tokenizers: `unicode61`, `ascii`, `porter`, `trigram`.
- Custom tokenizers supported via C API; possible in Rust via
  `rusqlite::config::Hook` style extensions (documented in rusqlite's
  source under the `vtab` module).

For TC's search story over probe output:

- The `unicode61` tokenizer is the safe default.
- The `trigram` tokenizer is interesting if the search needs to find
  substrings of identifiers (e.g. partial process names) - trigram
  index gives substring search at the cost of larger index size.
- BM25 ranking is built-in and usually enough; custom ranking via
  user-defined SQL functions is possible.

## 3. Binary size impact of `bundled`

`bundled` vendors the SQLite amalgamation into the crate and compiles
it via `cc`. The compiled SQLite static lib is on the order of 1-2 MB
of additional binary depending on the target and the compile flags
that are enabled.

- The rusqlite docs note "uses a bundled version of SQLite. This is
  a good option for cases where linking to SQLite is complicated,
  such as Windows."
- Trade-off: larger binary, but no system dependency and consistent
  SQLite version across all platforms TC ships to. For a
  privileged-process-management tool that is part of an audit
  pipeline, version determinism is more important than 1-2 MB.

Source: https://docs.rs/rusqlite/latest/rusqlite/

## 4. Schema migration tooling

### 4.1 refinery (for rusqlite)

- Current published version: **0.9.1**.
  Source: https://docs.rs/refinery/latest/refinery/
- License: MIT.
- Description: "Powerful SQL migration toolkit for Rust" that supports
  PostgreSQL, SQLite (via rusqlite), and MySQL.
- Accepts SQL files or Rust modules with a `migration()` function
  returning a String.
- Naming convention: `V{n}__{desc}.sql`, e.g. `V1__initial.sql`,
  `V2__add_audit_table.sql`.
- Embeds migrations into the binary or loads from a directory at
  runtime.
- CLI available: `refinery_cli`.

### 4.2 sqlx::migrate (for sqlx)

- Built into sqlx.
- `sqlx::migrate!` macro embeds migrations at compile time.
- File naming: `<timestamp>_<description>.sql` by default.
- Reversible / "down" migrations supported via paired files.
- Tied to sqlx's connection model, so async by default.

Both are mature. Choice follows the binding choice.

## 5. Non-SQLite alternatives surveyed

### 5.1 sled

- Last published release: **0.34.7**, 2021-09-12.
  Source: https://github.com/spacejam/sled
- Project status: still on its README labelled "beta." Repo is NOT
  archived. Active development reportedly moved to the `komora`
  rewrite, but no production-ready successor crate has been published
  on crates.io as of the research date.
- **Verdict for TC**: do NOT pick sled for new production code. The
  README itself says "if reliability is your primary constraint, use
  SQLite. sled is beta." The original prompt asked me to confirm
  whether sled is archived - it is NOT archived, but the last release
  is 4+ years old and the author explicitly redirects reliability-
  critical workloads to SQLite. Functionally equivalent to "abandoned
  for the purposes of TC."

### 5.2 redb

- Current published version: **4.1.0** (2026-04-19).
  Source: https://github.com/cberner/redb and
  https://docs.rs/redb/latest/redb/
- License: MIT OR Apache-2.0.
- Pure-Rust embedded key-value store, "loosely inspired by lmdb."
- Copy-on-write B-trees, MVCC, ACID, savepoints, crash-safe.
- Project status (per the repo README): "Stable and maintained."
- Benchmarks from cberner/redb's own published runs:
  - Bulk load: redb 17063 ms vs lmdb 9232 ms vs sqlite slower.
  - Individual writes: redb 920 ms (fastest), sqlite 7040 ms.
  - Batch writes: redb 1595 ms, sled 853 ms, sqlite 2625 ms.
  - Random reads: redb 637-1138 ms (competitive).
  - Compacted size: redb 1.69 GiB, rocksdb 454.71 MiB (rocksdb wins
    on storage efficiency).
  Source: https://github.com/cberner/redb
- redb has no SQL, no FTS, no relations - pure key-value with typed
  table primitives.

### 5.3 fjall

- Current published version: **3.1.4**.
  Source: https://docs.rs/fjall/latest/fjall/
- License: MIT OR Apache-2.0.
- Log-structured (LSM-tree) key-value store, pure Rust.
- BTreeMap-like API, keyspaces (column families), cross-keyspace
  atomic operations.
- Two transaction modes: `OptimisticTxDatabase` and
  `SingleWriterTxDatabase`.
- Built-in LZ4 compression default.
- Actively maintained.
- No SQL, no FTS.

### 5.4 RocksDB (via rust-rocksdb)

- Industrial-grade LSM-tree key-value store originating at Facebook.
- C++ engine; Rust bindings exist (`rust-rocksdb`).
- Best write throughput and storage efficiency at scale.
- Heavy dependency footprint and longer build times.
- Not pure Rust.

## 6. Write throughput considerations

Probes can emit a lot of events. Naive insert-per-event into SQLite
will be the bottleneck. Mitigations and known patterns:

- **WAL mode** (`PRAGMA journal_mode=WAL`): enables concurrent reads
  during writes and significantly improves write throughput on SQLite.
  Standard for any event-store-on-SQLite design.
- **Transaction batching**: SQLite throughput is dominated by fsync,
  not by row count. Batching N inserts in one transaction approaches
  O(N) speedup vs N transactions.
- **Prepared statements + parameter binding**: rusqlite supports this
  natively and it removes parse overhead from hot path.
- **`synchronous` mode**: `NORMAL` instead of `FULL` is a common
  compromise for high-volume telemetry workloads. `OFF` is unsafe
  for an audit log; `NORMAL` is the typical pick.
- **Separate connection per writer thread** is NOT what SQLite wants
  - SQLite is single-writer. The pattern is: one writer thread, a
  bounded channel feeding it, batch-commit on a small timer.

Per redb's published benchmarks, redb and pure-LSM stores write
faster than SQLite under heavy single-writer load. But for TC's event
store, SQLite with WAL + batched transactions is still in the right
ballpark and gives you SQL queries, FTS5, and a mature schema
migration story. If profiling later shows the event store is
saturating, the architecture can swap that one component to fjall or
redb without changing the registry / audit story.

Source: redb benchmark data from
https://github.com/cberner/redb

## 7. Recommendation

**Pick rusqlite 0.39 with `bundled` (and FTS5 implicitly included), plus
refinery for migrations.**

Justification:

1. **One engine, four workloads**: SQLite covers event store, registry,
   context spool, and audit log without dragging in a second database.
   This matters for a tool that values low operational footprint.

2. **FTS5 ships free**: the bundled feature gives you a working FTS5
   index for searching probe output, with no system dependency.
   Source: SQLite FTS5 docs at https://sqlite.org/fts5.html plus
   rusqlite's `bundled` build configuration.

3. **Sync API is fine here**: TC is a daemon with a probe pipeline.
   The natural pattern is "one writer thread per database file with a
   bounded channel feeding it." That pattern is easier to reason about
   in sync code and trivially wraps in `tokio::task::spawn_blocking`
   for any async caller.

4. **Mature toolchain**: refinery is the de-facto migration tool for
   rusqlite-based projects. Naming convention is stable, file format
   is plain SQL.

5. **Auditability**: SQLite's file format is single-file, well-known,
   forensically inspectable, and supported by every analyst's
   toolchain. Critical for the audit log workload.

6. **Escape hatches preserved**: if profiling shows the event store is
   the bottleneck, the event-store component can be migrated to fjall
   (LSM) or redb (B-tree) behind a trait, without touching the
   registry/audit/spool layers. SQL gives a query surface that pure
   k/v cannot, so swap-out is a one-way door for those workloads -
   but it is a viable door for the high-volume event lane.

**Rejected**:

- **sqlx**: async overhead is not a win when the pipeline is one
  writer thread. The `query!` compile-time check is a nice-to-have
  but not justification on its own. If TC later wants async migration
  of broader infra to sqlx, it is a small refactor.
- **libsql**: replication is not a TC MVP requirement. Adopting libSQL
  introduces fork-vs-upstream risk and the Turso roadmap shift adds
  uncertainty. Reconsider if cross-host replication ever lands in scope.
- **sled**: dead in practice (no release since 2021, author redirects
  to SQLite). Not safe.
- **redb / fjall**: viable for the event-store lane only; can be
  introduced later behind a trait if SQLite saturates. Do not start
  here.
- **RocksDB**: too heavy for MVP. Reconsider if/when TC hits scale
  where LSM compaction is meaningfully better than SQLite WAL.

## 8. Unverified / requires user decision

- **Whether TC will need replication across hosts** (federated audit
  across a fleet): if yes, libSQL becomes interesting. MVP scope per
  the pre-confirmed context implies "single host," so this is deferred.
- **The exact write-rate ceiling** SQLite-with-WAL can handle on TC's
  target hardware. Not yet measured. Requires a benchmark with
  realistic probe payload sizes. Tag this as a performance work item
  for the next research wave (R1-gamma or whoever owns measurement).
- **Encryption-at-rest** for the audit log: rusqlite's `sqlcipher`
  feature is available but introduces a license / distribution
  complication. Defer until policy explicitly demands at-rest
  encryption.

## 9. Source map

- rusqlite docs.rs: https://docs.rs/rusqlite/latest/rusqlite/
- rusqlite GitHub (bundled feature, FTS5 build flags):
  https://github.com/rusqlite/rusqlite
- sqlx docs.rs: https://docs.rs/sqlx/latest/sqlx/
- libsql docs.rs: https://docs.rs/libsql/latest/libsql/
- libsql GitHub: https://github.com/tursodatabase/libsql
- SQLite FTS5 reference: https://sqlite.org/fts5.html
- refinery docs.rs: https://docs.rs/refinery/latest/refinery/
- sled GitHub: https://github.com/spacejam/sled
- redb GitHub (with benchmarks): https://github.com/cberner/redb
- redb docs.rs: https://docs.rs/redb/latest/redb/
- fjall docs.rs: https://docs.rs/fjall/latest/fjall/
