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
import contingency.*
import ethereal.*
import exoskeleton.*
import gossamer.*
import parasite.*
import rudiments.*
import turbulence.*

import backstops.stackTraceBackstop
import executives.completionsExecutive
import interpreters.posixInterpreter
import threading.virtualThreading

// The end-to-end fixture: the smallest possible daemonized application. Packaging this with
// `Packager`, running the result, and seeing `Hello world` exercises everything at once — a
// real runner stub, a patched ETHRCFG block, an appended JAR, a daemon started over the
// launcher protocol, and a reply carried back to the invoking terminal.
//
// This is the one place in the repository where a daemon implementation appears, and it is
// here as a *test peer*: the launcher's other end has to be something for an end-to-end test
// to exist. Nothing published from this repository depends on it — see the `example` module in
// `build.mill`, which is outside every aggregate.
//
// It uses only the API of the Soundness *release* pinned in `etc/refs`, never of an unreleased
// daemon: Soundness pins this repository's release in its `etc/xeq.tsv`, so a fixture here
// that needed the daemon's next release would make the two unable to release at all. What a
// new protocol field actually does (say, the per-stream terminal flags of `xeq-0.7`) is
// asserted in Soundness's `ethereal` suite, which runs against a locally built stub; this
// fixture only has to say hello.
@main
def hello(): Unit = cli:
  execute:
    Out.println(t"Hello world")
    Exit.Ok
