# Linux glibc Floor Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the linux release binaries on ubuntu-22.04 (glibc 2.35) instead of ubuntu-24.04 (glibc 2.39), and add a build-time objdump guard that fails if any binary's glibc floor exceeds 2.35.

**Architecture:** A single reusable workflow `_build-platform-binary.yml` builds all five platform targets on native runners via a `runs-on` platform map (line 55). Only the two linux entries change; mac/windows keep their native-runner path (glibc does not apply; Apple SDK licensing rules out containers for mac). A new linux-only step runs after staging and asserts, with a numeric version comparator, that no staged binary requires a GLIBC symbol above 2.35.

**Tech Stack:** GitHub Actions (reusable `workflow_call`), bash, `objdump` (preinstalled on ubuntu runners), GNU coreutils `sort -V` (numeric version compare).

---

## Background: verified facts (do not re-derive)

- `.github/workflows/_build-platform-binary.yml` is the single source of truth for all platform builds; called by `npm-binary-build.yml` (PR gate) and `release-please.yml` (publish). Editing it covers both.
- Line 55 is the `runs-on` map:
  ```yaml
  runs-on: ${{ fromJSON('{"linux-x64":"ubuntu-24.04","linux-arm64":"ubuntu-24.04-arm","windows-x64":"windows-2022","mac-x64":"macos-15-intel","mac-arm64":"macos-14"}')[inputs.platform] }}
  ```
- Lines 46-54 are the comment block explaining native-runner choice + runner labels.
- Staging step (lines 86-106) copies the three binaries into `${PLATFORM_PKG_DIR}/bin` as `terminal-commanderd`, `terminal-commander-mcp`, `terminal-commander` (+`.exe` on windows via `EXE_SUFFIX`), chmod +x, strips `.placeholder`/`.gitkeep`, `ls -la "$bin_dir"`. `PLATFORM_PKG_DIR=packages/terminal-commander-${platform}`.
- A native `--version` smoke step (108-118) already runs after staging.
- `EXE_SUFFIX` is `.exe` only for windows-x64, else empty.
- `ubuntu-22.04` (glibc 2.35) and `ubuntu-22.04-arm` are GA GitHub-hosted runner labels; the repo already uses `ubuntu-24.04-arm`, so arm hosted runners work here.
- `objdump -T <bin>` lists dynamic symbols incl. `GLIBC_X.Y[.Z]` version tags. `sort -V` orders these numerically (verified: `2.39 > 2.35 > 2.4`), so it is a correct version comparator, NOT a lexical string sort.
- Verify jobs `release-please.yml:1532-1572` run the published npm binary in `node:22-bookworm-slim` (Debian 12, glibc 2.36). They FAILED on v0.1.13 with `GLIBC_2.39 not found`; they are the regression test.

## File Structure

- **Modify only:** `.github/workflows/_build-platform-binary.yml` (runner map, comment, new guard step). No other files.

## Local verification note

This host's WSL is Ubuntu 24.04 / glibc 2.39. A native `cargo build` here CANNOT produce a 2.35-floor binary (host glibc is the floor), so the runner-swap fix is CI-verified, not locally buildable. HOWEVER the guard's comparator logic and its ability to CATCH a too-new binary ARE locally testable: build a binary in WSL (it will be 2.39), run the guard snippet against it, and confirm it correctly fails. That proves the guard works; CI proves the runner swap lowers the floor.

---

## Task 1: Lower the linux runner floor + document why

**Files:**
- Modify: `.github/workflows/_build-platform-binary.yml` (line 55 map; comment block 46-54)

- [ ] **Step 1: Change the two linux runner labels**

Replace the `runs-on` line (55) so the linux entries use 22.04. Exact replacement (mac/windows entries byte-identical to current):

```yaml
    runs-on: ${{ fromJSON('{"linux-x64":"ubuntu-22.04","linux-arm64":"ubuntu-22.04-arm","windows-x64":"windows-2022","mac-x64":"macos-15-intel","mac-arm64":"macos-14"}')[inputs.platform] }}
```

- [ ] **Step 2: Document the glibc-floor rationale in the comment block**

Insert, immediately before the `# Runner labels (verified ...)` line in the 46-54 comment block, these lines (keep existing comment text):

```yaml
    # LINUX GLIBC FLOOR: linux-x64/linux-arm64 pin ubuntu-22.04 (glibc
    # 2.35) ON PURPOSE. A native cargo build links the host glibc as the
    # binary's MINIMUM required version; building on ubuntu-24.04 (glibc
    # 2.39) shipped binaries that fail on Debian 12 bookworm (2.36),
    # Ubuntu 22.04 (2.35), and older (v0.1.13 verify-linux-* failed with
    # "GLIBC_2.39 not found"). Do NOT bump linux to 24.04. When 22.04
    # hosted runners are retired, switch linux to cargo-zigbuild with a
    # pinned gnu.2.x target rather than a newer native runner. The
    # post-staging objdump guard below enforces the <=2.35 floor.
```

