#!/usr/bin/env bash
# Wait for the GitHub Actions runs of the newest commit on a branch and print
# the result. Uses the public GitHub API without a token (60 requests/hour),
# so polls are conditional (ETag; 304 answers do not count) and slow.
#
#   scripts/ci-status.sh <branch> [sha]
#
# sha defaults to the newest run's head commit on the branch. Exit code 0
# when every run for that commit succeeded, 1 when one failed, 2 on usage or
# API errors. On failure it prints the failed jobs, their failed steps and the
# check-run annotations (compiler errors show up there).
#
# Environment: CI_REPO (default mahope/tingskriver), CI_POLL_SECONDS (default
# 60), CI_TIMEOUT_MINUTES (default 120), GITHUB_TOKEN (optional, raises the
# rate limit).

set -euo pipefail

branch="${1:-}"
sha="${2:-}"
if [ -z "$branch" ]; then
  echo "usage: $0 <branch> [sha]" >&2
  exit 2
fi
command -v jq >/dev/null || { echo "jq is required" >&2; exit 2; }

repo="${CI_REPO:-mahope/tingskriver}"
poll="${CI_POLL_SECONDS:-60}"
deadline=$(( $(date +%s) + ${CI_TIMEOUT_MINUTES:-120} * 60 ))
api="https://api.github.com/repos/$repo"
cache_dir="$(mktemp -d)"
trap 'rm -rf "$cache_dir"' EXIT

# gh_get <path> prints the JSON body. Answers are cached per path and reused
# on 304 Not Modified.
gh_get() {
  local path="$1" key
  key="$(printf '%s' "$path" | md5sum | cut -c1-16)"
  local body="$cache_dir/$key.json" etag="$cache_dir/$key.etag" headers="$cache_dir/$key.h"
  local args=(-sS -H "Accept: application/vnd.github+json" -H "X-GitHub-Api-Version: 2022-11-28" -D "$headers")
  [ -n "${GITHUB_TOKEN:-}" ] && args+=(-H "Authorization: Bearer $GITHUB_TOKEN")
  [ -s "$etag" ] && args+=(-H "If-None-Match: $(cat "$etag")")
  local status
  status="$(curl "${args[@]}" -o "$body.new" -w '%{http_code}' "$api$path")"
  case "$status" in
    200)
      mv "$body.new" "$body"
      grep -i '^etag:' "$headers" | sed 's/^[Ee][Tt][Aa][Gg]: *//; s/\r$//' > "$etag" || true
      ;;
    304) rm -f "$body.new" ;;
    403|429)
      echo "GitHub API rate limit hit (status $status):" >&2
      grep -i '^x-ratelimit-reset:' "$headers" >&2 || true
      cat "$body.new" >&2
      return 1
      ;;
    *)
      echo "GitHub API $path answered $status" >&2
      cat "$body.new" >&2
      return 1
      ;;
  esac
  cat "$body"
}

encoded_branch="$(jq -rn --arg b "$branch" '$b|@uri')"
runs_path="/actions/runs?branch=$encoded_branch&per_page=30"

while :; do
  if ! runs="$(gh_get "$runs_path")"; then exit 2; fi
  if [ -z "$sha" ]; then
    sha="$(jq -r '.workflow_runs[0].head_sha // empty' <<<"$runs")"
  fi
  if [ -n "$sha" ]; then
    mine="$(jq --arg s "$sha" '[.workflow_runs[] | select(.head_sha == $s)]' <<<"$runs")"
    count="$(jq 'length' <<<"$mine")"
    pending="$(jq '[.[] | select(.status != "completed")] | length' <<<"$mine")"
    if [ "$count" -gt 0 ] && [ "$pending" -eq 0 ]; then
      break
    fi
    echo "$(date +%H:%M:%S) ${sha:0:7} on $branch: $count run(s), $pending still running" >&2
  else
    echo "$(date +%H:%M:%S) no runs for $branch yet" >&2
  fi
  if [ "$(date +%s)" -ge "$deadline" ]; then
    echo "timed out waiting for CI on $branch" >&2
    exit 2
  fi
  sleep "$poll"
done

echo "CI for $branch at ${sha:0:7}:"
jq -r '.[] | "  \(.name): \(.conclusion)  \(.html_url)"' <<<"$mine"

failed_runs="$(jq -r '.[] | select(.conclusion != "success" and .conclusion != "skipped") | .id' <<<"$mine")"
if [ -z "$failed_runs" ]; then
  echo "All runs succeeded."
  exit 0
fi

for run in $failed_runs; do
  jobs="$(gh_get "/actions/runs/$run/jobs?per_page=100")" || exit 2
  jq -r '.jobs[] | select(.conclusion == "failure" or .conclusion == "cancelled" or .conclusion == "timed_out")
    | "\nFailed job: \(.name) (\(.conclusion))  \(.html_url)\n" +
      ([.steps[]? | select(.conclusion == "failure") | "  failed step: \(.name)"] | join("\n"))' <<<"$jobs"
  for job in $(jq -r '.jobs[] | select(.conclusion == "failure") | .id' <<<"$jobs"); do
    annotations="$(gh_get "/check-runs/$job/annotations?per_page=50")" || exit 2
    jq -r '.[] | select(.annotation_level == "failure" or .annotation_level == "warning")
      | "  [\(.annotation_level)] \(.path):\(.start_line): \(.message | gsub("\n"; "\n      "))"' <<<"$annotations" | head -n 200
  done
done
exit 1
