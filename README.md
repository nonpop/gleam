# gleam-go

This is an experimental fork of the [Gleam](https://github.com/gleam-lang/gleam) compiler which adds
a Go code generation backend. I implemented enough that it can compile the stdlib and a few other
packages required by my [AoC 2024 solutions](https://github.com/nonpop/aoc2024).

## Status

This is an experiment. **Do not use in production** (or anywhere, really). It has both known and
probably lots of unknown bugs, and the code is a mess. The project is currently inactive but if
there's enough interest, I might pick it up again. Note that the Gleam maintainers mentioned
somewhere that they won't be merging a Go backend, so this will remain a fork probably forever.

## Performance

Here are timings of some of the slower AoC solutions:

entry          | erlang | javascript(node) | go
---------------|--------|------------------|------
day 6, part 2  | 17s    | 1m19s            | 1m33s
day 7, part 2  | 37s    | 1m4s             | 1m13s
day 9, part 2  | 54s    | 3m49s            | 1m39s
day 12, part 2 | 4s     | 16s              | 3s
day 14, part 2 | 10s    | 52s              | 16s
day 16, part 1 | 20s    | 4m22s            | 1m22s
day 16, part 2 | 20s    | 4m21s            | 1m23s
day 18, part 2 | 14s    | 2m2s             | 6m11s
day 20, part 2 | 6s     | 18s              | 24s
day 22, part 2 | 11s    | 1m13s            | 30s

For these, the Go version is about 4-5x slower than Erlang and about 1-2x faster than Javascript on
average. I'm sure the numbers can be improved a bit, but it's also possible that matching Erlang's
performance would be difficult since Go is not optimized for functional programming with immutable
data.

These might not be good benchmarks, though. I would imagine most real-world code would be quite
different in character. For example, while I haven't tested it, I would expect most web apps to have
smaller performance differences.

## Beauty

- The generating code is just hacks on top of hacks to see if this would even be doable
- The generated code is ugly (compared to javascript) for a few reasons, for example:

  - Go uses capitalization to denote public/private, which causes clashes with Gleam's conventions,
    so I've used various suffixes as an easy fix
  - Go requires all imports and variables to be used, so I've just added lots of `_ = ...` instead
    of tracking which are actually used and removing the unused ones
  - Go requires adding type arguments to many generic function calls. Many of them could be dropped,
    which would remove a lot of noise
  - I'm importing the prelude qualified, which makes many common identifiers extra long

  I believe most of these are fixable without much difficulty, though.

## Running

To run the generated code you need Go 1.24 (for generic type aliases). You'll also need customized
versions of packages which use FFI. I've made forks of all the packages required by my AoC
solutions, which you can use by cloning them and using e.g.
`gleam_stdlib = { path = "../gleam-stdlib" }` in `gleam.toml`.

For the AoCs you also need to tweak `util.gleam` so that it reads the input files from the correct
location; the Go version's working directory is the build directory, not the repository root.
