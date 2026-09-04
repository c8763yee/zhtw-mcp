#!/bin/sh

# Exercise the git hooks against a scratch repository.
#
# The hooks are the one part of this tree with no other gate behind them: they
# run on a contributor's machine, in a directory this repository's tests never
# look at, and a hook that stops rejecting is indistinguishable from a hook that
# passes. Every case below is one that was checked by hand while the hooks were
# written, which is exactly the set that would otherwise be checked by hand
# again on the next edit and eventually not at all.
#
# Scratch repository rather than this one: a test that stages files has to stage
# them somewhere, and it must not be here.

set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
failures=0
cases=0

work=$(mktemp -d) || exit 1

# The scratch repository has to be scratch all the way down. Without this, a
# contributor with a global core.hooksPath sends every hook path below into the
# directory that setting names, and the cases that remove a hook to prove the
# installer keeps somebody else's would remove theirs. make check runs this
# script, so the blast radius was every checkout on such a machine.
GIT_CONFIG_GLOBAL=/dev/null
GIT_CONFIG_SYSTEM=/dev/null
export GIT_CONFIG_GLOBAL GIT_CONFIG_SYSTEM

# HOME is deliberately left alone. These two variables replace both global
# config paths outright, so redirecting HOME adds nothing to the isolation and
# takes something away: rustup keeps its toolchains under HOME, so the rustfmt
# on PATH is a shim that cannot find a toolchain without it. That is how CI
# failed while every laptop with a rustfmt outside rustup passed.

# Git exports these to a hook it runs, and the pre-commit lane runs this suite
# when a hook script is staged. Inherited, they point every git command below at
# the index and object store of the repository being committed to, which is both
# the wrong repository and, from the scratch directory, a path that does not
# resolve. Cleared rather than overridden: the scratch repository supplies its
# own through the git init on the next line.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR GIT_PREFIX
unset GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES
trap 'rm -rf "$work"' EXIT
trap 'rm -rf "$work"; exit 130' HUP INT TERM

fail()
{
    echo "  FAIL  $1" >&2
    failures=$((failures + 1))
}

# "expected" is an exit status, "label" says what the case is about. The command
# runs with its output captured, and the output is printed only when the case
# fails: a passing run says nothing, a failing one says everything.
expect()
{
    expected=$1
    label=$2
    shift 2
    cases=$((cases + 1))
    output=$("$@" 2>&1)
    status=$?
    [ "$status" -eq "$expected" ] && return 0
    fail "$label (exit $status, expected $expected)"
    printf '%s\n' "$output" | sed 's/^/        /' >&2
}

contains()
{
    label=$1
    file=$2
    pattern=$3
    cases=$((cases + 1))
    grep -q "$pattern" "$file" && return 0
    fail "$label (no /$pattern/ in $file)"
}

absent()
{
    label=$1
    file=$2
    pattern=$3
    cases=$((cases + 1))
    grep -q "$pattern" "$file" || return 0
    fail "$label (unexpected /$pattern/ in $file)"
}

# Through the installed hook rather than the script it wraps, so a wrapper that
# resolves the wrong path fails these cases instead of passing them.
#
# Invoked through "expect", which shellcheck cannot follow. 0.9.0 calls that
# SC2317 and newer versions SC2329, and CI may run either.
# shellcheck disable=SC2317,SC2329
message()
{
    printf '%s\n' "$1" | "$hooks/commit-msg" -
}

# The scratch repository carries copies of the scripts, so the installer writes
# what a real clone would write and the hooks resolve their own paths the way
# they will in the field.
mkdir -p "$work/repo/scripts" || exit 1
cp "$ROOT"/scripts/git-*.sh "$ROOT/scripts/check-commit-log.sh" \
    "$ROOT/scripts/install-git-hooks.sh" "$ROOT/scripts/check-comments.sh" \
    "$work/repo/scripts/" || exit 1
