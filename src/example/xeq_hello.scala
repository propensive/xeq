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

import java.util.concurrent as juc

import ambience.*
import anticipation.*
import contingency.*
import distillate.*
import ethereal.*
import exoskeleton.*
import gossamer.*
import parasite.*
import profanity.*
import rudiments.*
import turbulence.*
import vacuous.*

import backstops.stackTraceBackstop
import executives.completionsExecutive
import interpreters.posixInterpreter
import threading.virtualThreading

// The end-to-end fixture: the smallest possible daemonized application. Packaging this with
// `Packager`, running the result, and seeing `Hello world` exercises everything at once — a
// real runner stub, a patched ETHRCFG block, an appended JAR, a daemon started over the
// launcher protocol, and a reply carried back to the invoking terminal.
//
// The subcommands beyond the greeting exist for `etc/ci/e2e.sh`, which needs an application
// at the other end of the socket that reads stdin to its end, exits with a chosen status,
// echoes its arguments, and reports the signals it receives, so that the launcher's handling
// of each can be observed from a shell.
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
  arguments match
    case Nil =>
      execute:
        Out.println(t"Hello world")
        Exit.Ok

    // Each argument on its own line, exactly as received.
    case Argument("args") :: rest =>
      execute(Out.print(rest.map(_()).join(t"\n")) yet Exit.Ok)

    // Standard input, copied to standard output once it has ended.
    case Argument("cat") :: Nil =>
      execute:
        val bytes = summon[Stdio].in.readAllBytes().nn
        Out.print(String(bytes, "UTF-8").tt)
        Exit.Ok

    case Argument("exit") :: Argument(As[Int](status)) :: Nil =>
      execute(Exit.Fail(status))

    case Argument("stderr") :: text :: Nil =>
      execute(Err.println(text()) yet Exit.Ok)

    // Sleeps, but accepts a TERM signal and ends early on it: the case in which the
    // launcher, rather than the daemon, decides how the invocation's death is reported.
    case Argument("sleep") :: Argument(As[Int](seconds)) :: Nil =>
      execute:
        val done: juc.CountDownLatch = juc.CountDownLatch(1)

        trap:
          case Interrupt.Term =>
            done.countDown()
            SignalResponse.Accept

        done.await(seconds.toLong, juc.TimeUnit.SECONDS)
        Exit.Ok

    // Prints the name of the first signal forwarded to it, or `(timeout)` after two seconds.
    case Argument("signal") :: Nil =>
      execute:
        val received: juc.LinkedBlockingQueue[Text] = juc.LinkedBlockingQueue()

        trap:
          case signal: UnixSignal =>
            received.offer(signal.shortName)
            SignalResponse.Accept

          case signal: WindowsSignal =>
            received.offer(signal.shortName)
            SignalResponse.Accept

        val raw: Text | Null = received.poll(2L, juc.TimeUnit.SECONDS)
        Out.print(if raw == null then t"(timeout)" else raw)
        Exit.Ok

    case Argument("env") :: Argument(variable) :: Nil =>
      execute:
        Out.print(safely(Environment[Text](variable)).or(t""))
        Exit.Ok

    case _ =>
      execute(Exit.Fail(1))
