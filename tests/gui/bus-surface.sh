#!/bin/bash
# What another program on the session bus can reach in Files: the application
# object KDBusService exports at /MainApplication (quit(), closeAllWindows(),
# setStyleSheet()) must not be there, org.freedesktop.Application and
# FileManager1 and a second launch must still work, and a call to quit() must
# not end the app.
#
# Run it in the dev container, under a session bus, with the app built in
# <build dir> (default /work/cmake/dev):
#   scripts/dev.sh dbus-run-session -- bash tests/gui/bus-surface.sh [<build dir>]
# The app runs offscreen with a home folder of its own under /var/tmp.
set -u
B=${1:-/work/cmake/dev}
T=$(mktemp -d /var/tmp/bus-surface.XXXXXX)
export HOME=$T/home XDG_CONFIG_HOME=$T/home/.config XDG_CACHE_HOME=$T/home/.cache \
    XDG_STATE_HOME=$T/home/.local/state XDG_DATA_HOME=$T/home/.local/share XDG_RUNTIME_DIR=$T/rt
export QT_QPA_PLATFORM=offscreen
mkdir -p "$HOME" "$XDG_RUNTIME_DIR" "$T/folder"
chmod 700 "$XDG_RUNTIME_DIR"
timeout 90 "$B/telamon-explorer" "$T/folder" > "$T/app.log" 2>&1 &
APP=$!
# shellcheck disable=SC2329  # run by the trap below
cleanup() { kill "$APP" 2>/dev/null; wait "$APP" 2>/dev/null; rm -rf "$T"; }
trap cleanup EXIT
NAME=net.eterneon.telamon.explorer
for _ in $(seq 100); do
    busctl --user status "$NAME" > /dev/null 2>&1 && break
    sleep 0.2
done
busctl --user status "$NAME" > /dev/null 2>&1 || { echo "FAIL: the app did not take its bus name"; cat "$T/app.log"; exit 1; }
fail=0
if busctl --user tree "$NAME" | grep -q '/MainApplication'; then
    echo "FAIL: /MainApplication is exported"; fail=1
fi
if busctl --user call "$NAME" /MainApplication org.qtproject.Qt.QCoreApplication quit > /dev/null 2>&1; then
    echo "FAIL: quit() was answered"; fail=1
fi
busctl --user call "$NAME" /net/eterneon/telamon/explorer org.freedesktop.Application Open 'asa{sv}' 1 "file://$T/folder" 0 > /dev/null 2>&1 \
    || { echo "FAIL: org.freedesktop.Application.Open does not work"; fail=1; }
busctl --user call org.freedesktop.FileManager1 /org/freedesktop/FileManager1 org.freedesktop.FileManager1 ShowFolders 'ass' 1 "file://$T/folder" "" > /dev/null 2>&1 \
    || { echo "FAIL: FileManager1.ShowFolders does not work"; fail=1; }
# A second launch hands its arguments to the running window through
# org.kde.KDBusService.CommandLine (which any bus peer can also call: that call
# cannot be told from a launch, so it opens what a command line may open).
timeout 30 "$B/telamon-explorer" "$T/folder" > /dev/null 2>&1 \
    || { echo "FAIL: a second launch does not forward to the running window"; fail=1; }
sleep 1
kill -0 "$APP" 2>/dev/null || { echo "FAIL: the app is gone"; fail=1; }
[ "$fail" = 0 ] && echo "PASS"
exit "$fail"
