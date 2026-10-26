#!/bin/sh
# Install appctl from an unpacked release directory.
#
#   sudo ./install.sh                          # /usr/local
#   PREFIX="$HOME/.local" ./install.sh         # user-local
#   DESTDIR=/tmp/stage PREFIX=/usr ./install.sh  # packaging
#   sudo ./install.sh --uninstall
#
# The binary is the whole program: no daemon, no service to enable, no runtime.

set -eu

PREFIX=${PREFIX:-/usr/local}
DESTDIR=${DESTDIR:-}
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

BIN="$DESTDIR$PREFIX/bin/appctl"
MAN="$DESTDIR$PREFIX/share/man/man1/appctl.1"
BASH_COMPLETION="$DESTDIR$PREFIX/share/bash-completion/completions/appctl"
ZSH_COMPLETION="$DESTDIR$PREFIX/share/zsh/site-functions/_appctl"
FISH_COMPLETION="$DESTDIR$PREFIX/share/fish/vendor_completions.d/appctl.fish"

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$BIN" "$MAN" "$BASH_COMPLETION" "$ZSH_COMPLETION" "$FISH_COMPLETION"
    echo "removed appctl from $DESTDIR$PREFIX"
    echo "configs, units and accounts were not touched; use 'appctl remove <app>' per app"
    exit 0
fi

if [ ! -x "$HERE/appctl" ]; then
    echo "install.sh: appctl not found next to this script" >&2
    exit 1
fi

install -Dm755 "$HERE/appctl" "$BIN"
install -Dm644 "$HERE/appctl.1" "$MAN"

[ -f "$HERE/completions/appctl.bash" ] &&
    install -Dm644 "$HERE/completions/appctl.bash" "$BASH_COMPLETION"
[ -f "$HERE/completions/_appctl" ] &&
    install -Dm644 "$HERE/completions/_appctl" "$ZSH_COMPLETION"
[ -f "$HERE/completions/appctl.fish" ] &&
    install -Dm644 "$HERE/completions/appctl.fish" "$FISH_COMPLETION"

echo "installed $("$HERE/appctl" --version) to $BIN"
echo "next: $PREFIX/bin/appctl --help"
