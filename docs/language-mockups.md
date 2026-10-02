# Language mock-ups (design only, not implemented)

Mock-ups for the languages the heuristic reader cannot serve faithfully. Each
section is concrete enough to implement: grammar choice, the tree-sitter
queries, how captures become rows, how the module taxonomy is assigned from
data, the fixture layout, and the tests that would pin it.

Nothing here is compiled in. The four target languages (C, C++, C#, Java) are
handled by the existing heuristic matcher — see
[`language-support.md`](language-support.md).

---

## 0. Shared back end: an optional, feature-gated grammar path

The crate is famously zero-dependency and must stay that way by default. So the
grammar path is opt-in:

```toml
# Cargo.toml
[features]
default  = []
grammars = [
  "dep:tree-sitter",
  "dep:tree-sitter-go",
  "dep:tree-sitter-rust",
  "dep:tree-sitter-javascript",
  "dep:tree-sitter-typescript",
  "dep:tree-sitter-kotlin-ng",
  "dep:tree-sitter-scala",
  "dep:tree-sitter-swift",
  "dep:tree-sitter-php",
  "dep:tree-sitter-dart",
]

[dependencies]
tree-sitter             = { version = "0.23", optional = true }
tree-sitter-go          = { version = "0.23", optional = true }
tree-sitter-rust        = { version = "0.23", optional = true }
tree-sitter-javascript  = { version = "0.23", optional = true }
tree-sitter-typescript  = { version = "0.23", optional = true }
tree-sitter-kotlin-ng   = { version = "1",    optional = true }
tree-sitter-scala       = { version = "0.23", optional = true }
tree-sitter-swift       = { version = "0.6",  optional = true }
tree-sitter-php         = { version = "0.23", optional = true }
tree-sitter-dart        = { version = "0.0.5", optional = true }
```

A default build (`cargo build`) has no dependencies and behaves exactly as
today. A build with `--features grammars` swaps in a real parser per language
and falls back to the heuristic matcher for anything not in the registry — so
the change is additive and never regresses the C family.

```rust
// src/lang/mod.rs
pub trait LanguageParser: Sync {
    /// Stable id, also the `queries/<id>/` directory name.
    fn id(&self) -> &'static str;
    /// Extensions this grammar owns (without dot).
    fn extensions(&self) -> &'static [&'static str];
    /// Same shape the heuristic matcher produces, so the store is untouched.
    fn parse(&self, src: &str) -> crate::parse::Parsed;
}

pub fn for_ext(ext: &str) -> Option<&'static dyn LanguageParser> {
    #[cfg(feature = "grammars")]
    {
        REGISTRY.iter().copied().find(|p| p.extensions().contains(&ext))
    }
    #[cfg(not(feature = "grammars"))]
    {
        let _ = ext;
        None
    }
}
```

The only change in `src/store.rs` `index_root` is the parse call (which today
passes no path at all):

```rust
let parsed = match crate::lang::for_ext(ext_of(&rel)) {
    Some(lang) => lang.parse(&src),
    None       => parse_source(&src),
};
```

Each grammar is a thin unit struct; the shared driver does the query work:

```rust
// src/lang/tsparse.rs
pub struct TsParser {
    pub id: &'static str,
    pub ext: &'static [&'static str],
    pub language: fn() -> tree_sitter::Language,
    pub symbols_scm: &'static str,  // include_str!("../../queries/<id>/symbols.scm")
    pub calls_scm:   &'static str,
    pub strings_scm: &'static str,
}
impl LanguageParser for TsParser { /* parse() below */ }
```

`parse()` creates the parser, runs each `.scm` through `QueryCursor`, and maps
captures by name:

| capture | becomes |
|---------|---------|
| `@name` + `@params` (+ `@body`, `@recv`, `@ret`) | `Func { name, params, sig, start, end }` |
| `@type` / `@ns` / `@field` / `@property` | `Sym { kind, name, line }` |
| `@call` | a call site resolved by the existing `scan_calls` name map |
| `@str` | `(line, text)`, applying the same ≥ 4-char / non-hex filter |

`start`/`end` are 1-based lines from `node.start_position().row + 1`. That is
the whole adapter; `db.rs`, `modules.rs`, and the query surface never change.

### Taxonomy stays data

Package-header languages (Go, Kotlin, Scala, PHP, Java-like) declare a
namespace the path does not always reflect. Keep the taxonomy language-neutral
by teaching `module_of` one extra source of a "namespace string": a
`header_namespace(src)` that recognises `namespace X`, `package X`,
`package X;`, and normalises `\` and `::` to `.`. The existing `x:` (exact) and
`n:` (prefix) rule kinds then match against it, unchanged:

```
# .gamedb/modules.txt
k:acme.core        core.player     Player simulation
k:acme.world       core.world      World + tiles
n:acme.render      core.render     Drawing
m:third_party      third_party     Vendored
```

(Selector letter is illustrative; the point is that a header namespace is
another data field, not compiled-in knowledge about any game.)

### Test harness

One golden file per language, driven by a shared helper:

```rust
// tests/languages.rs
#[test]
fn go_receiver_methods_are_functions() {
    let names = index_names("research/fixtures/go/player.go"); // via public API
    assert!(names.contains(&"Update"));          // func (p *Player) Update(...)
    assert!(names.contains(&"NewPlayer"));       // return-typed func
    assert!(!names.iter().any(|n| n == "func")); // no phantom
}
```

---

## 1. Go — `tree-sitter-go`

**Why a grammar.** Receiver methods (`func (p *Player) Update(...)`), result
types (`func f() error`), and named returns all defeat the `NAME(...) {` shape.

**Queries** (`queries/go/symbols.scm`):

```scheme
(function_declaration
  name: (identifier) @name
  parameters: (parameter_list) @params
  result: (_)? @ret
  body: (block) @body) @func

(method_declaration
  receiver: (parameter_list) @recv
  name: (field_identifier) @name
  parameters: (parameter_list) @params
  result: (_)? @ret
  body: (block) @body) @func

(type_declaration
  (type_spec name: (type_identifier) @type
             type: [(struct_type) (interface_type) (type_identifier)])) @type

(type_declaration
  (type_spec type: (struct_type
    (field_declaration name: (field_identifier) @field))))

(package_clause (package_identifier) @package)
```

`queries/go/calls.scm`: `(call_expression function: (identifier) @call)` and
`(call_expression function: (selector_expression field: (field_identifier) @call))`.
`queries/go/strings.scm`: `[(interpreted_string_literal) (raw_string_literal)] @str`.

Symbol extraction notes: name a receiver method `Player.Update` so it is unique
in the corpus; `@recv` is kept as a symbol attribute if the schema later grows
one. The `func` keyword is never a capture, so the phantom-function class is
structurally impossible.

**Taxonomy:** `package acme.core` → `header_namespace` = `acme.core`, matched
by `k:`/`n:` rules; files without a matching rule fall to `m:` path rules or the
derived taxonomy.

**Fixture layout:**

```
research/fixtures/go/player.go     package acme.core;  type Player struct{...}; func (p *Player) Update(dt int) error
research/fixtures/go/world.go      package acme.world; func NewWorld(...) *World
research/fixtures/go/.gamedb/modules.txt   k:acme.core -> core.player
```

**Tests:** `go_receiver_methods_are_functions`, `go_result_type_is_not_the_name`,
`go_struct_fields_are_indexed`, `go_package_header_maps_to_module`,
`go_selector_call_resolves_to_method`.

---

## 2. Rust — `tree-sitter-rust`

**Why a grammar.** `fn f() -> T`, `impl` methods, trait methods, and `mod`
nesting are all outside the heuristic shape (and `new` collides with the
stop-list).

**Queries** (`queries/rust/symbols.scm`):

```scheme
(function_item
  name: (identifier) @name
  parameters: (parameters) @params
  return_type: (_)? @ret
  body: (block) @body) @func

(impl_item
  type: (_) @owner
  body: (declaration_list
    (function_item name: (identifier) @name
                   parameters: (parameters) @params
                   body: (block) @body) @func))

(struct_item name: (type_identifier) @type)
(enum_item  name: (type_identifier) @type)
(trait_item name: (type_identifier) @type)

(struct_item body: (field_declaration_list
  (field_declaration name: (field_identifier) @field)))

(mod_item name: (identifier) @ns)
```

`queries/rust/calls.scm`: `(call_expression function: [(identifier) (scoped_identifier name: (identifier)) (field_expression field: (field_identifier))] @call)`.
Strings: `[(string_literal) (raw_string_literal)] @str`.

**Taxonomy:** `mod` items and file paths; `.gamedb/modules.txt` `m:` rules are
usually sufficient (Rust already lays modules out on disk).

**Fixture layout:**

```
research/fixtures/rust/src/lib.rs        pub mod player; pub mod render;
research/fixtures/rust/src/player.rs     impl Player { pub fn new() -> Self; pub fn update(&mut self, dt: i32); }
```

**Tests:** `rust_return_typed_fn`, `rust_impl_method_is_a_function`,
`rust_new_is_a_function`, `rust_struct_fields`, `rust_mod_derives_module`.

---

## 3. JavaScript / TypeScript — `tree-sitter-javascript`, `tree-sitter-typescript`

**Why a grammar.** Arrow functions assigned to `const`, method shorthand,
typed returns, and imports/exports are the backbone of JS/TS, and the heuristic
reader sees only `constructor`/method definitions. It also leaks arrow-body
`const`s as fields.

**Queries** (`queries/typescript/symbols.scm`; the JS file is the same minus
`: type_annotation`):

```scheme
(function_declaration
  name: (identifier) @name
  parameters: (formal_parameters) @params
  return_type: (type_annotation)? @ret
  body: (statement_block) @body) @func

(method_definition
  name: [(property_identifier) (private_property_identifier)] @name
  parameters: (formal_parameters) @params
  return_type: (type_annotation)? @ret
  body: (statement_block) @body) @func

(variable_declarator
  name: (identifier) @name
  value: (arrow_function parameters: (formal_parameters) @params
                         return_type: (type_annotation)? @ret
                         body: (statement_block) @body) @func)

(class_declaration name: (type_identifier) @type)
(interface_declaration name: (type_identifier) @type)
(type_alias_declaration name: (type_identifier) @type)
(public_field_definition name: (property_identifier) @field)
```

`queries/typescript/calls.scm`:
`(call_expression function: (identifier) @call)` and
`(call_expression function: (member_expression property: (property_identifier) @call))`.
`queries/typescript/imports.scm`: `(import_statement source: (string) @module)`
(edge `imports`, richer than today's call-only graph).
Strings: `(string) @str` plus `(template_string) @str`.

Key wins: arrow functions become real functions (so their bodies stop leaking
as fields), and typed returns stop being mistaken for type separators. `tsx`
selects the `tsx` grammar variant; `.d.ts` keeps declarations only.

**Taxonomy:** file path (ES modules), or cluster by the import graph — an
engine-neutral `imports` edge already gives the renderer/audio split.

**Fixture layout:**

```
research/fixtures/typescript/player.ts   export class Player {...}
research/fixtures/typescript/util.ts     export const fastPlayer = (): Player => {...}
research/fixtures/typescript/player.tsx  JSX component
```

**Tests:** `ts_typed_return_is_a_function`, `js_arrow_assigned_to_const_is_a_function`,
`tsx_component_is_a_function`, `ts_import_edge`, `js_arrow_locals_are_not_fields`.

---

## 4. Kotlin — `tree-sitter-kotlin` (crate `tree-sitter-kotlin-ng`)

**Why a grammar.** `fun`, expression bodies, `private var` properties, and the
primary-constructor line all need more than `NAME(...) {`.

**Queries:**

```scheme
(function_declaration
  (simple_identifier) @name
  (function_value_parameters) @params
  (function_body | block) @body) @func

(class_declaration (type_identifier) @type)
(object_declaration (type_identifier) @type)
(property_declaration
  (variable_declaration (simple_identifier) @field))
(package_header (qualified_identifier) @package)
```

Calls: `(call_expression (simple_identifier) @call)`. Strings:
`(string_literal) @str`.

**Taxonomy:** `package` header via `header_namespace`; `.gamedb/modules.txt`
`k:`/`n:` rules.

**Fixture layout:** `research/fixtures/kotlin/{Player.kt, Render.kt}` plus
`.kts` build-script sample (indexed but low-value — a `@type` for the script).

**Tests:** `kotlin_fun_and_expression_body`, `kotlin_primary_ctor_is_a_type`,
`kotlin_private_property_is_a_field`, `kotlin_package_maps_to_module`.

---

## 5. Scala — `tree-sitter-scala`

**Why a grammar.** `def f(x: Int): Int = { ... }` — the body opens after `=`,
which the heuristic reader rejects.

**Queries:**

```scheme
(function_definition name: (identifier) @name
  parameters: (parameters) @params body: (_)? @body) @func
(class_definition  name: (identifier) @type)
(object_definition name: (identifier) @type)
(trait_definition  name: (identifier) @type)
(val_definition  pattern: (identifier) @field)
(var_definition  pattern: (identifier) @field)
(package_clause (package_identifier) @package)
```

Calls: `(call_expression function: (identifier) @call)`. Strings:
`(string) @str` / `(interpolated_string_expression) @str`.

**Taxonomy:** nested `package a.b` clauses → `a.b`; `k:`/`n:` rules.

**Tests:** `scala_def_is_a_function`, `scala_val_is_a_field`,
`scala_nested_package_maps`.

---

## 6. Swift — `tree-sitter-swift`

**Why a grammar.** `func f(_ x: Int) -> Int { ... }`, `init`, computed
properties, and `extension` methods.

**Queries:**

```scheme
(function_declaration name: (simple_identifier) @name
  (parameter_clause) @params body: (_)? @body) @func
(init_declaration (parameter_clause) @params body: (_)? @body) @func
(class_declaration name: (type_identifier) @type)
(protocol_declaration name: (type_identifier) @type)
(extension_declaration type: (user_type (type_identifier) @owner))
(property_declaration (pattern (simple_identifier) @field))
```

Calls: `(call_expression (simple_identifier) @call)`. Strings:
`(line_string_literal) @str`.

**Taxonomy:** Swift has no namespaces; modules are targets → derive from path
(`m:`) or an explicit `.gamedb/modules.txt`.

**Tests:** `swift_func_return_type`, `swift_init_is_a_function`,
`swift_extension_method`, `swift_property_is_a_field`.

---

## 7. PHP — `tree-sitter-php`

**Why a grammar.** `public function f(): int { ... }` and namespaces with `\`.

**Queries:**

```scheme
(function_definition name: (name) @name
  parameters: (formal_parameters) @params body: (compound_statement) @body) @func
(method_declaration name: (name) @name
  parameters: (formal_parameters) @params body: (compound_statement) @body) @func
(class_declaration name: (name) @type)
(interface_declaration name: (name) @type)
(property_declaration (property_element (variable_name (name) @field)))
(namespace_definition name: (namespace_name) @package)
```

Calls: `(function_call_expression function: (name) @call)` and
`(member_call_expression name: (name) @call)`. Strings:
`[(string) (encapsed_string)] @str`.

**Taxonomy:** normalise `Acme\Core` → `Acme.Core` in `header_namespace`, then
the existing `x:`/`n:` rules apply unchanged.

**Tests:** `php_typed_return_is_a_function`, `php_backslash_namespace_maps`,
`php_property_is_a_field`.

---

## 8. Dart — `tree-sitter-dart`

**Why a grammar.** `Future<T> foo() async { ... }` — `async` between `)` and `{`.

**Queries** (`tree-sitter-dart` node names are less stable; the shape this
mock-up assumes):

```scheme
(function_signature name: (identifier) @name
  parameters: (formal_parameter_list) @params) @sig
(function_body (block) @body) @func
(method_signature name: (identifier) @name
  parameters: (formal_parameter_list) @params) @sig
(class_definition name: (identifier) @type)
(library_definition (library_name) @package)
```

Because Dart's grammar is the least mature of the set, the mock-up keeps a
fallback: if the grammar is unavailable, `Future<T> foo(...) async {` is a
documented miss rather than a wrong row.

**Taxonomy:** `library`/`part` headers then path-derived `m:` rules.

**Tests:** `dart_async_future_fn`, `dart_class_method`, `dart_library_maps`.

---

## 9. Effort and ordering

| order | language | grammar maturity | roughly |
|-------|----------|------------------|---------|
| 1 | Go | mature | driver + queries + 1 fixture → hours |
| 2 | JavaScript / TypeScript | mature | biggest payoff; adds `imports` edges |
| 3 | Rust | mature | |
| 4 | Kotlin | good | |
| 5 | Scala | good | |
| 6 | Swift | good | |
| 7 | PHP | mature | |
| 8 | Dart | weakest | expect grammar churn |

The shared driver (§0) is the real work; each language after that is a query
file, a unit struct, a fixture, and one test. The `grammars` feature keeps the
zero-dependency default intact, so this can land incrementally without ever
changing today's C/C++/C#/Java behaviour.
