#!/bin/sh

# Hold a commit message to the rules this log already follows, with the widths
# measured off it: subjects sit at 43 columns in the median and the bodies wrap
# at 72. See scripts/install-git-hooks.sh for installation.
#
# A message arriving as "-" is read from standard input instead of from a file,
# which is how scripts/check-commit-log.sh replays an existing commit through
# the same rules. One list, so a contributor with the hooks installed and one
# without are judged the same way.
#
# Chinese is allowed and the widths are counted in terminal columns rather than
# characters or bytes. This log names the term a change is about, in the script
# the change is about, and a rule that forbade that would be a rule against the
# project. A CJK character costs two columns, which is what git log --oneline
# has to fit, so that is what gets counted.

set -u

# The rules as prose, printed for scripts/git-prepare-commit-msg.sh to put in
# front of whoever is writing the message. Here rather than there so the list a
# contributor reads and the list this script enforces cannot drift apart.
if [ "${1:-}" = "--rules" ]; then
    cat << 'EOF'
1. Separate the subject from the body with a blank line
2. Keep the subject within 50 columns, and say more than one word
3. Capitalize the subject; no trailing period, no backticks
4. Use the imperative mood: "Fix", not "Fixed" or "Fixes"
5. No conventional-commit prefix: not "fix:", not "feat(extension):"
6. Wrap the body at 72 columns
7. Say what and why; the diff already says how
8. No tabs, no control characters, no bidirectional overrides
9. No em dash character (U+2014); a comma or a colon says it

The subject starts with a non-blank capital and is English prose that may
name a Chinese term; a body may be written in either. A CJK character
counts as two columns. Backticks are fine in the body and not in the
subject. One paragraph carries most changes here; the per-decision detail
belongs in the comment next to the code.
EOF
    exit 0
fi

message_file=${1:-}

if [ -z "$message_file" ]; then
    echo "usage: ${0##*/} [--rules | <message-file> | -]" >&2
    exit 2
fi

if [ "$message_file" = "-" ]; then
    message_file=$(mktemp) || exit 1
    trap 'rm -f "$message_file"' EXIT
    trap 'rm -f "$message_file"; exit 130' HUP INT TERM
    cat > "$message_file"
fi

# "git commit -v" puts the diff after a scissors line and drops it before the
# message is stored, so the rules must not see it. Comments go the same way.
# Under commit.cleanup=verbatim the comment lines are kept in the stored
# message, so stripping them would judge a message nobody commits. Everything
# below the scissors goes either way: git drops it before storing.
cleanup=$(git config --get commit.cleanup 2> /dev/null) || cleanup=default

# Each mode judged the way git stores it. Under verbatim nothing is removed, so
# the scissors line and everything below it is part of the message. Under
# whitespace git keeps comment lines and only trims blanks, so stripping them
# here would judge text nobody commits. The rest drop comments, which is what
# --strip-comments does.
case "$cleanup" in
    verbatim)
        message=$(cat "$message_file")
        ;;
    whitespace)
        message=$(sed '/-\{8,\}[[:space:]]*>8[[:space:]]*-\{8,\}/,$d' "$message_file" \
            | git stripspace)
        ;;
    *)
        message=$(sed '/-\{8,\}[[:space:]]*>8[[:space:]]*-\{8,\}/,$d' "$message_file" \
            | git stripspace --strip-comments)
        ;;
esac

# A carriage return at end of line is what an editor writing CRLF leaves behind,
# not something an author typed, and the control character check below would
# otherwise refuse every commit written in one while naming a character nobody
# can see. Spelled through a variable because BSD sed reads a backslash-r as an
# r. A stray return inside a line survives this and still fails there.
cr=$(printf '\r')
message=$(printf '%s\n' "$message" | sed "s/$cr\$//")

subject=$(printf '%s\n' "$message" | sed -n '1p')
second=$(printf '%s\n' "$message" | sed -n '2p')
body=$(printf '%s\n' "$message" | sed -n '2,$p')
failed=0

error()
{
    printf 'Commit message: %s\n' "$1" >&2
    failed=1
}

# One display width per input line. Python 3 is a build requirement of this
# repository, so this is not a new tool on anybody's machine, but a lane that
# cannot run is still a lane that says so rather than a hook that dies.
columns()
{
    python3 -c '
import sys, unicodedata

for line in sys.stdin.read().splitlines():
    print(sum(2 if unicodedata.east_asian_width(c) in "WF" else 1 for c in line))
'
}

# A width the measurement did not produce is not a width of zero, and treating
# it as one is how a check stops running without anybody noticing. Say which of
# the two happened and skip the lane either way.
measurable=yes
if ! command -v python3 > /dev/null 2>&1; then
    measurable=
    echo "Commit message: python3 not installed, skipping the width checks" >&2
elif ! width=$(printf '%s\n' "$subject" | columns); then
    measurable=
    echo "Commit message: python3 cannot measure widths, skipping those checks" >&2
fi

# Everything above the exemption below, because these three are about what the
# bytes do to a reader rather than about how the prose is written. A message
# nobody composed can still carry a tab, a bidirectional override, or the em
# dash an editor substituted, and an amend! message becomes the stored message
# of the commit it names. Tabs and control characters, checked on the bytes: a
# subject is text on its way to a terminal, and an escape sequence in one
# rewrites what the reader sees around it. Newlines are removed first because
# the message has them by construction.
if printf '%s' "$message" | tr -d '\n' | LC_ALL=C grep -q '[[:cntrl:]]'; then
    error "message must not contain tabs or control characters"
fi

