#!/bin/sh
# Release kit: scripts/release.sh 0.4.0
# Takes the list in UNRELEASED.md as the release notes (the tag message, which CI publishes
# with the installer and the in-app update shows), bumps the version, empties the list,
# commits, tags and pushes main and the tag together. CI then builds and publishes the release.
set -e
cd "$(dirname "$0")/.."
v=${1:?usage: scripts/release.sh X.Y.Z}
notes=$(sed -n '/^- /,$p' UNRELEASED.md)
[ -n "$notes" ] || { echo "UNRELEASED.md has no changes listed"; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "commit or stash your changes first"; exit 1; }
[ "$(git rev-parse --abbrev-ref HEAD)" = main ] || { echo "release from main"; exit 1; }
git fetch -q origin main
[ -z "$(git rev-list HEAD..origin/main)" ] || { echo "main is behind origin/main; pull first"; exit 1; }
git rev-parse -q --verify "refs/tags/v$v" >/dev/null && { echo "v$v already exists"; exit 1; }
git ls-remote --exit-code --tags origin "refs/tags/v$v" >/dev/null && { echo "v$v already exists on origin"; exit 1; }

# Stop before anything is tagged if this commit doesn't build for Windows or failed on GitHub.
# CI only builds the APO and the app on Windows, so without this a compile error only shows up
# after the tag is pushed (as 0.9.0 did).
if [ "$(gh run list --workflow check.yml --commit "$(git rev-parse HEAD)" --json conclusion -q '.[0].conclusion' 2>/dev/null)" = failure ]; then
    echo "the check workflow failed on this commit; fix it first"; exit 1
fi
export XWIN_ACCEPT_LICENSE=1
if [ ! -f target/release/kit_apo.dll ]; then
    # tauri-build needs the DLL it bundles to exist.
    cargo xwin build -q --release --locked --target x86_64-pc-windows-msvc -p kit-apo || { echo "the APO doesn't build for Windows"; exit 1; }
    mkdir -p target/release
    cp target/x86_64-pc-windows-msvc/release/kit_apo.dll target/release/
fi
cargo xwin check -q --locked --target x86_64-pc-windows-msvc --workspace --all-targets || { echo "kit doesn't build for Windows; nothing was changed"; exit 1; }

files="UNRELEASED.md Cargo.lock app/src-tauri/Cargo.toml app/src-tauri/tauri.conf.json"
undo() {
    # shellcheck disable=SC2086
    git checkout -q -- $files
    echo "release stopped; the version files are as they were"
    exit 1
}
sed -i "0,/^version = \".*\"/s//version = \"$v\"/" app/src-tauri/Cargo.toml || undo
sed -i "0,/\"version\": \".*\"/s//\"version\": \"$v\"/" app/src-tauri/tauri.conf.json || undo
cargo update -p kit --offline -q || undo
sed -i '/^- /,$d' UNRELEASED.md || undo

# shellcheck disable=SC2086
git add $files
git commit -qm "release $v"
printf '%s\n' "$notes" | git tag -a "v$v" -F -
# Both or neither: a release commit without its tag (or the reverse) can't happen.
git push -q --atomic origin main "v$v"
echo "pushed v$v; CI is building the release:"
printf '%s\n' "$notes"
