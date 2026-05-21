# tests/ - Fixture and test layout

Companion to `TESTING.md` at the repo root. Read that file first.

This file documents the per-fixture conventions and how to add new
fixtures without breaking the determinism / safety contract.

Language: ASCII only.

## 1. Layout

```text
tests/
  README.md                   # this file
  fixtures/
    terminal/                 # raw terminal stream excerpts
    command-output/           # whole-command run excerpts (small)
    files/                    # tailable file excerpts (rotating, growing)
    buckets/                  # structured bucket event examples (JSON)
    rules/                    # registry rule shapes (JSON)
    context/                  # event_context shape examples (JSON)
    policy/                   # policy decision fixtures (JSON)
    probes/
      wsl-mountinfo/          # /proc/self/mountinfo excerpts (Linux, WSL 9P, WSL ext4)
```

Per-crate Rust integration tests land later under
`crates/<crate>/tests/*.rs` and reference these fixtures by relative
path from the workspace root.

## 2. Naming

`tests/fixtures/<category>/<short-descriptor>.<ext>`

- `<category>` is one of the directories above. Adding a new
  category requires amending `TESTING.md` section 8 in a TC03-class
  goal.
- `<short-descriptor>` reads as the case being exercised. Examples:
  `apt-missing-package`, `cargo-compile-error`,
  `repeated-warning`, `read_only_observer-deny`.
- `<ext>` is `.stderr`, `.stdout`, `.txt`, `.json`, `.mountinfo`, or
  `.before` / `.after` (for rotation cases).

A fixture filename MUST NOT use camelCase or PascalCase. Hyphens
between words. Lowercase only.

## 3. Determinism

Fixtures MUST produce byte-identical output every time. Practical
rules:

- No live timestamps. Use the placeholder token `<TS>` and let the
  test substitute on compare.
- No process ids. Use `<PID>`.
- No absolute machine paths. Use `/home/dev/repo/` or `$REPO_ROOT/`
  inside the fixture text.
- No real hostnames. Use `host-a`, `host-b`.
- No real network identifiers (IPs, mac, ULIDs). Use synthetic
  prefixes (`evt_TEST_0001`, `192.0.2.0/24`, `10.0.0.0/8`).

## 4. Size

Hard cap per fixture: 256 lines OR 16 KiB, whichever comes first.
Larger inputs MUST be generated at test-time by a documented script
under `scripts/dev/`, not committed.

A fixture that needs more than the cap is two fixtures, or a
generated case.

## 5. Safety

No secrets, no real credentials, no real tokens, no real private
paths, no real customer data. If a real-world stream is a useful
template, redact it before committing.

`scripts/dev/verify-baseline.sh` greps for common leak patterns:
`-----BEGIN .* PRIVATE KEY-----`, `aws_secret_access_key=`,
`xoxb-`, `ghp_`, `sk-[A-Za-z0-9]{20}`, etc. The list is documented
in the script.

## 6. JSON fixtures

JSON fixtures MUST parse with `python3 -m json.tool`. The baseline
script enforces this.

JSON fixtures MUST conform to the schemas defined in TC05 once
TC05 lands. Until then, they document the SHAPE only and are
allowed to be informative-only. Each JSON fixture starts with a
top-level `_meta` field describing the fixture (it is part of the
shape only for fixtures; production payloads do not carry it):

```json
{
  "_meta": {
    "fixture_id": "buckets/build_42.events.json",
    "schema_anchor": "TC05/event-v1",
    "status": "informative-until-TC05"
  },
  ...
}
```

## 7. Adding a fixture

1. Identify the right category. If the category does not exist,
   amend `TESTING.md` section 8 first (TC03-class goal).
2. Author the fixture under 256 lines / 16 KiB, deterministic, ASCII,
   no secrets.
3. If JSON, validate with `python3 -m json.tool` locally.
4. Run `scripts/dev/verify-baseline.sh` and confirm PASS.
5. List the fixture path in the consuming test (Rust source).
6. Reference the fixture in the goal report's "Evidence" section.

## 8. WSL mountinfo fixtures

`tests/fixtures/probes/wsl-mountinfo/` is consumed by TC18 / TC20 /
TC25 to detect when the file probe MUST force `PollWatcher` (per
`docs/research/wsl-boundary.md`). Three fixtures:

- `native-linux.mountinfo` - typical Linux ext4 / btrfs / xfs roots.
- `wsl2-9p-drvfs.mountinfo` - `/mnt/c` and `/mnt/d` 9P mounts; the
  detection rule MUST flag these as inotify-broken and force poll.
- `wsl2-ext4.mountinfo` - WSL2 internal Linux filesystem; inotify
  works here.

These files contain real `mountinfo` line shapes (whitespace-separated
fields per `proc(5)`) but synthetic uids/paths. They are NOT pulled
from a live system without redaction.

## 9. Source-status notes per fixture category

| Category | Default status (until consumed) |
|---|---|
| terminal/ | test-only |
| command-output/ | test-only |
| files/ | test-only |
| buckets/ | informative-until-TC05 |
| rules/ | informative-until-TC05 |
| context/ | informative-until-TC05 |
| policy/ | informative-until-TC22 |
| probes/wsl-mountinfo/ | test-only (consumed by TC18/TC20/TC25) |

When a goal consumes a fixture in production-bound test code,
update its status in that goal's report.
