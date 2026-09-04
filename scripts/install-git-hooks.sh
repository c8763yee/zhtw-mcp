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

# A global hooksPath is often shared by unrelated repositories, and installing a
# wrapper there makes their hooks run scripts out of their own checkouts. Both
# directions refuse it, for the same reason in each: a directory this repository
# never wrote to is one it must not remove from either, since a wrapper sitting
# there was put there by whoever shares it.
if [ "$hooks" != "$default_hooks" ]; then
    printf '  NOTE    core.hooksPath points hooks at %s; skipping %s\n' \
        "$hooks" "$mode"
    exit 0
fi

if [ "$mode" = uninstall ]; then
    for target in "$hooks"/*; do

        # An empty hooks directory leaves the glob unexpanded, so the first
        # thing to establish is that the name is a file at all.
        [ -e "$target" ] || continue
        ours "$target" "${target##*/}" || continue
        if ! rm -f "$target"; then
            printf '  ERROR   cannot remove %s\n' "$target" >&2
            failed=1
            continue
        fi
        printf '  RM      %s\n' "${target##*/}"
    done
    exit "$failed"
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
    elif staging=$(mktemp "$hooks/.$name.XXXXXX") \
        && wrapper "$name" > "$staging" \
        && chmod +x "$staging" \
        && mv -f "$staging" "$target"; then
        printf '  HOOK    %s\n' "$name"
    else

        # Through a temporary name and a rename, because a wrapper interrupted
        # halfway is a file the next run reads as somebody else's hook and
        # refuses to touch, which is a broken hook nothing will repair. The name
        # comes from mktemp rather than the hook's own: a predictable one in a
        # directory this script does not own is a file it can truncate and a
        # symlink it can follow.
        [ -n "${staging:-}" ] && rm -f "$staging"
        printf '  ERROR   cannot write %s\n' "$target" >&2
        failed=1
    fi
done

exit "$failed"
