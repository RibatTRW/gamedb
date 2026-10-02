# Enhancements

Outstanding work for the languages gamedb already reads. Each section says what
the indexer produces **today**, what it **still misses**, and **what has to be
done** to close the gap.

Ordering is by language commonness (TIOBE / Stack Overflow style ranking, 2024),
**most common first**. It is a rough ordering and your corpus may disagree;
re-sort to taste. Two tiers:

* **supported** — the heuristic matcher in `src/rx.rs` covers the declaration
  shapes a decompiler emits; C, C++, Java, and C# are here.
* **heuristic** — only plain declarations land; a real grammar is needed and a
  mock-up already exists in [`docs/language-mockups.md`](docs/language-mockups.md).
  These entries are short because the fix is the same for all of them.

Evidence: `research/fixtures/` (per-language corpus) and `research/gaps/`
(deliberate gap probes). Reproduce any claim with:

```sh
gamedb index -r research/fixtures --db /tmp/f.sqlite --rules derive -q
gamedb sql   -r research/fixtures --db /tmp/f.sqlite \
  --sql "SELECT f.path, s.kind, s.name FROM symbols s JOIN files f ON f.id=s.file_id ORDER BY f.path, s.kind"
```

## Limits that apply to every language

Fix these once in the shared matcher, not per language:

1. **No scope resolution.** `a.Bar()` and `A::Bar()` resolve to `Bar`; overloads
   collapse to one name. Call edges are name-only within the indexed corpus.
