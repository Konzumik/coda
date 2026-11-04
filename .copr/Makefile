# COPR "make srpm" entry point: COPR runs `make -f .copr/Makefile srpm outdir=... spec=...`
# in a chroot WITH network, so crates are vendored here and the RPM itself builds offline.
top     := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))/..)
outdir  ?= $(top)
spec    ?= $(top)/coda.spec
version := $(shell sed -n 's/^version = "\(.*\)"/\1/p' $(top)/Cargo.toml | head -n1)
work    := $(top)/.copr/work

srpm:
	@if [ "$$(id -u)" = 0 ]; then dnf -y install git cargo rpm-build tar gzip; fi
	@grep -q '^Version: *$(version)$$' $(spec) || { echo "coda.spec Version != Cargo.toml version ($(version))"; exit 1; }
	rm -rf $(work) && mkdir -p $(work)
	git -c safe.directory='*' -C $(top) archive --prefix=coda-$(version)/ -o $(work)/coda-$(version).tar.gz HEAD
	cd $(top) && cargo vendor --quiet --locked --versioned-dirs $(work)/vendor > /dev/null
	tar -C $(work) -czf $(work)/coda-$(version)-vendor.tar.gz vendor
	rpmbuild -bs $(spec) --define '_sourcedir $(work)' --define '_srcrpmdir $(outdir)'
	rm -rf $(work)

.PHONY: srpm
