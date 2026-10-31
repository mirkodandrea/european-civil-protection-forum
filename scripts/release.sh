#!/bin/sh
# Release X.Y.Z from main: set the workspace version, commit, tag vX.Y.Z and
# push. The tag starts .github/workflows/desktop.yml, which builds the
# Windows, macOS and Linux zips and publishes them as the vX.Y.Z release.
#
#   ./scripts/release.sh 0.2.0
set -eu
cd "$(dirname "$0")/.."

version=${1:?usage: scripts/release.sh X.Y.Z}
echo "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || { echo "version must be X.Y.Z"; exit 1; }
tag="v$version"

[ "$(git branch --show-current)" = main ] || { echo "release from main"; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "the working tree is not clean"; exit 1; }
git fetch -q origin main --tags
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || { echo "main differs from origin/main: pull or push first"; exit 1; }
git rev-parse -q --verify "refs/tags/$tag" >/dev/null && { echo "$tag already exists"; exit 1; }

# The version lives in [workspace.package]; every crate inherits it.
sed -i.bak "/^\[workspace.package\]/,/^\[/ s/^version = \".*\"/version = \"$version\"/" Cargo.toml
rm Cargo.toml.bak
grep -q "^version = \"$version\"" Cargo.toml || { echo "could not set the version in Cargo.toml"; exit 1; }
cargo update -q --workspace

git commit -qam "Release $tag"
git tag -a "$tag" -m "PPRD East TTX $tag"
git push -q origin main "$tag"
echo "pushed $tag: the desktop builds publish the release when they finish"