# Above what the C locale can see: C1 controls, and the Unicode format
# characters, which include the bidirectional overrides. Those reorder what a
# terminal or a review page displays without changing the bytes anyone reads,
# which is the whole point of checking a subject at all. Skipped with the width
# lanes when python3 is missing, and the grep above is the floor either way.
if [ -n "$measurable" ]; then
    hidden=$(printf '%s' "$message" | python3 -c '
import sys, unicodedata

for c in sys.stdin.read():
    if ord(c) > 127 and unicodedata.category(c) in ("Cc", "Cf"):
        print("U+%04X" % ord(c))
' | sort -u | tr '\n' ' ')
    [ -z "$hidden" ] || error "message contains hidden characters: $hidden"
fi

# The house rule for prose in this tree, and the one substitution an editor
# makes without being asked.
if printf '%s' "$message" | grep -q -- '—'; then
    error "message must not contain an em dash"
fi

# What the exemption does cover is every style rule below. A fixup lands in the
# commit it names, a squash is folded into one, and a merge subject is git
# wording rather than the author's. Holding any of those to a width or a mood is
# judging a message nobody wrote. Exits with what the checks above found, so a
# hidden character still fails here.
case "$subject" in
    "fixup! "* | "squash! "* | "amend! "*) exit "$failed" ;;
    "Merge branch "* | "Merge branches "* | "Merge tag "* | "Merge commit "* | \
        "Merge pull request "* | "Merge remote-tracking branch "* | \
        "Merge "*" into "*) exit "$failed" ;;
esac

if [ -z "$subject" ]; then
    error "a descriptive subject is required"
elif [ -n "$measurable" ] && [ "$width" -gt 50 ]; then
    error "subject is $width columns; keep it within 50"
else
    case "$subject" in
        *[[:space:]]*) ;;

        # Which also settles the language of the subject line: a subject written
        # entirely in Chinese has no space in it. That matches every subject in
        # this log, where the prose is English and the term the change is about
        # is quoted in it.
        *) error "subject must say more than one word" ;;
    esac
fi

# Every subject in this log opens with an uppercase letter, and the rule was
# enforced only against a lowercase one, so a digit or a bracket opened a
# subject nothing judged.
case "$subject" in
    [[:space:]]*) error "subject must not start with whitespace" ;;
    [[:upper:]]*) ;;
    "") ;;
    *) error "capitalize the subject; it opens with a letter" ;;
esac

case "$subject" in
    *.) error "subject must not end with a period" ;;
esac

# The subject completes "this commit will ...", so a past tense or a gerund in
# the first word is the whole tell. Checking one word catches nearly all of it
# and never argues with a legitimate "Fix the scanner" style subject.
first_word=$(printf '%s' "$subject" \
    | sed 's/[[:space:]].*//; s/[[:punct:]]*$//' | tr '[:upper:]' '[:lower:]')
case "$first_word" in
    added | adds | adding | adjusted | adjusts | adjusting | allowed | allows | \
        allowing | avoided | avoids | avoiding | bumped | bumps | bumping | \
        changed | changes | changing | checked | checks | checking | cleaned | \
        cleans | cleaning | corrected | corrects | correcting | created | creates | \
        creating | deleted | deletes | deleting | disabled | disables | disabling | \
        dropped | drops | dropping | enabled | enables | enabling | fixed | fixes | \
        fixing | handled | handles | handling | implemented | implements | \
        implementing | improved | improves | improving | included | includes | \
        including | introduced | introduces | introducing | made | makes | making | \
        moved | moves | moving | refactored | refactors | refactoring | removed | \
        removes | removing | renamed | renames | renaming | replaced | replaces | \
        replacing | reverted | reverts | reverting | tested | tests | testing | \
        tidied | tidies | tidying | updated | updates | updating | used | uses | \
        using)
        error "use the imperative mood: \"$first_word\" describes what you did"
        ;;
esac

# Four of these reached this log before anyone wrote the rules down, and none
# recently. Both this and a backtick in a subject read as an import from another
# project's tooling.
#
# The type list is spelled out rather than matched as "a word and a colon". An
# area prefix is a different thing and a normal one, and a rule that rejected
# "Windows: retry the replace" would be enforcing something nobody wrote down.
if printf '%s' "$subject" \
    | grep -qE '^(build|chore|ci|docs|feat|fix|perf|refactor|revert|style|test)(\([^)]*\))?!?:'; then
    error "subject must not use conventional-commit syntax"
fi

case "$subject" in
    *'`'*) error "subject must not contain backticks" ;;
esac

# "second" is line 2 and "body" is line 2 onward, so a non-empty line 2 is a
# body that started without a blank line between it and the subject.
if [ -n "$second" ]; then
    error "separate the subject from the body with a blank line"
fi

if [ -n "$measurable" ]; then
    if ! body_widths=$(printf '%s\n' "$body" | columns); then
        error "cannot measure the body width"
    elif printf '%s\n' "$body_widths" | awk '$1 > 72' | grep -q .; then
        error "wrap body lines at 72 columns"
    fi
fi

# The reasoning per decision lives in the comment next to the code in this tree.
# A body that walks through the implementation duplicates it and then goes stale
# on its own.
if printf '%s\n' "$body" \
    | grep -qiE '^(how|implementation( (details|notes|steps))?|steps?|changes)[[:space:]]*:'; then
    error "the body carries what and why; how is the diff's job"
fi

# Not an error: long bodies exist in this history. Still worth saying once,
# because the house shape is a premise and its trade, not a retelling.
blanks=$(printf '%s\n' "$body" | git stripspace | grep -c '^$')
if [ "$blanks" -gt 1 ]; then
    printf 'Commit message: note, %s paragraphs; one usually carries it.\n' \
        "$((blanks + 1))" >&2
fi

exit "$failed"
