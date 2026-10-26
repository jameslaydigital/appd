#!/bin/sh
# Install the hello-world demo app for appctl.
#
# Writes /srv/hello-world, /etc/appctl/hello-world.json and appctl-hello-world.service,
# then enables and starts it. Run as root, and set APPCTL if the binary is not on PATH.

set -eu

APP=hello-world
DEST=/srv/$APP
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
APPCTL=${APPCTL:-appctl}

if [ "$(id -u)" -ne 0 ]; then
    echo "install.sh: run as root; it writes to /srv, /etc/appctl and /etc/systemd/system" >&2
    exit 1
fi

if ! command -v "$APPCTL" >/dev/null 2>&1; then
    if [ -x "$HERE/../../target/release/appctl" ]; then
        APPCTL="$HERE/../../target/release/appctl"
    elif [ -x "$HERE/../../target/debug/appctl" ]; then
        APPCTL="$HERE/../../target/debug/appctl"
    else
        echo "install.sh: cannot find appctl; build it or set APPCTL=/path/to/appctl" >&2
        exit 1
    fi
fi

install -d -m 0755 "$DEST/bin"
install -m 0755 "$HERE/hello" "$DEST/bin/hello"
# The directory is listable so non-root `status` can see app names; the config files
# themselves stay 0600 because they may hold secrets.
install -d -m 0755 /etc/appctl
install -m 0600 "$HERE/$APP.json" "/etc/appctl/$APP.json"

"$APPCTL" apply "$APP" --yes
"$APPCTL" enable "$APP"
"$APPCTL" restart "$APP"

cat <<EOF

$APP installed and started.

  appctl status $APP
  appctl logs $APP -f
  appctl logs $APP --since 1m
  appctl error-logs $APP        # one simulated problem every tenth beat
  appctl reload $APP            # prints a "reloaded" line
  appctl restart $APP
  systemctl cat appctl-$APP.service

To remove it again:

  $(dirname -- "$0")/uninstall.sh
EOF
