#!/bin/sh
# Serve app/ui in a normal browser with a fake backend (scripts/ui-mock.js), for UI work
# on a machine that can't run kit. Open http://127.0.0.1:8765/ and add query options:
#   ?kb=nk65|proxtkl|none  &color=white|black  &setup=1 (first-run screen)  &ghub=1 (G HUB "running")
#   &fw=old|kit|restore (NK65: stock firmware, kit's firmware, kit's just flashed)
#   &micerror=1 (the mic test fails to start)
set -e
here=$(cd "$(dirname "$0")" && pwd)
dir=$(mktemp -d)
ui="$here/../app/ui"
ln -s "$ui" "$dir/ui"
ln -s "$ui/img" "$dir/img"
ln -s "$ui/fonts" "$dir/fonts"
cp "$here/ui-mock.js" "$dir/mock.js"
sed -e 's#<script src="core.js"></script>#<script src="/mock.js"></script>\n  <script src="core.js"></script>#' \
    -e 's#href="\([a-z]*\.css\)"#href="ui/\1"#' -e 's#src="\([a-z]*\.js\)"#src="ui/\1"#g' \
    -e 's#href="fonts/#href="ui/fonts/#' "$ui/index.html" > "$dir/index.html"
echo "kit UI preview on http://127.0.0.1:8765/ (Ctrl+C to stop)"
cd "$dir" && exec python3 -m http.server 8765 --bind 127.0.0.1
