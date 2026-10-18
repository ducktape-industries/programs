# programs — the wasm32 gates. Every wasm artifact is a build output under
# $(BUILD_TARGET); none is committed.
CARGO ?= cargo
WASM_OPT ?= wasm-opt

# What a program links: abi, program and store build for wasm32 with nothing else.
PROGRAM_LINKABLE := abi program store

# Every program, system and app: root members whose program ABI (the SDK
# glue, the `alloc`/`call` exports, the `ducktape.*` imports) sits behind their
# `program` feature. Their views link the same crates with the feature off
# and name each program by its type (`Query<chat::Chat>`).
# One cargo invocation per program: forge links chat and identity links
# programs, and `-p a -p b --features program` in one call would unify
# `program` into the other's link (two `alloc`/`call`).
PROGRAMS := programs valset identity blobs awaits halo scheduler access chat forge activity files egress attribution harness actions actions-pods

# Views are wasm32 cdylibs. Chat, Forge and Files ride their own programs;
# Settings rides the programs role. These are the views that ship: `wasm-views`
# builds them and a genesis packs each into its program. `example-view`, the
# view a new author reads first, is a workspace member (fmt, clippy and its
# tests are the workspace's) and is kept out of this list on purpose: it
# rides identity, which ships members-view, and nothing packs it.
# `view-wasm-check` builds it for wasm32 so its `export_view!` cannot rot.
VIEWS := chat-view members-view node-view explorer-view settings-view forge-view harness-view files-view

# The views whose crate also builds its program's notifier (`notifier`
# feature, `export_notifier!`): `wasm-notifiers` builds each as
# <view>.notify.wasm, and a founding packs it as the program's
# `ducktape.notify` section.
NOTIFIERS := chat-view forge-view

# What a wasm32 view may link. A crate a view links must never reach the
# signing/identity graph (blst does not build for wasm32, and a view has no
# business holding keys). The system crates are here because the system views
# read their contracts: `programs`'s signing deps are dev-only, and `-e
# normal` below is what says so. Every program crate is linked with `program`
# off, which is what a plain `-p` build below checks.
VIEW_LINKABLE := ducklink program view-wire view design store programs valset identity settings-view chat forge activity files egress attribution harness actions actions-pods
VIEW_FORBIDDEN := blst commonware-cryptography wasm-bindgen js-sys web-sys

# Cargo uses this directory for both workspaces.
BUILD_TARGET := $(abspath $(or $(CARGO_TARGET_DIR),target))
# The Cargo.toml profile every wasm build below comes out of: `release`, the
# bytes a release ships (`make wasm-publish`), or `fast`,
# the edit loop's and garage's, the network that runs dev (`make dev` builds
# with it; garage's code is `make wasm-modules WASM_PROFILE=fast`), whose
# views wasm-opt skips. Each profile has its own output dir,
# so fast bytes are never where release bytes are read from. Only the command
# line sets it, never the environment: a shell that exports it must not turn
# the next founding's code into fast bytes.
WASM_PROFILE = release
WASM_OUT = $(BUILD_TARGET)/wasm32-unknown-unknown/$(WASM_PROFILE)
# The artifacts wasm-modules leaves there (the target dir may hold others).
ARTIFACTS := $(foreach a,$(PROGRAMS) $(VIEWS),$(subst -,_,$(a)).wasm) $(foreach p,$(PROGRAMS),$(subst -,_,$(p)).describe.wasm) $(foreach p,$(PROGRAMS),$(p).schema) $(foreach v,$(NOTIFIERS),$(subst -,_,$(v)).notify.wasm)