2. **No nested or anonymous functions.** A definition inside another body
   (C# local function, Java anonymous class, C++ lambda) is not reached — the
   function pass resumes after the outer body.
3. **No preprocessor.** `#define`, `#if`, and macro-generated declarations are
   read as flat text.
4. **No type or flow information.**
5. **Single-line type bodies.** A member on the same physical line as its type's
   `{` is not emitted, because the symbol pass consumes the whole type line.
   `struct Named { int x; int y; };` yields the type but not `x`/`y`.
6. **Bodyless declarations are not members.** `void Do();` inside an interface or
   an in-class prototype is neither a function (no body) nor a `method` symbol.

---

## 1. C — supported

**Currently indexes:** functions with a body (`FUN_00401234`, `helper_add`),
including single-line, Allman, and wrapped parameter lists; calling conventions
(`__thiscall`, `__cdecl`, `__fastcall`) and storage classes; named
`struct`/`union`/`enum` types; no-modifier `Type name;` fields (array extents
stripped); enum constants; string literals; call edges.

**Still misses** (see `research/gaps/gaps.c`, `gaps2.c`):

* Anonymous typedef aggregates — `typedef struct { int a; } Anon;` yields **no
  type and no fields** (there is no tag for `type_decl` to return).
* `typedef` aliases — `typedef int MyInt;` is not recorded.
* Function-pointer members — `void (*handler)(int);` is skipped.
* Bitfields — `unsigned a : 3;` is rejected (lone `:`).
* Multi-declarator members — `int a, b, c;` produces nothing.
* Nested aggregates and the member holding them — `union { int i; float f; }
  value;` yields `i`,`f` as if direct, and loses `value`.

**To close the gap** (all still syntax-shaped, so engine-neutral):

1. In `parse_source`, when a `struct`/`union`/`enum` keyword has no tag, open a
   member scope anyway via `brace_body_range` and attach it to the trailing
   declarator or the `typedef ... Name;` that follows, as a `type`.
2. Add a `typedef <type> <name>;` matcher emitting kind `type`.
3. Extend `match_bare_member` to (a) allow a single `:` for bitfields,
   (b) accept a parenthesised pointer name `(*name)`, (c) split top-level commas
   and emit one field per declarator.
4. Nested aggregates: recurse the member scope for `struct|union|enum { ... }`
   that appears as a field's type.

Alternatively adopt the `tree-sitter-c` path from
[`docs/language-mockups.md`](docs/language-mockups.md); the shapes above are
common enough that the heuristic fix is probably cheaper.

## 2. C++ — supported

**Currently indexes:** everything C does, plus namespaces (`namespace Game::Core`),
out-of-line definitions (`Player::Update`), trailing `const`/`override`/
`noexcept`, namespace-qualified and generic return types
(`std::shared_ptr<Player> Make()`), constructor init-lists, `class` fields, and
template class names.

**Still misses** (see `research/gaps/gaps.cpp`):

* Destructors — `Vec::~Vec() { }` is not matched (the `~` breaks the name scan).
* Operator overloads — `bool Vec::operator==(const Vec&) const { }`.
* In-class prototypes — `Vec(); ~Vec(); bool operator==(...) const;` are neither
  functions nor `method` symbols (no body).
* Dependent template members — `T Box<T>::get() const { }`.
* Brace init-lists — `Vec::Vec() : x_{0} { }` is only partially handled.
* Concepts/`requires`, `[[attributes]]`, anonymous namespaces, function-try-
  blocks, lambdas.
* A false positive: `operator` leaks into `symbols` as a `field`.

**To close the gap:**

1. Accept `operator` (and the following symbol characters) as a name in
   `qualified_name`/`wide_tail`, and fold a leading `~` into the name.
2. Record bodyless declarations inside a type body as kind `method` (shared
   limit 6).
3. Deeper metaprogramming needs `tree-sitter-cpp`
   ([`docs/language-mockups.md`](docs/language-mockups.md)); do not keep growing
   the heuristic for it.

## 3. Java — supported

**Currently indexes:** methods and constructors, including `throws`, generic
bounds, array/varargs parameters, and **single-line bodies**; types `class`,
`interface`, `enum`, `record`, `@interface`, nested and static-nested; fields
with or without modifiers; **enum constants** as fields; **record components**
as fields; a record's **compact constructor** as a method; `package` namespaces;
string literals; call edges.

**Still misses** (see `research/gaps/gaps.java`):

* Anonymous class methods — `new Runnable() { public void run() { } }` loses
  `run` (it is inside `go`'s body).
* Local classes.
* Abstract interface methods — `void onDone(int code);` is not recorded.
* Types/methods led by an annotation **with arguments** —
  `@SuppressWarnings("x") class Foo {` defeats the type scan (it stops at `(`).
* Enum constants with constant-specific bodies — `RUNNING { int speed() {...} }`.
* Lambda bodies are not scanned for calls.

**To close the gap:**

1. Record bodyless `Type name(params);` inside an interface/abstract body as
   kind `method` (shared limit 6).
2. In `type_decl`, skip a leading annotation plus its balanced `(...)`.
3. The expensive one — walk anonymous/local class bodies with a scope prefix
   (`Anon.run`) — is the only item that argues for the grammar path.

## 4. C# — supported

**Currently indexes:** methods including expression bodies (`=> expr;`), types
`class`, `struct`, `interface`, `enum`, `record`, `delegate`; fields, properties,
and events; `namespace` namespaces; generic parameters and `where` clauses;
single-line bodies.

**Still misses** (see `research/gaps/gaps.cs`):

* Positional records — `public record Person(string Name, int Age);` finds the
  type but not `Name`/`Age` (the components are only extracted when a `{` body
  exists).
* Primary constructors on classes — `class Widget(int size) {` finds the type
  but not `size`.
* Operators — `public static Widget operator +(Widget, Widget)`.
* Destructors — `~Widget()`.
* Indexers — `public int this[int i] { get; }`.
* Interface members without bodies — `void Do();` (shared limit 6).
* Local functions and lambdas.

**To close the gap:**

1. Call `record_components` for a bodyless `record ...;` too, and treat a
   class/struct primary-constructor parameter list as fields.
2. Record bodyless `[modifiers] Type Name(params);` inside a type body as
   `method` (shared limit 6).
3. `operator`/`~`/`this[...]` handling follows the same fix as C++.

## 5. JavaScript — heuristic

**Currently indexes:** `function name(params) {` declarations and `class` method
definitions (`constructor`, `update`, …); `class` names; string literals; call
edges.

**Still misses:** arrow functions assigned to `const`/`let`, object-literal
methods, imports/exports (no module graph), and JSX.

**To close the gap:** adopt `tree-sitter-javascript` per
[`docs/language-mockups.md`](docs/language-mockups.md) §3. No further heuristic
work — the missing constructs are structural.

## 6. TypeScript — heuristic

**Currently indexes:** the same shapes as JavaScript (its output is usually
emitted by the same code paths).

**Still misses:** typed arrow functions, typed method shorthand, interfaces,
type aliases, and imports. Arrow-body locals leak as fields (`p`, `anonymous`
in `research/fixtures/others/ts_out.ts`).

**To close the gap:** `tree-sitter-typescript` (plus the `tsx` variant), same
mock-up. The arrow-function fix also removes the false fields.

## 7. PHP — heuristic

**Currently indexes:** `function name() { }` functions.

**Still misses:** `public function f(): int { }`, methods, classes, interfaces,
`namespace A\B` (backslash), and properties.

**To close the gap:** `tree-sitter-php`; normalise `\` to `.` in
`header_namespace` so existing `x:`/`n:` rules apply.

## 8. Go — heuristic

**Currently indexes:** `func name() { }` and `func name() T { }` only when the
shape survives; the `func` keyword is in the stop list so it never becomes a
phantom.

**Still misses:** receiver methods (`func (p *Player) Update() error`) and
result types.

**To close the gap:** `tree-sitter-go`; name receiver methods `Player.Update`.

## 9. Rust — heuristic

**Currently indexes:** `fn name(params) { }`; `struct`/`enum`/`trait` names.

**Still misses:** `-> T` return types, `impl`/trait methods, and `new` (which
collides with the stop list).

**To close the gap:** `tree-sitter-rust`.

## 10. Kotlin — heuristic

**Currently indexes:** `fun name(params) { }` and `class` names; the primary-
constructor line is correctly **not** a function (type guard).

**Still misses:** expression bodies, properties, `object`, and expression-typed
signatures.

**To close the gap:** `tree-sitter-kotlin` (`tree-sitter-kotlin-ng`).

## 11. Swift — heuristic

**Currently indexes:** `init`, `func update(...)`, and `class`/`struct` names.

**Still misses:** `-> T` returns, computed properties, and extensions.

**To close the gap:** `tree-sitter-swift`.

## 12. Scala — heuristic

**Currently indexes:** `class`/`object`/`trait` names and little else.

**Still misses:** `def f(x: Int): Int = { ... }` (the body opens after `=`),
`val`/`var`, and nested `package` clauses.

**To close the gap:** `tree-sitter-scala`.

## 13. Dart — heuristic

**Currently indexes:** `int foo() { }`-style functions.

**Still misses:** `Future<T> foo() async { }` (a keyword sits between `)` and
`{`), classes, and `library` headers.

**To close the gap:** `tree-sitter-dart` (least mature grammar; keep the
heuristic fallback).

## 14. asm — strings only, by design

Disassembly listings contribute string literals (and nothing else) so a string
can be located in a binary even when no source is available. No change planned.

## 15. txt — header dumps, by design

Decompiler header/type dumps contribute string literals and any declaration that
happens to match the shared shapes. No change planned.
