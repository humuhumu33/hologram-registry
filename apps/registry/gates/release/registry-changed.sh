#!/usr/bin/env sh
# Did this change touch Hologram Registry? Prints `true` or `false`.
#
#   registry-changed.sh <base-sha> <head-sha>
#
# The gate workflows run on every pull request and in the merge queue, so their
# verdict jobs can be required checks. A change that touches none of these
# paths skips the gates and passes. Pushes to main always run the gates in
# full, so every commit a release can be tagged from carries its own evidence.
set -eu

BASE=${1:?usage: registry-changed.sh <base-sha> <head-sha>}
HEAD=${2:?usage: registry-changed.sh <base-sha> <head-sha>}

PATHS='^(src/oci_store/|src/modules/oci/|src/registry_compat/|src/admin_socket\.rs$|src/tls\.rs$|src/server\.rs$|src/config\.rs$|src/app\.rs$|src/main\.rs$|src/cli/(mod|serve|registry_argv|oci)\.rs$|apps/registry/|third_party/kappa/|third_party/dcbor/|scripts/check-kappa-pin\.sh$|scripts/check-oci-streaming\.sh$|tests/oci_|Cargo\.toml$|Cargo\.lock$|\.github/workflows/(gates|registry-os)\.yml$)'

git cat-file -e "$BASE^{commit}" 2>/dev/null || git fetch --quiet --depth=1 origin "$BASE"
changed=$(git diff --name-only "$BASE" "$HEAD")
if printf '%s\n' "$changed" | grep -Eq "$PATHS"; then
  echo true
else
  echo false
fi
