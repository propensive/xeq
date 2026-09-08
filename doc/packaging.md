# Packaging

### About

Shipping a JVM application to someone who just wants to run it starts with distribution: a JAR
becomes a self-contained executable — a native launcher per platform, or a single polyglot
installer script that runs as shell script, batch file and PowerShell alike. That is what XEQ
does.

### On distribution

"Install the JVM, download the JAR, run this command" loses users at every step. What a
command-line tool should ship as is one file that runs — finding or fetching a suitable JVM
itself — and what it should weigh is its own code, not megabytes of dependencies that already sit
on a public repository. And when the tool is a library, its bundled dependencies must not fight
the host application's: the oldest deployment problem on the JVM.

A distributable described as a value in the build is direct style applied to packaging.

Everything comes from the `xeq` package:

```scala
import xeq.*
```

### Executables and installers

A packaging configuration names the application, its targets and its delivery, and `pack` produces
the artifact. *Native* delivery assembles one launcher binary for one platform; *embed-all*
produces a polyglot installer script carrying every platform's launcher and the application,
choosing the right one where it runs; *download* keeps the script small, fetching the platform's
launcher on demand and verifying it by hash:

```scala
val jarPath = t"/tmp/mytool.jar".as[Path on Linux]
val outputPath = t"/tmp/mytool".as[Path on Linux]
val runnerSource = Runners.standard

val packaging = Packaging
  ( name         = t"mytool",
    targets      = List(t"linux-x64", t"macos-arm64"),
    delivery     = Packaging.Delivery.EmbedAll,
    dependencies = Packaging.Dependencies.FatJar(jarPath),
    output       = outputPath,
    runnerSource = runnerSource )

Packager.pack(packaging)
```

The launchers locate or fetch a JVM within the configured version policy, and support signed
self-upgrade.

### Bundling as a toolchain format

The same packaging is reachable as a toolchain format, so an application can be
compiled and bundled in one path rather than packaged as a separate step afterwards. An
`Executable` runs from `Jar` rather than from a universe, and the delivery mode is part of the
node's identity, since each is a different distributable:

```scala
Toolchain(jarEdges(), executableEdges()).produce
  ( Deliverable.Emission(out, classpath),
    Universe.Classfile,
    Executable(Packaging.Delivery.EmbedAll),
    destination,
    List(executableOptions.name(t"mytool"), executableOptions.runners.standard),
    List(EntryPoint(fqcn"com.example.Main")) )
```

`executableOptions.runners.standard` names the published runner release, verified against its
committed manifest, while `runners.local` reads prebuilt stubs from a directory instead. Targets
default to every platform the runner source names, and `executableOptions.target` adds one
explicitly. `executableOptions.java` sets the minimum and preferred JVM versions, `bundle.jre`
and `bundle.jdk` ship one alongside, and `signing` and `buildId` configure the self-upgrade
signing and upgrade ordering recorded in each stub.

### Where the stubs come from

`Runners.standard` is the published release recorded in this repository's resources, verified
against its committed manifest; `Packaging.RunnerSource.Local` reads prebuilt stubs from a
directory instead — the output of `make runners-build` or `make runners-fetch` — which is what
the test suite and `make e2e` use.

The stubs are not built by the Scala build and are never stored in a jar. They are published on
their own cadence by `make runners-release`, which also rewrites the resources the packager
reads, so adopting a new runner is a data change.

### The other end

An XEQ executable is only half of a running application: the launcher starts a *daemon*, and the
two speak the protocol in [`spec/`](../spec/README.md). An application being packaged here must
therefore be one that implements that protocol — `ethereal`, in Soundness, is the reference
implementation, and `src/example` is the smallest application that uses it.
