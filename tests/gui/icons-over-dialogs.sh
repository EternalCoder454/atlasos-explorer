#!/bin/bash
# Pixel test: with Quick's software renderer no icon of the folder view behind
# is drawn over a dialog. Opens Batch Rename on a folder of images, types in its
# Find field (every keystroke repaints part of the dialog) and counts the
# icon-green pixels inside the dialog's area: there must be none.
#
# Run it in the dev container, under a virtual display and a session bus, with
# the app built in <build dir> (default /work/cmake/dev):
#   scripts/dev.sh xvfb-run -a -s "-screen 0 1600x900x24" \
#       dbus-run-session -- bash tests/gui/icons-over-dialogs.sh [<build dir>]
# QT_QUICK_BACKEND=software is the default; QT_QUICK_BACKEND=opengl is not
# available on a virtual display, so the software path is what is tested.
# The shot is kept in $OUT (default: the fixture's own dir) as icons-over-dialogs.png.
set -u
B=${1:-/work/cmake/dev}
T=$(mktemp -d /var/tmp/icons-over-dialogs.XXXXXX)
OUT=${OUT:-$T}
export HOME=$T/home XDG_CONFIG_HOME=$T/home/.config XDG_CACHE_HOME=$T/home/.cache \
    XDG_STATE_HOME=$T/home/.local/state XDG_DATA_HOME=$T/home/.local/share XDG_RUNTIME_DIR=$T/rt
export QT_QPA_PLATFORM=xcb
mkdir -p "$HOME/Documents/Batch" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
for i in $(seq 1 24); do
    n=$(printf %02d "$i")
    magick -size 120x90 gradient:'#3060d0-#d06030' "$HOME/Documents/Batch/IMG_$n.JPG"
done
for i in 1 2 3 4 5 6; do echo "text $i" > "$HOME/Documents/Batch/notes-$i.txt"; done

timeout 60 "$B/telamon-explorer" "$HOME/Documents/Batch" > "$T/app.log" 2>&1 &
APP=$!
cleanup() { kill "$APP" 2>/dev/null; wait "$APP" 2>/dev/null; [ "$OUT" = "$T" ] || rm -rf "$T"; }
trap cleanup EXIT

wid=
for _ in $(seq 100); do
    wid=$(xdotool search --onlyvisible --name "Files" | head -1)
    [ -n "$wid" ] && break
    sleep 0.2
done
[ -n "$wid" ] || { echo "FAIL: no window"; exit 1; }
sleep 3
xdotool windowfocus "$wid"; sleep 0.3
eval "$(xdotool getwindowgeometry --shell "$wid")"    # X Y WIDTH HEIGHT
xdotool mousemove $((X + WIDTH / 2)) $((Y + HEIGHT / 3)); xdotool click 1; sleep 0.4
xdotool key --clearmodifiers ctrl+a; sleep 0.4
xdotool key --clearmodifiers F2; sleep 1.5
xdotool type --delay 120 -- "IMG_"; sleep 1
import -window root "$OUT/icons-over-dialogs.png"

# The dialog is centered: its middle 60% by 72% lies inside it at any size.
cw=$((WIDTH * 60 / 100)); ch=$((HEIGHT * 72 / 100))
cx=$((X + WIDTH * 20 / 100)); cy=$((Y + HEIGHT * 14 / 100))
green=$(magick "$OUT/icons-over-dialogs.png" -crop "${cw}x${ch}+${cx}+${cy}" +repage \
    -fx 'g>r+0.12&&g>b+0.12?1:0' -format '%[fx:round(mean*w*h)]' info:)
echo "icon-green pixels inside the Batch Rename dialog: $green"
if [ "$green" -eq 0 ]; then echo "PASS"; else echo "FAIL: icons are drawn over the dialog"; exit 1; fi
