#!/bin/sh
# Install appd from an unpacked release directory.
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

BIN="$DESTDIR$PREFIX/bin/appd"
MAN="$DESTDIR$PREFIX/share/man/man1/appd.1"
BASH_COMPLETION="$DESTDIR$PREFIX/share/bash-completion/completions/appd"
ZSH_COMPLETION="$DESTDIR$PREFIX/share/zsh/site-functions/_appd"
FISH_COMPLETION="$DESTDIR$PREFIX/share/fish/vendor_completions.d/appd.fish"

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$BIN" "$MAN" "$BASH_COMPLETION" "$ZSH_COMPLETION" "$FISH_COMPLETION"
    echo "removed appd from $DESTDIR$PREFIX"
    echo "configs, units and accounts were not touched; use 'appd remove <app>' per app"
    exit 0
fi

if [ ! -x "$HERE/appd" ]; then
    echo "install.sh: appd not found next to this script" >&2
    exit 1
fi

install -Dm755 "$HERE/appd" "$BIN"
install -Dm644 "$HERE/appd.1" "$MAN"

[ -f "$HERE/completions/appd.bash" ] &&
    install -Dm644 "$HERE/completions/appd.bash" "$BASH_COMPLETION"
[ -f "$HERE/completions/_appd" ] &&
    install -Dm644 "$HERE/completions/_appd" "$ZSH_COMPLETION"
[ -f "$HERE/completions/appd.fish" ] &&
    install -Dm644 "$HERE/completions/appd.fish" "$FISH_COMPLETION"

echo "installed $("$HERE/appd" --version) to $BIN"
echo "next: $PREFIX/bin/appd --help"
