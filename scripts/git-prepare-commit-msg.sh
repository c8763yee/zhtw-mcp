#!/bin/sh

# Put the rules in front of the person writing the message, so the commit-msg
# hook stops being the first place they hear about them.

set -u

message_file=${1:-}
source=${2:-}

if [ -z "$message_file" ]; then
    echo "usage: ${0##*/} <message-file> [source]" >&2
    exit 2
fi

# A message that already exists is somebody's, not a blank slate: -m, a
# template, a merge, a squash, or an amend.
case "$source" in
    message | template | merge | squash | commit) exit 0 ;;
esac

comment_char=$(git config --get core.commentChar) || comment_char='#'
case "$comment_char" in
    '' | auto) comment_char='#' ;;
esac

scissors='-\{8,\}[[:space:]]*>8[[:space:]]*-\{8,\}'
cut=$(grep -n -- "$scissors" "$message_file" | sed -n '1s/:.*//p')

# Nothing to help with once there is prose in the file.
if sed "/$scissors/,\$d" "$message_file" | grep -qE "^[^[:space:]$comment_char]"; then
    exit 0
fi

# Both temporary files go in the trap, and both names exist before it is set:
# under set -u a handler naming an unset variable fails when it is needed most.
rules=$(mktemp) || exit 0
spliced=
trap 'rm -f "$rules" "$spliced"' EXIT
trap 'rm -f "$rules" "$spliced"; exit 130' HUP INT TERM

# The list comes from the hook that enforces it, so the two cannot drift.
{
    printf '\nCommit rules, enforced by scripts/git-commit-msg.sh:\n\n'
    "$(git rev-parse --show-toplevel)/scripts/git-commit-msg.sh" --rules
    printf '\nStaged:\n'
    git diff --cached --name-only | sed 's/^/  /'
} | awk -v char="$comment_char" '{sub(/[[:space:]]+$/, ""); print char " " $0}' \
    | sed 's/[[:space:]]*$//' > "$rules"

# Appended after the scissors line the block would be stripped with the diff
# that "git commit -v" puts there, so it is spliced in above it instead. An
# absent scissors line is the same splice with the cut one past the end.
[ -n "$cut" ] || cut=$(($(wc -l < "$message_file") + 1))

# awk rather than head and tail: an empty message file puts the cut at line 1,
# and "head -n 0" is an error on the BSD tools macOS ships.
spliced=$(mktemp) || exit 0
{
    awk -v cut="$cut" 'NR < cut' "$message_file"
    cat "$rules"
    awk -v cut="$cut" 'NR >= cut' "$message_file"
} > "$spliced" && cat "$spliced" > "$message_file"

# A helper that could not help must not stop the commit. Every path above that
# gives up already exits 0, and without this the last line's status becomes the
# hook's: a full disk under mktemp would refuse the commit rather than leave the
# author's message the way they wrote it.
exit 0
