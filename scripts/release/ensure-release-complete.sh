#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Ensure manifest version on main has a GitHub release at HEAD and npm is publishable.
# Emits GitHub Actions outputs: publish (true|false), version, need_release_please.

set -euo pipefail

GITHUB_OUTPUT="${GITHUB_OUTPUT:-/dev/stdout}"

manifest_path=".github/.release-please-manifest.json"
root_pkg="packages/terminal-commander/package.json"

version="$(
  node -e "
    const m = require('./${manifest_path}');
    const vals = Object.values(m);
    if (new Set(vals).size !== 1) process.exit(2);
    process.stdout.write(vals[0]);
  "
)"

root_ver="$(node -p "require('./${root_pkg}').version")"
if [ "$version" != "$root_ver" ]; then
  echo "::error::manifest ${version} != root package.json ${root_ver}"
  exit 1
fi

for pkg in packages/terminal-commander-linux-x64/package.json packages/terminal-commander-linux-arm64/package.json; do
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
    echo "retagging ${tag} from ${tag_sha} to HEAD ${head_sha}"
    git tag -f "$tag" "$head_sha"
    git push origin "$tag" --force
  fi
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