- [ ] **Step 3: Validate workflow YAML syntax**

Run:
```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && python3 -c 'import yaml,sys; yaml.safe_load(open(\".github/workflows/_build-platform-binary.yml\")); print(\"yaml ok\")'"
```
Expected: `yaml ok`. (If PyYAML is absent, fall back to `ruby -ryaml -e` or skip with a note; the `fromJSON` string is a plain scalar so a YAML parser validates structure, not the embedded JSON.)

- [ ] **Step 4: Validate the embedded JSON map parses**

Run (extract the JSON literal and parse it):
```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && python3 -c \"import json; print(json.loads('{\\\"linux-x64\\\":\\\"ubuntu-22.04\\\",\\\"linux-arm64\\\":\\\"ubuntu-22.04-arm\\\",\\\"windows-x64\\\":\\\"windows-2022\\\",\\\"mac-x64\\\":\\\"macos-15-intel\\\",\\\"mac-arm64\\\":\\\"macos-14\\\"}'))\""
```
Expected: prints the dict (no JSON error). Confirms the map literal is well-formed after the edit.

- [ ] **Step 5: Commit**

```
git add .github/workflows/_build-platform-binary.yml
git commit -F <msg-file>
```
Subject: `fix(ci): build linux release binaries on ubuntu-22.04 (glibc 2.35)`
Body: native cargo build links host glibc; 24.04's 2.39 broke bookworm/22.04 (v0.1.13 verify-linux-*); mac/windows unchanged; comment records the do-not-bump rationale + zigbuild escape hatch.

---

## Task 2: Add the post-staging glibc-floor guard (linux only)

**Files:**
- Modify: `.github/workflows/_build-platform-binary.yml` (new step after the staging step, ~line 106, before the `--version` smoke step)

- [ ] **Step 1: Prove the guard logic locally first (catches a too-new binary)**

In WSL, build a binary natively (it will be glibc 2.39 on this host) and run the exact comparator the step will use, to confirm it FAILS on >2.35:

```
wsl.exe bash -lc '
cd /mnt/c/Users/poslj/terminal-commander
cargo build --release -p terminal-commander-mcp 2>/dev/null
BIN=target/release/terminal-commander-mcp
MAXG=$(objdump -T "$BIN" | grep -oE "GLIBC_[0-9]+\.[0-9]+(\.[0-9]+)?" | sed "s/GLIBC_//" | sort -V | tail -1)
echo "max GLIBC required: $MAXG"
HIGHEST=$(printf "%s\n%s\n" "$MAXG" "2.35" | sort -V | tail -1)
if [ "$HIGHEST" != "2.35" ]; then echo "GUARD WOULD FAIL (floor $MAXG > 2.35) -- correct on a 24.04-built binary"; else echo "GUARD WOULD PASS"; fi
'
```
Expected on this 2.39 WSL host: `max GLIBC required: 2.39` then `GUARD WOULD FAIL ... -- correct`. This proves the comparator detects a too-new floor. (Then `cargo clean` after, per the global rule — but Task 3 also cleans; one clean at the end is fine.)

- [ ] **Step 2: Add the guard step to the workflow**

Insert this step immediately AFTER the "Stage real binaries into platform package" step (after its closing, ~line 106) and BEFORE the "Smoke — --version" step:

```yaml
      - name: Enforce glibc floor (linux only, <= 2.35)
        if: startsWith(inputs.platform, 'linux')
        shell: bash
        run: |
          set -euo pipefail
          # Native cargo build links the host glibc as the binary's
          # minimum. We build on ubuntu-22.04 (glibc 2.35); assert no
          # staged binary slipped in a higher requirement (e.g. a runner
          # image bump). sort -V is a numeric version comparator, so
          # 2.39 > 2.35 > 2.4 order correctly (NOT a lexical sort).
          floor="2.35"
          bin_dir="${PLATFORM_PKG_DIR}/bin"
          fail=0
          for bin in terminal-commanderd terminal-commander-mcp terminal-commander; do
            path="${bin_dir}/${bin}"
            maxg=$(objdump -T "$path" \
              | grep -oE 'GLIBC_[0-9]+\.[0-9]+(\.[0-9]+)?' \
              | sed 's/GLIBC_//' \
              | sort -V | tail -1)
            if [ -z "$maxg" ]; then
              echo "::error::no GLIBC version symbols found in $path (unexpected for a glibc-linked binary)"
              fail=1
              continue
            fi
            highest=$(printf '%s\n%s\n' "$maxg" "$floor" | sort -V | tail -1)
            if [ "$highest" != "$floor" ]; then
              echo "::error::$bin requires GLIBC_${maxg}, above the ${floor} floor (build host glibc leaked into the binary; do not build linux on a runner newer than ubuntu-22.04)"
              fail=1
            else
              echo "$bin: max GLIBC_${maxg} <= ${floor} OK"
            fi
          done
          exit "$fail"
```

