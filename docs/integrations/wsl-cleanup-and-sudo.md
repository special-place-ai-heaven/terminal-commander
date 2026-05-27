# WSL Cleanup + Scoped Sudo with Terminal Commander

How to run disk/cache cleanup inside WSL through Terminal Commander (TC), and
how to grant the narrow privileged access cleanup needs WITHOUT putting a
password anywhere it can leak. Distilled from a live cleanup session that
reclaimed ~12 GiB of WSL ext4 driven entirely through TC.

Related: the `terminal-commander doctor` setup-readiness section detects each
prerequisite below and prints the exact fix line when one is missing.

---

## 1. SUDO setup discipline (the supported privileged path)

This makes TC privileged cleanup work WITHOUT putting a password anywhere it
can leak (daemon env, logs, child argv, crash dumps).

**Principle:** never pipe a sudo password through an env var or stdin for
routine operation. Grant passwordless sudo scoped to ONLY the specific cleanup
binaries via a sudoers drop-in. The credential never travels; the grant is
auditable and narrow.

**One-time setup, run by the user inside the WSL distro** (interactive sudo
once):

```bash
echo "$USER ALL=(root) NOPASSWD: /usr/bin/apt-get, /usr/bin/journalctl, /usr/sbin/fstrim" \
  | sudo tee /etc/sudoers.d/tc-cleanup
sudo chmod 440 /etc/sudoers.d/tc-cleanup
sudo visudo -c -f /etc/sudoers.d/tc-cleanup   # must print: parsed OK
```

**Rules:**

- **Scope tight.** List only the exact binaries cleanup needs. Never
  `NOPASSWD: ALL`. To extend, add specific absolute paths, not wildcards.
- **`chmod 440` is mandatory** -- sudoers ignores group/other-writable
  drop-ins.
- **Always `visudo -c`** before trusting the file; a malformed sudoers can lock
  you out of sudo entirely.
- TC then drives privileged cleanup with `sudo -n <ABSOLUTE-PATH>`. The `-n`
  makes sudo fail loud instead of hanging on a password prompt. Verified end to
  end: `sudo -n /usr/sbin/fstrim -av` reported `/: 259.1 MiB ... trimmed`
  through TC with no prompt.

### GOTCHA: sudoers matches by EXACT command path

sudoers NOPASSWD grants match by the EXACT command path listed. The drop-in
lists absolute paths (`/usr/sbin/fstrim`), so TC MUST invoke the absolute path:

```bash
sudo -n /usr/sbin/fstrim -av     # MATCHES the grant -> runs, no prompt
sudo -n fstrim -av               # bare name via PATH -> does NOT match
                                 # -> password demand under -n -> false
                                 #    "sudo broken" signal
```

Always invoke the absolute path that appears in the sudoers line. This bit a
real session: a bare-name probe loop reported SUDO_FAIL while the grant was
fine; switching to the absolute path returned exit 0.

### Explicitly rejected: env-var password

The `WSL_SUDO_CREDENTIAL` env var + `WSLENV` forwarding + `sudo -S` approach is
REJECTED. It is fragile (a long-lived daemon freezes its process env, so a
freshly-set var needs a full client restart to take effect) and it leaks the
password into the daemon's environment and any child's view of it. The
env-forwarding allowlist (see the F6 daemon change: `WSLENV`, `TC_WSL_DISTRO`)
is for non-secret operational vars ONLY, never a password.

---

## 2. Activating cleanup rules (the `cleanup` pack)

TC suppresses unmatched stdout and returns only rule-matched signal events. For
cleanup commands, import the built-in `cleanup` rule pack so df/du/docker-df/
fstrim/reclaimed-space output is extracted as structured events:

```text
registry_import_pack name=cleanup activate=true scope={kind:"global"}
```

The pack ships embedded in the daemon -- no repo checkout needed at runtime. It
covers: `cleanup.disk-usage` (df -h), `cleanup.dir-size` (du -sh),
`cleanup.docker-usage` (docker system df), `cleanup.fstrim`, and
`cleanup.space-reclaimed`.

### Activation is future-only

Activating a rule (or importing the pack) binds NEWLY started commands only;
commands already running are NOT hot-rebound. The pattern is: **activate (or
import the pack) FIRST, THEN start the command.** Scope is REQUIRED on activate
(`{kind:"global"}` for a single agent watching its own commands); an omitted
scope is rejected, never silently widened to global.

### Reading output without a rule

For a one-off/exploratory command whose format you do not know yet (and for
which no rule matched), read the last lines without authoring a rule:

```text
command_output_tail job_id=<id> max_lines=50
```

Bounded to 200 lines / 64 KiB, truncation-flagged. For recurring signals you
care about, define a rule (or use the `cleanup` pack) instead.

---

## 3. Rule template syntax: `${name}`, not `{name}`

Rule `summary_template` placeholders use `${name}` (dollar-brace). Bare
`{name}` is treated as LITERAL text and will appear verbatim in summaries.

```json
"summary_template": "trimmed ${mount}: ${human}"     // correct -> "trimmed /: 2.1 GiB"
"summary_template": "trimmed {mount}: {human}"        // WRONG -> literal "{mount}"/"{human}"
```

The `cleanup` pack and all shipped packs use the correct `${name}` syntax.

---

## 4. Reclaiming WSL disk: fstrim frees blocks, but the .vhdx does not shrink

`fstrim` inside WSL tells the kernel which blocks are free, but it does NOT
shrink the Windows-side `ext4.vhdx` backing file. Shrinking the virtual disk is
a host-side, distro-down operation and is OUT OF SCOPE for TC (not automated):

```powershell
wsl --shutdown
# then, from an elevated Windows shell, either:
Optimize-VHD -Path "<path-to-ext4.vhdx>" -Mode Full   # Hyper-V module, or:
# diskpart: select vdisk file="<path>"; attach vdisk readonly; compact vdisk; detach vdisk
```

Run cleanup + `fstrim` through TC first (reclaims blocks inside the guest), then
do the manual `.vhdx` compaction above if you need the Windows file itself to
shrink.

---

## 5. What the doctor checks

`terminal-commander doctor` prints a "setup readiness" section. For each MISSING
item it prints the exact fix line:

- **WSL sudo cleanup grant (sudoers)** -> the scoped-NOPASSWD one-liner above.
- **daemon up-to-date** -> `terminal-commander restart`.
- **cleanup rule pack available** -> `registry_import_pack name=cleanup
  scope={kind:'global'}` (it ships built-in).

The sudoers probe uses the ABSOLUTE `fstrim` path (per the gotcha above), so a
green check means the grant actually works as TC will invoke it.
