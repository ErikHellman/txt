#!/usr/bin/env bash
# Bump the version, commit, tag, and push a new release.
# See RELEASES.md for the full release process.
#
# Usage: scripts/release.sh <version>
#   version  X.Y.Z or X.Y.Z-label.N  (e.g. 1.2.3 or 1.2.3-rc.1)
#            A leading 'v' is stripped automatically.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CARGO_TOML="${ROOT}/Cargo.toml"

# ── Helpers ───────────────────────────────────────────────────────────────────

die() { echo "error: $*" >&2; exit 1; }

usage() {
  cat >&2 <<EOF
Usage: $(basename "$0") <version>

Bumps the version in Cargo.toml, commits, tags, and pushes.

  version  X.Y.Z or X.Y.Z-label.N  (e.g. 1.2.3 or 1.2.3-rc.1)
           A leading 'v' is stripped automatically.
EOF
  exit 1
}

# Returns 0 (true) if $1 < $2, comparing X.Y.Z numerically.
# Pre-release suffixes are ignored in the comparison.
version_lt() {
  local v1="${1%%-*}" v2="${2%%-*}"
  local IFS='.'
  local -a a=($v1) b=($v2)
  for i in 0 1 2; do
    local ai="${a[$i]:-0}" bi="${b[$i]:-0}"
    [ "$ai" -lt "$bi" ] && return 0
    [ "$ai" -gt "$bi" ] && return 1
  done
  return 1  # bases are equal
}

# ── Argument ──────────────────────────────────────────────────────────────────

[ "$#" -eq 1 ] || usage
NEW_VERSION="${1#v}"  # strip leading 'v' if present

echo "${NEW_VERSION}" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$' \
  || die "invalid version '${NEW_VERSION}' — expected X.Y.Z or X.Y.Z-label.N"

# ── Current version ───────────────────────────────────────────────────────────

cd "${ROOT}"
CURRENT_VERSION="$(grep '^version = ' "${CARGO_TOML}" | head -1 | sed 's/version = "\(.*\)"/\1/')"

printf "Current version : %s\n" "${CURRENT_VERSION}"
printf "New version     : %s\n" "${NEW_VERSION}"
echo ""

# ── Version checks ────────────────────────────────────────────────────────────

[ "${NEW_VERSION}" != "${CURRENT_VERSION}" ] \
  || die "${NEW_VERSION} is already the current version"

version_lt "${NEW_VERSION}" "${CURRENT_VERSION}" \
  && die "${NEW_VERSION} is older than the current version ${CURRENT_VERSION}"

# Prevent creating a pre-release for an X.Y.Z base that has already been released.
# e.g. current=1.0.0, new=1.0.0-rc.1 would be going backwards.
BASE_NEW="${NEW_VERSION%%-*}"
BASE_CUR="${CURRENT_VERSION%%-*}"
if [ "${BASE_NEW}" = "${BASE_CUR}" ]; then
  CUR_PRE="${CURRENT_VERSION#"${BASE_CUR}"}"  # empty if no pre-release label
  NEW_PRE="${NEW_VERSION#"${BASE_NEW}"}"       # empty if no pre-release label
  if [ -z "${CUR_PRE}" ] && [ -n "${NEW_PRE}" ]; then
    die "cannot create pre-release ${NEW_VERSION} after already releasing ${CURRENT_VERSION}"
  fi
fi

# ── Git checks ────────────────────────────────────────────────────────────────

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
[ "${BRANCH}" = "main" ] \
  || die "must be on the 'main' branch (currently on '${BRANCH}')"

git diff --quiet && git diff --cached --quiet \
  || die "working tree has uncommitted changes — commit or stash them first"

TAG="v${NEW_VERSION}"
git tag --list "${TAG}" | grep -q . \
  && die "tag ${TAG} already exists"

# ── Update Cargo.toml ─────────────────────────────────────────────────────────

echo "Updating Cargo.toml ..."
sed -i.bak "s/^version = \"${CURRENT_VERSION}\"/version = \"${NEW_VERSION}\"/" "${CARGO_TOML}"
rm -f "${CARGO_TOML}.bak"

# ── Regenerate Cargo.lock ─────────────────────────────────────────────────────

echo "Updating Cargo.lock ..."
cargo build -q

# ── Commit, tag, push ─────────────────────────────────────────────────────────

echo "Committing ..."
git commit -am "Release ${TAG}"

echo "Tagging ${TAG} ..."
git tag "${TAG}"

echo "Pushing ..."
git push origin main
git push origin "${TAG}"

# ── Done ──────────────────────────────────────────────────────────────────────

REPO_URL="$(git remote get-url origin \
  | sed 's|git@github\.com:|https://github.com/|' \
  | sed 's|\.git$||')"

echo ""
echo "Released ${TAG}. Monitor the build at:"
echo "  ${REPO_URL}/actions"
echo ""
echo "Once the release is published, remember to update:"
echo "  - Homebrew tap formula (see RELEASES.md)"
echo "  - AUR PKGBUILD (see RELEASES.md)"