# A wasm artifact is the same bytes from any checkout on any machine of one
# host triple: panic locations would otherwise carry this checkout's, cargo's
# and the toolchain's absolute paths. Across host triples (a Mac and a Linux
# box) the bytes differ and no flag here changes that: cargo mixes the host
# into the `-C metadata` of every crate with a build script or a proc macro
# beneath it. The reference host is x86_64-unknown-linux-gnu (owner,
# 2026-10-09): bytes someone will compare by hash (a founding, a release) are
# built there, and a Mac's build is a development build.
CARGO_HOME_DIR := $(or $(CARGO_HOME),$(HOME)/.cargo)
SYSROOT := $(shell rustc --print sysroot)
# Where the rust-src component is installed (rust-analyzer asks for it), rustc
# names std's sources by their place under the sysroot, not by /rustc/<commit>
# as it does without it. The last matching remap wins: both read /rustc/<commit>.
RUSTC_COMMIT := $(shell rustc -vV | sed -n 's/^commit-hash: //p')
WASM_RUSTFLAGS := --remap-path-prefix=$(CURDIR)=/build --remap-path-prefix=$(CARGO_HOME_DIR)=/cargo --remap-path-prefix=$(SYSROOT)=/rustc --remap-path-prefix=$(SYSROOT)/lib/rustlib/src/rust=/rustc/$(RUSTC_COMMIT)
WASM_CARGO := RUSTFLAGS="$(WASM_RUSTFLAGS)" $(CARGO) build --target wasm32-unknown-unknown
WASM_BUILD = $(WASM_CARGO) --profile $(WASM_PROFILE)
# The view gate: wasm-opt, then the ABI check; `--fast` skips wasm-opt for
# any profile but release (tools/view-gate.sh).
VIEW_GATE = tools/view-gate.sh $(if $(filter release,$(WASM_PROFILE)),,--fast)

# A workspace member with a cdylib crate type gets no metadata hash in its
# output name, so `chat` built with `program` (the program, a root) and `chat`
# built without it (a dependency of chat-view, or of forge's program) are one
# unit to cargo's fingerprint and rebuild each other on every invocation.
# Each program therefore builds in its own target dir, where its crate only
# ever appears with `program` on, and its artifact is copied into $(WASM_OUT)
# beside the views (which build in $(BUILD_TARGET) itself: every view links
# the program crates with `program` off, one unit).
PROGRAM_TARGET = $(BUILD_TARGET)/programs/$1
program_artifact = $(call PROGRAM_TARGET,$1)/wasm32-unknown-unknown/$(WASM_PROFILE)/$(subst -,_,$1).wasm
program_build = $(WASM_BUILD) --target-dir $(call PROGRAM_TARGET,$1) -p $1 --features program
export CARGO BUILD_TARGET WASM_PROFILE WASM_OUT WASM_BUILD WASM_OPT

.PHONY: dev wasm-why new-program new-view scaffold-check
.PHONY: program-wasm-check wasm-programs wasm-views wasm-notifiers view-wasm-check test

# `make dev P=forge` / `V=forge-view` narrow the loop to one artifact.
DEV_PROGRAMS = $(if $(or $P,$V),$P,$(PROGRAMS))
DEV_VIEWS = $(if $(or $P,$V),$V,$(VIEWS))

## the edit loop: runs the native tests of the crates whose test binaries
## cargo rebuilt, then rebuilds the programs and views cargo finds stale and
## gates the rebuilt views (ABI: wasm-tools, python3); one line per artifact.
## It builds with the fast profile; `make dev WASM_PROFILE=release` builds
## and gates the bytes that ship (Binaryen wasm-opt 132 too).
dev: WASM_PROFILE = fast
dev:
	@tools/dev.sh "$(DEV_PROGRAMS)" "$(DEV_VIEWS)"

## where a view's bytes go: `twiggy top` over a release build that keeps its
## names (`--profile why`: release + `strip = "debuginfo"`, the name section
## kept, its own output dir, so the release artifact is untouched). Install:
## `cargo install twiggy`.
wasm-why:
	@test -n "$V" || { echo "usage: make wasm-why V=<view>"; exit 1; }
	@command -v twiggy >/dev/null || { echo "twiggy is not on PATH: cargo install twiggy"; exit 1; }
	@$(WASM_CARGO) --target-dir $(BUILD_TARGET) --profile why -p $V
	twiggy top -n 25 $(BUILD_TARGET)/wasm32-unknown-unknown/why/$(subst -,_,$V).wasm

## scaffolds crates/app/NAME in valset's shape and registers it (PROGRAMS,
## workspace members and dependencies); `make dev P=NAME` must pass on it.
new-program:
	@test -n "$(NAME)" || { echo "usage: make new-program NAME=<program>"; exit 1; }
	@tools/scaffold.sh program $(NAME)

