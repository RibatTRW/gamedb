# gamedb

A single native command-line utility that indexes a directory of decompiled source into
SQLite so you can search, read, and traverse it from a shell — no editor plugin, no GUI,
no language server, no host process.

```
gamedb index -r C:\src
gamedb search -r C:\src TakeDamage
gamedb read -r C:\src Main
gamedb graph -r C:\src Main --direction callees
```

`gamedb.exe` is a standalone executable. There is nothing to install alongside it and no
runtime to host it in. Drop it on `PATH` (or keep it anywhere and call it by path) and it
works on a machine that has never seen this repository.

## Why it exists

Decompiling a game gives you a directory of `.c`, `.cpp`, `.java`, or `.cs` files. Finding
one function in it means `grep`, and `grep` cannot tell you callers, callees, string
literals, or which subsystem a file belongs to. `gamedb` parses the tree once and answers
those questions from SQLite.

It is optimized for the way an agent (or a person at 3am) consumes it: terse text by
default, one `--json` flag for machine parsing, short help, and results instead of
narration.

## Build

```sh
cargo build --release --target x86_64-pc-windows-msvc
# -> target\x86_64-pc-windows-msvc\release\gamedb.exe
```

Requires the Rust toolchain and the MSVC linker. Nothing else — see *Dependencies*.

## Dependencies

**None.** `Cargo.toml` has no `[dependencies]` section, deliberately:

| Usually needed | Replaced by |
|---|---|
| `rusqlite` or `libsqlite3-sys` | `src/db.rs` — a direct FFI binding to `winsqlite3.dll`, the SQLite library that ships with Windows 10 1809 and later |
| `regex` | `src/rx.rs` — hand-written matchers mirroring the reference patterns |
| `clap` | `src/cli.rs` — hand-rolled argument parsing |
| `serde` / `serde_json` | a small string escaper in `src/cli.rs` |

SQLite itself is not vendored or compiled in; it is the one already on the machine.

## Commands

```
gamedb index    -r SRC [-v] [--dry-run] [--force]   build/refresh the index (incremental)
gamedb search   -r SRC QUERY [--limit N]             find functions by name substring
gamedb strings  -r SRC QUERY [--limit N]             find string literals
gamedb read     -r SRC NAME [--out F] [--force]      print one function body verbatim
gamedb stats    -r SRC                               counts per table
gamedb modules  -r SRC                               which subsystem each file belongs to
gamedb set-module -r SRC --module ID [--state S] [--verified|--unverified]
                    [--verified-by WHO] [--remaining TEXT]        track rewrite progress
gamedb graph    -r SRC NAME [--direction both|callers|callees]     call graph around a function
gamedb sql      -r SRC --sql Q [--param V]...        escape hatch: raw SQLite
gamedb selftest                                         run the built-in checks
```

Global flags: `-r/--root SRC` (default `.`), `--db PATH`, `--json`, `-q`, `-v`, `-h`, `-V`.

The index is written to `<root>\.gamedb\index.sqlite`. Delete that directory to discard it.

## Examples

```sh
# what is in here?
gamedb stats -r C:\src
# files=1549 functions=14022 strings=10269 symbols=47791 edges=427173 db=C:\src\.gamedb\index.sqlite

# who calls this, and what does it call?
gamedb graph -r C:\src Main --direction callers
# caller  RunGame    (Terraria/Program.cs)      called at Terraria/Program.cs:219 (x1)
# caller  Main       (Terraria/WindowsLaunch.cs) called at Terraria/WindowsLaunch.cs:54 (x1)
gamedb graph -r C:\src Main --direction callees
# callee  Load       (Terraria/WorldBuilding/AWorldGenerationOption.cs) called at Terraria/Main.cs:6530 (x1)

# machine-readable
gamedb search -r C:\src TakeDamage --limit 2 --json
[{"name":"TakeDamageFromJellyfish","params":"int npcIndex","sig":"public void TakeDamageFromJellyfish(int npcIndex) \t{","start_line":44619,"end_line":44625,"path":"Terraria/Player.cs"},
 {"name":"PopAllAttachedProjectilesAndTakeDamageForThem","params":"","sig":"public void PopAllAttachedProjectilesAndTakeDamageForThem() \t{","start_line":18960,"end_line":18971,"path":"Terraria/NPC.cs"}]

# see everything
gamedb read -r C:\src Main --out Main.cs
```

