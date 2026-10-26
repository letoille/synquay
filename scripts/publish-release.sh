#!/usr/bin/env bash
set -euo pipefail

: "${GH_REPO:?}"
: "${VERSION:?}"
: "${COMMIT:?}"
: "${PRERELEASE:?}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.+-]+)?$ ]] || exit 1
[[ "$COMMIT" =~ ^[0-9a-f]{40}$ ]] || exit 1
[[ "$PRERELEASE" == true || "$PRERELEASE" == false ]] || exit 1
cd "${1:-packages}"
sha256sum --check SHA256SUMS
TAG="v$VERSION"

# Successful list requests distinguish missing objects from API/auth failures.
release_id=$(gh api --paginate "repos/$GH_REPO/releases?per_page=100" \
  --jq ".[] | select(.tag_name == \"$TAG\") | .id")
if [[ -n "$release_id" ]]; then
  immutable=$(gh api "repos/$GH_REPO/releases/$release_id" --jq '.immutable // false')
  if [[ "$immutable" == true ]]; then
    echo 'Release is immutable; publish a new version.' >&2
    exit 1
  fi
fi
reference=$(gh api "repos/$GH_REPO/git/matching-refs/tags/$TAG" \
  --jq ".[] | select(.ref == \"refs/tags/$TAG\") | .ref")
if [[ -n "$reference" ]]; then
  # The commits endpoint resolves annotated as well as lightweight tags.
  tagged_commit=$(gh api "repos/$GH_REPO/commits/$TAG" --jq '.sha')
  if [[ "$tagged_commit" != "$COMMIT" ]]; then
    if [[ "${REPLACE_TAG:-false}" != true || "${GITHUB_EVENT_NAME:-}" != workflow_dispatch || "${GITHUB_REF_TYPE:-}" == tag ]]; then
      echo 'Tag points to another commit. Run on that tag, use a new version, or manually publish from a branch with replace_tag enabled.' >&2
      exit 1
    fi
    gh api --method PATCH "repos/$GH_REPO/git/refs/tags/$TAG" \
      -f "sha=$COMMIT" -F force=true >/dev/null
  fi
fi

printf 'Synquay %s\n\nCommit: %s\n\nInstall: sudo apt install ./synquay_*.deb\nConsole: http://127.0.0.1:8080/\nService: systemctl status synquay\n' "$VERSION" "$COMMIT" > notes.txt
if [[ -n "$release_id" ]]; then
  # Upload first: failed uploads must not advertise new release notes.
  gh release upload "$TAG" ./*.deb SHA256SUMS --clobber
  gh release edit "$TAG" --title "Synquay $VERSION" --notes-file notes.txt --prerelease="$PRERELEASE"
else
  gh release create "$TAG" ./*.deb SHA256SUMS --target "$COMMIT" \
    --title "Synquay $VERSION" --notes-file notes.txt --prerelease="$PRERELEASE"
fi
