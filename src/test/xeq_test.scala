                                                                                                  /*
┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃                                                                                                  ┃
┃                                 ╭───╮ ╭───╮╭────────╮╭─────────╮                                 ┃
┃                                 │   ╰─╯   ││   ╭─╮  ││   ╭─╮   │                                 ┃
┃                                 ╰───╮ ╭───╯│   ╰─╯  ││   │ │   │                                 ┃
┃                                 ╭───╯ ╰───╮│   ╭────╯│   │ ╰─╮ │                                 ┃
┃                                 │   ╭─╮   ││   ╰────╮│   ╰─╮ │ │                                 ┃
┃                                 ╰───╯ ╰───╯╰────────╯╰─────╯ ╰─╯                                 ┃
┃                                                                                                  ┃
┃    XEQ, version 0.1.0.                                                                      ┃
┃    © Copyright 2021-25 Jon Pretty, Propensive OÜ.                                                ┃
┃                                                                                                  ┃
┃    The primary distribution site is:                                                             ┃
┃                                                                                                  ┃
┃        https://github.com/propensive/xeq/                                                        ┃
┃                                                                                                  ┃
┃    Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file     ┃
┃    except in compliance with the License. You may obtain a copy of the License at                ┃
┃                                                                                                  ┃
┃        https://www.apache.org/licenses/LICENSE-2.0                                               ┃
┃                                                                                                  ┃
┃    Unless required by applicable law or agreed to in writing,  software distributed under the    ┃
┃    License is distributed on an "AS IS" BASIS,  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,    ┃
┃    either express or implied. See the License for the specific language governing permissions    ┃
┃    and limitations under the License.                                                            ┃
┃                                                                                                  ┃
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
                                                                                                  */
package xeq

import ambience.*
import anticipation.*
import aperture.*
import contingency.*
import denominative.*
import digression.*
import distillate.*
import eucalyptus.*
import fulminate.*
import galilei.*
import gastronomy.*
import gossamer.*
import guillotine.*
import hieroglyph.*
import inimitable.*
import monotonous.*
import prepositional.*
import probably.*
import rudiments.*
import serpentine.*
import spectacular.*
import symbolism.*
import turbulence.*
import vacuous.*

import systems.javaBaseSystem
import environments.javaBaseEnvironment
import temporaryDirectories.systemTemporaryDirectory
import workingDirectories.javaBaseWorkingDirectory
import logging.silentLogging
import stdios.fileDescriptorStdio
import termcaps.environmentTermcap
import strategies.throwUnsafely
import charEncoders.utf8Encoder
import charDecoders.utf8Decoder
import alphabets.hexLowerCase
import providers.javaBaseProvider
import textSanitizers.skipSanitizer
import errorDiagnostics.stackTracesDiagnostics
import filesystemOptions.deleteRecursively
import filesystemBackends.javaBaseFilesystem

