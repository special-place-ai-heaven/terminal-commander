#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Ensure manifest version on main has a GitHub release at HEAD and npm is publishable.
# Emits GitHub Actions outputs: publish (true|false), version, need_release_please.

set -euo pipefail

GITHUB_OUTPUT="${GITHUB_OUTPUT:-/dev/stdout}"

manifest_path=".github/.release-please-manifest.json"
root_pkg="packages/terminal-commander/package.json"

# The npm package graph + manifest + Cargo versions are kept in lockstep by
# release-pr-sync.yml (sync-optional-dependencies.js + sync-cargo-versions.py)
# on the release PR. If that sync is ever SKIPPED (e.g. the workflow was
# temporarily disabled, or a pull_request event was missed) the release PR can
# merge with root bumped but the 5 platform packages / manifest / Cargo still on
# the OLD version. That split previously killed this script (silent exit 2) and
# left the release with no tag. Self-heal it here: the root package.json is the
# version release-please bumps first and is the source of truth.
synced_paths=(
  .github/.release-please-manifest.json
  packages/terminal-commander/package.json
  packages/terminal-commander-linux-x64/package.json
  packages/terminal-commander-linux-arm64/package.json
  packages/terminal-commander-windows-x64/package.json
  packages/terminal-commander-mac-x64/package.json
  packages/terminal-commander-mac-arm64/package.json
  Cargo.toml
  Cargo.lock
  crates/daemon/Cargo.toml
  crates/mcp/Cargo.toml
  crates/probes/Cargo.toml
)

manifest_consistent() {
  node -e "
    const m = require('./${manifest_path}');
    process.exit(new Set(Object.values(m)).size === 1 ? 0 : 1);
  "
}

root_ver="$(node -p "require('./${root_pkg}').version")"

# Self-heal a skipped sync, but ONLY at a genuine release boundary: the root
# version has NO tag yet (a pending release), and the manifest is split or lags
# the root. This guard means normal post-release pushes (root == an existing tag)
# NEVER trigger a sync/push from here.
if ! git rev-parse "v${root_ver}" >/dev/null 2>&1 && ! manifest_consistent; then
  echo "::warning::manifest is split (root package.json=${root_ver}); syncing platform/manifest/Cargo versions (release-pr-sync was skipped for this release)."
  node scripts/release/sync-optional-dependencies.js
  python3 scripts/release/sync-cargo-versions.py "${root_ver}"
  if git diff --quiet "${synced_paths[@]}"; then
    echo "::error::root is ${root_ver} with no tag and a split manifest, but the sync produced no change -- cannot self-heal. Manual investigation needed."
    exit 2
  fi
  git config user.name "github-actions[bot]"
  git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
  git add "${synced_paths[@]}"
  git commit -m "chore(release): sync platform + Cargo versions to ${root_ver}

Self-heal from ensure-release: release-pr-sync did not run the version sync for
this release, leaving a split manifest. Bringing all packages, the manifest, and
Cargo versions to the root version so the tag/publish can proceed."
  # NOTE: the workflow checks ensure-release out with the RELEASE_PLEASE_TOKEN_TC
  # PAT so this push authenticates correctly AND re-triggers release automation
  # (a push under the default GITHUB_TOKEN would not re-trigger -- see
  # release-pr-sync.yml). The fresh push:main re-runs this job on the now-consistent
  # tree, which proceeds past the checks below and cuts the tag.
  git push origin HEAD:main
  echo "synced and pushed; the resulting push:main re-runs release with a consistent manifest."
  {
    echo "version=${root_ver}"
    echo "publish=false"
    echo "need_release_please=false"
  } >>"$GITHUB_OUTPUT"
  exit 0
fi

version="$(
  node -e "
    const m = require('./${manifest_path}');
    const vals = Object.values(m);
    if (new Set(vals).size !== 1) process.exit(2);
    process.stdout.write(vals[0]);
  "
)"

if [ "$version" != "$root_ver" ]; then
  echo "::error::manifest ${version} != root package.json ${root_ver}"
  exit 1
fi

# Verify ALL 5 platform packages share the manifest version, not just the 2 linux ones.
# Codex review I3 (2026-05-25): the prior 2-platform check let drift slip through on
# windows-x64 + mac-x64 + mac-arm64.
for pkg in packages/terminal-commander-linux-x64/package.json \
           packages/terminal-commander-linux-arm64/package.json \
           packages/terminal-commander-windows-x64/package.json \
           packages/terminal-commander-mac-x64/package.json \
           packages/terminal-commander-mac-arm64/package.json; do
  pv="$(node -p "require('./${pkg}').version")"
  if [ "$pv" != "$version" ]; then
    echo "::error::${pkg} version ${pv} != manifest ${version}"
    exit 1
  fi
done

tag="v${version}"
head_sha="$(git rev-parse HEAD)"
need_rp="false"
publish="false"

if git rev-parse "$tag" >/dev/null 2>&1; then
  tag_sha="$(git rev-parse "${tag}^{commit}")"
  if [ "$tag_sha" != "$head_sha" ]; then
    # Existing tag at ${tag_sha} != HEAD ${head_sha} is the NORMAL case:
    # the manifest version (e.g. 0.1.4) was already released, and subsequent
    # commits have landed on main since that release. ensure-release runs on
    # EVERY main push, so most invocations land here. Do nothing — this is
    # not a release PR merge; release-please will detect the next release
    # boundary when conventional commits accumulate enough to bump the version.
    #
    # Codex C1 fix (2026-05-25): the prior version force-retagged here, which
    # silently rewrote published release history. The new version exits 0 with
    # publish=false (the workflow's downstream publish jobs won't fire).
    echo "tag ${tag} exists at ${tag_sha}; HEAD ${head_sha} is post-release. No publish work for this push."
    publish="false"
    need_rp="false"
    {
      echo "version=${version}"
      echo "publish=${publish}"
      echo "need_release_please=${need_rp}"
    } >>"$GITHUB_OUTPUT"
    exit 0
  fi
  # tag_sha == head_sha: the release PR was just merged and the tag is
  # already at HEAD. Continue to npm-publish-gap check below.
else
  need_rp="true"
fi

if ! gh release view "$tag" >/dev/null 2>&1; then
  echo "creating missing GitHub release ${tag}"
  notes_file="packages/terminal-commander/CHANGELOG.md"
  if [ -f "$notes_file" ]; then
    gh release create "$tag" --title "$tag" --notes-file "$notes_file"
  else
    gh release create "$tag" --title "$tag" --notes "Release ${version}"
  fi
fi

if ! npm view "terminal-commander@${version}" version >/dev/null 2>&1; then
  publish="true"
  echo "npm publish needed for terminal-commander@${version}"
else
  echo "npm already has terminal-commander@${version}"
fi

{
  echo "version=${version}"
  echo "publish=${publish}"
  echo "need_release_please=${need_rp}"
} >>"$GITHUB_OUTPUT"
