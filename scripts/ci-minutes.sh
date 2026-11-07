#!/usr/bin/env bash
# Median wall time and runner minutes of the newest successful runs of a
# workflow (#192). Wall time is created_at to updated_at of the run, so it
# includes time spent queued. Runner minutes are the sum of the job durations
# (started_at to completed_at), which the jobs endpoint needs one call per
# run for; the public API allows 60 calls per hour, so the default is 10 runs
# and GITHUB_TOKEN is recommended.
#
#   scripts/ci-minutes.sh rust.yml [runs]      (default 10)
#
# Prints one line per run and a median line. Environment: CI_REPO
# (default mahope/tingskriver), GITHUB_TOKEN (optional).

set -euo pipefail
wf="${1:?usage: $0 <workflow-file> [runs]}"
n="${2:-10}"
repo="${CI_REPO:-mahope/tingskriver}"
api="https://api.github.com/repos/$repo"
auth=()
[ -n "${GITHUB_TOKEN:-}" ] && auth=(-H "Authorization: Bearer $GITHUB_TOKEN")

get() { curl -fsS -H "Accept: application/vnd.github+json" "${auth[@]}" "$1"; }

rows=""
while read -r id created updated; do
  wall="$(jq -n --arg a "$created" --arg b "$updated" '((($b|fromdate)-($a|fromdate))/60)*10|round/10')"
  runner="$(get "$api/actions/runs/$id/jobs?per_page=100&filter=latest" | jq '[.jobs[]|select(.started_at and .completed_at)|((.completed_at|fromdate)-(.started_at|fromdate))]|add/60*10|round/10')"
  printf '%s wall=%s min runner=%s min\n' "$id" "$wall" "$runner"
  rows+="$wall $runner"$'\n'
done < <(get "$api/actions/workflows/$wf/runs?status=success&per_page=$n" | jq -r '.workflow_runs[]|"\(.id) \(.created_at) \(.updated_at)"')

[ -n "$rows" ] || { echo "no successful runs"; exit 0; }
median() { sort -n | awk '{a[NR]=$1} END {print (NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2}'; }
printf 'median wall=%s min, median runner=%s min (n=%s)\n' \
  "$(printf '%s' "$rows" | awk '{print $1}' | median)" \
  "$(printf '%s' "$rows" | awk '{print $2}' | median)" \
  "$(printf '%s' "$rows" | wc -l)"
