# Policy Enforcement Prior Art (Topics F1, F2)

Research agent: R1-beta. All claims tied to a cited URL. ASCII only.

## Scope

Terminal Commander needs a "policy" layer that decides what a probe is
allowed to read, watch, write, and exfiltrate. TC02 locks four profiles:
`developer_local`, `repo_only`, `read_only_observer`, and `admin_debug`.
This document surveys the available enforcement mechanisms on Linux,
covers their kernel/userspace boundaries, and pins a recommendation for
the MVP layer.

## 1. Two enforcement worlds: capability vs advisory

Before listing tools: there is a frame-level distinction that the
architect must absorb before picking primitives.

- **Capability / kernel enforcement** = the kernel itself rejects the
  operation. The process cannot perform the forbidden action because
  the OS will return `EPERM` (or similar). Examples: Landlock,
  seccomp-bpf, AppArmor, SELinux. The process can be malicious or
  buggy; the policy still holds.

- **Advisory / in-process enforcement** = the application's own code
  checks policy before performing an action. Examples: path-allow-list
  checks in TC's probe-runner; cap-std's directory-handle pattern when
  used as a coding discipline rather than a kernel-enforced sandbox.
  If the process is compromised, the policy is irrelevant - the
  attacker just calls `open()` directly.

For MVP, the realistic posture is **advisory enforcement** (TC's own
code policing TC's own probes), with the kernel-enforcement path as a
named future hardening goal. Justification is in section 7.

## 2. Linux Landlock

### 2.1 What it is

From https://landlock.io/ :

> "Landlock empowers any process, including unprivileged ones, to
> securely restrict themselves... Landlock is a stackable Linux
> Security Module (LSM) that makes it possible to create such
> security sandboxes."

Landlock is **opt-in self-sandboxing**. A process calls into the
kernel to install a ruleset restricting its own future access (and
its descendants). Crucially, no privilege is required - any
unprivileged process can apply Landlock to itself. This is the
opposite of SELinux / AppArmor, which require system-wide policy
configured by an admin.

### 2.2 Kernel version timeline

Landlock landed in Linux **5.13** (June 2021). It has grown in
capability across kernel releases, exposed as an "ABI version":

- **ABI v1** (Linux 5.13): filesystem access control (read, write,
  execute, etc.).
- **ABI v2**: adds `LANDLOCK_ACCESS_FS_REFER` - controlled rename and
  hardlink across directories.
- **ABI v3**: adds file truncation control.
- **ABI v4**: adds network access control (TCP bind/connect to
  specific ports).
- **ABI v5**: adds IOCTL on character/block devices via
  `AccessFs::IoctlDev`.
- **ABI v6**: adds abstract UNIX socket and signal scoping (IPC
  isolation).

Source: https://landlock.io/rust-landlock/landlock/enum.ABI.html and
https://docs.kernel.org/userspace-api/landlock.html

### 2.3 Rust binding

- Crate: `landlock`, current version **0.4.4**.
  Source: https://docs.rs/landlock/latest/landlock/
- License: MIT OR Apache-2.0.
- Minimum supported kernel: Linux **5.13** for the basic feature; the
  binding exposes features up to the current ABI (filesystem, network,
  and scope rights).
- Project home: https://landlock.io/ with the binding at
  https://github.com/landlock-lsm/rust-landlock
- Minimum supported Rust: 1.68.

### 2.4 Landlock on WSL2

Per the WSL2 kernel release notes:

> "5.15.57.1 ... Enable the Landlock Linux Security Module (LSM) -
> https://landlock.io/"

Source:
https://learn.microsoft.com/en-us/windows/wsl/kernel-release-notes

Every WSL2 kernel since (5.15.57.1 in 2022, through the current
6.18.y branch at
https://github.com/microsoft/WSL2-Linux-Kernel ) has Landlock
enabled. **Landlock is viable on WSL2 today.**

### 2.5 Landlock limits

- **Filesystem-rule granularity**: rules attach to file descriptors of
  directories. The ruleset says "this set of paths is granted these
  access rights." Effective for read/write/exec restriction; less
  effective for fine-grained per-syscall control.
- **No retroactive effect**: rules apply to future syscalls only;
  already-open file descriptors continue to work.
- **No content-level inspection**: Landlock decides "can this path be
  opened" not "is this file's content permitted." Content-based policy
  remains an application-layer responsibility.
- **No process-tree introspection**: Landlock is about what this
  process can do, not about observing what other processes do. For
  TC, this aligns with the in-process probe-restriction story.

## 3. seccomp-bpf

### 3.1 What it is

Linux's seccomp-bpf lets a process install a BPF program that filters
its own syscalls. Like Landlock, it is self-applied and unprivileged.
Unlike Landlock, the granularity is **syscall + arguments**, not
filesystem paths.

### 3.2 Rust bindings

**`seccompiler`** (rust-vmm):
- Current version **0.5.0**.
  Source: https://docs.rs/seccompiler/latest/seccompiler/
- License: Apache-2.0 OR BSD-3-Clause.
- Maintained by the rust-vmm group (Firecracker contributors).
- "High-level wrappers for working with system call filtering." Filters
  defined via Rust code or JSON.
- No system dependency - pure Rust BPF program generation.

**`libseccomp`** (binding to the C libseccomp):
- Current version **0.4.0**.
  Source: https://docs.rs/libseccomp/latest/libseccomp/
- License: MIT OR Apache-2.0.
- Requires the system `libseccomp` library (FFI binding).
- Higher-level API but adds a system dependency.

### 3.3 Fit for TC

seccomp-bpf is the right primitive when you want to assert "no probe
ever performs `execve` of an arbitrary binary" or "no probe ever
opens raw sockets." It is the wrong primitive for "no probe reads
files outside `/repo/...`" - that is Landlock's job.

The two layers compose: seccomp catches "wrong kind of syscall,"
Landlock catches "right syscall, wrong path."

## 4. cap-std (capability-based filesystem)

### 4.1 What it is

From https://github.com/bytecodealliance/cap-std :

> "Cap-std is a capability-based version of the Rust standard library...
> Rather than using ambient authority (requesting any resource by name),
> cap-std requires a capability object. For filesystem access,
> developers need a `Dir` handle."

> "Paths attempting to escape the directory - through `..`, symlinks,
> or absolute paths - return `PermissionDenied` errors, enabling
> fine-grained sandboxing within applications."

- Current version of `cap-std`: **4.0.2**.
  Source: https://docs.rs/cap-std/latest/cap_std/
- License: Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT.
- Supports Linux, macOS, Windows, FreeBSD.
- On Linux 5.6+, uses `openat2(RESOLVE_BENEATH)` for single-call safe
  path resolution. On older kernels it emulates the same semantics in
  userspace. On Windows it uses analogous APIs.
- It is the foundation of Wasmtime's WASI implementation.

### 4.2 Capability vs kernel

cap-std is best described as **advisory enforcement enforced by API
shape**. If TC's code uses cap-std exclusively for filesystem access,
the type system makes it very hard to accidentally read outside a
granted `Dir`. But cap-std cannot prevent a compromised process from
calling `libc::open()` directly. It is a "make-it-hard-to-screw-up"
layer for in-process discipline, not a sandbox the kernel enforces.

In practice cap-std pairs beautifully with Landlock: cap-std for
clean code, Landlock as the kernel-side belt-and-braces.

## 5. Out-of-process sandbox tools (brief mention)

These are full sandboxes operated by an admin (or a launching parent),
not in-process libraries. Listed for completeness; not recommended as
TC primitives.

### 5.1 AppArmor

- Linux LSM, profile-based, name resolution by path.
- Profiles live in `/etc/apparmor.d/`.
- Distro-specific deployment (Ubuntu and SUSE ship it; Debian
  optionally; RHEL family typically does not).
- A TC deployment that wants kernel-enforced policy on a host without
  Landlock could in principle ship an AppArmor profile, but this
  shifts policy out of TC into the host admin's responsibility.

### 5.2 SELinux

- Linux LSM, label-based.
- Used heavily on RHEL family and Fedora.
- Same out-of-process concern as AppArmor.

### 5.3 bubblewrap (`bwrap`)

- User-namespace-based sandbox tool (Flatpak's sandbox primitive).
- Launches a child process inside a custom namespace with bind-mounted
  filesystem.
- Useful for "run this probe in a private filesystem view," but
  out-of-process: TC would shell out to `bwrap` to launch the probe.
- Project: https://github.com/containers/bubblewrap (not fetched here
  but cited for completeness; verify before adopting).

### 5.4 firejail

- SUID-helper sandbox launcher; uses namespaces + seccomp + AppArmor
  underneath. Distro-packaged on most Linuxes.
- Similar trade-offs to bubblewrap.

For MVP, neither AppArmor, SELinux, bubblewrap, nor firejail is the
right primitive. They are useful in a defense-in-depth story that
puts TC itself inside one of these sandboxes; that is a deployment
concern, not a TC design concern.

## 6. Mapping to TC02 policy profiles

TC02 names four profiles. Mapping each to enforcement mechanism:

| Profile | Advisory (MVP) | Future kernel layer |
| --- | --- | --- |
| `developer_local` | Path allow-list + denylist enforced in TC | Landlock ruleset matching the allow-list |
| `repo_only` | cap-std `Dir` rooted at the repo | Landlock + cap-std combo |
| `read_only_observer` | Path allow-list + write-denied flag in TC | Landlock RO ruleset; optional seccomp filter blocking write syscalls |
| `admin_debug` | Audit-log-only, no path restriction | No kernel restriction (this is the "diagnostic" profile) |

Note the unverified assumption that `admin_debug` is intentionally
unrestricted - I am inferring from the name; the architect should
confirm.

## 7. MVP recommendation: advisory enforcement

**Pick advisory enforcement for MVP.** Implement policy as:

1. **Static policy file per profile** (TOML or similar), declaring:
   - Allowed read roots.
   - Allowed write roots (often empty).
   - Allowed probe kinds.
   - Maximum stream/event rates.
   - Audit-log requirements.

2. **In-process gatekeeping**: the probe-runner consults the policy
   before opening any file, spawning any process, or emitting any
   event. Violations are logged to the audit store and the offending
   probe is killed.

3. **cap-std for filesystem access**: every probe receives a `Dir`
   handle to its allowed root and is expected to use it exclusively.
   Audit-log any fallback to raw filesystem APIs.

4. **Audit log is authoritative**: every probe action records a
   policy-evaluation record. This is the integrity story for MVP.

### 7.1 Why advisory, not kernel

- **Portability**: WSL2 has Landlock, but the development surface
  includes pre-5.13 kernels in CI and bare Linux distros, and macOS
  is a deferred-but-known target. Landlock is Linux-only.
- **MVP scope**: kernel sandboxing requires careful integration
  testing per kernel ABI version. The Landlock binding's compatibility
  layer helps, but each new ABI brings work.
- **Threat model**: TC's adversary in MVP is mostly accidental misuse
  by operators, plus careless probe authors. Advisory enforcement
  catches both. The "fully compromised process" threat is not the
  TC02 MVP threat.
- **Composition**: an advisory-policy layer is the right interior
  even when kernel enforcement is added later. Landlock and seccomp
  reuse the same allow-list, they just push it through a different
  enforcement point.

### 7.2 Hardening roadmap (post-MVP)

In rough order of value:

1. **Landlock on Linux** (and WSL2): translate the existing policy
   profile into a Landlock ruleset at probe startup. ABI v4+ also
   gives network controls.
2. **seccomp-bpf**: a narrow allow-list of syscalls that probes are
   expected to perform. Catches `execve`, raw sockets, and ptrace
   without needing path-level reasoning.
3. **bubblewrap or systemd unit hardening** at the deployment layer:
   `ProtectSystem=strict`, `ReadOnlyPaths=`, `NoNewPrivileges=true`.
   But: per the pre-confirmed constraint, ARCH must NOT assume
   systemd in WSL. Document this option for bare-metal Linux only.
4. **macOS sandbox / Seatbelt**: out of MVP scope; revisit when
   macOS lands.

### 7.3 What "policy" means if MVP doesn't kernel-enforce

This is the honest framing for the spec:

> "MVP policy is advisory. The TC daemon polices its own probes and
> audits every action. Policy violations do not crash the kernel
> caller - they fail the probe, log to the audit store, and may halt
> the daemon depending on the profile. Future versions will compile
> the same policy into a Landlock ruleset for kernel-level
> enforcement, and a seccomp filter for syscall-level enforcement.
> The advisory layer is the source of truth; kernel layers will be
> derived from it."

This framing keeps the door open to defense-in-depth while being
honest about the MVP guarantee. It also matches how every serious
production policy system has historically been built: policy as
data, then multiple compilation backends.

## 8. Unverified / requires user decision

- **The `admin_debug` profile's intent**: I assumed it is the
  "diagnostic" profile with the loosest restrictions. ARCH should
  confirm whether it imposes any restrictions.
- **Cross-host policy**: if TC ever needs a fleet-wide policy
  (push policy from a control plane), that is a separate research
  topic. Not in MVP scope.
- **Per-probe vs per-process Landlock**: Landlock applies to a process
  and its descendants. If TC runs all probes in a single process,
  Landlock can sandbox at the process level - cheap and clean. If
  each probe is a child process, each child can install its own
  ruleset. ARCH must pick the process model first; policy follows.

## 9. Source map

- Landlock home: https://landlock.io/
- Landlock Linux kernel docs:
  https://docs.kernel.org/userspace-api/landlock.html
- rust-landlock crate docs:
  https://docs.rs/landlock/latest/landlock/
- rust-landlock ABI enum (per-ABI feature list):
  https://landlock.io/rust-landlock/landlock/enum.ABI.html
- WSL2 kernel release notes (5.15.57.1 enables Landlock):
  https://learn.microsoft.com/en-us/windows/wsl/kernel-release-notes
- WSL2-Linux-Kernel repo (current 6.18.y branch):
  https://github.com/microsoft/WSL2-Linux-Kernel
- seccompiler crate docs:
  https://docs.rs/seccompiler/latest/seccompiler/
- libseccomp Rust crate docs:
  https://docs.rs/libseccomp/latest/libseccomp/
- cap-std docs.rs: https://docs.rs/cap-std/latest/cap_std/
- cap-std GitHub: https://github.com/bytecodealliance/cap-std