## scaffolds crates/app/NAME (NAME ends in -view) over the program it names,
## in members-view's shape, and registers it (VIEWS, workspace members).
new-view:
	@test -n "$(NAME)" || { echo "usage: make new-view NAME=<program>-view"; exit 1; }
	@tools/scaffold.sh view $(NAME)

## new-program and new-view in a throwaway worktree, then `make dev` and the
## describe build over what they wrote (tools/scaffold-check.sh).
scaffold-check:
	@tools/scaffold-check.sh

## builds abi, program and store for wasm32-unknown-unknown.
program-wasm-check:
	@for crate in $(PROGRAM_LINKABLE); do \
	  $(CARGO) build --target wasm32-unknown-unknown -p $$crate || exit 1; \
	done; \
	echo "abi, program and store build for wasm32"

## builds every program (with `program` on) into $(WASM_OUT)/<name>.wasm. The
## founding suite reads the boot set from there.
wasm-programs:
	@mkdir -p $(WASM_OUT)
	@$(foreach p,$(PROGRAMS),$(call program_build,$(p)) && cp $(call program_artifact,$(p)) $(WASM_OUT)/ || exit 1;)

## the whole suite; the founding suite runs `make wasm-programs` itself.
test:
	$(CARGO) test --workspace

.PHONY: wasm-describes

# Each program's describe module builds in a target dir of its own, for the
# reason `PROGRAM_TARGET` gives: the crate with `describe` on is another unit.
DESCRIBE_TARGET = $(BUILD_TARGET)/describe/$1
describe_build = $(WASM_BUILD) --target-dir $(call DESCRIBE_TARGET,$1) -p $1 --features describe
describe_artifact = $(call DESCRIBE_TARGET,$1)/wasm32-unknown-unknown/$(WASM_PROFILE)/$(subst -,_,$1).wasm

## builds every program's describe module (its crate with `describe` on: the
## `alloc`/`describe` exports over its `describe` fn, no imports) into
## $(WASM_OUT)/<name>.describe.wasm, ABI-checked. qa's pack embeds each as
## the `ducktape.describe` section of its program.
wasm-describes:
	@mkdir -p $(WASM_OUT)
	@$(foreach p,$(PROGRAMS),$(call describe_build,$(p)) && cp $(call describe_artifact,$(p)) $(WASM_OUT)/$(subst -,_,$(p)).describe.wasm && python3 tools/check-describe-abi.py $(WASM_OUT)/$(subst -,_,$(p)).describe.wasm || exit 1;)

.PHONY: wasm-schemas

## writes every program's schema (its Op, Query and Reply as one borsh schema
## container, from its crate linked natively) into $(WASM_OUT)/<name>.schema.
## A founding packs each as the `ducktape.schema` section of its program.
wasm-schemas:
	@mkdir -p $(WASM_OUT)
	@$(CARGO) run --locked -p founding --bin schemas -- $(WASM_OUT) $(PROGRAMS)

.PHONY: wasm-modules wasm-reproducible wasm-founding wasm-publish wasm-publish-check dl-stage dl-publish-check

## builds every program, its describe module and schema, every view and every
## notifier under $(WASM_OUT)/, unpacked.
wasm-modules: wasm-programs wasm-describes wasm-schemas wasm-views wasm-notifiers
	@cd $(WASM_OUT) && ls -l $(ARTIFACTS)

## packs what wasm-modules built as a founding (crates/sdk/founding):
## founding.toml and every file it names, under $(WASM_OUT)/founding/ and as
## $(WASM_OUT)/founding.tar.gz. Its SHA256SUMS lists every file the bundle
## wrote into its fresh directory, which is every file the founding names and
## founding.toml (the founding crate's bundle test holds the two equal). The
## archive is the same bytes each time the same modules are packed at the
## same commit (names sorted, owner 0, each file dated at the commit), so a
## second publish of it finds it unchanged.
wasm-founding: wasm-modules
	@rm -rf $(WASM_OUT)/founding $(WASM_OUT)/founding.tar.gz
	@$(CARGO) run --locked --release -p founding -- $(WASM_OUT) $(WASM_OUT)/founding
	@cd $(WASM_OUT)/founding && sha256sum * > SHA256SUMS
	@tar --sort=name --format=gnu --owner=0 --group=0 --numeric-owner --mtime=@$$(git show -s --format=%ct HEAD) \
	  -cf $(WASM_OUT)/founding.tar -C $(WASM_OUT) founding && gzip -n $(WASM_OUT)/founding.tar
	@echo "founding.tar.gz  $$(wc -c < $(WASM_OUT)/founding.tar.gz)"