A `graph` row is `{direction, name, path, call_path, line, hits}`: `name`/`path` are the other
endpoint, and `line` is the call site inside `call_path` — which is the file the call is
written in, not necessarily the file the callee lives in.

Notes on behaviour worth knowing:

- `index` is **incremental**. Unchanged files are skipped by mtime + size; a re-run over an
  unchanged tree writes nothing and finishes in milliseconds. `--force` re-parses
  everything, `--dry-run` reports what would change without writing.
- Search is a case-sensitive substring match with SQL wildcards escaped, so `%` and `_`
  are literal. Results are ordered by shortest name first, then path, then line.
- `read` needs the exact function name (use `search` to find it).
- Exit codes: `0` success, `1` runtime error (nothing found, unindexed root, bad SQL),
  `2` usage error.

## Modules

Every indexed file is assigned to a module from its basename, namespace, or directory, so
`gamedb modules` answers "what is this codebase made of" without asking anyone. Rules are
ordered, first match wins.

`set-module` tracks a human or agent's progress per module and records who verified what:

```sh
gamedb set-module -r C:\src --module core.player --state PARTIALLY_IMPLEMENTED --remaining "12 functions left"
gamedb set-module -r C:\src --module core.player --state IMPLEMENTED --verified --verified-by alice
```

`--state IMPLEMENTED` requires `--verified`. Omitting `--verified` on a later call preserves
the previous sign-off rather than wiping it.

## Languages

Brace-delimited source with one-line signatures, which covers decompiler output:

- C, C++ (Ghidra and friends)
- Java (Ghidra, JADX, CFR)
- C# (ILSpy, dnSpy)

Declarations must be brace-delimited. `int x = f();` on one line with no `{}` body is a
declaration, not a function, and is deliberately not indexed.

## What is in the index

| Table | Contents |
|---|---|
| `files` | path, mtime, size, module |
| `functions` | name, params, signature, line range, owning file |
| `symbols` | namespace / type / method / property / field declarations |
| `strings` | string literals, 4+ characters |
| `edges` | caller → callee, with line and hit count |
| `module_status` | per-module rewrite state and sign-off |

`gamedb sql` gives you direct access if you need something the commands do not cover:

```sh
gamedb sql -r C:\src --sql "SELECT p.path, count(*) c FROM edges e JOIN functions f ON f.id=e.src_id JOIN files p ON p.id=f.file_id GROUP BY p.path ORDER BY c DESC LIMIT 10"
```

## Development

```sh
cargo build --release --target x86_64-pc-windows-msvc
cargo clippy --release --target x86_64-pc-windows-msvc --all-targets
cargo fmt --check
./gamedb.exe selftest        # 52 checks
```

| File | Role |
|---|---|
| `src/main.rs` | entry point, exit codes |
| `src/cli.rs` | argument parsing, text/JSON output |
| `src/store.rs` | directory walk, indexing, queries |
| `src/parse.rs` | comment/string masking, function and symbol parsing |
| `src/rx.rs` | hand-written pattern matchers |
| `src/modules.rs` | module assignment rules |
| `src/db.rs` | winsqlite3 FFI, schema, migrations |
| `src/selftest.rs` | built-in checks |

## Porting note

This began as a TypeScript extension for the Pi coding agent and was rewritten as a
standalone CLI. The on-disk format is preserved exactly: a database built by the TypeScript
version is read and incrementally updated by this one, and vice versa. Query output was
diffed byte-for-byte against the original across two real corpora — 1,549 C# files and
689 C/C++/Java files — with all six tables identical.

## License

MIT