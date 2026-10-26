PREFIX ?= /usr/local
DESTDIR ?=
CARGO ?= cargo

BIN := target/release/appctl
DIST := target/dist
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
TARGET ?= x86_64-unknown-linux-musl
DIST_NAME := appctl-$(VERSION)-$(TARGET)
DIST_DIR := $(DIST)/$(DIST_NAME)
TARGET_BIN := target/$(TARGET)/release/appctl
BINDIR := $(PREFIX)/bin
MANDIR := $(PREFIX)/share/man/man1
BASH_COMPLETION_DIR := $(PREFIX)/share/bash-completion/completions
ZSH_COMPLETION_DIR := $(PREFIX)/share/zsh/site-functions
FISH_COMPLETION_DIR := $(PREFIX)/share/fish/vendor_completions.d

.PHONY: all build test check install uninstall completions man dist clean

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

# A self-contained release for a machine without a Rust toolchain:
# binary, man page, completions, docs and an install script.
dist: man completions
	$(CARGO) build --release --target $(TARGET)
	rm -rf $(DIST_DIR)
	install -d $(DIST_DIR)/completions
	install -m 0755 $(TARGET_BIN) $(DIST_DIR)/appctl
	install -m 0644 $(DIST)/appctl.1 $(DIST_DIR)/appctl.1
	install -m 0644 $(DIST)/appctl.bash $(DIST_DIR)/completions/appctl.bash
	install -m 0644 $(DIST)/_appctl $(DIST_DIR)/completions/_appctl
	install -m 0644 $(DIST)/appctl.fish $(DIST_DIR)/completions/appctl.fish
	install -m 0755 packaging/install.sh $(DIST_DIR)/install.sh
	install -m 0644 README.md TUTORIAL.md $(DIST_DIR)/
	tar -czf $(DIST_DIR).tar.gz -C $(DIST) $(DIST_NAME)
	cd $(DIST) && sha256sum $(DIST_NAME).tar.gz > $(DIST_NAME).tar.gz.sha256
	@echo
	@echo "release: $(DIST_DIR).tar.gz"
	@echo "  scp it to the server, extract, then: sudo ./$(DIST_NAME)/install.sh"

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/appctl
	rm -f $(DESTDIR)$(MANDIR)/appctl.1
	rm -f $(DESTDIR)$(BASH_COMPLETION_DIR)/appctl
	rm -f $(DESTDIR)$(ZSH_COMPLETION_DIR)/_appctl
	rm -f $(DESTDIR)$(FISH_COMPLETION_DIR)/appctl.fish

clean:
	$(CARGO) clean
