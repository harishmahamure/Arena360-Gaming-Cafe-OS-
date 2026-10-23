#!/usr/bin/env bash
# One-time: protect the default branch so the `ci-ok` check (from GitHub Actions) must pass
# before merging.
# Usage: scripts/protect-main.sh [owner/repo] [branch]
set -euo pipefail

repo="${1:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}"
branch="${2:-$(gh repo view "$repo" --json defaultBranchRef --jq .defaultBranchRef.name)}"

gh api --method PUT "repos/${repo}/branches/${branch}/protection" --input - <<'JSON'
{
  "required_status_checks": { "strict": true, "checks": [{ "context": "ci-ok", "app_id": 15368 }] },
  "enforce_admins": true,
  "required_pull_request_reviews": null,
  "restrictions": null
}
JSON

echo "${branch} on ${repo} now requires the ci-ok status check."
