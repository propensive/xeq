# Runner stub manifests

Each `<version>.tsv` records the SHA-256 hashes of the reusable native runner stubs published as
the GitHub release `runners-<version>` (assets `runner-<label>[.exe]`). It is generated and
committed by `etc/ci/runners-release.sh` (`make runners-release RUNNERS_VERSION=<version>`).

Format — one tab-separated line per platform, sorted by label:

```
<label>	<sha256>
```

where `<label>` is one of `linux-x64`, `linux-arm64`, `macos-x64`, `macos-arm64`, `windows-x64`.

These hashes are the source of truth for application packaging: an online polyglot launcher
embeds them to verify the stub it downloads at runtime; a monoglot or offline build verifies the
stub bytes it downloads at build time against them. The stubs are version-independent and
reusable across applications — they are republished only when the Rust runner source changes.

## The manifest the packager reads

`res/packager/xeq/runners.{tsv,version,url}` is the copy compiled into `xeq-packager`, and is
what `Runners.standard` names. `runners-release.sh` rewrites all three, so publishing a release
is a data change rather than a code change.

## Provenance

Releases `runners-0.1` through `runners-0.5` were published from the Soundness repository,
before this project was extracted, and `res/packager/xeq/runners.url` still points there. Those
stubs are byte-identical to what this repository builds from the same sources.

The first release made from here supersedes that: build with `make runners-release
RUNNERS_VERSION=0.6`, or — to keep the published bytes and their hashes exactly as they are —
fetch `runners-0.5` from `propensive/soundness` and re-upload those files under a `propensive/xeq`
release, then point `runners.url` at it.
