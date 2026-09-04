#!/bin/sh

# Judge the commit messages this push would publish.
#
# Merges included. git-commit-msg.sh exempts a merge subject from the width and
# mood rules because it is git's wording rather than the author's, but not from
# the tab, override and em dash checks, and filtering merges out here meant they
# reached the remote without those. The commit-msg hook only sees a message as
# it is written, so a rebase, an amend, or a commit made with --no-verify
# reaches the remote unread. This is the last place to catch that while the
# history is still local and cheap to rewrite.

set -u

# From the repository, not from $0: git invokes the hook through the wrapper in
# the hooks directory, so dirname of $0 names that directory and not scripts/.
# Hooks run with the working tree root as the working directory.
script_dir=$(git rev-parse --show-toplevel)/scripts
remote=${1:-}
zero=0000000000000000000000000000000000000000
failed=0

# What this remote already carries is not this push's to judge, and the refs it
# carries are the only honest answer to what that is. Scoped to the named remote
# and never widened to all of them: a commit sitting on a fork is still new to
# origin, and excluding it because some other remote has it is how a message
# nobody has read reaches a remote.
#
# Both halves are load-bearing and each is a case in the suite. Without the
# scoping, a push of a new branch has no remote tip to diff against and replays
# every commit reachable from the tip, so the first "git push -u origin topic"
# is refused by whatever in the history predates the rules. Without the
# emptiness, a first push to a remote nothing is tracked for judges nothing.
published=
if [ -n "$remote" ] \
    && [ -n "$(git for-each-ref --count=1 --format='%(refname)' "refs/remotes/$remote/")" ]; then
    published="--remotes=$remote"
fi

while read -r local_ref local_sha remote_ref remote_sha; do
    [ -n "${local_ref:-}" ] || continue
    [ "$local_sha" != "$zero" ] || continue
    git cat-file -e "${local_sha}^{commit}" 2> /dev/null || continue

    # The remote tip is whatever the other side advertised, which a clone that
    # has not fetched since does not have. Judging what this remote has not
    # published is the same question asked a wider way, and it beats refusing
    # the push over an object nobody here can read.
    if [ "$remote_sha" = "$zero" ] \
        || ! git cat-file -e "${remote_sha}^{commit}" 2> /dev/null; then
        if [ -n "$published" ]; then
            commits=$(git rev-list "$local_sha" --not "$published")
        else

            # Nothing is tracked for this remote, so nothing is known to be on
            # it. Everything reachable is what this push would publish there.
            commits=$(git rev-list "$local_sha")
        fi
    else
        commits=$(git rev-list "${remote_sha}..${local_sha}")
    fi || {
        echo "Push rejected: cannot list commits for $local_ref" >&2
        failed=1
        continue
    }

    [ -n "$commits" ] || continue
    printf '%s\n' "$commits" | "$script_dir/check-commit-log.sh" || {
        echo "Push rejected for $local_ref -> $remote_ref." >&2
        failed=1
    }
done

exit "$failed"
