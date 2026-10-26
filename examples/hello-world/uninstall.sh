#!/bin/sh
# Remove the hello-world demo app: unit, config, provisioned account and state directory.
#
# `appd remove` owns everything it created; the binary under /srv is ours, so it goes too.
# Run as root.

set -eu

APP=hello-world
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
APPD=${APPD:-appd}

if [ "$(id -u)" -ne 0 ]; then
    echo "uninstall.sh: run as root" >&2
    exit 1
fi

if ! command -v "$APPD" >/dev/null 2>&1; then
    if [ -x "$HERE/../../target/release/appd" ]; then
        APPD="$HERE/../../target/release/appd"
    elif [ -x "$HERE/../../target/debug/appd" ]; then
        APPD="$HERE/../../target/debug/appd"
    else
        echo "uninstall.sh: cannot find appd; build it or set APPD=/path/to/appd" >&2
        exit 1
    fi
fi

"$APPD" remove "$APP" --yes

# appd does not install or own the application binary, so removing /srv is up to us.
rm -rf "/srv/$APP"

echo "$APP uninstalled."
