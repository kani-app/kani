# Extraction DSL

The extraction DSL is a small typed, pipeline-oriented language embedded in YAML strings. It is
parsed by `kani-yaml`, converted to the shared `Expr` AST, and evaluated by Kani's HTML or JSON
evaluator.

## Lexical rules

- Identifiers match `[a-zA-Z_][a-zA-Z0-9_]*`.
- Variables start with `$`.
- DSL strings use double quotes; wrap them in YAML single quotes where convenient.
- Numbers may be signed integers or floats.
- `/* ... */` block comments are supported; they do not nest.
- Method calls chain with `.`, and `let` statements end with a semicolon or newline.

## Grammar

```ebnf
program     = let-expression | expression ;
let-expression = "let" variable "=" expression (";" | newline) program ;
expression  = logical-or ;
logical-or  = logical-and { "||" logical-and } ;
logical-and = comparison { "&&" comparison } ;
comparison  = addition { ("==" | "!=" | "<" | ">" | "<=" | ">=") addition } ;
addition    = multiply { ("+" | "-") multiply } ;
multiply    = chain { ("*" | "/") chain } ;
chain       = atom { "." method-call } ;
atom        = "self" | "dom" "(" string ")" | "json" "(" string ")"
            | "pref" "(" string ")" | "scalar" "(" string ")"
            | "index" "(" ")" | variable | string | number
            | "null" | "true" | "false" | list
            | "if" expression "then" expression "else" expression
            | "merge" "(" list ")" | "format" "(" string { "," expression } ")"
            | "(" expression ")" ;
method-call = identifier "(" [ expression { "," expression } ] ")" ;
```

A map literal is valid only as the argument to `lookup`.

## Value model

Expressions produce string, number, integer, boolean, null, list, HTML element, or JSON node
values. Elements and JSON nodes are intermediate values and cannot be emitted as final fields.

Most missing DOM and JSON navigation yields null and propagates through later operations. Numeric
and date parse failures also yield null. `fallback` replaces null or an empty string; it does not
catch evaluator errors such as an out-of-range list access.

## Roots

| Expression | Result |
|---|---|
| `self` | Current row element or JSON node |
| `dom("selector")` | First matching document element |
| `json("/pointer")` | JSON node at an RFC 6901 pointer |
| `index()` | Zero-based row index |
| `pref("key")` | Extension preference value |
| `scalar("name")` | Previously evaluated document scalar |
| `$name` | Bound variable |

## DOM methods

| Method | Purpose |
|---|---|
| `attr("name")` | Attribute or null |
| `text()` / `inner_html()` | Descendant text or raw inner HTML |
| `select("selector")` | All matching descendants |
| `first("selector")` | First matching descendant or null |
| `has_class("name")` | Class test |
| `children()` | Direct child elements |

## String and scalar methods

| Group | Methods |
|---|---|
| Cleanup | `trim`, `lower`, `replace`, `slice` |
| Split and compose | `split`, `split_n`, `prepend`, `append`, `format`, `format_padded` |
| Tests | `matches`, `capture`, `starts_with`, `ends_with` |
| Conversion | `parse_float`, `parse_int`, `to_string`, `string_len` |
| URLs | `url_encode`, `url_decode`, `resolve_url` |
| Dates | `date_parse`, `date_parse_rfc3339` |

## Lists and control flow

| Method | Purpose |
|---|---|
| `at(n)` | Index, with negative indices counted from the end |
| `join`, `take`, `skip`, `reverse`, `unique` | Basic list operations |
| `sort_by(expr)` | Stable sort using `$item` |
| `map`, `flat_map`, `filter` | Transform using `$item` and `$index` |
| `fold(base, expr)` | Reduce using `$acc`, `$item`, and `$index` |
| `merge([lists])` | Concatenate lists |
| `fallback(expr)` | Replace null or empty string |
| `lookup({...})` | Map a string to another string |
| `not()` | Boolean negation |

`if condition then a else b`, `&&`, and `||` short-circuit. Arithmetic and comparison follow the
usual precedence. Incompatible operands are evaluator errors rather than automatic string
coercions.

## JSON methods

| Method | Purpose |
|---|---|
| `ptr("/pointer")` | Navigate to a child node |
| `str`, `int`, `float`, `bool` | Extract a scalar of the requested type |
| `array_len`, `keys` | Inspect collections |
| `get(expr)` | Dynamic object-key access |
| `find(key, value)` | Find the first matching object in an array |
| `json_fold()` | Merge objects or concatenate arrays in a JSON array |

## Variables and examples

```text
let $base = dom("meta[property='og:url']").attr("content");
self.first("img.cover").attr("src").resolve_url($base)
```

```text
dom(".status").text().trim().lower().lookup({
  "publishing": "ongoing",
  "finished": "completed"
}).fallback("unknown")
```

```text
self.select(".tag").map($item.text().trim()).filter($item.matches("[^\\s]")).unique().join(", ")
```

## Pure user functions

Functions declared under `scripts.pure` are called as `.user.name(args...)`. The receiver becomes
`arg0`, followed by explicit arguments. Null inputs skip the call and return null. Pure functions
can use scalar and string-list values but cannot access request, response, or cache bindings.

Use the CLI to parse and explain an expression while authoring:

```bash
cargo run -p kani-cli -- dsl 'self.first("h2").text().trim()'
cargo run -p kani-cli -- repl explain 'self.first("h2").text().trim()'
```
