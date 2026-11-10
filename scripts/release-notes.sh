#!/usr/bin/env bash
# Release notes for tag v<VERSION> at <COMMIT>: GitHub's generated notes (the merged PRs and the changelog link), without
# the "New Contributors" section, plus the issues the release closed.
#
#   scripts/release-notes.sh VERSION COMMIT > notes.md
#
# An issue counts when it is closed and either a PR merged in the release closes it (a "Closes #N" in the PR, or one
# linked in its sidebar) or a commit in the release says "closes/fixes/resolves #N" (this repo also pushes to main
# directly). The range starts at the previous v* tag, so the checkout needs the full history and tags.
# Needs: gh (authenticated, GH_TOKEN in CI), git, jq. GITHUB_REPOSITORY defaults to this repo's.
set -euo pipefail
version=${1:?usage: release-notes.sh VERSION COMMIT}
commit=${2:?usage: release-notes.sh VERSION COMMIT}
repo=${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}
owner=${repo%/*} name=${repo#*/}

# The newest v* tag reachable from the commit, other than this release's own.
prev=$(git describe --tags --abbrev=0 --match 'v*' --exclude "v$version" "$commit" 2>/dev/null || true)
range=${prev:+$prev..}$commit

notes=$(gh api "repos/$repo/releases/generate-notes" -f tag_name="v$version" -f target_commitish="$commit" \
  ${prev:+-f previous_tag_name="$prev"} --jq .body |
  awk '/^## New Contributors/{skip=1; next} skip && /^$/{skip=0; next} !skip')

# Issue numbers from the commit messages, then from the PRs those commits came in through.
issues=$(git log --format=%B "$range" |
  grep -oiE '\b(close[sd]?|fix(e[sd])?|resolve[sd]?)[: ]+#[0-9]+' | grep -oE '[0-9]+$' || true)
for sha in $(git rev-list "$range"); do
  for pr in $(gh api "repos/$repo/commits/$sha/pulls" --jq '.[] | select(.merged_at != null) | .number'); do
    issues+=$'\n'$(gh api graphql -F owner="$owner" -F name="$name" -F pr="$pr" --jq \
      '.data.repository.pullRequest.closingIssuesReferences.nodes[].number' -f query='
      query($owner: String!, $name: String!, $pr: Int!) {
        repository(owner: $owner, name: $name) {
          pullRequest(number: $pr) { closingIssuesReferences(first: 50) { nodes { number } } }
        }
      }')
  done
done

closed=""
for n in $(printf '%s\n' "$issues" | grep -E '^[0-9]+$' | sort -nu); do
  # only real issues that are closed now (a "#N" can also be a PR, or an issue that was reopened)
  line=$(gh api "repos/$repo/issues/$n" --jq \
    'select(.pull_request == null and .state == "closed") | "* \(.title) (#\(.number))"' 2>/dev/null || true)
  [ -n "$line" ] && closed+=$line$'\n'
done

if [ -n "$closed" ]; then
  # before the "Full Changelog" link, which GitHub always puts last
  section=$'## Closed issues\n'$closed
  notes=$(awk -v s="$section" '/^\*\*Full Changelog\*\*/ && !done {printf "%s\n", s; done=1} {print} END {if (!done) printf "\n%s", s}' <<<"$notes")
fi
printf '%s\n' "$notes"
