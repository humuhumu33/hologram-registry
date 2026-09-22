#!/usr/bin/env sh
# The release condition (FR-017, plan P9 T6): no gate green, no release.
#
#   check-gates.sh <sha> [--nightly]
#
# Exits 0 only when gates.yml and registry-os.yml each have a run on exactly
# <sha> that concluded `success` with every job run, none skipped: a skipped
# gate proves nothing. Pushes to main run them in full, so a commit on main
# qualifies once its runs finish; this waits for runs still in progress.
# With --nightly (every release without a pre-release suffix), the latest
# gates-nightly.yml run on the default branch must also be `success` and
# younger than 36 hours.
#
# Needs `gh` with a token that may read Actions, and GITHUB_REPOSITORY
# (owner/name). WAIT_MINUTES bounds the wait (default 120).
set -eu

SHA=${1:?usage: check-gates.sh <sha> [--nightly]}
NIGHTLY=${2:-}
REPO=${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is not set}
WAIT_MINUTES=${WAIT_MINUTES:-120}
WORKFLOWS="gates.yml registry-os.yml"

deadline=$(( $(date +%s) + WAIT_MINUTES * 60 ))

command -v gh > /dev/null || { echo "error: gh is required" >&2; exit 2; }

runs_of() {
  gh api "repos/$REPO/actions/workflows/$1/runs?head_sha=$SHA&per_page=50" --jq "$2"
}

# Prints `ok`, `wait` or `red: <reason>` for one workflow on SHA.
judge() {
  if [ "$(runs_of "$1" '.workflow_runs | length')" = 0 ]; then
    echo "wait"; return
  fi
  for id in $(runs_of "$1" '.workflow_runs[] | select(.conclusion == "success") | .id'); do
    unrun=$(gh api "repos/$REPO/actions/runs/$id/jobs?per_page=100" \
      --jq '[.jobs[] | select(.conclusion != "success")] | length')
    if [ "$unrun" = 0 ]; then
      echo "ok"; return
    fi
  done
  if [ "$(runs_of "$1" '[.workflow_runs[] | select(.status != "completed")] | length')" != 0 ]; then
    echo "wait"; return
  fi
  echo "red: $(runs_of "$1" '[.workflow_runs[] | "\(.event) \(.conclusion)"] | join(", ")'), and no run with every gate green"
}

for wf in $WORKFLOWS; do
  while :; do
    verdict=$(judge "$wf")
    case "$verdict" in
      ok) echo "$wf: every gate green on $SHA"; break ;;
      wait)
        if [ "$(date +%s)" -ge "$deadline" ]; then
          echo "error: $wf has no finished run with every gate green on $SHA after $WAIT_MINUTES minutes" >&2
          echo "hint: tag a commit on main (pushes to main run the gates in full), or dispatch $wf on it" >&2
          exit 1
        fi
        echo "$wf: waiting for a finished run on $SHA"
        sleep 60 ;;
      *) echo "error: $wf on $SHA: ${verdict#red: }" >&2; exit 1 ;;
    esac
  done
done

if [ "$NIGHTLY" = "--nightly" ]; then
  latest=$(gh api "repos/$REPO/actions/workflows/gates-nightly.yml/runs?branch=main&status=completed&per_page=1" \
    --jq '.workflow_runs[0] // empty
      | if .conclusion == "success" and (now - (.created_at | fromdateiso8601)) < 36 * 3600
        then "green" else "\(.conclusion) at \(.created_at)" end' 2>/dev/null) || {
    echo "error: gates-nightly.yml does not exist; a release needs three green nights first" >&2
    exit 1
  }
  case "$latest" in
    green) echo "gates-nightly.yml: latest run green and younger than 36 hours" ;;
    "") echo "error: gates-nightly.yml has never finished a run on main" >&2; exit 1 ;;
    *) echo "error: the latest nightly is not a green run younger than 36 hours: $latest" >&2; exit 1 ;;
  esac
fi
