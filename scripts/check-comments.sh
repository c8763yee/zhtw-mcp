#!/bin/sh

# Hold source comments to the two prose rules this tree has that no formatter
# knows about: no em dash, and no backtick outside a doc comment.
#
# Both were swept out of the tree in one pass, and a sweep nothing guards grows
# back one comment at a time. This is what makes the rule cost a contributor a
# message now rather than a reviewer a remark later.
#
# Paths named on the command line are checked as given; with none, every tracked
# and untracked-but-not-ignored source file is.

set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT" || exit 1

if [ "$#" -gt 0 ]; then

    # Word splitting carries the list below, so a path it cannot carry has to
    # stop the run rather than be checked as two paths or expanded as a glob.
    # Tested one argument at a time, because the joined list separates its own
    # entries with the blank this is looking for.
    unusable=
    for arg in "$@"; do
        case "$arg" in
            *[[:blank:]]* | *'*'* | *'?'* | *'['*) unusable="$unusable$arg
" ;;
        esac
    done
    if [ -n "$unusable" ]; then
        printf '%s' "$unusable" | sed 's/^/  /' >&2
        echo "${0##*/}: the paths above cannot be passed through a shell list" >&2
        exit 2
    fi
    files=$*
else
    files=$(git ls-files --cached --others --exclude-standard \
        -- 'build.rs' '*.rs' '*.sh' '*.py' | sort -u)
fi

[ -n "$files" ] || exit 0
failed=0

# grep says "No such file" on standard error and the pipelines below read only
# standard output, so a path this script cannot read would come back as no hits
# and be reported as clean. Refuse the run instead: a gate that passes what it
# never opened is worse than one that is not there.
missing=
for file in $files; do
    [ -f "$file" ] && [ -r "$file" ] || missing="$missing $file"
done
if [ -n "$missing" ]; then
    printf '%s\n' "$missing" | tr ' ' '\n' | sed '/^$/d; s/^/  /' >&2
    echo "${0##*/}: cannot read the paths above" >&2
    exit 2
fi

# Full-line comments only. A trailing comment sits on a line that also holds
# code, and telling the two apart needs a parser rather than a grep: a string
# literal holding a backtick or a dash would fail a gate it never broke. The
# same limit applied to the sweep this guards, and what it lets through is a
# trailing comment, which is short by nature and rarely carries either.
#
# The hash opens a comment in shell and in python and opens an attribute in
# Rust, so a bare hash would read every "#[derive(...)]" and "#![doc = ...]" as
# prose. An attribute is code, and the string inside one carries backticks and
# dashes for reasons this rule has nothing to say about, so the bracket forms
# are excluded and a shebang, which is neither, is not.
comment_lines='^[[:space:]]*(//|#($|[^![/]|![^[/]))'

# Any em dash, spaced or not: the sweep that emptied this tree put a space on
# each side of every one it found, so an unspaced one is the shape that would
# arrive next and the one a space-anchored pattern would miss. The doubled form
# is the zh-TW 破折號 and is data here rather than punctuation, so it is left
# alone, as is one inside quotes, which names the character rather than uses it.
report()
{
    printf '%s\n' "$2" | sed 's/^/  /' >&2
    printf '%s\n' "$1" >&2
    failed=1
}

# shellcheck disable=SC2086
hits=$(grep -nE "$comment_lines" $files /dev/null \
    | grep -F '—' \
    | grep -vF '——' | grep -vF "'—'" | grep -vF '"—"')
[ -z "$hits" ] || report \
    "check-comments: the comments above use an em dash; a comma or a colon says it" \
    "$hits"

# Doc comments are rustdoc markdown, which renders a backticked span as code and
# resolves an intra-doc link inside one. That is the same reason the Markdown
# files are exempt from this rule, so /// and //! are exempt too.
# shellcheck disable=SC2086
hits=$(grep -nE "$comment_lines" $files /dev/null \
    | grep -vE '^[^:]*:[0-9]+:[[:space:]]*(///|//!)' \
    | grep -F '`')
[ -z "$hits" ] || report \
    "check-comments: the comments above quote with backticks; name the identifier instead" \
    "$hits"

[ "$failed" -eq 0 ] && echo "Comment prose clean."
exit "$failed"
