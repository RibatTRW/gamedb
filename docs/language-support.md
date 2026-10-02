# Language support

gamedb reads decompiled / source text and stores a queryable index of
declarations, symbols, string literals, and call edges. Nothing in that pipeline
knows what game the corpus came from: the module taxonomy is either derived from
the directory tree or read from `.gamedb/modules.txt`, and every matcher keys on
*syntax shape*, not on engine or project names. Adding a language is therefore a
parser question, never a per-game question.

This document is the empirical survey behind the current matcher, the changes
that closed the gaps in the four target languages (C, C++, C#, Java), the honest
limits of a heuristic reader, and a pointer to the mock-ups for the languages
that need a real grammar ([`language-mockups.md`](language-mockups.md)).

## 1. What the indexer extracts

For every file whose extension is in `src/store.rs` `SRC_EXT`:

```
c h cpp hpp cc cs java kt kts scala swift go rs dart js jsx mjs cjs ts tsx php txt asm
```

`txt` (header dumps) and `asm` are present on purpose; they yield string
literals and little else.

The parser (`src/parse.rs`, matchers in `src/rx.rs`) emits one `Parsed`:

| table | what lands in it |
|-------|------------------|
| `functions` | name, params, reconstructed signature, body line range |
| `symbols` | `namespace` / `type` / `method` / `property` / `field` |
| `strings` | double-quoted literals ≥ 4 chars that are not hex blobs |
| `edges` | `calls` from a function body to a resolved symbol |

The reader is deliberately heuristic and dependency-free: it finds
`NAME(params) { body }` headers, tolerates decompiler calling conventions, joins
signatures wrapped over several lines, and walks braces to find the body. That
is enough for the brace-delimited C family; it is *not* a grammar, and the
limits are stated in §4.

## 2. How the survey was run

A fixture per language, each written as the output of the decompiler a person
would actually use, lives under `research/fixtures/`:

```
research/fixtures/c/ghidra_out.c          Ghidra "Decompile" C
research/fixtures/cpp/ida_out.cpp         IDA/Hex-Rays C++
research/fixtures/csharp/ilspy_out.cs     ILSpy C#
research/fixtures/java/cfr_out.java       CFR / JD-GUI Java
research/fixtures/others/go_out.go        Go (garble / go-decompiler text)
research/fixtures/others/rust_out.rs      Rust source / decompiled
research/fixtures/others/ts_out.ts        TypeScript / JS output
research/fixtures/others/kotlin_out.kt    Kotlin / Fernflower-Kotlin
research/fixtures/others/scala_out.scala  Scala
research/fixtures/others/swift_out.swift  Swift
```

Index them and dump the result:

```sh
gamedb index -r research/fixtures --db research/test.sqlite --rules derive -q
gamedb sql -r research/fixtures --db research/test.sqlite \
  --sql "SELECT f.path, fn.name FROM functions fn JOIN files f ON f.id=fn.file_id ORDER BY f.path, fn.start_line"
gamedb sql -r research/fixtures --db research/test.sqlite \
  --sql "SELECT f.path, s.kind, s.name FROM symbols s JOIN files f ON f.id=s.file_id ORDER BY f.path, s.kind"
```

## 3. Verdict per language

Measured against the corpus above, after the changes in this pass:

| language | extensions | verdict | recovered from the fixture |
|----------|-----------|---------|---------------------------|
| **C** | `c h` | ✅ supported | `FUN_00401234`, `FUN_00401299`, `helper_add`; types `SavedEntity`, `WorldState`; fields `id name x y tick entities` |
| **C++** | `cpp hpp cc` | ✅ supported (approximated) | `Update`, `GetHealth`, `Create`, `MakePlayer`, `main`; fields `health_ name_`; namespaces `Game`, `Core` |
| **C#** | `cs` | ✅ supported | ctor `Player`, `Update`, expr-bodied `TakeDamage` / `Create`, `Wrap`; properties `Health Name`; fields `health name Died` |
| **Java** | `java` | ✅ supported | ctor `Player`, `update` (with `throws`), `getHealth`, `wrap`, `reset`, single-line `isHealthy`; enum ctors + `speed`; record compact ctor `Point` + `sum`; types `Player State Point Outer Inner`; fields `health name`, enum constants `IDLE RUNNING DEAD`, record components `x y` |
| Go | `go` | ⚠️ partial → mock-up | `NewPlayer`, `main`; **receiver methods (`func (p *Player) Update`) and return-typed funcs are missed** |
| Rust | `rs` | ⚠️ partial → mock-up | `update`, `get_health`, `helper`, `main`; **`-> T` returns, `impl`/trait methods, `new` missed** |
| TypeScript / JS | `ts tsx js jsx mjs cjs` | ⚠️ partial → mock-up | `constructor`, `update`, `getHealth`, `createPlayer`; **arrow functions missed; typed returns heuristic; arrow-local `const`s leak as fields** |
| Kotlin | `kt kts` | ⚠️ partial → mock-up | `update`, `getHealth`, `createPlayer`, type `Player`; **`fun` keyword layers and return types need a grammar** |
| Scala | `scala` | ⚠️ partial → mock-up | type `Player` only; **`def f(...): T = {` not matched** |
| Swift | `swift` | ⚠️ partial → mock-up | `init`, `update`, `getHealth`, `createPlayer`; **`-> T` and properties need a grammar** |
| PHP | `php` | ⚠️ partial → mock-up | (not in corpus; `function foo() {` shape works, `public function foo(): int {` does not) |
| Dart | `dart` | ⚠️ partial → mock-up | (not in corpus; `int foo() {` works, `Future<T> foo() async {` does not) |
| asm | `asm` | strings only | by design |
| txt | `txt` | header dumps | by design |

The four target languages are now solid for symbol/function/call extraction.
The others are *occasionally* right, which is worse than nothing when the stated
purpose is 1:1 code generation, so they are documented as grammar-backed
mock-ups rather than claimed as support.

## 4. What changed for C / C++ / C# / Java

The single biggest cause of misses was a header matcher that only accepted
`NAME(params) {` with nothing between `)` and `{`. Decompiled code almost never
looks like that. The fix generalises the matcher instead of special-casing a
language:

* **Qualified names.** `void Player::Update(int dt) {` — the matcher now keeps
  only the final segment (`Update`) as the function name and treats
  `A::B::name` as one name.
* **Trailing qualifiers / clauses.** `int GetHealth() const`, `void f()
  override`, `void f() noexcept`, `public void update() throws IOException` —
  the span between `)` and `{` may now contain words, templates, `::`, `->`,
  and balanced parenthesis groups (also covers constructor initialiser lists
  such as `Player::Player() : health_(0) {`).
* **Namespace-qualified return types.** `std::shared_ptr<Player> MakePlayer() {`
  — a leading `A::B` and a balanced `<...>` are skipped before the name.
* **Type-declaration guard.** `class Player(private var health: Int) {` is a
  type with a primary constructor, not a function called `Player`; the matcher
  checks the line against the type keywords before accepting it.
* **Expression bodies.** `public int TakeDamage(int n) => health -= n;` and
  `static Player Create() => new Player();` now produce a function whose body
  range is the single `=>` line.
* **Bare struct fields.** C has no access modifiers, so `match_member` (which
  requires one) never saw `int id;`. Inside a matched type body, a
  no-modifier `Type name;` line is now a `field`.
* **Single-line bodies.** `public int isHealthy() { return this.health > 0; }`
  used to be dropped because the header matcher wanted the `{` at end of line.
  The joiner now truncates such a line at its opening brace, so the declaration
  reads as the plain `NAME(params) {` shape and the body walk still covers the
  rest of the line. This is general, not Java-specific.
* **Java-only member sources.** An `enum`'s enumerators (`IDLE, RUNNING(3),
  DEAD;`) and a `record`'s header components (`record Point(int x, int y)`) are
  implicit fields, and are now indexed as such; a record's compact constructor
  (`public Point { ... }`) is recorded as a constructor, not a field named after
  the type; a leading annotation (`@interface Marker`) no longer hides the type.
* **Statement keywords are not functions.** `func`, `def`, `fn`, `fun`, `when`,
  `synchronized` joined the stop list so `when (state) {` and
  `func (r *R) M()` can never be read as declarations named `when` / `func`.

Regression pins: `tests/audit.rs` covers each of the above
(`cpp_out_of_line_definitions_are_functions`,
`a_java_throws_clause_does_not_hide_the_method`,
`a_c_struct_body_yields_its_fields`, `an_expression_body_is_the_function_body`,
`statement_keywords_and_primary_constructors_are_not_functions`,
`a_single_line_body_is_a_function`, `java_enum_constants_are_fields`,
`java_record_components_are_fields`,
`a_record_compact_constructor_is_not_a_field`), plus the built-in `selftest`.

### Deliberate divergence from the reference implementation

The original behaviour treated an expression body as *not a function*. That was
changed on purpose: an `=> expr;` member is a real declaration with a
reconstructible body, and dropping it loses code you cannot regenerate.

The same reasoning applies to a single-line brace body (`void b() {}`): the
reference build deliberately skipped it because its tail pattern required the
`{` at end of line, but it is a function with a body you can regenerate. Both
`selftest` pins now assert the wider behaviour.

## 5. Honest limits of the heuristic reader

These are inherent to "brace scanning plus declaration shapes" and are why the
mock-ups exist rather than more heuristics:

* **No scope resolution.** `A::B()` and `obj.method()` record the final
  segment; overloads collapse to one name.
* **No anonymous constructs.** Lambdas, closures, local functions, and arrow
  functions are invisible (their bodies' `const`s can leak as fields — the
  `p` / `anonymous` rows in the TS fixture).
* **Preprocessor blindness in C/C++.** Macros that generate declarations,
  `#if` branches, and `template` metaprogramming are read as flat text.
* **Comment/string masking is line-local for block comments** across
  pathological spellings.
* **No type or flow information**, so call edges are name-resolved within the
  indexed corpus only.

## 6. Engine neutrality

The survey added no game knowledge. Language handling is keyed on extension and
on syntax shape; the taxonomy is still derived from paths or read from
`.gamedb/modules.txt`. A different decompiled engine with the same file
languages indexes the same way with zero code change. The grammar-backed path
proposed in [`language-mockups.md`](language-mockups.md) keeps the same
property: grammars are keyed on language, and modules remain data.