cp "$ROOT/.editorconfig" "$ROOT/.shellcheckrc" "$ROOT/.clang-format" \
    "$work/repo/" || exit 1
cd "$work/repo" || exit 1
git init -q . || exit 1
git config user.email hooks@test
git config user.name "Hook Test"
git config commit.gpgsign false

# Asked of git, not spelled ".git/hooks": a contributor with a global
# core.hooksPath sends the installer somewhere else, and assertions against the
# default path would then test a directory nothing writes to.
hooks=$(git rev-parse --path-format=absolute --git-path hooks) || exit 1

# Staged, not merely copied: the pre-commit hook reads its checkers' config out
# of the index like everything else, so a working-tree copy would leave the
# shell lanes judging the scratch scripts by their own defaults.
git add .editorconfig .shellcheckrc .clang-format 2> /dev/null

echo "  HOOKS   installer"
expect 0 "installer installs the hooks" ./scripts/install-git-hooks.sh
for hook in commit-msg pre-commit pre-push prepare-commit-msg; do
    cases=$((cases + 1))
    if [ ! -x "$hooks/$hook" ]; then
        fail "installer did not install an executable $hook"
    fi
done
expect 2 "installer rejects an unknown flag" ./scripts/install-git-hooks.sh --bogus

# A wrapper that lost its executable bit reads as installed while git silently
# runs nothing, so a second install has to put the bit back.
chmod -x "$hooks/commit-msg"
./scripts/install-git-hooks.sh > /dev/null 2>&1
cases=$((cases + 1))
if [ ! -x "$hooks/commit-msg" ]; then
    fail "installer did not restore a lost executable bit"
fi

# An installer that overwrites is an installer that deletes somebody's work, so
# a name it wants but does not own has to come back as KEEP rather than a
# wrapper.
rm -f "$hooks/pre-push"
printf '#!/bin/sh\nexit 0\n' > "$hooks/pre-push"
chmod +x "$hooks/pre-push"
./scripts/install-git-hooks.sh > "$work/install.out" 2>&1
contains "installer keeps a hook it did not write" "$work/install.out" "KEEP"
cases=$((cases + 1))
if [ ! -e "$hooks/pre-push" ]; then
    fail "installer removed a hook it did not write"
fi
rm -f "$hooks/pre-push"
./scripts/install-git-hooks.sh > /dev/null 2>&1

shared_hooks=$work/shared-hooks
git config core.hooksPath "$shared_hooks"
expect 0 "installer skips a shared hooks path" ./scripts/install-git-hooks.sh
cases=$((cases + 1))
if [ -e "$shared_hooks" ]; then
    fail "installer wrote to a shared hooks path"
fi

# The other direction of the same rule. A wrapper in a shared directory was put
# there by whoever shares it, so removing it is not this repository's to do.
mkdir -p "$shared_hooks"
printf '#!/bin/sh\n' > "$shared_hooks/commit-msg"
printf '%s\n' '#!/bin/sh' \
    "exec \"\$(git rev-parse --show-toplevel)/scripts/git-pre-push.sh\" \"\$@\"" \
    > "$shared_hooks/pre-push"
expect 0 "uninstaller skips a shared hooks path" \
    ./scripts/install-git-hooks.sh --uninstall
cases=$((cases + 1))
if [ ! -e "$shared_hooks/pre-push" ]; then
    fail "uninstaller removed a wrapper from a shared hooks path"
fi
rm -rf "$shared_hooks"
git config --unset core.hooksPath

echo "  HOOKS   commit-msg"
expect 0 "a message that follows the rules" \
    message "Add the scratch repository

One paragraph saying why this exists, wrapped inside the
limit the hook enforces."
expect 0 "the rules can be printed" sh "$ROOT/scripts/git-commit-msg.sh" --rules
expect 0 "a fixup is left alone" message "fixup! Add the scratch repository"
expect 0 "a merge subject is left alone" message "Merge branch 'topic' into main"

