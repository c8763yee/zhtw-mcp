#!/bin/sh

# Validate the commit messages whose object IDs arrive on standard input.
#
# The shared body of the pre-push hook and of the pull-request commit-log step
# in .github/workflows/main.yml, so the rules live in scripts/git-commit-msg.sh
# alone and cannot drift into a second list that agrees with the first only by
# accident.

set -u

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
message_hook="$script_dir/git-commit-msg.sh"
failed=0

while read -r commit; do
    [ -n "$commit" ] || continue

    # A pipeline reports its last command, so without this an object nobody can
    # read would be reported as a message that breaks the rules.
    git cat-file -e "${commit}^{commit}" 2> /dev/null || {
        printf 'Cannot read commit: %s\n' "$commit" >&2
        failed=1
        continue
    }
    git show -s --format=%B "$commit" | "$message_hook" - && continue

    # Through cat -v: a subject is text on its way to a terminal or a CI log,
    # and an escape sequence in one can rewrite what the reader sees around it.
    printf 'Invalid commit: %s\n' \
        "$(git show -s --format='%h %s' "$commit" | cat -v)" >&2
    failed=1
done

exit "$failed"
