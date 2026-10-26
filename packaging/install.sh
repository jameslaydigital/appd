#!/bin/sh
# Install appd from an unpacked release directory.
#
#   sudo ./install.sh                            # /usr/local
#   PREFIX="$HOME/.local" ./install.sh           # user-local
#   DESTDIR=/tmp/stage PREFIX=/usr ./install.sh  # packaging
#   sudo ./install.sh --uninstall
#
# The bundled binary does the work, so the tarball and `cargo install` share one
# implementation. No daemon, no service to enable, no runtime.

set -eu

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PREFIX=${PREFIX:-/usr/local}
DESTDIR=${DESTDIR:-}

if [ ! -x "$HERE/appd" ]; then
    echo "install.sh: appd not found next to this script" >&2
    exit 1
fi

if [ -n "$DESTDIR" ]; then
    exec "$HERE/appd" install --prefix "$PREFIX" --destdir "$DESTDIR" "$@"
fi
exec "$HERE/appd" install --prefix "$PREFIX" "$@"
