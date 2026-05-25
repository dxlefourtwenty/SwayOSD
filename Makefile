BINDIR ?= $(HOME)/bin
CARGO ?= cargo
INSTALL ?= install

BINARIES := swayosd-server swayosd-client swayosd-libinput-backend
RELEASE_BINARIES := $(addprefix target/release/,$(BINARIES))
INSTALLED_BINARIES := $(addprefix $(BINDIR)/,$(BINARIES))

.PHONY: build install-bin install-bin-dry-run

build:
	$(CARGO) build --release

install-bin: build
	$(INSTALL) -d "$(BINDIR)"
	$(INSTALL) -m 0755 $(RELEASE_BINARIES) "$(BINDIR)"

install-bin-dry-run:
	@printf '%s\n' 'Would run: $(CARGO) build --release'
	@printf '%s\n' 'Would run: $(INSTALL) -d "$(BINDIR)"'
	@printf '%s\n' 'Would install: $(RELEASE_BINARIES)'
	@printf '%s\n' 'Into: $(BINDIR)'