# The exemption covers the style rules and not what the bytes do to a reader. A
# merge subject is git's wording, but nothing about that makes an override in it
# safe, and an amend! message becomes the stored message of the commit it names.
expect 1 "a fixup carrying an em dash" \
    message "fixup! Add the scratch repository — badly"
expect 1 "a merge subject carrying an override" \
    message "$(printf "Merge branch 'topic' \342\200\256 into main")"

# The shape git writes when it merges two revisions by name, which is what the
# merge ref GitHub builds for a pull request carries. Exempt from the width
# rule, since 92 columns of two object names is nobody's prose, and held to the
# character rules like every other message.
expect 0 "a merge of two revisions by name" \
    message "Merge e023b4beffd14a005ac196471d6b2a4970d5ead4 into b732125feaee70b86243b63aeb6361b8979f870b"
expect 1 "a lowercase subject" message "add the scratch repository"
expect 1 "a past-tense subject" message "Added the scratch repository"
expect 1 "a subject ending in a period" message "Add the scratch repository."
# Single quotes on purpose: the backtick is the subject of the case.
# shellcheck disable=SC2016
expect 1 "a subject with a backtick" message 'Add the `scratch` repository'
expect 1 "a conventional-commit prefix" message "feat: add the scratch repository"
expect 1 "a one-word subject" message "Scratch"
expect 1 "a subject past 50 columns" \
    message "Add the scratch repository the hook tests run inside of"
expect 1 "a body with no blank line above it" message "Add the repository
Body on the second line."
expect 1 "a body line past 72 columns" message "Add the repository

This line exists to run past the seventy-two column limit that the hook is
supposed to enforce, and it does."

# The rule this project cannot inherit from anywhere else. A subject that names
# the term a change is about is the house style here, and the width that has to
# be counted is the one a terminal spends: two columns per CJK character, not
# one character and not three bytes.
expect 0 "a subject naming a Chinese term" \
    message "Narrow 聯繫 flagging to contact copy"
expect 1 "the same subject once it is 52 columns wide" \
    message "Narrow 聯繫 flagging to contact copy in 跨海峽 prose"

expect 1 "an em dash in the body" message "Add the repository

The dash below is the one an editor substitutes without being asked
— and this log does not carry it."
expect 1 "a tab in the body" \
    message "$(printf 'Add the repository\n\nA tab\tsits in this line.')"

# The character the C locale cannot see. A right-to-left override reorders what
# a terminal and a review page display without touching a byte anybody reads, so
# it is the case the ASCII control check alone would pass.
expect 1 "a bidirectional override in the body" \
    message "$(printf 'Add the repository\n\nThe override is here: \342\200\256 and it hides.')"

echo "  HOOKS   prepare-commit-msg"
printf '' > "$work/empty.msg"
expect 0 "an empty message gets the rules" \
    sh ./scripts/git-prepare-commit-msg.sh "$work/empty.msg"
contains "the rules are rendered" "$work/empty.msg" "Commit rules"
contains "the enforced list is what is shown" "$work/empty.msg" "imperative mood"

# "git commit -v" puts the diff below a scissors line and drops everything after
# it, so the block has to land above the scissors or the author never sees it.
printf '\n# hint\n# ------------------------ >8 ------------------------\ndiff --git a/x b/x\n' \
    > "$work/scissors.msg"
expect 0 "a commit -v message gets the rules" \
    sh ./scripts/git-prepare-commit-msg.sh "$work/scissors.msg"
cases=$((cases + 1))
rules_line=$(grep -n "Commit rules" "$work/scissors.msg" | cut -d: -f1)
cut_line=$(grep -n ">8" "$work/scissors.msg" | cut -d: -f1)
if [ -z "$rules_line" ] || [ -z "$cut_line" ] || [ "$rules_line" -ge "$cut_line" ]; then
    fail "the rules landed below the scissors line"
fi
contains "the diff survives" "$work/scissors.msg" "^diff --git"