// The suite drives the published `xeq` builder script (dist/xeq — build it with
// `make xeq-script`) rather than any in-JVM assembler: the script is the single implementation
// of the ETHRCFG v3 format (spec/ethrcfg.md). `Packager` is a thin front end over it, tested
// here through the same shell-out an Anthology build uses. Suites that need real stubs
// (dist/runners) or a Windows host skip cleanly when those are absent.
object Tests extends Suite(m"XEQ tests"):
  def run(): Unit =
    val tempDirs = scala.collection.mutable.ListBuffer.empty[Path on Linux]

    def tempDir(): Path on Linux =
      val dir: Path on Linux = temporaryDirectory[Path on Linux]/Uuid().show
      dir.create[Directory]()
      tempDirs += dir
      dir

    try body(tempDir) finally tempDirs.each { dir => safely(dir.delete()) }

  private def body(tempDir: () -> Path on Linux): Unit =
    val here: Path on Linux = workingDirectory
    val script: Path on Linux =
      safely(Environment.xeq[Text].as[Path on Linux]).or(unsafely(t"${here.encode}/dist/xeq".as[Path on Linux]))

    val hostLabel: Text = sh"uname -s".exec[Text]().trim match
      case t"Darwin" => sh"uname -m".exec[Text]().trim match
        case t"arm64" | t"aarch64" => t"macos-arm64"
        case _                     => t"macos-x64"
      case _ => sh"uname -m".exec[Text]().trim match
        case t"aarch64" | t"arm64" => t"linux-arm64"
        case _                     => t"linux-x64"

    val labels: List[Text] = List(t"linux-x64", t"linux-arm64", t"macos-x64", t"macos-arm64")
    val scriptOk: Boolean = script.existent()

    // Hash and size via the shell — the suite's business is the script's output bytes, not
    // galilei's capture-checked streaming. (`wc -c FILE`, not `< FILE`: guillotine execs
    // directly, so a `<` would be a literal argument.)
    def sha(path: Path on Linux): Text =
      sh"shasum -a 256 ${path.encode}".exec[Text]().cut(t" ").prim.or(t"")
    def size(path: Path on Linux): Text =
      sh"wc -c ${path.encode}".exec[Text]().trim.cut(t" ").prim.or(t"")

    def writeText(path: Path on Linux, text: Text): Unit =
      path.open[File](Write, OpenFlag.Create, OpenFlag.Truncate)(file.write(Chain(text.in[Data])))

    // Path with a computed Text segment (avoids the Admissible ambiguity of `dir / textValue`).
    def sub(dir: Path on Linux, name: Text): Path on Linux =
      unsafely(t"${dir.encode}/$name".as[Path on Linux])
    def stubOf(dir: Path on Linux, label: Text): Path on Linux =
      sub(dir, if label.starts(t"windows") then t"runner-${label}.exe" else t"runner-$label")

    // A directory of fake "stubs": shell scripts that echo and exit before the appended record
    // and JAR are ever reached, so the whole chain runs with no daemon and no real runner.
    def fakeRunners(): Path on Linux =
      val dir = tempDir()
      labels.each: label =>
        val stub = stubOf(dir, label)
        val content: Text = t"#!/bin/sh\necho ran-"+label+t"\nexit 0\n"
        writeText(stub, content)
        sh"chmod +x ${stub.encode}".exec[Exit]()
      dir

    def fakeJar(dir: Path on Linux): Path on Linux =
      val jar = dir/t"app.jar"; writeText(jar, t"JARBYTES\n"); jar

    if !scriptOk then
      Out.println(t"dist/xeq not found; run `make xeq-script` — skipping builder tests")
    else
      suite(m"record"):
        test(m"is exactly 3764 bytes and starts with the v3 magic"):
          val dir = tempDir(); val rec = dir/t"rec"
          sh"$script record --out $rec --build-id 42".exec[Exit]()
          (size(rec), sh"head -c 7 ${rec.encode}".exec[Text]().trim)
        .assert(_ == (t"3764", t"ETHRCFG"))

      suite(m"build (native)"):
        test(m"output equals stub \u2016 record \u2016 jar, byte for byte"):
          val dir = tempDir(); val runners = fakeRunners(); val jar = fakeJar(dir)
          val out = dir/t"tool"; val rec = dir/t"rec"
          sh"$script build --jar $jar --out $out --target $hostLabel --runners $runners".exec[Exit]()
          sh"$script record --out $rec --target $hostLabel".exec[Exit]()
          val stub = stubOf(runners, hostLabel)
          val cat = dir/t"cat"
          sh"sh -c ${t"cat '${stub.encode}' '${rec.encode}' '${jar.encode}' > '${cat.encode}'"}".exec[Exit]()
          sha(out) == sha(cat)
        .assert(_ == true)

      suite(m"embed-all"):
        test(m"unpacks to a runnable binary that selects the host payload"):
          val dir = tempDir(); val runners = fakeRunners(); val jar = fakeJar(dir); val out = dir/t"tool"
          sh"$script embed-all --jar $jar --out $out --runners $runners".exec[Exit]()
          sh"$out".exec[Text]().trim
        .assert(_ == t"ran-$hostLabel")

      suite(m"download (online launcher)"):
        test(m"fetches the stub over file://, appends record and jar, runs"):
          val dir = tempDir(); val runners = fakeRunners(); val jar = fakeJar(dir); val out = dir/t"tool"
          val manifest = dir/t"m.tsv"
          val body = labels.map { l => t"$l\t${sha(stubOf(runners, l))}" }.join(t"\n")
          writeText(manifest, t"$body\n")
          sh"$script download --jar $jar --out $out --runners-url file://${runners.encode} --runners-manifest $manifest".exec[Exit]()
          sh"$out".exec[Text]().trim
        .assert(_ == t"ran-"+hostLabel)

      suite(m"dispatch"):
        test(m"downloads a complete executable and re-execs it"):
          val dir = tempDir()
          val exe = dir/t"real"
          val exeBody: Text = t"#!/bin/sh\necho dispatched\nexit 0\n"
          writeText(exe, exeBody)
          sh"chmod +x ${exe.encode}".exec[Exit]()
          val manifest = dir/t"d.tsv"
          writeText(manifest, t"$hostLabel\tfile://${exe.encode}\t${sha(exe)}\n")
          val out = dir/t"tool"
          sh"$script dispatch --out $out --manifest $manifest".exec[Exit]()
          sh"$out".exec[Text]().trim
        .assert(_ == t"dispatched")

      // Packager is a thin front end over the same script.
      def config
         (delivery:     Packaging.Delivery,
          dependencies: Packaging.Dependencies,
          runnerSource: Packaging.RunnerSource = Packaging.RunnerSource.Remote(t"https://x.test/", Map()),
          targets:      List[Text]             = List(t"linux-x64"))
      :   Packaging =
        val dir = tempDir()
        Packaging(name = t"hello", targets = targets, delivery = delivery, dependencies = dependencies,
                  output = dir/t"hello", runnerSource = runnerSource)

      val fatJar: Packaging.Dependencies = Packaging.Dependencies.FatJar(tempDir()/t"app.jar")

      suite(m"Packager validation"):
        test(m"Burdock remote dependencies are rejected"):
          capture[Packager.Error](Packager.pack(config(Packaging.Delivery.EmbedAll,
            Packaging.Dependencies.BurdockRemote(tempDir()/t"app.jar"))))
        .assert(_ => true)

        test(m"remote runner with no hash for the target is rejected"):
          capture[Packager.Error](Packager.pack(config(Packaging.Delivery.Native, fatJar,
            Packaging.RunnerSource.Remote(t"https://example.invalid/", Map()))))
        .assert(_ => true)

        test(m"native delivery with multiple targets is rejected"):
          capture[Packager.Error]:
            Packager.pack(config(Packaging.Delivery.Native, fatJar, targets = List(t"linux-x64", t"macos-arm64")))
        .assert(_ => true)

      suite(m"Packager assembly via the script"):
        test(m"Native delivery builds a byte-correct host binary"):
          val dir = tempDir(); val runners = fakeRunners()
          val jar = dir/t"app.jar"; writeText(jar, t"JARBYTES\n")
          val out = dir/t"hello"
          Packager.pack(Packaging(name = t"hello", targets = List(hostLabel),
            delivery = Packaging.Delivery.Native, dependencies = Packaging.Dependencies.FatJar(jar),
            output = out, runnerSource = Packaging.RunnerSource.Local(runners)))
          sh"$out".exec[Text]().trim
        .assert(_ == t"ran-$hostLabel")

    // Linux via docker, using the same fake shell stubs (which run on Linux too).
    val dockerOk = safely(sh"docker info".exec[Exit]()) == Exit.Ok
    if scriptOk && dockerOk then
      def linuxCheck(platform: Text, label: Text): Boolean =
        val dir = tempDir(); val runners = fakeRunners(); val jar = fakeJar(dir); val out = dir/t"tool"
        sh"$script embed-all --jar $jar --out $out --runners $runners".exec[Exit]()
        val mount = t"${dir.encode}:/work"
        val outName = out.encode.cut(t"/").reverse.prim.or(t"tool")
        sh"docker run --rm --platform $platform -v $mount -w /work ubuntu:24.04 ./$outName".exec[Text]().trim == t"ran-$label"
      suite(m"docker linux/amd64"):
        test(m"embed-all unpacks and selects linux-x64")(linuxCheck(t"linux/amd64", t"linux-x64")).assert(_ == true)
      suite(m"docker linux/arm64"):
        test(m"embed-all unpacks and selects linux-arm64")(linuxCheck(t"linux/arm64", t"linux-arm64")).assert(_ == true)
