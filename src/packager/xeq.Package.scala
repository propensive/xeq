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

import java.lang as jl

import ambience.*
import anticipation.*
import contingency.*
import distillate.*
import galilei.*
import gossamer.*
import prepositional.*
import rudiments.*
import serpentine.*
import vacuous.*


import workingDirectories.javaBaseWorkingDirectory

// A minimal command-line front end to `Packager.pack`, so that a build which is not a Mill
// build — a shell script, another language's build tool — can package an application without
// linking against the library. `Xeq`'s own `main` is the counterpart for the script forms.
//
// Usage: xeq.Package <jar> <output> <label> <runners-dir>
//
// One platform, from local stubs: the `Native` delivery. The richer deliveries are reached
// through `Packager.pack` or the toolchain edge, both of which take a full `Packaging`.
object Package:
  // An argument may be absolute or relative to where the command was run; a relative one is
  // resolved against the working directory rather than rejected.
  private def path(text: Text)(using WorkingDirectory): Path on Linux = unsafely:
    safely(text.as[Path on Linux]).or:
      val work: Path on Linux = workingDirectory
      work + text.as[Relative on Linux]

  def main(args: scala.Array[String]): Unit = unsafely:
    args.iterator.toList.to(List) match
      case jar :: output :: label :: runners :: Nil =>
        val packaging =
          Packaging
            ( name         = output.tt.cut(t"/").reverse.prim.or(t"app"),
              targets      = List(label.tt),
              delivery     = Packaging.Delivery.Native,
              dependencies = Packaging.Dependencies.FatJar(path(jar.tt)),
              output       = path(output.tt),
              runnerSource = Packaging.RunnerSource.Local(path(runners.tt)) )

        Packager.pack(packaging)
        ()

      case _ =>
        jl.System.err.nn.println("usage: xeq.Package <jar> <output> <label> <runners-dir>")
        jl.System.exit(1)
