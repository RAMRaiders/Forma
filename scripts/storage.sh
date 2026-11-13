#!/usr/bin/env bash
# The organization's GitHub storage: what this month has used of the free quota, and the
# artifacts each repository keeps. Actions artifacts and GitHub Packages share 500 MB on the
# free plan, counted in gigabyte-hours over the month: once it is used up, publishing the SDK
# fails ("Account has reached its billing limit") until the month ends. The repositories keep
# artifacts one day (their retention setting); `--clean` deletes every one now.
#
#   scripts/storage.sh           # report
#   scripts/storage.sh --clean   # report, then delete every artifact
set -euo pipefail

org=RAMRaiders
repos=(Forma forma-theme-cyber romdex)
clean=false
[ "${1:-}" = --clean ] && clean=true

# 500 MB for every hour of the month.
year=$(date -u +%Y)
month=$(date -u +%-m)
days=$(date -u -d "$year-$month-01 +1 month -1 day" +%d)
quota=$(awk "BEGIN { print 0.5 * 24 * $days }")
id=$(gh api "/orgs/$org" -q .id)
used=$(gh api "/organizations/$id/settings/billing/usage?year=$year&month=$month" \
  -q '[.usageItems[] | select(.unitType == "GigabyteHours") | .quantity] | add // 0')
awk -v u="$used" -v q="$quota" 'BEGIN { printf "This month: %.0f of %.0f GB-hours (%.0f%%)\n", u, q, 100 * u / q }'

for repo in "${repos[@]}"; do
  ids=$(gh api --paginate "repos/$org/$repo/actions/artifacts?per_page=100" \
    -q '.artifacts[] | select(.expired | not) | "\(.id) \(.size_in_bytes)"')
  count=$(printf '%s' "$ids" | grep -c . || true)
  bytes=$(printf '%s\n' "$ids" | awk '{ s += $2 } END { print s + 0 }')
  echo "$repo: $count artifacts, $((bytes / 1024 / 1024)) MB"
  if $clean; then
    printf '%s\n' "$ids" | while read -r artifact _; do
      [ -n "$artifact" ] && gh api -X DELETE "repos/$org/$repo/actions/artifacts/$artifact"
    done
  fi
done
$clean && echo "Deleted every artifact."
exit 0