printf 'Already written\n' > "$work/written.msg"
expect 0 "a message that already has prose" \
    sh ./scripts/git-prepare-commit-msg.sh "$work/written.msg"
absent "prose is left alone" "$work/written.msg" "Commit rules"

echo "  HOOKS   pre-commit"
printf '#!/bin/sh\n\nset -eu\n\necho staged\n' > tidy.sh
git add tidy.sh
expect 0 "a staged file the checkers accept" sh ./scripts/git-pre-commit.sh

# The whole reason the hook checks out the index: an edit nobody staged must
# neither fail this commit nor ride along in it.
#
# The defect is a syntax error rather than a formatting one, because "sh -n" is
# always there while shfmt and shellcheck are lanes the hook skips when the tool
# is missing. A case that needs an optional tool turns a laptop without it red.
printf '#!/bin/sh\nif true; then\n  echo "no fi below me"\n' > untidy.sh
expect 0 "an unstaged file is not judged" sh ./scripts/git-pre-commit.sh
git add untidy.sh
expect 1 "the same file once staged" sh ./scripts/git-pre-commit.sh
git rm -q --cached untidy.sh
rm -f untidy.sh

# The prose rules no formatter knows about, through the real checker rather than
# a stand-in: it needs no toolchain, and the two characters it rejects are the
# whole of what there is to assert.
printf '#!/bin/sh\n\n# a dash \342\200\224 in a comment\necho hi\n' > prose.sh

# The checker is staged too, because the lane reads it out of the index like
# everything else the hook runs.
git add prose.sh scripts/check-comments.sh
expect 1 "a staged comment carrying an em dash" sh ./scripts/git-pre-commit.sh
# Single quotes on purpose: the backtick is the subject of the case.
# shellcheck disable=SC2016
printf '#!/bin/sh\n\n# a `backtick` in a comment\necho hi\n' > prose.sh
git add prose.sh
expect 1 "a staged comment quoting with backticks" sh ./scripts/git-pre-commit.sh
printf '#!/bin/sh\n\n# a comment naming prose.sh plainly\necho hi\n' > prose.sh
git add prose.sh
expect 0 "the same comment written the house way" sh ./scripts/git-pre-commit.sh
git rm -q --cached prose.sh
rm -f prose.sh

# rustfmt follows a module declaration into the file that defines it, and this
# repository generates one of those files and gitignores it. Without the stub
# the hook writes, staging any file that reaches src/engine/s2t.rs would fail on
# a path the author cannot produce.
mkdir -p src/engine
printf 'src/engine/s2t_data.rs\n' > .gitignore
printf 'mod s2t_data;\n\npub use s2t_data::TABLE;\n' > src/engine/s2t.rs
printf 'pub const TABLE: &str = "generated";\n' > src/engine/s2t_data.rs
printf '[package]\nname = "scratch"\nversion = "0.0.0"\nedition = "2024"\n' > Cargo.toml
git add .gitignore Cargo.toml src/engine/s2t.rs
expect 0 "a staged file whose child module is generated" sh ./scripts/git-pre-commit.sh
git rm -q --cached .gitignore Cargo.toml src/engine/s2t.rs
rm -rf src Cargo.toml .gitignore

# The ruleset lane runs the checker out of the index checkout, so it has to find
# the script and the asset beside each other there. Three stand-ins rather than
# 70k of real linter in a scratch repository: one that reports a conflict, one
# that rewrites the file the way hand formatting would be rewritten, and one
# that leaves it alone. The third is what proves the second is a check and not
# an unconditional failure.
mkdir -p assets
printf '{}\n' > assets/ruleset.json
printf '#!/usr/bin/env python3\nimport sys\n\nsys.exit(1)\n' > scripts/check-ruleset.py
git add assets/ruleset.json scripts/check-ruleset.py
expect 1 "a staged ruleset the checker rejects" sh ./scripts/git-pre-commit.sh

cat > scripts/check-ruleset.py << 'STANDIN'
#!/usr/bin/env python3
import pathlib

