PREFIX ?= /usr/local
DESTDIR ?=
CARGO ?= cargo

BIN := target/release/appctl
DIST := target/dist
BINDIR := $(PREFIX)/bin
MANDIR := $(PREFIX)/share/man/man1
BASH_COMPLETION_DIR := $(PREFIX)/share/bash-completion/completions
ZSH_COMPLETION_DIR := $(PREFIX)/share/zsh/site-functions
FISH_COMPLETION_DIR := $(PREFIX)/share/fish/vendor_completions.d

.PHONY: all build test check install uninstall completions man clean

all: build

build:
	$(CARGO) build --release

test:
	$(CARGO) test

check:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings

# Generated from the clap definitions, so they cannot drift from the CLI.
man: build
	install -d $(DIST)
	$(BIN) man > $(DIST)/appctl.1

completions: build
	install -d $(DIST)
	$(BIN) completions bash > $(DIST)/appctl.bash
	$(BIN) completions zsh > $(DIST)/_appctl
	$(BIN) completions fish > $(DIST)/appctl.fish

install: build man completions
	install -Dm755 $(BIN) $(DESTDIR)$(BINDIR)/appctl
	install -Dm644 $(DIST)/appctl.1 $(DESTDIR)$(MANDIR)/appctl.1
	install -Dm644 $(DIST)/appctl.bash $(DESTDIR)$(BASH_COMPLETION_DIR)/appctl
	install -Dm644 $(DIST)/_appctl $(DESTDIR)$(ZSH_COMPLETION_DIR)/_appctl
	install -Dm644 $(DIST)/appctl.fish $(DESTDIR)$(FISH_COMPLETION_DIR)/appctl.fish

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/appctl
	rm -f $(DESTDIR)$(MANDIR)/appctl.1
	rm -f $(DESTDIR)$(BASH_COMPLETION_DIR)/appctl
	rm -f $(DESTDIR)$(ZSH_COMPLETION_DIR)/_appctl
	rm -f $(DESTDIR)$(FISH_COMPLETION_DIR)/appctl.fish

clean:
	$(CARGO) clean
