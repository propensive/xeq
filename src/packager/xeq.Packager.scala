                                                                                                  /*
┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃                                                                                                  ┃
┃                                 ╭───╮ ╭───╮╭────────╮╭─────────╮                                 ┃
┃                                 │   ╰─╯   ││   ╭─╮  ││   ╭─╮   │                                 ┃
┃                                 ╰──╮   ╭──╯│   ╰─╯  ││   │ │   │                                 ┃
┃                                 ╭──╯   ╰──╮│   ╭────╯│   │ │   │                                 ┃
┃                                 │   ╭─╮   ││   ╰────╮│   ╰─╯   │                                 ┃
┃                                 ╰───╯ ╰───╯╰────────╯╰─────╮   │                                 ┃
┃                                                            │   ╰╮                                ┃
┃                                                            ╰────╯                                ┃
┃                                                                                                  ┃
┃    Cross-build Executable Quickstart, version ${VERSION}.                                        ┃
┃    © Copyright 2021-26 Jon Pretty, Propensive OÜ.                                                ┃
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
import distillate.*
import denominative.size
import denominative.dysasymptotics.linearSize
import eucalyptus.*
import fulminate.*
import galilei.*, galilei.Platform.pathReadable
import gossamer.*
import guillotine.*
import hieroglyph.*
import prepositional.*
import rudiments.*
import serpentine.*
import spectacular.*
import turbulence.*
import vacuous.*

import environments.javaBaseEnvironment
import errorDiagnostics.emptyDiagnostics
import logging.silentLogging

import filesystemOptions.createNonexistentParents
import filesystemOptions.dereferenceSymlinks
import filesystemOptions.overwritePreexisting

import filesystemBackends.javaBaseFilesystem

// Turns a `Packaging` into a distributable by invoking the published `xeq` builder script —
// the single implementation of the ETHRCFG v3 format and the polyglot launchers
// (`src/script`, `spec/ethrcfg.md`). The script is located from the `XEQ` environment
// variable, else `dist/xeq` under the working directory. `Native` runs `xeq build`, `EmbedAll`
// runs `xeq embed-all`, `Download` runs `xeq download`; each delivery's flags come straight
// from the `Packaging` fields.
//
// Nothing here reimplements the byte format: the split's whole point is that one script,
// released with the runners, does the joining, and this is a thin front end over it so an
// Anthology build reaches the same code a shell user does.
object Packager:
  def pack(config: Packaging)(using WorkingDirectory): Path on Linux raises Packager.Error =
    val appJar: Path on Linux = config.dependencies.absolve match
      case Packaging.Dependencies.FatJar(jar) => jar
      case Packaging.Dependencies.BurdockRemote(_) =>
        abort(Packager.Error(m"Burdock remote dependencies are not yet supported (Stage C)"))

    config.delivery match
      case Packaging.Delivery.Native if config.targets.size != 1 =>
        val length: Int = config.targets.size
        abort(Packager.Error(m"Native delivery requires exactly one target, but $length were given"))
      case _ => ()

    val subcommand: Text = config.delivery match
      case Packaging.Delivery.Native   => t"build"
      case Packaging.Delivery.EmbedAll => t"embed-all"
      case Packaging.Delivery.Download => t"download"

    // The remote runner source's per-label hashes reach the script as a temporary manifest in
    // the `label<TAB>sha256` format `etc/runners/<v>.tsv` uses; a local directory is passed
    // straight through. A missing hash is caught here, before the script runs, so the error
    // matches the pre-shell-out behaviour the tests pin.
    val runnerArgs: List[Text] = config.runnerSource.absolve match
      case Packaging.RunnerSource.Local(directory) =>
        List(t"--runners", directory.encode)

      case Packaging.RunnerSource.Remote(baseUrl, hashes) =>
        config.targets.each: label =>
          hashes(label).lest(Packager.Error(m"No runner hash given for $label"))

        val manifest: Path on Linux = temporaryManifest(hashes, config.output)
        List(t"--runners-url", baseUrl, t"--runners-manifest", manifest.encode)

    val args = scala.collection.mutable.ListBuffer[Text]()
    args += resolveScript.encode
    args += subcommand
    args += t"--jar"; args += appJar.encode
    args += t"--out"; args += config.output.encode
    config.targets.each { label => args += t"--target"; args += label }
    args += t"--java-min";  args += config.java.minimum.show
    args += t"--java-pref"; args += config.java.preferred.show
    args += t"--build-id";  args += config.buildId.show
    if config.java.bundle == Packaging.Bundle.Jdk then args += t"--jdk"
    config.signing.let(_.publicKey).let { path => args += t"--public-key"; args += path.encode }
    if config.signing.let(_.allowDowngrade).or(false) then args += t"--allow-downgrade"
    runnerArgs.each(args += _)

    val exit: Exit =
      mitigate:
        case Exec.Error(_, _, _) => Packager.Error(m"Could not run the xeq builder script")
      . protect:
          Command(args.toList*).exec[Exit]()

    exit match
      case Exit.Ok         => config.output
      case Exit.Fail(code) =>
        abort(Packager.Error(m"The xeq builder exited with status $code (see its output above)"))

  // Locate the builder script: `$XEQ`, else `dist/xeq` under the working directory. Absent, a
  // clear instruction rather than a download — every in-repo caller (tests, `make e2e`) has run
  // `make xeq-script`, and a downstream build sets `XEQ` to the release asset it fetched.
  private def resolveScript(using WorkingDirectory): Path on Linux raises Packager.Error =
    safely(Environment.xeq[Text].as[Path on Linux]).or:
      val work: Path on Linux = workingDirectory
      val candidate: Path on Linux = unsafely(t"${work.encode}/dist/xeq".as[Path on Linux])
      if candidate.existent() then candidate
      else abort(Packager.Error(m"No xeq builder found: set XEQ or run `make xeq-script` to write dist/xeq"))

  // A temporary manifest for the script, beside the output so it shares its writable directory.
  private def temporaryManifest(hashes: Map[Text, Text], output: Path on Linux)
  :   Path on Linux raises Packager.Error =
    mitigate:
      case Io.Error(_, _, _, _) => Packager.Error(m"Could not write a temporary runner manifest")
      case Truncation.Error(_)  => Packager.Error(m"Could not write a temporary runner manifest")
    . protect:
        val body: Text = hashes.to[List].map((label, hash) => t"$label\t$hash").join(t"\n")
        val dir: Path on Linux = unsafely(output.parent.assume)
        val path: Path on Linux = unsafely(t"${dir.encode}/.xeq-manifest.tsv".as[Path on Linux])
        path.open[File](Write, OpenFlag.Create, OpenFlag.Truncate):
          file.write(Chain(body.in[Data](using charEncoders.utf8Encoder)))
        path

  // PackageError → Packager.Error
  case class Error(detail: Message)(using Diagnostics) extends fulminate.Error(detail)
