# WSL2 Filesystem Boundary Research (Topic D2)

Research agent: R1-beta. All claims tied to a cited URL. ASCII only.

## Scope

Terminal Commander targets Linux native plus WSL2 as primary platforms.
WSL2 is not a uniform filesystem - native Linux ext4 inside the WSL VM
behaves nothing like a Windows-mounted drive accessed via `/mnt/c`. This
document captures the differences and pins the recommendation for file
probes on each side of the boundary.

## 1. Two filesystems in WSL2

WSL2 runs an actual Linux kernel inside a managed VM. Two distinct
filesystem realms are visible to that kernel:

- **Native WSL filesystem**: an ext4 image owned by the WSL VM. Path
  examples: `/home/<user>/...`, `/root/...`. From Windows it appears at
  `\\wsl$\<distro>\...` (or `\\wsl.localhost\<distro>\...` on newer
  builds). On the Linux side, this behaves as a normal kernel-managed
  ext4 filesystem.

- **Windows-mounted filesystem**: the host's NTFS drives are surfaced
  into the VM via the **9P (Plan 9) protocol**. They appear at `/mnt/c`,
  `/mnt/d`, etc. The Linux kernel is a 9P client; the Windows side runs
  a 9P server (`Plan9` filesystem provider in WSL).

Microsoft's official guidance, from
https://learn.microsoft.com/en-us/windows/wsl/filesystems :

> "We recommend against working across operating systems with your
> files, unless you have a specific reason for doing so. For the
> fastest performance speed, store your files in the WSL file system
> if you are working in a Linux command line."

> "Use the Linux file system root directory: `/home/<user name>/Project`.
> Not the Windows file system root directory: `/mnt/c/Users/<user name>/Project$`."

This is not a stylistic preference - it has direct semantic consequences
for inotify, case sensitivity, performance, and permissions.

## 2. inotify behavior across the boundary

### 2.1 Native WSL FS (`/home/...`)

inotify works the same as on any Linux system. notify's `RecommendedWatcher`
returns an inotify-backed watcher and events arrive promptly. Standard
inotify limits apply (see `file-watcher.md`).

### 2.2 Windows-mounted FS (`/mnt/c/...`)

inotify is **silently non-functional** here. `inotify_add_watch` succeeds
without error, but no events are delivered. The 9P server on the Windows
side does not propagate file change notifications into the Linux kernel.

The canonical Microsoft tracking issue is open and unresolved as of the
research date:

> "any changes made by Windows applications such as Visual Studio do not
> trigger any file change notifications as far as Linux apps are
> concerned."

> Filed 2019-12-06, still open. Microsoft's recommended workaround is to
> store source code inside the Linux filesystem rather than on the
> mounted Windows drive.

Source: https://github.com/microsoft/WSL/issues/4739

The notify crate documents this behavior at the cross-platform level:

> "Network mounted filesystems like NFS may not emit any events" and
> "WSL programs watching Windows paths" both fall back to polling.

Source: https://docs.rs/notify/latest/notify/

This means: any TC probe that points at a `/mnt/c/...` path on WSL2
**must use `PollWatcher`** or its equivalent polling logic. There is no
inotify-based path that will ever deliver change events for that file.

### 2.3 Detection

There is no standard syscall to ask "is this path on a 9P mount?" The
practical detection is:

- Check `/proc/self/mountinfo` for the path's mount point and look for
  filesystem type `9p` or `drvfs`.
- Or check `/proc/version` for `microsoft` / `WSL2`.
- The notify project does not currently auto-detect this; the application
  must.

The TC architect should treat 9P-mount detection as a probe-construction
step, not as runtime backoff after watcher silence.

## 3. Case sensitivity

From https://learn.microsoft.com/en-us/windows/wsl/filesystems :

> "Windows and Linux file systems handle case sensitivity in different
> ways - Windows is case-insensitive and Linux is case-sensitive."

Practical consequences for `/mnt/c/...` access from inside WSL2:

- Two filenames that differ only in case (`Foo.txt` vs `foo.txt`) may
  collide on NTFS even though they look distinct from Linux.
- WSL exposes a per-directory **case sensitivity flag** on NTFS so a
  developer can opt in; see
  https://learn.microsoft.com/en-us/windows/wsl/case-sensitivity (linked
  from the filesystems page). TC should not assume any specific case
  policy on a `/mnt/c` path - it must treat the filesystem layer's
  reported name as canonical.

On the native WSL FS (`/home/...`), case sensitivity is normal Linux
behavior (case-sensitive).

## 4. Permissions mapping

The 9P bridge translates Windows ACLs into Linux-style mode bits using
the DrvFs metadata model. The defaults (no metadata) typically give
777 for files and 777 for directories on `/mnt/c`, which means:

