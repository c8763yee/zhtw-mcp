#!/bin/sh

# Run the fast half of the gate over what is staged, and only over what is
# staged. The gate itself reads the working tree, which is the wrong tree at
# commit time: a half-finished edit next to a staged one either fails a commit
# that is fine or passes one that is not. Everything below therefore runs
# against a checkout of the index, not against the files on disk.
#
# The whole index is checked out rather than the staged paths alone. It costs
# half a second for this tree and it buys the two things a partial checkout
# cannot give: rustfmt resolves the child modules of a staged file, and
# scripts/check-ruleset.py finds the schema facts and the ruleset it reads
# beside itself. Configuration files come along for free, from the index, so a
# staged rule change judges the files staged with it.
#
# What is deliberately absent: cargo build, cargo test, clippy, the corpus
# evaluation and the s2t table generator. Those need the whole toolchain and
# take minutes, and "make check" already owns them. A hook that costs a coffee
# break is a hook people disable.

set -u

ROOT=$(git rev-parse --show-toplevel) || exit 1
cd "$ROOT" || exit 1

git diff --cached --quiet && exit 0

failed=0
work=$(mktemp -d) || exit 1

# The signal traps exit rather than falling through: a handler that only cleans
# up leaves the rest of this script checking a directory it just deleted.
trap 'rm -rf "$work"' EXIT
trap 'rm -rf "$work"; exit 130' HUP INT TERM

staged=$(git diff --cached --name-only --diff-filter=ACMR)
git checkout-index -a -f --prefix="$work/" || {
    echo "pre-commit: cannot read the staged content" >&2
    exit 1
}

# src/engine/s2t_data.rs is generated and gitignored, so it is not in the index
# and rustfmt cannot resolve the module declaration that names it. A stub
# resolves it, and the stub is a newline rather than nothing: rustfmt reports a
# diff against a zero-byte file. Nothing here reads what is in it.
mkdir -p "$work/src/engine" || exit 1
[ -e "$work/src/engine/s2t_data.rs" ] \
    || printf '\n' > "$work/src/engine/s2t_data.rs" || exit 1

# Word splitting on the lists below is the point, which makes a path containing
# whitespace two paths and the check meaningless. A leading dash reaches a
# checker as an option, and git renders a name holding a newline in quotes.
# Refuse all three by name instead: there are none in this tree, and a commit
# that adds one should hear why it was not checked rather than be told
# everything passed.
if printf '%s\n' "$staged" | grep -qE '^-|[[:blank:]]|^"'; then
    printf '%s\n' "$staged" | grep -E '^-|[[:blank:]]|^"' | sed 's/^/  /' >&2
    echo "pre-commit: the staged paths above cannot be checked as written" >&2
    exit 1
fi

matching()
{
    printf '%s\n' "$staged" | grep -E "$1"
}

run_in_work()
{
    (cd "$work" && "$@") || failed=1
}

# Every lane is optional in the same way the gate treats an uninstalled tool: a
# contributor without it gets a note rather than a failure they cannot act on,
# and CI installs the ones that must not skip.
have()
{
    command -v "$1" > /dev/null 2>&1 && return 0
    echo "pre-commit: $1 not installed, skipping that lane" >&2
    return 1
}

# The edition is rustfmt's only piece of Cargo.toml, and reading it from the
# index copy keeps a staged edition bump judging the files staged with it.
rs=$(matching '\.rs$')
if [ -n "$rs" ] && have rustfmt; then
    edition=$(sed -n 's/^edition *= *"\([0-9]*\)".*/\1/p' "$work/Cargo.toml" | sed -n 1p)

    # rustfmt formats whatever it is handed as a module root. A file that some
    # other file include!s is not one: tests_generated.rs is pulled into a "mod
    # tests" block and carries that block's indent, so on its own rustfmt wants
    # to dedent every line in it and the commit fails over a file the gate calls
    # clean. cargo fmt walks the crate and gets this right, which is why
    # scripts/indent.sh uses it; here the fragments name themselves through the
    # include! that pulls them in.
    fragments=$(grep -rh 'include!("' "$work" --include='*.rs' 2> /dev/null \
        | sed 's/.*include!("//; s/").*//' | sort -u)
    formattable=
    for file in $rs; do
        printf '%s\n' "$fragments" | grep -qxF "${file##*/}" && continue
        formattable="$formattable $file"
    done
    if [ -n "$formattable" ]; then
        # shellcheck disable=SC2086
        run_in_work rustfmt --edition "${edition:-2021}" --check $formattable
    fi
