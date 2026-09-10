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

import anticipation.*
import gossamer.*
import hellenism.*
import hieroglyph.*
import rudiments.*
import turbulence.*
import vacuous.*

import charDecoders.utf8Decoder
import classloaders.threadContextClassloader
import textSanitizers.skipSanitizer

// The reusable native runner stubs are published independently of any application, as a
// GitHub release whose assets are one bare stub per platform, and are verified against a
// manifest of their SHA-256 hashes. Which release that is, and where it lives, is not
// compiled in: `make runners-release` writes the three resources read below, so publishing
// a new set of stubs changes data, not code.
//
// The manifest is the same tab-separated format as `etc/runners/<version>.tsv` — one
// `<label>\t<sha256>` line per platform — so the archived manifests and the embedded one
// are interchangeable, and `etc/ci/runners-fetch.sh` reads either.
object Runners:
  // The published release these hashes came from.
  lazy val version: Text = cp"/xeq/runners.version".read[Text].trim

  // Where that release's assets are downloaded from, without a trailing slash. Held as data
  // rather than derived from `version`, so that a set of stubs can be republished — or
  // mirrored — without changing this code.
  lazy val baseUrl: Text = cp"/xeq/runners.url".read[Text].trim

  // Lowercase SHA-256 hex of each published stub, by platform label.
  lazy val hashes: Map[Text, Text] =
    val lines = cp"/xeq/runners.tsv".read[Text].cut(t"\n").map(_.trim).filter: line =>
      line != t"" && !line.starts(t"#")

    lines.map: line =>
      val fields = line.cut(t"\t")
      (fields.prim.or(t""), fields.reverse.prim.or(t""))

    . to[Map]

  // The published stubs, as a runner source a `Packaging` can be built with.
  def standard: Packaging.RunnerSource = Packaging.RunnerSource.Remote(baseUrl, hashes)

  // Every platform the published release names.
  def labels: List[Text] = hashes.keys.to[List]

  // The published filename for a platform's bare runner stub (Windows stubs carry `.exe`).
  def runnerName(label: Text): Text =
    if label.starts(t"windows") then t"runner-$label.exe" else t"runner-$label"

  // The URL a platform's bare runner stub is published at.
  def url(label: Text): Text = t"$baseUrl/${runnerName(label)}"