path = pathlib.Path(__file__).resolve().parent.parent / "assets" / "ruleset.json"
path.write_text('{"normalized": true}\n')
STANDIN
git add scripts/check-ruleset.py
expect 1 "a staged ruleset the checker rewrites" sh ./scripts/git-pre-commit.sh

printf '#!/usr/bin/env python3\npass\n' > scripts/check-ruleset.py
git add scripts/check-ruleset.py
expect 0 "a staged ruleset already normalized" sh ./scripts/git-pre-commit.sh
git rm -q --cached assets/ruleset.json scripts/check-ruleset.py
rm -rf assets scripts/check-ruleset.py

# Git runs the wrapper, which resolves the working tree's hook, so a staged
# change to a hook is judged by the hook it replaces. The lane that answers that
# runs the staged suite; a stand-in proves the lane fires and carries its status
# out, without the real suite running itself inside itself.
printf '#!/bin/sh\nexit 1\n' > scripts/test-git-hooks.sh
git add scripts/test-git-hooks.sh
expect 1 "a staged hook change runs the staged suite" sh ./scripts/git-pre-commit.sh
printf '#!/bin/sh\nexit 0\n' > scripts/test-git-hooks.sh
git add scripts/test-git-hooks.sh
expect 0 "the same lane when the suite passes" sh ./scripts/git-pre-commit.sh
git rm -q --cached scripts/test-git-hooks.sh
rm -f scripts/test-git-hooks.sh

echo "  HOOKS   pre-push"
cases=$((cases + 1))
git commit -q -m "Add the scratch repository" \
    -m "The push tests need a commit that the rules accept." > /dev/null 2>&1 \
    || fail "the hooks refused a commit that follows the rules"
git init -q --bare "$work/remote.git"
git remote add origin "$work/remote.git"
expect 0 "a branch whose messages pass" git push -q origin HEAD:refs/heads/main
git commit -q --allow-empty --no-verify -m "wip: skipped the hook"
expect 1 "a commit that skipped the hook" git push -q origin HEAD:refs/heads/main

# A new branch has no remote tip to diff against, so what it is judged against
# is what this remote already carries. Publish the commit above the way a
# --no-verify push does, then branch past it: without that exclusion every push
# of a new branch replays the whole history, and this repository's own log has
# four conventional-commit subjects and a past tense in it. The case reads as
# exotic and is not: it is what "git push -u origin topic" does every time.
git push -q --no-verify origin HEAD:refs/heads/legacy
git checkout -q -b topic
git commit -q --allow-empty -m "Add the topic branch" \
    -m "A body, so the message the hook sees is a whole one."
expect 0 "a new branch is not judged by published history" \
    git push -q origin topic:refs/heads/topic
git push -q --no-verify origin HEAD:refs/heads/main
git init -q --bare "$work/second-remote.git"
git remote add second "$work/second-remote.git"
expect 1 "a published bad commit reaches a new remote" \
    git push -q second HEAD:refs/heads/main

# Linked worktrees share one hooks directory. The installed wrapper must
# therefore find the worktree that runs it rather than the one it was installed
# from, which is what a path baked into the wrapper would get wrong.
git worktree add -q -b linked "$work/linked" || exit 1
(mkdir -p "$work/linked/scripts" \
    && cp scripts/git-*.sh scripts/install-git-hooks.sh "$work/linked/scripts/") \
    || exit 1
printf '#!/bin/sh\nexit 1\n' > scripts/git-pre-commit.sh
cases=$((cases + 1))
(cd "$work/linked" \
    && printf '#!/bin/sh\n\necho staged\n' > linked.sh \
    && git add linked.sh \
    && git commit -q -m "Add the linked worktree") \
    || fail "a linked worktree runs its own hook"

if [ "$failures" -eq 0 ]; then
    printf '  HOOKS   %d checks passed\n' "$cases"
    exit 0
fi

printf '  HOOKS   %d of %d checks failed\n' "$failures" "$cases" >&2
exit 1