.PHONY: wasm-program

## builds one program's code as wasm-founding packs it, into
## $(WASM_OUT)/program/<P>.wasm: `make wasm-program P=chat`. It builds only what
## that program carries (the program, its describe module and schema, and the
## view and notifier `founding`'s `modules` names for it) with the recipes
## wasm-modules runs, so its bytes are founding/<P>.wasm's. A deploy of one
## program builds this, not the whole founding. A standalone view ships as
## wasm-views builds it: `make wasm-views VIEWS=explorer-view`.
wasm-program:
	@test -n "$(P)" || { echo "usage: make wasm-program P=<program>"; exit 1; }
	@modules=$$($(CARGO) run --locked -q -p founding --bin modules -- $(P)) && \
	  $(MAKE) --no-print-directory wasm-programs wasm-describes wasm-schemas wasm-views wasm-notifiers PROGRAMS=$(P) $$modules
	@$(CARGO) run --locked --release -p founding -- $(WASM_OUT) $(WASM_OUT)/program $(P)
	@cd $(WASM_OUT)/program && sha256sum $(P).wasm

# `make dl-stage` without a channel stops here, before its build
ifneq ($(filter dl-stage,$(MAKECMDGOALS)),)
ifeq ($(filter dev main,$(CHANNEL)),)
$(error usage: make dl-stage CHANNEL=dev|main [WASM_PROFILE=fast] [DL_OUT=dir])
endif
endif

# wasm-publish and dl-stage take what wasm-founding builds, their
# prerequisite. A caller that has just built runs them with `-o wasm-founding`
# (make builds nothing then): the release workflow builds main once for both.

## builds and publishes this commit's release modules and founding as the
## GitHub release wasm-<commit> (tools/publish-wasm.sh). Its refusals (not
## the reference host, not the release profile, uncommitted changes, a commit
## on no branch of origin, a commit published already) come before the build,
## and again before the release.
wasm-publish: wasm-publish-check wasm-founding
	@tools/publish-wasm.sh $(ARTIFACTS)

wasm-publish-check:
	@tools/publish-wasm.sh check

# Where dl-stage writes the stage.
DL_OUT = $(BUILD_TARGET)/dl/$(CHANNEL)

## builds this commit's modules and founding and stages them for
## https://dl.ducktape.industries/programs/$(CHANNEL)/ (tools/publish-dl.py
## stage): `make dl-stage CHANNEL=dev WASM_PROFILE=fast`, or `CHANNEL=main` for
## the release profile. $(DL_OUT) then holds each file and the manifest, and
## the upload order is printed; this needs no credentials. The release
## workflow's upload job uploads the stage (`tools/publish-dl.py upload`).
dl-stage: wasm-founding
	@tools/publish-dl.py stage $(CHANNEL) $(WASM_PROFILE) $(DL_OUT) \
	  $(addprefix $(WASM_OUT)/,$(ARTIFACTS)) $(WASM_OUT)/founding.tar.gz

## runs tools/publish-dl.py against a scratch repository and a stand-in for
## `aws` (tools/publish-dl-check.py): the version, the manifest, the upload
## order, and the refusal of a stage or a version's file with other bytes.
dl-publish-check:
	@tools/publish-dl-check.py

## builds every program and view twice, the second time from a fresh target
## directory, and requires the same sha256 for every artifact and no path of
## this checkout, this home or this toolchain's own sources inside any of them.
wasm-reproducible:
	$(MAKE) wasm-modules
	@cd $(WASM_OUT) && sha256sum $(ARTIFACTS) > first.sha256 && cat first.sha256
	$(MAKE) wasm-modules CARGO_TARGET_DIR=$(BUILD_TARGET)/repro
	@cd $(BUILD_TARGET)/repro/wasm32-unknown-unknown/$(WASM_PROFILE) && sha256sum $(ARTIFACTS) > second.sha256 && cat second.sha256
	@diff $(WASM_OUT)/first.sha256 $(BUILD_TARGET)/repro/wasm32-unknown-unknown/$(WASM_PROFILE)/second.sha256 && echo "every program and view rebuilds to the same bytes"
	@for a in $(ARTIFACTS); do f=$(WASM_OUT)/$$a; \
	  if strings $$f | grep -qE "$(CURDIR)|$(HOME)|/rustc/lib/"; then echo "$$f embeds a path of this machine"; strings $$f | grep -E "$(CURDIR)|$(HOME)|/rustc/lib/" | head -3; exit 1; fi; \
	done; echo "no program or view embeds a path of this checkout, home or toolchain"

