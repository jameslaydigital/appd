#!/bin/sh
# Install the hello-world demo app for appd.
#
# Writes /srv/hello-world, /etc/appd/hello-world.json and appd-hello-world.service,
# then enables and starts it. Run as root, and set APPD if the binary is not on PATH.

set -eu

APP=hello-world
DEST=/srv/$APP
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
APPD=${APPD:-appd}

if [ "$(id -u)" -ne 0 ]; then
    echo "install.sh: run as root; it writes to /srv, /etc/appd and /etc/systemd/system" >&2
    exit 1
fi

if ! command -v "$APPD" >/dev/null 2>&1; then
    if [ -x "$HERE/../../target/release/appd" ]; then
        APPD="$HERE/../../target/release/appd"
    elif [ -x "$HERE/../../target/debug/appd" ]; then
        APPD="$HERE/../../target/debug/appd"
    else
        echo "install.sh: cannot find appd; build it or set APPD=/path/to/appd" >&2
        exit 1
    fi
fi

install -d -m 0755 "$DEST/bin"
install -m 0755 "$HERE/hello" "$DEST/bin/hello"
# The directory is listable so non-root `status` can see app names; the config files
# themselves stay 0600 because they may hold secrets.
install -d -m 0755 /etc/appd
install -m 0600 "$HERE/$APP.json" "/etc/appd/$APP.json"

"$APPD" apply "$APP" --yes
"$APPD" enable "$APP"
"$APPD" restart "$APP"

cat <<EOF

$APP installed and started.

  appd status $APP
  appd logs $APP -f
  appd logs $APP --since 1m
  appd error-logs $APP        # one simulated problem every tenth beat
  appd reload $APP            # prints a "reloaded" line
  appd restart $APP
  systemctl cat appd-$APP.service

To remove it again:

  $(dirname -- "$0")/uninstall.sh
EOF