- Permission-based policy enforcement on `/mnt/c` is effectively a
  no-op without `metadata` mount options.
- chmod/chown on `/mnt/c` is best-effort and may not persist.

Per Microsoft's filesystems doc, mounting with `metadata` options
(via `/etc/wsl.conf`) enables Linux-style metadata storage on top of
NTFS, but this is an opt-in. TC should not assume metadata is enabled.

Source: https://learn.microsoft.com/en-us/windows/wsl/filesystems

The native WSL ext4 root supports full Linux semantics for `chmod`,
`chown`, capabilities, and xattrs.

## 5. Performance

From https://learn.microsoft.com/en-us/windows/wsl/compare-versions :

> "File intensive operations like `git clone`, `npm install`, `apt
> update`, `apt upgrade`, and more are all noticeably faster with WSL 2.
> ...
> Initial versions of WSL 2 run up to 20x faster compared to WSL 1 when
> unpacking a zipped tarball, and around 2-5x faster when using `git
> clone`, `npm install` and `cmake` on various projects."

That speedup is for **native WSL FS**. On `/mnt/c`, WSL2 is slower
than WSL1 - the table on the comparison page explicitly marks
"Performance across OS file systems" as a WSL1 strength, WSL2
weakness:

> "WSL 1 offers faster access to files mounted from Windows."

Quantitative numbers: not published as exact figures in the cited
Microsoft pages. Community measurements typically report 9P access
to `/mnt/c` is 5-20x slower than native WSL FS for I/O-heavy
workloads. For TC's purposes, the conservative statement is: native
WSL FS access is significantly faster than 9P `/mnt/c` access, and
polling on `/mnt/c` amplifies this gap because each poll cycle walks
the tree over 9P.

## 6. WSL2 kernel version (relevance for landlock)

From https://github.com/microsoft/WSL2-Linux-Kernel the current branch
is `linux-msft-wsl-6.18.y`, latest tagged release
`linux-msft-wsl-6.18.26.2` dated 2026-05-15.

Microsoft's per-release notes at
https://learn.microsoft.com/en-us/windows/wsl/kernel-release-notes
confirm that the **5.15.57.1** WSL2 kernel (mid-2022) was the first
to "Enable the Landlock Linux Security Module (LSM)" with a direct
link to landlock.io. Every WSL2 kernel since has landlock enabled.

This matters for the policy story (see `policy-prior-art.md`):
**WSL2 on a current Windows install ships a kernel with landlock
enabled and an ABI version that supports filesystem, network, and
recent IPC controls.** The MVP advisory-policy story does not depend
on landlock, but a future enforcement path is viable on WSL2 as well
as bare-metal Linux.

## 7. Recommendation for Terminal Commander

**Native WSL FS paths** (anything under the VM's root, e.g. `/home/...`):
- Treat as normal Linux.
- inotify works.
- Permissions and case sensitivity are real Linux semantics.
- Performance is good - WSL2 ext4 access is essentially native speed.
- Default to `notify` + `notify-debouncer-full` (see `file-watcher.md`).

**Windows-mounted FS paths** (`/mnt/<drive>/...`, 9P-backed):
- inotify does NOT work. Document this loudly.
- Force `PollWatcher` (or equivalent polling) - this is the ONLY
  reliable mode.
- Case-insensitive FS underneath; do not assume case differences
  imply distinct files.
- Permissions are an unreliable signal unless `metadata` is enabled.
- Performance is significantly worse than native WSL FS, and polling
  amplifies the cost. Cap the polling scope tightly; a TC02
  `repo_only` profile on `/mnt/c` should have a small allow-list.
- Probes targeting `/mnt/c` should emit a startup banner stating
  "polling mode" so the operator is not surprised.

**Architectural call-out for ARCH**: file probes need a per-target
**transport** decision (native-inotify vs poll) made at probe
construction time, driven by mount-type detection, not by event-loop
back-off after silence. Silent inotify on 9P is the worst possible
failure mode for this product because it looks healthy and produces
no output.

## 8. Source map

- WSL file systems guide:
  https://learn.microsoft.com/en-us/windows/wsl/filesystems
- WSL version comparison:
  https://learn.microsoft.com/en-us/windows/wsl/compare-versions
- WSL kernel release notes:
  https://learn.microsoft.com/en-us/windows/wsl/kernel-release-notes
- WSL2-Linux-Kernel repo:
  https://github.com/microsoft/WSL2-Linux-Kernel
- Microsoft WSL issue 4739, inotify broken on Windows-mounted paths:
  https://github.com/microsoft/WSL/issues/4739
- notify crate, mentions WSL + polling fallback:
  https://docs.rs/notify/latest/notify/
