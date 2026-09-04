#!/bin/sh

# Install wrappers for every scripts/git-*.sh into the repository's hooks
# directory. A worktree shares its hooks directory with its siblings, so each
# wrapper finds the worktree that invoked it instead of naming one by path.

set -u

mode=install
case "${1:-}" in
    "") ;;
    --uninstall) mode=uninstall ;;
    *)
        echo "usage: ${0##*/} [--uninstall]" >&2
        exit 2
        ;;
esac

ROOT=$(git rev-parse --show-toplevel) || exit 1
hooks=$(git rev-parse --path-format=absolute --git-path hooks) || exit 1
common=$(git rev-parse --path-format=absolute --git-common-dir) || exit 1
default_hooks=$common/hooks
failed=0

wrapper()
{
    printf '%s\n' '#!/bin/sh' \
        "exec \"\$(git rev-parse --show-toplevel)/scripts/git-$1.sh\" \"\$@\""
}

ours()
{
    [ -f "$1" ] && wrapper "$2" | cmp -s - "$1"
}

if [ "$mode" = uninstall ]; then
    for target in "$hooks"/*; do

        # An empty hooks directory leaves the glob unexpanded, so the first
        # thing to establish is that the name is a file at all.
        [ -e "$target" ] || continue
        ours "$target" "${target##*/}" || continue
        rm -f "$target"
        printf '  RM      %s\n' "${target##*/}"
    done
    exit 0
fi

# A global hooksPath is often shared by unrelated repositories. Installing a
# wrapper there makes their hooks try to run scripts from their own checkouts.
if [ "$hooks" != "$default_hooks" ]; then
    printf '  NOTE    core.hooksPath points hooks at %s; skipping installation\n' "$hooks"
    exit 0
fi

mkdir -p "$hooks" || exit 1
for hook in "$ROOT"/scripts/git-*.sh; do

    # No hook scripts leaves the glob unexpanded, and installing a hook named
    # for the pattern is worse than installing nothing.
    [ -e "$hook" ] || continue
    name=${hook##*/git-}
    name=${name%.sh}
    target="$hooks/$name"

    # An existing hook is somebody's, even when it looks like ours: overwriting
    # it is how a local workflow disappears without anyone noticing.
    if ours "$target" "$name"; then

        # Content is not enough: git skips a hook without the executable bit and
        # says nothing, so a wrapper that lost it reads as installed while
        # nothing runs.
        chmod +x "$target" || failed=1
        printf '  OK      %s\n' "$name"
    elif [ -e "$target" ] || [ -L "$target" ]; then
        printf '  KEEP    %s already exists; remove it to install ours\n' "$target"
    elif wrapper "$name" > "$target" && chmod +x "$target"; then
        printf '  HOOK    %s\n' "$name"
    else
        printf '  ERROR   cannot write %s\n' "$target" >&2
        failed=1
    fi
done

exit "$failed"