## builds every view for wasm32 under $(WASM_OUT)/, optimized (the bytes
## before wasm-opt stay beside it as <name>.wasm.unoptimized; a fast build
## is not optimized), ABI-checked and its size printed: `name  bytes`.
wasm-views:
	@$(foreach v,$(VIEWS),$(WASM_BUILD) --target-dir $(BUILD_TARGET) -p $(v) && $(VIEW_GATE) $(v) $(WASM_OUT)/$(subst -,_,$(v)).wasm || exit 1;)

# Each notifier builds in a target dir of its own, for the reason
# `PROGRAM_TARGET` gives: the view crate with `notifier` on is another unit.
# A notifier stays resident for every program on every network and runs on
# an event, not per frame, so it is built for size whatever WASM_PROFILE
# says: the `notify` profile, wasm-opt always, and a cdylib alone, since
# cargo runs no LTO for a crate that is an rlib too. Its stack is 64 KiB,
# not rustc's 1 MiB: chat's catch-up over 100 rooms and 5,600 notices
# went 6 KiB deep.
NOTIFY_TARGET = $(BUILD_TARGET)/notify/$1
NOTIFY_STACK = 65536
notifier_build = RUSTFLAGS="$(WASM_RUSTFLAGS)" $(CARGO) rustc --target wasm32-unknown-unknown --profile notify \
  --target-dir $(call NOTIFY_TARGET,$1) -p $1 --features notifier --lib --crate-type cdylib -- -C link-arg=-zstack-size=$(NOTIFY_STACK)
notifier_artifact = $(call NOTIFY_TARGET,$1)/wasm32-unknown-unknown/notify/$(subst -,_,$1).wasm

## builds every notifier into $(WASM_OUT)/<view>.notify.wasm, optimized,
## ABI-checked (alloc/init/tick) and its size printed.
wasm-notifiers:
	@mkdir -p $(WASM_OUT)
	@$(foreach v,$(NOTIFIERS),$(call notifier_build,$(v)) && cp $(call notifier_artifact,$(v)) $(WASM_OUT)/$(subst -,_,$(v)).notify.wasm && tools/view-gate.sh $(v).notify $(WASM_OUT)/$(subst -,_,$(v)).notify.wasm notifier || exit 1;)

## builds every VIEW_LINKABLE crate for wasm32-unknown-unknown, plus the
## exported view probe of view, the example view and every notifier, then fails
## if the normal wasm32 dependency tree of any of them names a VIEW_FORBIDDEN crate.
view-wasm-check:
	@for crate in $(VIEW_LINKABLE); do \
	  $(CARGO) build --target wasm32-unknown-unknown -p $$crate || exit 1; \
	done; \
	$(CARGO) build --target wasm32-unknown-unknown -p view --example exported_view || exit 1; \
	$(CARGO) build --target wasm32-unknown-unknown -p example-view || exit 1; \
	for view in $(NOTIFIERS); do \
	  $(CARGO) build --target wasm32-unknown-unknown --target-dir $(call NOTIFY_TARGET,check) -p $$view --features notifier || exit 1; \
	done; \
	reached=""; \
	for crate in $(VIEW_LINKABLE); do \
	  tree=$$($(CARGO) tree --target wasm32-unknown-unknown -e normal -p $$crate --prefix none) || exit 1; \
	  for dep in $(VIEW_FORBIDDEN); do \
	    if echo "$$tree" | grep -q "^$$dep v"; then reached="$$reached $$crate->$$dep"; fi; \
	  done; \
	done; \
	if [ -z "$$reached" ]; then \
	  echo "every view-linkable crate builds for wasm32 and stays off the signing/identity and JavaScript graphs"; \
	else \
	  echo "view-linkable crates reach a forbidden dependency:$$reached"; \
	  exit 1; \
	fi
