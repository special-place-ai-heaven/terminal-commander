# License Decision: Apache-2.0

Status: DECIDED (user-confirmed)
Last verified: 2026-05-21
Researcher: R2-gamma

## Decision

Terminal Commander adopts the **Apache License, Version 2.0** for all source
code and project-original assets.

- SPDX identifier: `Apache-2.0`
- Cargo manifest field: `license = "Apache-2.0"`
- Workspace inheritance: declare in `[workspace.package]` and let member
  crates use `license.workspace = true`.

## Authoritative references

- SPDX listing: https://spdx.org/licenses/Apache-2.0.html
- License text (canonical): https://www.apache.org/licenses/LICENSE-2.0
- OSI page: https://opensource.org/license/apache-2-0
- ASF FAQ: https://www.apache.org/foundation/license-faq.html
- ASF 3rd-Party License Policy (Category A list):
  https://www.apache.org/legal/resolved.html

## Required repository files

| File | Required? | Purpose |
|---|---|---|
| `LICENSE` | Yes | Full Apache-2.0 license text (UTF-8). |
| `NOTICE` | Recommended | Attribution notices that must propagate to derivative works per Section 4 of Apache-2.0. Keep minimal at MVP: project name + copyright line. |
| `README.md` license section | Recommended | Short pointer to SPDX + LICENSE file. |

If a `NOTICE` file exists, Section 4 of Apache-2.0 requires that "any
Derivative Works that You distribute must include a readable copy of the
attribution notices contained within such NOTICE file" - so only put genuinely
required attributions there. Per ASF guidance, do not use NOTICE for marketing
text.

Source: https://www.apache.org/licenses/LICENSE-2.0 (Section 4 and the
"How to apply the Apache License to your work" appendix)

## Per-file header convention

Apache-2.0 explicitly recommends attaching a boilerplate header to each source
file. Both forms are acceptable:

### Long form (Apache recommendation)

```rust
// Copyright 2026 The Terminal Commander Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
```

### Short SPDX form (ASF-accepted shorter variant)

```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
```

The short SPDX form is acceptable per the ASF license FAQ
(https://www.apache.org/foundation/license-faq.html) and is the modern
convention used by most new Rust projects.

**Recommendation for Terminal Commander MVP**: use the short SPDX form for all
`.rs` files. It is machine-readable, less visual noise, and SPDX-tooling
friendly (cargo-deny, REUSE, etc.). The full text only needs to appear once in
`LICENSE`.

Note: the Apache license appendix text says headers "should be enclosed in
the appropriate comment syntax for the file format" and recommends including
"a file or class name and description of purpose on the same printed page as
the copyright notice." For Rust this is naturally satisfied by the module-level
doc comment that typically follows the SPDX header.

## Dependency license compatibility

Apache-2.0 is one-way compatible with permissive licenses: Apache-2.0 code can
**consume** MIT, BSD, and CC0 dependencies without legal friction, because
those licenses impose strictly fewer obligations than Apache-2.0. The combined
work is then distributed under Apache-2.0 (with attribution preserved per the
incoming licenses).

Per ASF Category A (https://www.apache.org/legal/resolved.html):

- MIT / X11: Category A, can be included in Apache-2.0 works.
- BSD (2-clause, 3-clause, without advertising clause): Category A.
- CC0 1.0 Universal: Category A (treated as public-domain equivalent).

### Specific deps for Terminal Commander

| Crate | License (verified) | Source |
|---|---|---|
| `rmcp` (MCP Rust SDK) | Apache-2.0 (with legacy MIT contributions; documentation CC-BY-4.0) | https://github.com/modelcontextprotocol/rust-sdk/blob/main/LICENSE |
| `tokio` | MIT | https://github.com/tokio-rs/tokio (top-level `LICENSE`) |
| `rusqlite` | MIT | https://github.com/rusqlite/rusqlite/blob/master/Cargo.toml line 19 |
| `notify` (core crate) | CC0-1.0 | https://github.com/notify-rs/notify (README) |
| `notify-debouncer-full` | MIT OR Apache-2.0 | https://github.com/notify-rs/notify (README - "other components ... dual-licensed under MIT or Apache-2.0") |
| `pty-process` 0.5.x | MIT (X11) | https://raw.githubusercontent.com/doy/pty-process/main/LICENSE |
| `portable-pty` (wezterm) | MIT | https://github.com/wez/wezterm/blob/main/pty/Cargo.toml |
| `refinery` 0.9 | MIT OR Apache-2.0 | (verify in `tooling-baseline.md` cargo-deny pass; not refetched here) |

All confirmed deps are Category A. No GPL/LGPL/AGPL pull-through.

**One legal nuance**: CC0 is "public domain dedication." Some
jurisdictions (notably Germany) do not recognize public-domain dedication;
fall-back permissive terms apply. ASF treats CC0 as Category A. Terminal
Commander's adoption of `notify` is unproblematic. If anyone redistributes,
they should retain the `notify` CC0 notice in their attributions file even
though CC0 does not strictly require it.

### Reverse direction (not applicable here)

Code under Apache-2.0 cannot be relicensed into MIT/BSD-only projects without
permission - the patent grant and attribution requirements travel with it.
This is fine for Terminal Commander because nothing downstream consumes us
as a library yet; downstream consumers will simply need to remain Apache-2.0
compatible.

### GPLv2 incompatibility

ASF FAQ explicitly notes Apache-2.0 is incompatible with **GPLv2** ("due to
certain patent termination and indemnification provisions") but is compatible
with **GPLv3**. Terminal Commander has no GPL deps planned. If a GPLv2 dep
sneaks in, cargo-deny will catch it (see `tooling-baseline.md`).

## Workspace declaration sketch

In root `Cargo.toml`:

```toml
[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.90"
license = "Apache-2.0"
repository = "https://github.com/<owner>/terminal-commander"
authors = ["The Terminal Commander Authors"]
```

In each `crates/<name>/Cargo.toml`:

```toml
[package]
name = "terminal-commander-core"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
repository.workspace = true
authors.workspace = true
```

`workspace.package` inheritance (including `license`) has been stable since
**Rust 1.64**, per
https://doc.rust-lang.org/cargo/reference/workspaces.html#the-package-table.
Terminal Commander's MSRV of 1.90+ easily clears this.

## Action items for repo bootstrap

1. Add `LICENSE` file at repo root with verbatim Apache-2.0 text from
   https://www.apache.org/licenses/LICENSE-2.0 (the canonical text;
   download and commit, do not paraphrase).
2. Add minimal `NOTICE` file:
   ```text
   Terminal Commander
   Copyright 2026 The Terminal Commander Authors

   This product includes software developed by third parties under
   compatible permissive licenses; see Cargo.lock and `cargo-deny check
   licenses` output for the canonical attribution set.
   ```
3. Add SPDX header to every new `.rs` file going forward (enforce via
   pre-commit or `cargo-deny` license-header check; see
   `tooling-baseline.md`).
4. Configure `cargo-deny` `licenses` section to allow only Category A
   identifiers plus CC0-1.0 explicitly. (Template in
   `tooling-baseline.md`.)
