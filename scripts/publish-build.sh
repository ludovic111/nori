#!/usr/bin/env bash
# Publish a nori build to lsuite (lsuite's DISTRIBUTION.md): the apps' builds are obtained only
# through the lsuite app, from the private ludovic111/lsuite-builds repository.
#
#   scripts/publish-build.sh <version> <run-id> [--dry-run] [--replace]
#
# Takes the artifacts of a run of ludovic111/kimchi's suite-build.yml (app=nori), checks every
# file's update signature against nori's key and the version, writes latest.json (nori-release
# manifest, notes from CHANGELOG.md) and SHA256SUMS, and creates the release nori-v<version> in
# ludovic111/lsuite-builds with exactly those files. The lsuite server reads it (LSUITE_BUILDS_TOKEN)
# and serves nori's updater and the lsuite app; signatures are unchanged, so nothing on the way
# can alter a build unnoticed.
#
#   --dry-run   do everything but create the release (the files stay in a folder it prints)
#   --replace   the release exists already: upload over its files
#
# Linux only during lsuite's beta (macOS and Windows are coming soon): only the targets in
# NORI_TARGETS (default x86_64-unknown-linux-gnu) are taken, and their jobs must have succeeded,
# so a run cancelled after its Linux job (or one that built other platforms too) still publishes
# Linux alone. latest.json then lists only those platforms.
#
# Needs gh (signed in, with access to both repositories) and cargo. SHA256SUMS is signed too
# (SHA256SUMS.sig) when TAURI_SIGNING_PRIVATE_KEY holds nori's update key. NORI_BUILD_REPO and
# LSUITE_BUILDS_REPO override the repositories (for a test run).
set -euo pipefail

die() {
  echo "publish-build: $*" >&2
  exit 1
}

version="" run_id="" dry_run=0 replace=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    --replace) replace=1 ;;
    -h | --help)
      awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
      exit 0
      ;;
    -*) die "unknown option $arg" ;;
    *)
      if [ -z "$version" ]; then version="$arg"
      elif [ -z "$run_id" ]; then run_id="$arg"
      else die "too many arguments"
      fi
      ;;
  esac
done
version="${version#v}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || die "usage: publish-build.sh <version> <run-id> [--dry-run] [--replace] (version like 0.2.0)"
[[ "$run_id" =~ ^[0-9]+$ ]] || die "usage: publish-build.sh <version> <run-id> (the run id of kimchi's suite-build.yml, from its URL)"

build_repo="${NORI_BUILD_REPO:-ludovic111/kimchi}"
builds_repo="${LSUITE_BUILDS_REPO:-ludovic111/lsuite-builds}"
read -r -a targets <<< "${NORI_TARGETS:-x86_64-unknown-linux-gnu}"
tag="nori-v$version"
root="$(cd "$(dirname "$0")/.." && pwd)"
command -v gh > /dev/null || die "gh (GitHub CLI) is needed"
command -v cargo > /dev/null || die "cargo is needed"

workspace_version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$root/Cargo.toml")
if [ "$workspace_version" != "$version" ]; then
  echo "publish-build: note: this checkout is $workspace_version; the files are checked against $version" >&2
fi

# The run: kimchi's suite build, finished, with a green job and an artifact for each target.
read -r workflow status url < <(gh run view "$run_id" -R "$build_repo" --json workflowName,status,url \
  --jq '[(.workflowName | gsub(" "; "_")), .status, .url] | @tsv')
[ "$workflow" = "Suite_release_build" ] || die "run $run_id of $build_repo is \"${workflow//_/ }\", not the suite release build"
[ "$status" = "completed" ] || die "run $run_id is still $status: wait for it ($url)"
artifacts=$(gh api "repos/$build_repo/actions/runs/$run_id/artifacts" --jq '.artifacts[] | select(.expired | not) | .name')
for target in "${targets[@]}"; do
  job=$(gh run view "$run_id" -R "$build_repo" --json jobs --jq ".jobs[] | select(.name == \"nori · $target\") | .conclusion")
  [ "$job" = "success" ] || die "run $run_id has no green \"nori · $target\" job (${job:-none}): publish only a green build ($url)"
  echo "$artifacts" | grep -qx "nori-$target" || die "run $run_id has no nori-$target artifact (artifacts: $(echo "$artifacts" | tr '\n' ' '))"
done

work="$(mktemp -d "${TMPDIR:-/tmp}/nori-publish-XXXXXX")"
if [ "$dry_run" -eq 0 ]; then trap 'rm -rf "$work"' EXIT; fi
echo "Downloading the nori artifacts of run $run_id ($url)…"
for target in "${targets[@]}"; do
  gh run download "$run_id" -R "$build_repo" -n "nori-$target" -D "$work/artifacts/$target"
done
mkdir -p "$work/release"
while IFS= read -r -d '' f; do
  name="$(basename "$f")"
  [ -e "$work/release/$name" ] && die "two artifacts carry $name"
  cp "$f" "$work/release/$name"
done < <(find "$work/artifacts" -type f -print0)

# Every update file signed, for this version, with nori's key.
echo "Checking signatures…"
cargo build --release --locked -q -p nori-release --manifest-path "$root/Cargo.toml"
release_tool="$root/target/release/nori-release"
signed=0
for f in "$work/release"/*; do
  case "$f" in
    *.sig | *.dmg) continue ;;
    *.app.tar.gz | *.exe | *.zip | *.AppImage | *.deb | *.tar.gz)
      [ -f "$f.sig" ] || die "$(basename "$f") has no signature (.sig)"
      "$release_tool" verify "$f" --version "$version"
      signed=$((signed + 1))
      ;;
  esac
done
[ "$signed" -gt 0 ] || die "no signed update files in the artifacts"

# What's new: the version's section of CHANGELOG.md.
notes="$work/notes.md"
awk -v v="$version" '
  /^## / { if (found) exit; if (index($0, "## " v " ") == 1 || $0 == "## " v) { found = 1; next } }
  found { print }
' "$root/CHANGELOG.md" > "$notes"
[ -s "$notes" ] || die "CHANGELOG.md has no \"## $version\" section"

# latest.json in the updater's format. Its URLs point at this release; the lsuite server rewrites
# each one to its own file route for nori's updater.
"$release_tool" manifest "$work/release" --version "$version" \
  --base-url "https://github.com/$builds_repo/releases/download/$tag" \
  --notes-file "$notes" --out "$work/release/latest.json"

(
  cd "$work/release"
  rm -f SHA256SUMS SHA256SUMS.sig
  if command -v sha256sum > /dev/null; then sha256sum -- * > "$work/SHA256SUMS"; else shasum -a 256 -- * > "$work/SHA256SUMS"; fi
  mv "$work/SHA256SUMS" SHA256SUMS
  if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
    "$release_tool" sign SHA256SUMS --version "$version"
  fi
)

echo "Files for $tag:"
(cd "$work/release" && ls -l)
if [ "$dry_run" -eq 1 ]; then
  echo "Dry run: nothing published. The files are in $work/release"
  exit 0
fi

if gh release view "$tag" -R "$builds_repo" > /dev/null 2>&1; then
  [ "$replace" -eq 1 ] || die "$tag already exists in $builds_repo (--replace uploads over its files)"
  gh release upload "$tag" -R "$builds_repo" --clobber "$work/release"/*
else
  gh release create "$tag" -R "$builds_repo" --title "nori $version" --notes-file "$notes" "$work/release"/*
fi
echo "Published $tag to $builds_repo: the lsuite app and nori's updater get it from lsuite.xyz within 5 minutes."