fi

py=$(matching '\.py$')
if [ -n "$py" ]; then
    if have black; then
        # shellcheck disable=SC2086
        run_in_work black --check $py
    elif have python3; then

        # py_compile is not black, but a Python file that does not parse is the
        # failure worth catching before it is committed. Behind have() like
        # every other lane: a machine carrying neither black nor python3 should
        # hear that the lane skipped, not have the commit fail on a command not
        # found.
        for file in $py; do
            run_in_work python3 -m py_compile "$file"
        done
    fi
fi

# Through --diff rather than --check: both report the same verdict through the
# same exit status, and only one of them says what it wanted. A lane that fails
# without printing sends the author to a bisect.
#
# Shell gets commentflow and shfmt; Rust cannot. commentflow puts a blank line
# before a comment inside a chain and cargo fmt takes it back out, so that pair
# is only stable as a composition, which is what scripts/indent.sh checks in the
# gate. Shell has no such argument, and shfmt reads the .editorconfig that came
# out of the index with everything else rather than its own defaults.
sh_files=$(matching '\.sh$')
if [ -n "$sh_files" ]; then
    for file in $sh_files; do
        run_in_work sh -n "$file"
    done
    # shellcheck disable=SC2086
    have shellcheck && run_in_work shellcheck $sh_files
    # shellcheck disable=SC2086
    have commentflow && run_in_work commentflow --diff $sh_files
    # shellcheck disable=SC2086
    have shfmt && run_in_work shfmt -d $sh_files
fi

# The two prose rules no formatter knows about. Cheap, and a comment is much
# easier to fix while it is still staged than after it is in the log.
if [ -n "$rs$py$sh_files" ]; then
    if [ -e "$work/scripts/check-comments.sh" ]; then
        # shellcheck disable=SC2086
        run_in_work sh scripts/check-comments.sh $rs $py $sh_files
    else

        # Absent from the index rather than absent from the machine, which is
        # the shape a deleted or renamed checker takes here. The Makefile calls
        # it directly, so the gate still fails; a commit should not.
        echo "pre-commit: no scripts/check-comments.sh in the index, skipping" >&2
    fi
fi

# The ruleset is the file this project gets edited by hand most often and the
# one place a hand edit is a mistake: scripts/check-ruleset.py owns its dedup,
# sort and formatting. Run the writer rather than --lint and compare, which
# reports the conflicts --lint would and also catches the hand formatting that
# would otherwise pass here and fail the indent gate later. Writing is safe
# because the file it rewrites is the index copy.
if printf '%s\n' "$staged" | grep -qx 'assets/ruleset.json' && have python3; then
    cp "$work/assets/ruleset.json" "$work/ruleset.staged" || failed=1
    run_in_work python3 scripts/check-ruleset.py
    cmp -s "$work/ruleset.staged" "$work/assets/ruleset.json" || {
        echo "pre-commit: assets/ruleset.json is not normalized; run 'make indent'" >&2
        failed=1
    }
fi

# The one file this hook cannot judge by reading the index. Git runs the
# installed wrapper, which resolves the working tree's copy, so a staged change
# to a hook is judged by the hook it is replacing. The suite reads the tree it
# is run from, so running the staged copy of it is what closes that.
if [ -n "$(matching '^scripts/(git-.*|check-commit-log|install-git-hooks|test-git-hooks)\.sh$')" ]; then
    run_in_work sh scripts/test-git-hooks.sh
fi

# Trailing whitespace, a space before a tab, and a conflict marker. Cheap, and
# each one is noise that a reviewer would otherwise have to spend a comment on.
git diff --cached --check || failed=1

if [ "$failed" -ne 0 ]; then

    # The checkers name files inside the index checkout, which is not a path
    # anyone can edit. Say so once rather than have someone go looking for it.
    echo "Paths above are a checkout of the index; fix the file in the tree." >&2
    exit 1
fi

echo "Staged checks passed. The message rules run next: what and why, 50 and 72."
