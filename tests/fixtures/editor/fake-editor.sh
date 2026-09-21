# Stand-in for $EDITOR in integration tests. Run as `/bin/sh fake-editor.sh`,
# so it needs no executable bit.
#
# Appends one line per invocation to $FAKE_EDITOR_LOG: the arguments it was
# given, tab-separated. If $FAKE_EDITOR_APPEND is set, appends that text to
# the last argument (the file being edited), like a user typing notes.

(IFS="$(printf '\t')"; printf '%s\n' "$*") >> "$FAKE_EDITOR_LOG"

if [ -n "$FAKE_EDITOR_APPEND" ]; then
    for last in "$@"; do :; done
    printf '%s\n' "$FAKE_EDITOR_APPEND" >> "$last"
fi
