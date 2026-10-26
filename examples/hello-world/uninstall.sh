#!/bin/sh
# Remove the hello-world demo app: unit, config, provisioned account and state directory.
#
# `appctl remove` owns everything it created; the binary under /srv is ours, so it goes too.
# Run as root.

set -eu

APP=hello-world
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
APPCTL=${APPCTL:-appctl}

if [ "$(id -u)" -ne 0 ]; then
    echo "uninstall.sh: run as root" >&2
    exit 1
fi

if ! command -v "$APPCTL" >/dev/null 2>&1; then
    if [ -x "$HERE/../../target/release/appctl" ]; then
        APPCTL="$HERE/../../target/release/appctl"
    elif [ -x "$HERE/../../target/debug/appctl" ]; then
        APPCTL="$HERE/../../target/debug/appctl"
    else
        echo "uninstall.sh: cannot find appctl; build it or set APPCTL=/path/to/appctl" >&2
        exit 1
    fi
fi

"$APPCTL" remove "$APP" --yes

# appctl does not install or own the application binary, so removing /srv is up to us.
rm -rf "/srv/$APP"

echo "$APP uninstalled."