- [ ] **Step 3: Validate workflow YAML syntax after the insert**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && python3 -c 'import yaml; yaml.safe_load(open(\".github/workflows/_build-platform-binary.yml\")); print(\"yaml ok\")'"
```
Expected: `yaml ok`.

- [ ] **Step 4: Dry-run the guard's shell body against a local binary**

Confirm the step's exact bash (copied out, with `PLATFORM_PKG_DIR` faked to a dir holding the local build) parses and runs. On this 2.39 host it MUST exit nonzero (floor 2.39 > 2.35), proving the guard is wired correctly:

```
wsl.exe bash -lc '
cd /mnt/c/Users/poslj/terminal-commander
cargo build --release -p terminal-commander-mcp -p terminal-commanderd -p terminal-commander-cli 2>/dev/null
mkdir -p /tmp/tcbin && cp target/release/terminal-commanderd target/release/terminal-commander-mcp target/release/terminal-commander /tmp/tcbin/ 2>/dev/null || true
PLATFORM_PKG_DIR=/tmp ; mkdir -p /tmp/bin && cp /tmp/tcbin/* /tmp/bin/ 2>/dev/null || true
floor="2.35"; bin_dir="/tmp/bin"; fail=0
for bin in terminal-commanderd terminal-commander-mcp terminal-commander; do
  path="${bin_dir}/${bin}"; [ -f "$path" ] || continue
  maxg=$(objdump -T "$path" | grep -oE "GLIBC_[0-9]+\.[0-9]+(\.[0-9]+)?" | sed "s/GLIBC_//" | sort -V | tail -1)
  highest=$(printf "%s\n%s\n" "$maxg" "$floor" | sort -V | tail -1)
  [ "$highest" != "$floor" ] && { echo "$bin GLIBC_$maxg > $floor (guard fires)"; fail=1; } || echo "$bin OK"
done
echo "guard exit would be: $fail (expect 1 on this 2.39 host)"
rm -rf /tmp/tcbin /tmp/bin
'
```
Expected: each binary reports `GLIBC_2.39 > 2.35 (guard fires)`, `guard exit would be: 1`. Confirms the guard correctly rejects a too-new binary. (On CI's ubuntu-22.04 the same logic passes at 2.35.)

- [ ] **Step 5: Commit**

```
git add .github/workflows/_build-platform-binary.yml
git commit -F <msg-file>
```
Subject: `fix(ci): enforce <=2.35 glibc floor on staged linux binaries`
Body: post-staging objdump guard, linux-only (startsWith linux), scans all three binaries, numeric sort -V comparator (not lexical), fails the build if floor > 2.35; catches runner-image drift before the downstream bookworm smoke.

---

## Task 3: cargo clean + final check

- [ ] **Step 1: cargo clean (task boundary, after the local guard dry-runs)**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo clean"
```

- [ ] **Step 2: Confirm the diff is exactly the intended workflow edits**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && git show --stat HEAD~1 HEAD -- .github/workflows/_build-platform-binary.yml | head -20"
```
Expected: only `_build-platform-binary.yml` changed across the two commits.

- [ ] **Step 3: Push and let CI be the proof**

```
git push heaven main
```
Then watch the next release/PR-gate run: `npm-binary-build` builds linux on 22.04, the guard step prints `max GLIBC_2.35 <= 2.35 OK` for all three binaries, and on the next release the `verify-linux-x64` / `verify-linux-arm64` jobs (node:22-bookworm-slim) flip from FAIL to PASS.

```
gh run list --repo special-place-ai-heaven/terminal-commander --limit 5
```

- [ ] **Step 4: Report (DSPIVR)**

Objective, changes (one file, two commits), verification (local: guard correctly fires on the 2.39 WSL binary; CI: 22.04 build + guard pass + bookworm verify flips green), evidence (objdump floor output), known gaps (v0.1.13 linux artifacts stay 2.39 -- add the "use >= v0.1.14" known-issue note on the next release; RHEL 9 / Ubuntu 20.04 still uncovered -> zigbuild if needed; 22.04-runner deprecation horizon).

---

## Spec coverage check

- Runner swap to ubuntu-22.04 / -arm, mac/windows untouched -> Task 1 Step 1.
- Comment-block rationale + do-not-bump + zigbuild-when-retired note -> Task 1 Step 2.
- objdump guard, linux-only, all three binaries, numeric comparator, fail >2.35, after staging -> Task 2.
- CI-only verification acknowledgement + local guard-logic proof -> Task 2 Steps 1 & 4, Task 3 Step 3.
- v0.1.13 known-issue follow-up + sub-2.35 zigbuild note -> Task 3 Step 4 report (out-of-scope items, surfaced not implemented).

## Notes carried from review

- Comparator is `sort -V` (numeric), explicitly NOT a string sort -- handles 2.4 vs 2.39.
- Guard placed AFTER staging (operates on the staged `bin/` copies, ~line 106), before the --version smoke.
- 2.35 floor does NOT cover RHEL 9 (2.34) or Ubuntu 20.04 (2.31); documented, not promised.
