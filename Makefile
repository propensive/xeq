# XEQ — see README.md
#
# The Scala modules are built by Mill; the runner stubs are built by Cargo. The two are
# deliberately separate: Mill never compiles Rust, and the stubs are published on their own
# cadence (`runners-release`) rather than with the jars.

MILL = ./mill

.PHONY: build test cargo-test runners-build runners-fetch runners-release publishLocal sync-deps e2e clean

# Everything published from this repository. `example` is deliberately excluded: it is the
# end-to-end fixture, and the only module that depends on a daemon implementation.
build:
	$(MILL) xeq.all

# Suites carry no `main`; a host runner discovers them from the `META-INF/services/probably.Suite`
# index and drives them over the test-event protocol. `$(TESTS)` are fume selection terms.
test:
	$(MILL) xeq.test.assembly
	fume run -c out/xeq/test/assembly.dest/out.jar $(TESTS)

# The runner's own unit tests — the BinTEL codec, the ETHRCFG verifier, the state machine.
cargo-test:
	cargo test

# Cross-compile the five reusable stubs into dist/runners. Needs cargo with the zigbuild
# subcommand (`cargo install cargo-zigbuild`) and zig on the path.
runners-build:
	./etc/ci/runners-build.sh

# Download the stubs of a published release into dist/runners, verified against the committed
# manifest — the path to take when the Rust toolchain isn't available.
runners-fetch:
	@if [ -z "$(RUNNERS_VERSION)" ]; then echo "Usage: make runners-fetch RUNNERS_VERSION=X [REPO=owner/repo]" >&2; exit 1; fi
	./etc/ci/runners-fetch.sh "$(RUNNERS_VERSION)" "$(REPO)"

# Build, publish and record a new set of stubs. Also rewrites res/packager/xeq/runners.{tsv,
# version,url}, which is how the packager learns about the release — commit those.
runners-release:
	@if [ -z "$(RUNNERS_VERSION)" ]; then echo "Usage: make runners-release RUNNERS_VERSION=X [REPO=owner/repo]" >&2; exit 1; fi
	./etc/ci/runners-release.sh "$(RUNNERS_VERSION)" "$(REPO)"

# Install the Soundness release pinned in etc/refs into ~/.ivy2/local, as CI does.
sync-deps:
	./etc/shared sync-deps.sh

# Install the jars into ~/.ivy2/local, where coursier finds them with no repository
# configuration — how a downstream build consumes XEQ before it has a published home.
publishLocal:
	$(MILL) xeq.core.publishLocal
	$(MILL) xeq.packager.publishLocal
	$(MILL) xeq.toolchain.publishLocal

# The end-to-end check: package the example application around a real runner stub and run it.
# Needs dist/runners (from `runners-build` or `runners-fetch`), and resolves a daemon
# implementation — the one place anything here does.
e2e:
	./etc/ci/e2e.sh

clean:
	$(MILL) clean
	rm -rf dist target
