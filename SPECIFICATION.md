# Kani Declarative Extraction: Specification

This document defines the extraction DSL, JSON intermediate model (IM), and YAML extension format.

Every DSL expression in this document's unlabelled and `yaml` code blocks is parsed and evaluated
by `kani-core/tests/spec_examples.rs`. A block that sketches a schema rather than showing an example
is preceded by `<!-- schema sketch: not an executable example -->`.

---

## 1. Extraction DSL

The extraction DSL is a functional, pipeline-oriented language embedded in YAML field definitions.
Each expression extracts one value from a document element.

### 1.1 Lexical Structure

**Identifiers:** `[a-zA-Z_][a-zA-Z0-9_]*`

**Variable names:** `$` followed by an identifier: `$base`, `$cover_path`

**String literals:** Double-quoted: `"hello"`, `"/manga"`, `"Chapter\s+"`. Single quotes are not supported to avoid YAML quoting conflicts. `\n`, `\t`, `\r`, `\"`, and `\\` are decoded; every other backslash sequence is passed through unchanged, so a regex literal such as `"Chapter\s+(\d+)"` reaches the regex engine intact.

**Numeric literals:** Integer or float: `0`, `2`, `3.14`, `-1`

**Keywords:** `let`, `self`, `dom`, `json`, `index`, `null`, `true`, `false`, `if`, `then`, `else`, `pref`, `scalar`, `merge`, `format`

**Operators:** `.` (method chain), `=` (binding), `;` (statement separator), `,` (argument separator), `{` `}` (map literal), `(` `)` (grouping/call), `+` `-` `*` `/` (arithmetic), `==` `!=` `<` `>` `<=` `>=` (comparison), `&&` `||` (logical)

**Whitespace:** Ignored between tokens, newlines included — an expression may be laid out over as many lines as it likes (one method of a chain per line, one list element per line), which is how a YAML block scalar naturally reads. `let` statements are separated by `;`; a newline alone does not end one.

**Comments:** `/* */` block comments; nesting is unsupported.

### 1.2 Grammar

```ebnf
program        = let_expr | expr ;
let_expr       = "let" variable "=" expr ";" ( let_expr | expr ) ;

(* Binary operators, in ascending precedence order *)
expr           = or_expr ;
or_expr        = and_expr { "||" and_expr } ;
and_expr       = cmp_expr { "&&" cmp_expr } ;
cmp_expr       = add_expr { ( "==" | "!=" | "<" | ">" | "<=" | ">=" ) add_expr } ;
add_expr       = mul_expr { ( "+" | "-" ) mul_expr } ;
mul_expr       = chain { ( "*" | "/" ) chain } ;
chain          = atom { "." method_call } ;

atom           = "self"
               | "dom" "(" string ")"
               | "json" "(" string ")"
               | "pref" "(" string ")"
               | "scalar" "(" string ")"
               | "index" "(" ")"
               | "merge" "(" "[" [ expr { "," expr } ] "]" ")"
               | "format" "(" string [ "," expr { "," expr } ] ")"
               | variable
               | string
               | number
               | "null"
               | "true"
               | "false"
               | "[" [ expr { "," expr } ] "]"
               | "if" expr "then" expr "else" expr
               | "(" expr ")" ;
method_call    = IDENT "(" [ arg_list ] ")" ;
arg_list       = expr { "," expr } ;
variable       = "$" IDENT ;
string         = '"' { CHAR } '"' ;
number         = [ "-" ] DIGIT+ [ "." DIGIT+ ] ;
integer        = [ "-" ] DIGIT+ ;
(* map_literal is only valid as the sole argument to .lookup() — it is not a general atom *)
map_literal    = "{" [ map_entry { "," map_entry } ] "}" ;
map_entry      = string ":" string ;
```

### 1.3 Semantics

Every expression evaluates to a **Value**, which is one of:

| Type | Description |
|------|-------------|
| `String` | UTF-8 text |
| `Number` | 64-bit float |
| `Int` | 64-bit integer |
| `Bool` | Boolean |
| `Null` | Absent/missing value |
| `List` | Ordered sequence of values (used as iterator input/output) |
| `Element` | Reference to an HTML element (opaque, cannot be serialized) |
| `Json` | Reference to a JSON sub-tree (opaque until extracted) |

**Type coercion rules:**
- DOM and string operations on a `Null` input propagate `Null` (null-safe chaining). For example, `self.first(".title").text()` returns `Null` if `.first()` finds no match rather than erroring.
- DOM operations (`attr`, `text`, `inner_html`) return `String` or `Null`. `text()` returns an empty string (not Null) if the element exists but has no text content.
- `parse_float()` and `parse_int()` on a non-numeric string return `Null` (soft failure, not an error).
- `date_parse()` and `date_parse_rfc3339()` on a malformed string return `Null` (soft failure).
- `fallback(default)` replaces `Null` or an empty string with the default value. It does **not** catch runtime errors — only null/empty values.
- Final field values must be `String`, `Number`, `Int`, or `Null` (for optional fields). `Element` and `Json` cannot be output directly.

### 1.4 Root Expressions

These are the starting points for extraction chains:

| Expression | Description |
|------------|-------------|
| `self` | The current element in the container iteration. Only valid inside a field extraction (not in top-level bindings). |
| `dom("selector")` | Select the **first** matching element from the document root. Returns `Element` or `Null` if no match. Use `self.select("selector")` to get all matching elements as a `List`. |
| `json("pointer")` | Navigate to a JSON value using a JSON Pointer (RFC 6901). Returns a `Json` value. |
| `index()` | The 0-based iteration index of the current container element. Returns `Int`. |
| `pref("key")` | Read an extension preference value by key. Returns `String` or `Null` if the preference is unset. |
| `scalar("name")` | Read a named scalar value computed at the document level (from the `scalars` section of the blueprint). Returns whatever type the scalar expression produces. Useful for feeding document-level computation into per-element expressions without re-evaluating it per row. |
| `$variable` | Reference a previously bound variable. |

### 1.5 Method Reference

#### DOM Methods

| Method | Input Type | Return Type | Description |
|--------|-----------|-------------|-------------|
| `.attr("name")` | Element | String/Null | Get the value of an HTML attribute. Returns `Null` if the attribute doesn't exist. |
| `.text()` | Element | String | Get the combined text content of the element and all descendants. Returns empty string if no text. |
| `.select("selector")` | Element | List&lt;Element&gt; | Select all matching descendant elements. Returns an empty `List` if none match. |
| `.first("selector")` | Element | Element/Null | Select the first matching descendant element, or `Null` if none match. |
| `.inner_html()` | Element | String | Get the inner HTML of the element as a raw HTML string. |
| `.has_class("name")` | Element | Bool | Test if the element has a given CSS class. |
| `.children()` | Element | List&lt;Element&gt; | Return the direct child elements as a `List`. |

#### String Methods

| Method | Input Type | Return Type | Description |
|--------|-----------|-------------|-------------|
| `.split("delim")` | String | List&lt;String&gt; | Split the string on `delim`. Returns a `List` of all segments. Use `.at(n)` to extract a specific segment. |
| `.split_n("delim", n)` | String | List&lt;String&gt; | Split into at most `n` parts. The last part contains the remainder of the string (unsplit). Useful when a delimiter appears multiple times but only the first few splits matter. |
| `.replace("from", "to")` | String | String | Replace all occurrences of `from` with `to`. |
| `.trim()` | String | String | Remove leading and trailing whitespace. |
| `.lower()` | String | String | Convert to lowercase. |
| `.prepend(expr)` | String | String | Prepend the result of `expr` to this string. |
| `.append(expr)` | String | String | Append the result of `expr` to this string. |
| `.matches("regex")` | String | Bool | Test if the string matches the regex pattern. |
| `.capture("regex")` | String | List&lt;String/Null&gt; | Capture all groups from the first match. Index `0` is the full match; `1`+ are capture groups. Returns an empty `List` if there is no match. Use `.at(n)` to extract a specific group. |
| `.starts_with("prefix")` | String | Bool | Test if the string starts with the given prefix. |
| `.ends_with("suffix")` | String | Bool | Test if the string ends with the given suffix. |
| `.slice(start, end)` | String | String | Substring by character index (0-based, exclusive end). Negative values count from the end. `end` is optional; omitting it slices to the end of the string. |
| `.to_string()` | Int/Number/Bool | String | Convert a numeric or boolean value to its string representation. `Null` propagates as `Null`. |
| `.string_len()` | String | Int | Number of Unicode characters (not bytes) in the string. |
| `.url_encode()` | String | String | Percent-encode a string for use as a URL query parameter value (e.g. `"hello world"` → `"hello%20world"`). Also accepted as `.urlencode()`. |
| `.url_decode()` | String | String | Decode a percent-encoded string. Invalid `%`-sequences are passed through unchanged. Also accepted as `.urldecode()`. |
| `.format_padded(width, fill, align)` | String | String | Pad or align a string to at least `width` Unicode characters using `fill` (a single character) and `align` (`"left"`, `"right"`, or `"center"`). If the string is already at least `width` characters, it is returned unchanged. |
| `format("template {}", arg1, arg2, ...)` | — | String | Interpolate `{}` placeholders in the template string with the evaluated arguments in order. Each `{}` is replaced by the corresponding argument's string value. Returns `String`. |

#### List Methods

| Method | Input Type | Return Type | Description |
|--------|-----------|-------------|-------------|
| `.at(n)` | List | Any | Get the element at index `n`. Negative indices count from the end: `-1` is the last element. Returns `Null` if out of bounds, so an absent segment or capture group can be defaulted with `.fallback()`. Works on any `List`, including results from `.split()`, `.select()`, and `.children()`. |
| `.join("delim")` | List&lt;String&gt; | String | Join a list of strings into a single string using `delim` as the separator. `Null` elements are skipped. |
| `.take(n)` | List | List | Return the first `n` elements. Returns the whole list if `n` exceeds its length. |
| `.skip(n)` | List | List | Drop the first `n` elements and return the rest. Returns an empty list if `n` exceeds the length. |
| `.reverse()` | List | List | Reverse the list in place. |
| `.sort_by(key_expr)` | List | List | Sort the list by a key expression evaluated per element with `$item` in scope. Numeric types (`Int`, `Number`) sort numerically; `String` sorts lexicographically; `Bool` sorts false&lt;true; `Null`, `Element`, `Json`, and nested `List` values sort to the end (stable, equal among themselves). |
| `.unique()` | List | List | Remove duplicate elements. First occurrence is kept; subsequent duplicates are dropped. Order is preserved. |
| `merge([list1, list2, ...])` | — | List | Concatenate multiple lists into a single flat list. Each argument must evaluate to a `List`. Unlike `.flat_map()`, the lists are given directly rather than derived from a parent collection. Useful for merging results from disjoint selectors. |

#### Type Coercion Methods

| Method | Input Type | Return Type | Description |
|--------|-----------|-------------|-------------|
| `.parse_float()` | String | Number/Null | Parse as a 64-bit float. Returns Null on parse failure. |
| `.parse_int()` | String | Int/Null | Parse as a 64-bit integer. Returns Null on parse failure. |

#### Control Flow Methods

| Method | Input Type | Return Type | Description |
|--------|-----------|-------------|-------------|
| `.fallback(expr)` | Any | Any | If the target is `Null` or an empty string, evaluate and return `expr` instead. |
| `.lookup({"k1": "v1", ...})` | String | String/Null | Look up the target string in the provided map literal. Returns the mapped value, or `Null` if not found. Chain `.fallback("default")` to supply a default. |
| `.map(body)` | List | List | Iterate over the list. For each element, evaluate `body` with `$item` bound to the current element and `$index` bound to its 0-based position. `Null` results are dropped. Returns a new `List`. |
| `.flat_map(body)` | List | List | Like `.map(body)`, but each `body` evaluation must return a `List`; all result lists are concatenated into a single flat `List`. Useful when each element expands into multiple values. |
| `.fold(base, body)` | List | Any | Left fold over the list. Evaluates `base` as the initial accumulator, then for each element evaluates `body` with `$acc` bound to the running accumulator, `$item` to the current element, and `$index` to its 0-based position. The result of each `body` evaluation becomes the new `$acc`. Returns the final accumulator value. |
| `.filter(predicate)` | List | List | Keep only elements for which `predicate` evaluates to `true`. `predicate` is evaluated with `$item` and `$index` in scope. Elements where the predicate returns `false` or `Null` are dropped. Produces a `List` of the same element type. |
| `if cond then a else b` | — | Any | If `cond` is `true`, evaluates and returns `a`; if `false` or `Null`, evaluates and returns `b`. Short-circuits: only the selected branch is evaluated. `cond` must be `Bool` (or `Null`, which is treated as `false`). |
| `.not()` | Bool/Null | Bool | Boolean negation. `Null` is treated as `false`, so `.not()` on `Null` returns `true`. |

#### Binary Operators

Binary operators are written infix: `lhs op rhs`. Both operands are expressions and can be arbitrarily complex chains. Operator precedence follows standard rules: `*`/`/` bind tighter than `+`/`-`, which bind tighter than comparisons, which bind tighter than `&&`, which binds tighter than `||`. Use parentheses to override.

| Operator | Operand Types | Return Type | Description |
|----------|--------------|-------------|-------------|
| `+` | Number, Number | Number | Addition. Also valid for Int + Int → Int, or Int + Number → Number. |
| `-` | Number, Number | Number | Subtraction. |
| `*` | Number, Number | Number | Multiplication. |
| `/` | Number, Number | Number | Division. |
| `==` | Any, Any | Bool | Equality. Compares `String`, `Number`, `Int`, `Bool`, and `Null`. |
| `!=` | Any, Any | Bool | Inequality. |
| `<` | Number/Int, Number/Int | Bool | Less than. |
| `>` | Number/Int, Number/Int | Bool | Greater than. |
| `<=` | Number/Int, Number/Int | Bool | Less than or equal. |
| `>=` | Number/Int, Number/Int | Bool | Greater than or equal. |
| `&&` | Bool, Bool | Bool | Logical and. Short-circuits: right side not evaluated if left is `false`. |
| `\|\|` | Bool, Bool | Bool | Logical or. Short-circuits: right side not evaluated if left is `true`. |

Applying an operator to incompatible types (e.g., `String + Number`) is a runtime error.

#### Date Methods

| Method | Input Type | Return Type | Description |
|--------|-----------|-------------|-------------|
| `.date_parse("format")` | String | Int/Null | Parse a date string using the given format pattern (Rust `time` crate syntax). Returns a Unix timestamp (`Int`) or `Null` on parse failure. `Null` input propagates as `Null`. |
| `.date_parse_rfc3339()` | String | Int/Null | Parse an RFC 3339 / ISO 8601 date string. Returns a Unix timestamp (`Int`) or `Null` on parse failure. `Null` input propagates as `Null`. |

#### URL Methods

| Method | Input Type | Return Type | Description |
|--------|-----------|-------------|-------------|
| `.resolve_url(base_expr)` | String | String | Resolve a relative URL against a base URL. `base_expr` can be a string literal or any expression evaluating to a `String`. |

#### JSON Methods

The following methods operate on `Json` values from `json()` or a JSON-mode blueprint:

| Method | Input Type | Return Type | Description |
|--------|-----------|-------------|-------------|
| `.ptr("pointer")` | Json | Json/Null | Navigate deeper using a JSON Pointer (RFC 6901). Returns `Null` if the path does not exist. |
| `.str()` | Json | String/Null | Extract the JSON value as a string. Returns `Null` if not a string. |
| `.int()` | Json | Int/Null | Extract the JSON value as a 64-bit integer. Returns `Null` if not a number. |
| `.float()` | Json | Number/Null | Extract the JSON value as a 64-bit float. Returns `Null` if not a number. |
| `.bool()` | Json | Bool/Null | Extract the JSON value as a boolean. Returns `Null` if not a boolean. |
| `.array_len()` | Json | Int | Get the length of a JSON array. Returns `0` if the value is not an array. |
| `.keys()` | Json | List&lt;String&gt; | Get the keys of a JSON object as a `List` of strings. Returns an empty `List` if not an object. |
| `.get(key_expr)` | Json | Json/Null | Access an object field by a dynamically-evaluated key expression. Unlike `.ptr()`, the key can be a variable or any expression. Returns `Null` if the field is absent or the target is not an object. |
| `.find(key_expr, value_expr)` | Json | Json/Null | Search a JSON array for the first element (object) where `element[key] == value`. Both `key` and `value` are expressions evaluated to strings. Returns `Null` if no match is found or the target is not an array. |
| `.json_fold()` | Json | Json | Reduce all elements of a JSON array into a single merged value. Objects are merged by key (later keys win); arrays are concatenated. E.g. `[{"en":"A"},{"ja":"B"}]` → `{"en":"A","ja":"B"}`. Returns the target unchanged if it is not an array. |
| `.coalesce_keys([key1, key2, ...])` | Json | String/Null | Try each key expression in order via `.get(key).str()`, returning the first non-null string value. Equivalent to chained `.get(k1).str().fallback(.get(k2).str()).fallback(...)`. Keys can be literals, `pref()` calls, variables, or any expression returning a string. **Rust builder convenience only** — in the text DSL, write the equivalent `.get().str().fallback()` chain directly. |

#### User Script Methods

Pure functions declared in `scripts.pure:` (§3.10) are callable from any DSL expression via `.user.<name>(args...)`.
Each entry is Rhai source that must define a function with the same name as its key; the host
calls that function by name. The same definitions are also prepended to every hook body, so hooks
can call them too.

| Syntax | Description |
|--------|-------------|
| `.user.<name>()` | Call the named pure function with no arguments. Receiver (the value before the dot) is passed as the first argument. |
| `.user.<name>(arg1, arg2, ...)` | Call with additional arguments. Each argument is a DSL expression evaluated before the call. |

**Null propagation:** If the receiver evaluates to `Null`, the call returns `Null` without invoking
the Rhai function. A `Null` in any other argument is passed to the script as `()`.

**Constraints:** Pure functions use the Rhai sandbox described in §3.10 but cannot access `req`,
`ctx`, or `HookAction` constructors. Supported input and output types are `String`, `Int`, `Number`,
`Bool`, `List<String>`, and `Null`; other types cause a runtime error.

**Example:**

```yaml
scripts:
  pure:
    slugify: |
      fn slugify(title) {
        let slug = title.to_lower();
        slug.replace(" ", "-");
        slug
      }

endpoints:
  popular:
    fields:
      slug: 'self.first("h2").text().user.slugify()'
```

### 1.6 Examples

**Simple attribute extraction:**
```
self.attr("href").split("/").at(2)
```

**Multi-step with variable binding:**
```
let $base = dom("meta[property='og:url']").attr("content").split("/manga").at(0);
self.first("img.cover").attr("src").prepend($base)
```

**Status mapping:**
```
dom("div.status").text().trim().lower().lookup({
  "publishing": "ongoing",
  "finished": "completed",
  "on hiatus": "hiatus",
  "discontinued": "cancelled"
}).fallback("unknown")
```

**Chapter number parsing with fallback:**
```
self.text().trim().split(" ").at(-1).parse_float().fallback(0.0)
```

**Date parsing:**
```
self.attr("datetime").date_parse_rfc3339()
```

**Regex capture (group 1):**
```
self.text().capture("Chapter\s+(\d+(?:\.\d+)?)").at(1).parse_float().fallback(0.0)
```

**Collecting text from child elements (iteration):**
```
self.children().map($item.text().trim())
```

**Flattening nested lists (flat iteration):**
```
self.children().flat_map($item.children().map($item.attr("href")))
```

**Summing parsed numbers (fold):**
```
self.select("td.price").map($item.text().trim().parse_float()).fold(0.0, $acc + $item)
```

**Checking all items satisfy a condition (fold):**
```
self.select("input.required").map($item.attr("value").matches(".+")).fold(true, $acc && $item)
```

**Conditional based on page state:**
```
if dom("span.status").text().trim().lower().matches("adult") then "nsfw" else "safe"
```

**Collecting tags as a comma-joined string:**
```
self.select("a.tag").map($item.text().trim()).filter($item.matches("[^\s]")).join(", ")
```

**List literal for static values:**
```
["Ongoing", "Completed", "Hiatus"].at(0)
```

**Number to string for URL construction:**
```
let $n = dom("span.count").text().trim().parse_int().fallback(0);
"/api/items?count=".append($n.to_string())
```

**Find first matching element in a JSON array:**
```
json("/data/relationships").find("type", "cover_art").ptr("/attributes/fileName").str()
```

**Merging tags from multiple selectors into one list:**
```
merge([
  self.select("ul li:first-child a").map($item.text().trim()),
  self.select("ul li:nth-child(2) a").map($item.text().trim()),
  self.select("ul li:nth-child(3) span").map($item.text().trim())
])
```

**URL construction with format:**
```
format("https://cdn.example.com/covers/{}/{}.jpg", json("/manga_id").str(), json("/filename").str())
```

**Reading a preference value:**
```
json("/data/attributes/title").get(pref("language")).str().fallback(json("/data/attributes/title/en").str())
```

**Coalescing over a localised JSON object (try multiple keys in order):**
```
json("/data/attributes/title").get(pref("language")).str()
  .fallback(json("/data/attributes/title").get("en").str())
  .fallback(json("/data/attributes/title").get("ja-ro").str())
  .fallback(json("/data/attributes/title").get("ja").str())
```

In the Rust builder this pattern is available as `.coalesce_keys([Expr::pref("language"), Expr::lit("en"), Expr::lit("ja-ro"), Expr::lit("ja")])` on any `Json`-typed expression.

**Merging title objects from multiple sources (Rust builder only):**
```rust
Expr::json_array(vec![
    Expr::self_ref().ptr("/attributes/altTitles").json_fold(), // [{en:"A"}, {ja:"B"}] → {en:"A",ja:"B"}
    Expr::self_ref().ptr("/attributes/title"),                  // {en:"Primary Title"}
])
.filter(Expr::var("$item").ne(Expr::null()))
.json_fold()                                                    // merge all objects into one
.coalesce_keys(["en", "ja-ro", "ja"].map(Expr::lit))           // pick best available key
.fallback_str("Unknown Title")
```
`json_array([...])` constructs a `Json` array of `Json` values (unlike `[...]` which produces a `List`). This allows `.json_fold()` to merge the objects and `.coalesce_keys()` to pick the first non-null string across preferred keys.

**Boolean negation:**
```
if dom("span.is-completed").text().trim().matches("Completed").not() then "ongoing" else "completed"
```

**String length check:**
```
if dom("p.description").text().string_len() > 0 then dom("p.description").text() else null
```

**Split into at most 2 parts (everything after the first `/`):**
```
self.attr("href").split_n("/", 2).at(1)
```

**First 5 chapters of a list:**
```
self.select("a.chapter").map($item.text().trim()).take(5)
```

**Drop the pinned first entry and sort the rest by chapter number:**
```
self.select("a.chapter").map($item.text().trim().parse_float()).skip(1).sort_by($item)
```

**Deduplicate tags extracted from multiple elements:**
```
merge([
  self.select("a.genre").map($item.text().trim()),
  self.select("a.category").map($item.text().trim())
]).unique()
```

**URL-encode a search query for manual URL construction:**
```
let $q = dom("input#search").attr("value").url_encode();
format("https://example.com/search?q={}", $q)
```

**Right-pad a chapter number display string to 6 characters:**
```
self.text().trim().format_padded(6, " ", "right")
```

**Read a document-level scalar inside a per-element expression:**
```
scalar("base_url").append(self.attr("href"))
```

---

## 2. JSON Intermediate Model (IM)

The JSON IM is the serialized representation of the `Expr` AST. It is what gets embedded in compiled WASM extensions and transmitted across the FFI boundary as part of a blueprint. It uses a tagged-object format for clarity and debuggability.

### 2.1 Encoding Rules

Each `Expr` node is encoded as a JSON object with a `"op"` field identifying the node type, plus fields specific to that type. Nested expressions are encoded recursively.

### 2.2 Node Encodings

#### Leaf Nodes

```json
{ "op": "self" }
```

```json
{ "op": "dom", "selector": ".title" }
```

```json
{ "op": "json", "pointer": "/data/title" }
```

```json
{ "op": "var", "name": "$base" }
```

```json
{ "op": "lit", "value": "https://example.com" }
```

```json
{ "op": "num", "value": 3.14 }
```

```json
{ "op": "null" }
```

```json
{ "op": "bool", "value": true }
```

```json
{ "op": "index" }
```

#### DOM Operations

```json
{ "op": "attr", "target": { "op": "self" }, "name": "href" }
```

```json
{ "op": "text", "target": { "op": "self" } }
```

```json
{ "op": "inner_html", "target": { "op": "self" } }
```

```json
{ "op": "select", "target": { "op": "self" }, "selector": "img.cover" }
```

```json
{ "op": "first", "target": { "op": "self" }, "selector": "img.cover" }
```

```json
{ "op": "has_class", "target": { "op": "self" }, "class": "active" }
```

```json
{ "op": "children", "target": { "op": "self" } }
```

#### List Operations

```json
{ "op": "at", "target": { ... }, "index": 2 }
```

`"index"` may be negative: `-1` is the last element. Returns `Null` if out of bounds.

#### String Operations

```json
{ "op": "split", "target": { ... }, "delimiter": "/" }
```

Always returns a `List<String>`. Use `.at(n)` to extract a specific segment.

```json
{ "op": "replace", "target": { ... }, "from": "Chapter ", "to": "" }
```

```json
{ "op": "trim", "target": { ... } }
```

```json
{ "op": "lower", "target": { ... } }
```

```json
{ "op": "prepend", "target": { ... }, "prefix": { "op": "var", "name": "$base" } }
```

```json
{ "op": "append", "target": { ... }, "suffix": { "op": "lit", "value": ".jpg" } }
```

```json
{ "op": "matches", "target": { ... }, "pattern": "^Chapter" }
```

```json
{ "op": "capture", "target": { ... }, "pattern": "Chapter\\s+(\\d+)" }
```

```json
{ "op": "slice", "target": { ... }, "start": 0, "end": 5 }
```

`"end"` is optional; omit to slice to the end of the string. Negative values count from the end.

```json
{ "op": "starts_with", "target": { ... }, "prefix": "Chapter" }
```

```json
{ "op": "ends_with", "target": { ... }, "suffix": ".jpg" }
```

#### Coercion

```json
{ "op": "parse_float", "target": { ... } }
```

```json
{ "op": "parse_int", "target": { ... } }
```

```json
{ "op": "to_string", "target": { ... } }
```

#### Control Flow

```json
{
  "op": "let",
  "name": "$base",
  "value": { "op": "attr", "target": { "op": "dom", "selector": "meta" }, "name": "content" },
  "body": { "op": "prepend", "target": { "op": "attr", "target": { "op": "self" }, "name": "src" }, "prefix": { "op": "var", "name": "$base" } }
}
```

```json
{ "op": "fallback", "target": { ... }, "default": { "op": "lit", "value": "unknown" } }
```

```json
{
  "op": "lookup",
  "target": { ... },
  "entries": [
    ["publishing", "ongoing"],
    ["finished", "completed"],
    ["on hiatus", "hiatus"]
  ]
}
```

```json
{ "op": "list", "items": [{ ... }, { ... }, { ... }] }
```

```json
{ "op": "concat", "parts": [{ ... }, { ... }, { ... }] }
```

```json
{ "op": "join", "target": { ... }, "delimiter": ", " }
```

```json
{ "op": "if", "condition": { ... }, "then": { ... }, "else": { ... } }
```

```json
{
  "op": "map",
  "target": { "op": "children", "target": { "op": "self" } },
  "body": { "op": "text", "target": { "op": "var", "name": "$item" } }
}
```

`"target"` must evaluate to a `List`. The `"body"` expression is evaluated once per element with `$item` bound to the current element and `$index` bound to its 0-based position. `Null` results are dropped. Returns a `List`.

```json
{
  "op": "flat_map",
  "target": { "op": "children", "target": { "op": "self" } },
  "body": { "op": "children", "target": { "op": "var", "name": "$item" } }
}
```

Like `"map"`, but each body evaluation must return a `List`; all result lists are concatenated into a single flat `List`.

```json
{
  "op": "filter",
  "target": { "op": "dom", "selector": "a.chapter-link" },
  "predicate": { "op": "binop", "kind": "==", "lhs": { "op": "has_class", "target": { "op": "var", "name": "$item" }, "class": "active" }, "rhs": { "op": "lit", "value": "true" } }
}
```

`"predicate"` is evaluated for each element with `$item` and `$index` in scope. Elements where the predicate returns `false` or `Null` are dropped. Must return `Bool`.

```json
{
  "op": "fold",
  "target": { "op": "children", "target": { "op": "self" } },
  "base": { "op": "num", "value": 0 },
  "body": { "op": "binop", "kind": "+", "lhs": { "op": "var", "name": "$acc" }, "rhs": { "op": "var", "name": "$item" } }
}
```

Left fold. `"base"` is evaluated once to produce the initial accumulator. For each element, `"body"` is evaluated with `$acc` bound to the running accumulator, `$item` to the current element, and `$index` to its 0-based position. The result becomes the new `$acc`.

#### Binary Operators

```json
{ "op": "binop", "kind": "+",  "lhs": { ... }, "rhs": { ... } }
{ "op": "binop", "kind": "-",  "lhs": { ... }, "rhs": { ... } }
{ "op": "binop", "kind": "*",  "lhs": { ... }, "rhs": { ... } }
{ "op": "binop", "kind": "/",  "lhs": { ... }, "rhs": { ... } }
{ "op": "binop", "kind": "==", "lhs": { ... }, "rhs": { ... } }
{ "op": "binop", "kind": "!=", "lhs": { ... }, "rhs": { ... } }
{ "op": "binop", "kind": "<",  "lhs": { ... }, "rhs": { ... } }
{ "op": "binop", "kind": ">",  "lhs": { ... }, "rhs": { ... } }
{ "op": "binop", "kind": "<=", "lhs": { ... }, "rhs": { ... } }
{ "op": "binop", "kind": ">=", "lhs": { ... }, "rhs": { ... } }
{ "op": "binop", "kind": "&&", "lhs": { ... }, "rhs": { ... } }
{ "op": "binop", "kind": "||", "lhs": { ... }, "rhs": { ... } }
```

`&&` and `||` short-circuit: `rhs` is not evaluated if `lhs` determines the result.

#### Date Operations

```json
{ "op": "date_parse", "target": { ... }, "format": "[year]-[month]-[day]" }
```

```json
{ "op": "date_parse_rfc3339", "target": { ... } }
```

#### JSON Operations

```json
{ "op": "json_ptr", "target": { ... }, "pointer": "/attributes/title" }
```

```json
{ "op": "json_str", "target": { ... } }
```

```json
{ "op": "json_int", "target": { ... } }
```

```json
{ "op": "json_float", "target": { ... } }
```

```json
{ "op": "json_bool", "target": { ... } }
```

```json
{ "op": "array_len", "target": { ... } }
```

```json
{ "op": "keys", "target": { ... } }
```

Returns the keys of a JSON object as a `List<String>`. Returns an empty `List` if the target is not an object.

```json
{ "op": "json_get", "target": { ... }, "key": { "op": "var", "name": "$lang" } }
```

```json
{ "op": "json_find", "target": { ... }, "key": { "op": "lit", "value": "type" }, "value": { "op": "lit", "value": "cover_art" } }
```

```json
{ "op": "json_fold", "target": { ... } }
```

```json
{ "op": "json_array", "items": [{ ... }, { ... }] }
```

Constructs a `Json` array from N evaluated expressions. Unlike `{ "op": "list" }` (which produces a `List` value), `json_array` produces a `Json` value that supports `.json_fold()`, `.filter()`, and other JSON-native operations. **Rust builder only** — not directly parseable from the text DSL.

#### Boolean Operations

```json
{ "op": "not", "target": { ... } }
```

`"target"` must evaluate to `Bool` or `Null`. `Null` is treated as `false`.

#### String Utilities

```json
{ "op": "string_len", "target": { ... } }
```

Returns the Unicode character count of the string as an `Int`.

```json
{ "op": "format", "template": "Hello {}, you have {} items", "args": [{ ... }, { ... }] }
```

Interpolates `{}` placeholders with the evaluated string arguments in order.

#### Preference Access

```json
{ "op": "pref", "key": "cover_size" }
```

Reads the extension preference named `key`. Returns `String` or `Null` if unset.

#### List Merging

```json
{
  "op": "merge",
  "lists": [
    { "op": "select", "target": { "op": "self" }, "selector": "ul:first-child a" },
    { "op": "select", "target": { "op": "self" }, "selector": "ul:nth-child(2) a" }
  ]
}
```

Concatenates multiple lists. Each element of `"lists"` must evaluate to a `List`.

#### URL Operations

```json
{ "op": "resolve_url", "target": { ... }, "base": { "op": "lit", "value": "https://example.com" } }
```

#### DSL v2 List Operations

```json
{ "op": "split_n", "target": { ... }, "delimiter": "/", "n": 2 }
```

```json
{ "op": "take", "target": { ... }, "n": 5 }
```

```json
{ "op": "skip", "target": { ... }, "n": 1 }
```

```json
{ "op": "reverse", "target": { ... } }
```

```json
{ "op": "sort_by", "target": { ... }, "key": { "op": "var", "name": "$item" } }
```

`"key"` is evaluated per element with `$item` bound to the current element. Numeric types sort numerically; `String` lexicographically; `Bool` false&lt;true; non-comparable types (`Null`, `Element`, `Json`, `List`) sort to the end, stable.

```json
{ "op": "unique", "target": { ... } }
```

#### DSL v2 String Operations

```json
{ "op": "url_encode", "target": { ... } }
```

```json
{ "op": "url_decode", "target": { ... } }
```

```json
{ "op": "format_padded", "target": { ... }, "width": 6, "fill": " ", "align": "right" }
```

`"align"` is one of `"left"`, `"right"`, `"center"`.

#### DSL v2 Scalar Access

```json
{ "op": "scalar", "name": "base_url" }
```

Reads the named value from the document-level `scalars` map, computed before per-element iteration begins. The scalar must be declared in the `scalars` section of the blueprint.

#### Composite ID Encoding (`encoded_field`)

```json
{
  "op": "encoded_field",
  "subfields": [["manga_id", { "op": "var", "name": "$id" }], ["ch_id", { ... }]],
  "delimiter": "|",
  "encoding": "Base64Url"
}
```

Evaluates each subfield expression to a string, joins the results with `delimiter`, then encodes the concatenated value with `encoding`. The result is a single string suitable for use as a composite identifier.

Decoding splits on the first `n - 1` delimiters, so only the **last** subfield may contain the delimiter. If any earlier subfield contains it, evaluation fails with an `encoded_field` error rather than producing an ID that would decode into the wrong parts. Order `fields` so a free-text value such as a slug comes last, or choose a delimiter the source never emits.

`"encoding"` is one of:

| Value | Encoding |
|-------|----------|
| `"Base64Url"` | URL-safe Base64 without padding |
| `"Base64"` | Standard Base64 |
| `"Passthrough"` | No encoding; the joined string is returned as-is |
| `"Hex"` | Hexadecimal encoding of the UTF-8 bytes |

`"subfields"` is an array of `[field_name, expr]` pairs. Field names are used as keys when decoding the composite ID back into its constituent parts (the inverse is `decode_composite`). Available via the Rust builder as `Expr::encoded_field(subfields, delimiter, IdEncoding::Base64Url)`.

#### Sub-blueprint Fetch (Rust builder only)

```json
{
  "op": "fetch",
  "url_expr": { ... },
  "blueprint": { ... },
  "method": "Get",
  "headers": [],
  "kind": "Json",
  "endpoint_id": "manga_details/chapters"
}
```

`"method"` is one of `"Get"`, `"Post"`, `"Put"`, `"Delete"`. `"kind"` is `"Html"` or `"Json"` and determines how the fetched response is parsed before the sub-blueprint is applied. The sub-blueprint is a full blueprint object (§2.3). The host evaluates `url_expr`, fetches the URL (subject to the SSRF `AllowedHost` gate and the per-extension I/O budget), and returns the first row of the sub-extraction as a `Json` value, or `Null` if the result is empty. Nesting `fetch` inside another fetch's sub-blueprint is rejected at evaluation time.

`"endpoint_id"` is an optional string identifying the logical source endpoint that owns this sub-fetch. Codegen sets it automatically to `"<parent_endpoint>/<merge_as>"` for `then:` and `for_each:` steps (e.g. `"manga_details/chapters"`). It is exposed to hooks as `req.endpoint_id`, and a sub-fetch runs its parent endpoint's per-endpoint hooks (§3.10). Available in the Rust builder via `Expr::fetch_html(url_expr, blueprint)` / `Expr::fetch_json(...)` followed by `.with_endpoint_id(id)`.

#### User Function Call

Calls a pure function registered under `scripts.pure:` by name. Produced by the text DSL when `.user.<name>(args...)` syntax is encountered; not emittable as raw JSON (the `user_fn` variant is a DSL-parser artifact, not a hand-authored node).

```json
{
  "op": "user_fn",
  "name": "slugify",
  "args": [{ "op": "self" }]
}
```

`"name"` matches a key in `ExtensionMetadata.scripts.pure`. `"args"` is the evaluated argument list; the receiver (the value before `.user.`) is always prepended as `args[0]` by the parser. Null propagation applies to the receiver only: if `args[0]` is `Null`, the host returns `Null` without calling the function; any other `Null` argument is passed to the script as `()`. The function runs in the Rhai pure sandbox (§3.10 sandbox limits).

### 2.3 Blueprint Encoding

A complete blueprint is a JSON object with the following fields:

| Field | Type | Description |
|-------|------|-------------|
| `request` | object/null | HTTP request definition — `url`, `method`, `headers`, `queries`. Omit when an existing document handle is passed instead. |
| `container` | string | CSS selector (HTML) or JSON Pointer (JSON) for the repeating container. Use `":root"` (HTML) for a single-element HTML container. For JSON: use `""` to target the document root, or a JSON Pointer like `"/data"` to target a nested value. If the resolved JSON value is an array it is iterated element-by-element; any other JSON type (object, string, etc.) is treated as a single-item container — useful for detail endpoints where the document itself is the row. |
| `fields` | array | Field definitions extracted per container element. Each has `name`, `expr`, `optional`. |
| `bindings` | array | Document-level variable bindings evaluated once before iteration. Each has `name` and `expr`. |
| `scalars` | array | Document-level output values evaluated once (not per-element). Same shape as `fields` — each entry has `name`, `expr`, and `optional`. When `optional: true`, a `Null` result is included in the output as JSON `null` rather than causing an error. Returned in the `scalars` map of the output alongside `rows`. |
| `pagination` | object/null | When set, enables `paginated-extract-html` mode. See Pagination Config below. |

**Output format**: `{ "rows": [{...}, ...], "scalars": {"key": value, ...} }`

**Example:**

```json
{
  "request": {
    "url": "https://example.com/search/data",
    "method": "GET",
    "headers": [],
    "queries": [["display_mode", "Full Display"], ["sort", "Popularity"]]
  },
  "container": "body > article",
  "fields": [
    {
      "name": "id",
      "expr": { "op": "at", "target": { "op": "split", "target": { "op": "attr", "target": { "op": "first", "target": { "op": "self" }, "selector": "a.line-clamp-1" }, "name": "href" }, "delimiter": "/" }, "index": -2 },
      "optional": false
    },
    {
      "name": "title",
      "expr": { "op": "text", "target": { "op": "first", "target": { "op": "self" }, "selector": "a.line-clamp-1" } },
      "optional": false
    },
    {
      "name": "cover_url",
      "expr": { "op": "attr", "target": { "op": "first", "target": { "op": "self" }, "selector": "img" }, "name": "src" },
      "optional": true
    }
  ],
  "bindings": [],
  "scalars": [
    {
      "name": "has_next_page",
      "expr": { "op": "matches", "target": { "op": "text", "target": { "op": "dom", "selector": ".col-span-2" } }, "pattern": ".+" },
      "optional": false
    }
  ],
  "pagination": {
    "native_page_size": 32,
    "offset_param": "offset",
    "offset_type": "ItemOffset"
  }
}
```

**Pagination Config** (`pagination` field):

| Field | Type | Description |
|-------|------|-------------|
| `native_page_size` | integer | How many items the source returns per chunk (its real page size). |
| `offset_param` | string | Query parameter name the source uses for the offset/page (e.g. `"offset"`, `"page"`). |
| `offset_type` | string/object | `"ItemOffset"` (param = absolute item count: 0, 32, 64, …), `{"PageNumber": {"start": 1}}` (param = page number starting at `start`), or `{"CursorToken": {"next_cursor_field": "/next"}}` (JSON Pointer to the cursor field in each chunk's response). |

When `pagination` is set, the blueprint must be submitted via `paginated-extract-html` / `paginated-extract-json` rather than `extract-html` / `extract-json`. The host handles chunk-fetching, stitching, and `has_next_page` detection automatically.

**`CursorToken` mode:** The host reads the cursor value from `next_cursor_field` (a JSON Pointer into the chunk response) after each fetch, injects it as the `offset_param` query value on the next request, and stops when the field is absent or `null`. Use this for APIs that return a next-page token rather than a numeric offset (an API exposing `offset`+`total` can also be expressed this way, but opaque-token APIs require it).

### 2.4 Binary Encoding

Blueprints are serialized with **[`postcard`](https://docs.rs/postcard)** (a compact binary format) for the FFI call across the WASM boundary. The `Expr` enum's `serde` derives handle this transparently. Call `blueprint.to_bytes()` (from `BlueprintBuilder::build()`) to get the postcard bytes; the host deserializes via `postcard::from_bytes(&blueprint)`.

**DSL schema versioning.** The binary payload is prefixed with a `u32` schema version (`DSL_SCHEMA_VERSION` constant in `kani-shared/src/ast.rs`). `decode_blueprint` on the host accepts the versions listed as readable below and rejects any other with a human-readable "recompile the extension" error rather than an opaque decode failure.

| Version | Change |
|---------|--------|
| 1 | Original blueprint encoding (no version prefix). |
| 2 | Added `SplitN`, `Take`, `Skip`, `Reverse`, `SortBy`, `Unique`, `UrlEncode`, `UrlDecode`, `FormatPadded`, `ScalarOverride`, `Fetch` variants. Version prefix introduced. |
| 3 | Added `Expr::UserFn { name, args }` for pure-script bridge (§3.10). |
| 4 | Added `endpoint_id: Option<String>` to `RequestDef` for per-endpoint hook dispatch (§3.10). |
| 5 | Added `endpoint_id: Option<String>` to `Expr::Fetch` so sub-fetches (`then:` / `for_each:` steps) participate in per-endpoint hook dispatch. |
| 6 | Added `Expr::Arena`, flat storage for large expressions. Appended as a new variant, so version 5 payloads still decode. |

The current version is **6**. The host reads versions **5 and 6**; versions 1–4 are rejected.

**Compatibility rule.** A WASM extension depends on the host in two independent ways, and each
has its own rule. Both are checked when an artifact is installed, reloaded, and loaded at startup,
so an incompatible extension is refused up front instead of failing on its first request.

- *Blueprint format.* postcard is not self-describing: appending a variant to an enum leaves
  older payloads decodable, but adding, removing, or reordering a field or variant does not.
  Within 1.x, a version bump may only append enum variants, and the host keeps reading every
  version from 5 onwards. A change that cannot be expressed that way needs a new variant, not a
  changed one. The extension's metadata records the version it was built with
  (`dsl_schema_version`); install and reload refuse an unreadable one, and at startup it is
  registered as a load degradation for that source. `decode_blueprint` still checks the prefix on
  every extraction, which covers extensions built before the version was recorded.
- *WIT imports.* The `kani:extension` world is unversioned. Within 1.x it only grows: new
  functions and interfaces may be added, and an existing function's name, parameters, and results
  never change and are never removed. An extension built against an older world therefore still
  links. Every path links the component against the host's imports before running it, so an
  extension that imports something this host lacks (built for a newer Kani) is refused with the
  linker's error, never started.

The JSON IM described in §2.2 reflects the logical structure of the AST and is useful for debugging; the wire format is binary, not JSON.

---

## 3. YAML Extension Format

The YAML format is the developer-facing representation of a kani extension. It is compiled to Rust source code by `kani-cli`.

### 3.1 Top-Level Structure

A key the schema does not define is an error, reported with the field's name, its line, and
the keys allowed there, so a misspelt key cannot silently leave its setting without effect. The
only exception is the retired `for_each.concurrency` (§3.2).

<!-- schema sketch: not an executable example -->
```yaml
# === Required metadata ===
id: string              # Unique extension identifier (lowercase, alphanumeric + hyphens)
name: string            # Human-readable display name
version: string         # Semantic version (e.g., "0.1.0")
base_url: string        # Base URL for the source website
language: string        # ISO 639-1 code or "multi" (default: "en")

# === Optional metadata ===
nsfw: bool              # Whether the source contains NSFW content (default: false)
unrestricted_http: bool # Whether the extension needs to contact external hosts (default: false)

# === Schema/compatibility versioning ===
schema_version: integer            # YAML schema version this file targets (default: current; error if newer than this kani-cli supports)
min_kani_version: string           # Optional semver floor on the host version required to install this extension
requires_capabilities: [string]    # Optional list of host capability flags this extension requires

# === Extended metadata (icon, rate limiting, languages, sections, ...) ===
metadata: MetadataBlock

# === Endpoint definitions ===
endpoints:
  popular: PopularEndpoint
  search: SearchEndpoint
  manga_details: DetailsEndpoint
  chapter_list: ChapterListEndpoint
  pages: PagesEndpoint

# === Canonical manga URL (optional) ===
get_url: string         # URL template for a manga's page on the source site.
                        # Use `$manga_id$` as the placeholder. Without it the
                        # host cannot produce an "open on source site" link.

# === Optional sections ===
filters: FilterList
preferences: PreferenceList
option_sets: OptionSetMap   # Named, reusable option lists referenced via `options_ref`
id_encoding: IdEncodingBlock  # Composite ID encode/decode for manga and/or chapter IDs
cache: CacheMap              # Cache namespaces its hook scripts may use (§3.2)
chapter_sort: ChapterSortBlock # Optional. Chapter sort options exposed to the host.

# === Scripting (optional) ===
scripts:
  pure:                        # Named pure Rhai functions callable from the DSL via `.user.<name>()`
    <name>: string             # Rhai source defining `fn <name>(...)`; also shared with hooks

pre_request: string            # Top-level Rhai hook body: runs before every HTTP request (§3.10)
on_status:                     # Top-level Rhai hook bodies keyed by status pattern (§3.10)
  "401": string
  "5xx": string
  "default": string
```

### 3.2 Endpoint Types

Each endpoint corresponds to a method in the `manga-provider` WIT interface. The endpoint defines how to construct the HTTP request and how to extract data from the response.

#### Common Endpoint Fields

<!-- schema sketch: not an executable example -->
```yaml
endpoint_name:
  # --- Request construction ---
  route: string           # URL path appended to base_url. Supports {variable} templates.
  method: string          # HTTP method (default: "GET")
  headers:                # Additional headers (optional)
    Header-Name: value
  queries:                # Query parameters (optional)
    param: value          # Static value
    param: $variable$     # Dynamic value from function arguments
  type: string            # Response type: "html" (default) or "json"

  # --- Extraction ---
  container: string       # CSS selector (HTML) or JSON Pointer (JSON) for the list container
  bindings:               # Top-level variable bindings evaluated before iteration (optional)
    $var_name: "dsl expression"
  fields:                 # Field extractions from each container element
    field_name: "dsl expression"
    field_name:
      expr: "dsl expression"
      optional: true
  scalars:                # Document-level output values evaluated once before iteration (optional)
    scalar_name: "dsl expression"
    scalar_name:
      expr: "dsl expression"
      optional: true
```

#### Variable Interpolation

Inside `route`, `queries`, and `headers`, values wrapped in `$...$` are replaced with function arguments:

| Variable | Available In | Description |
|----------|-------------|-------------|
| `$query$` | search | The search query string |
| `$page$` | popular, search, chapter_list | The page number |
| `$page_size$` | popular, search, chapter_list | The requested page size |
| `$manga_id$` | manga_details, chapter_list, pages | The manga identifier |
| `$chapter_id$` | pages | The chapter identifier |
| `$pref:key$` | any | Value of a user preference |
| `$<role>.<field>$` | manga_details, chapter_list, pages | Subfield of a composite ID declared in `id_encoding.<role>` (see [Composite ID Encoding](#composite-id-encoding) below) |

#### Composite ID Encoding

Some sources expose IDs that are naturally composed of multiple sub-values (e.g. a manga "hid" plus a slug, or a chapter id plus a language code). The top-level `id_encoding` block declares how a multi-field composite ID is packed into the single string the client sees, and how to unpack it again when building requests.

```yaml
id_encoding:
  manga:
    fields: [hid, slug]
    delimiter: "|"          # Default: "|"
    encoding: base64_url    # base64_url (default) | base64 | passthrough | hex
  chapter:
    fields: [hid]
    encoding: passthrough
```

`encoding` controls how the delimiter-joined field string is encoded for transport:

| Value | Description |
|-------|-------------|
| `base64_url` | URL-safe base64, no padding (default) |
| `base64` | Standard base64 |
| `passthrough` | No encoding — the joined string is used as-is |
| `hex` | Hex-encoded |

**Encoding (building a composite ID field):** in `popular`/`search`/`manga_details` use role `manga`; in `chapter_list` use role `chapter`. Instead of a single DSL string, give `id` (or any field) a map of subfield name → DSL expression, with keys matching exactly the declared `fields` list for that role:

```yaml
fields:
  id:
    hid: 'self.attr("data-hid")'
    slug: 'self.attr("data-slug")'
```

This compiles to an `encoded_field` expression that evaluates each subfield, joins the results with `delimiter`, and encodes the joined string per `encoding`. Only the last field may contain `delimiter`; see [`encoded_field`](#composite-id-encoding-encoded_field).

**Decoding (unpacking a composite ID in a route or query):** reference an individual subfield with a dotted placeholder `$<role>.<field>$`, e.g.:

```yaml
chapter_list:
  route: "/manga/$manga.hid$/$manga.slug$"
```

Codegen decodes the incoming `manga_id`/`chapter_id` function argument once at the top of the method and binds one local per referenced subfield (sanitized to `<role>_<field>`, since `.` is not a valid Rust identifier character). Only roles actually referenced by an endpoint's `route`/`queries` incur a decode call.

#### Cache Namespaces

The top-level `cache` block declares the namespaces an extension's hook scripts may store values
in (§3.10). A hook that names any other namespace fails with a script error, so the number of
namespaces an extension can create, and with it its total cache storage, is bounded by what it
declares.

```yaml
cache:
  auth:
    ttl: 3600            # Seconds; the default and maximum entry lifetime. Default 3600, max 30 days
  search_results:
    ttl: 1800
    max_entries: 200     # Optional; lowers the host's per-namespace limit of 4096
```

Validation rules: at most 16 namespaces; names are non-empty and contain no `:` or `/`;
`ttl` ≤ 30 days; `max_entries` between 1 and 4096. `ttl: 0` sets no maximum.

Declarations reach the host through `ExtensionMetadata.cache` (and directly from an interpreted
YAML source), so they apply to generated WASM extensions too.

**Caches are instance-wide.** Kani's source preferences, including `secret: true` credentials,
belong to the source, not to a user, and background work such as scans and downloads runs with
no user at all. An auth token a hook caches is therefore shared by every user of the source. That
is intended: all users of a source act as one account on the upstream site. Per-user source
accounts are a possible future feature, not a cache setting.

#### Extension Metadata

The top-level `metadata` block carries the parts of an extension's identity that aren't required to construct a request or run extraction — icon, rate limiting, supported languages, a description, and content sections — plus the top-level `schema_version`/`min_kani_version`/`requires_capabilities` fields that gate installation:

```yaml
schema_version: 1                  # Default: current schema version kani-cli supports
min_kani_version: "0.5.0"          # Optional semver floor; install is rejected on older hosts
requires_capabilities:
  - "unrestricted_http"            # Optional; install is rejected if the host lacks a listed capability

metadata:
  icon: "<base64-encoded PNG/WebP/SVG, ≤ 64KB decoded>"
  rate_limit:
    rps: 2.0                       # Requests per second. Default: 2.0
    burst: 8                       # Default: 8
    max_concurrent: 4              # Default: 4
  languages:
    - "en"
    - "ja"
  description: "A short, human-readable description of this source."
  sections:
    - id: "latest"
      name: "Latest"
      nsfw: false                  # Default: false
```

All `metadata` fields are optional and additive — an extension YAML with no `metadata`/`schema_version`/`min_kani_version`/`requires_capabilities` keys at all continues to work unchanged. `metadata` is encoded into the generated `ExtensionMetadata` struct (`kani-shared/src/extension.rs`), which crosses the WIT boundary as a single JSON-encoded string returned by `get-metadata` — adding further fields to `ExtensionMetadata` in the future is a serde-only change and does not require touching the WIT interface.

##### Install-time gating and host persistence

`install_source` (`kani-web/src/rest/mod.rs`) decodes the `ExtensionMetadata` JSON string returned by `get-metadata` and, before persisting anything, runs two compatibility checks (`kani-web/src/install_gating.rs`):

- **`min_kani_version`** — parsed as semver and compared against the running host's version (`kani_web::KANI_VERSION`). Install is rejected if the host is older than the declared floor.
- **`requires_capabilities`** — each entry must appear in the host's `HOST_CAPABILITIES` allow-list (currently `["unrestricted_http"]`). Install is rejected if any requested capability is unrecognized.

Both checks return a `Result<(), String>`; failures are surfaced as `AppError::ValidationError` with a human-readable message naming the offending version/capability. Installation that passes gating persists `icon`, `description`, `languages` (JSON-encoded `Vec<String>`), and `schema_version` onto the `sources` table row, alongside the existing `name`/`version`/`base_url`/`unrestricted_http` columns (`migrations/20260818000002_baseline.sql`). These four columns are also added to `kani_shared::types::Source` and round-trip through `get_source`/`list_sources`/library scan queries. The frontend (`static/js/pages/source-details.js`, `static/js/components/sources-sidebar.js`) reads them directly off the `Source` object: the sidebar list item swaps the initial-letter avatar for the decoded `icon` (`data:image/png;base64,...`) when present, and the source details page's "About" card shows the icon, description, and a `languages` chip list (parsed from the JSON column).

#### Chapter Sort

The top-level `chapter_sort` block declares the sort options this extension exposes for chapter lists. When present, codegen emits a real `get_chapter_sort_list()` returning the declared options and, if `default` is set, overrides `default_chapter_sort()` on the extension struct. When absent, the stub `get_chapter_sort_list()` returns an empty vec.

```yaml
chapter_sort:
  default: "number_desc"   # Optional. Must match one of the declared option ids.
  options:
    - id: "number_desc"
      label: "Chapter (descending)"
    - id: "number_asc"
      label: "Chapter (ascending)"
    - id: "date_desc"
      label: "Date added"
```

Validation rules: `options` must be non-empty; each option `id` must be non-empty; `default`, when present, must name one of the declared option ids.

#### Endpoint Chaining (`then` / `for_each`)

Endpoints can chain sub-fetches to enrich their results without writing Rust. Two flavours:

- **`then`** — document-level (a single sub-fetch per main request). The result is bound as `$merge_as` in the blueprint's variable environment; subsequent field expressions can reference it.
- **`for_each`** — per-element (one sub-fetch per main-result row). The result is stored as the `merge_as` field in each row's JSON object.

```yaml
endpoints:
  search:
    route: "https://example.com/search"
    fields:
      id: 'self.first(".id").text()'
      title: 'self.first(".title").text()'
    for_each:
      - endpoint: manga_details    # Name of another declared endpoint; its blueprint is used.
        url_expr: 'self.first(".link").attr("href")'  # DSL expression (evaluated per-element for for_each).
        merge_as: details          # Output field / binding name.
        on_failure: skip           # "skip" | "fail" | "<dsl fallback expr>" (default: "fail").
    then:
      - endpoint: manga_details
        url_expr: 'dom(".banner-link").attr("href")'
        merge_as: banner_info
        on_failure: '""'           # DSL fallback expression on error.
```

**`on_failure`** controls what happens when the sub-fetch or extraction fails:
- `skip` — produce `null` for this field/binding; continue processing other rows.
- `fail` — propagate the error (default).
- `"<dsl expr>"` — any other string is treated as a DSL expression evaluated as a fallback value.

**`for_each` keeps only the sub-fetch's first row.** The sub-endpoint is extracted
normally, but the value stored as `merge_as` is its first row, not the whole list —
so a sub-endpoint whose container matches several elements silently contributes only
the first.

**`deduplicate_by`** is a DSL expression evaluated against each *main-result* row once
its sub-fetch has merged in; the row is a JSON object, so `self` and `json()` both address
it. Rows repeating an earlier row's key are dropped, the first
occurrence is kept, and the original order is preserved. Use it where a source lists the
same entry under several categories on one page and only the sub-fetch reveals they are
the same.

```yaml
    for_each:
      - endpoint: manga_details
        url_expr: 'self.first(".link").attr("href")'
        merge_as: details
        deduplicate_by: 'self.ptr("/details/canonical_id").str()'
```

It is applied by the host after extraction, so it is available to **interpreted YAML
sources only**. A generated Rust extension builds its blueprint in the guest, where the
key cannot be evaluated; `kani-cli generate` and factory `kani-cli build` reject a source that
sets it rather than emitting a crate that ignores it (§5).

Sub-fetch parallelism is not configurable per step. Every request a source makes,
including sub-fetches, is bounded by `metadata.rate_limit.max_concurrent` (default 4).
A `concurrency:` key on a `for_each` step is accepted and ignored: it was
documented and range-checked before 1.0 but never read, so honouring it now would
change behaviour for anyone who set it. Remove it from your source; use
`max_concurrent` to be gentle on a fragile host.

**Validation rules:**
- `endpoint` must name one of `popular`, `search`, `manga_details`, `chapter_list`, or `pages` declared in the same YAML.
- `merge_as` must be non-empty.
- `url_expr` must parse as a valid DSL expression.
- `deduplicate_by` (optional) must parse as a DSL expression.
- Nested chaining (a referenced endpoint that itself has `then`/`for_each` steps) is not evaluated — the sub-blueprint is built from the referenced endpoint's fields only.

#### PopularEndpoint

When the source has no distinct popular endpoint, use `delegate_to` to reuse another endpoint:

```yaml
popular:
  delegate_to: search
  empty_without_filters: true   # Optional: return empty list when no filters active
```

Otherwise define it as a full endpoint. JSON API example:

```yaml
popular:
  route: "/manga"
  queries:
    limit: $page_size$
    offset: "$page_size$ * ($page$ - 1)"
    includes[]: cover_art
    order[followedCount]: desc
  type: json
  container: "/data"
  scalars:
    has_next_page: 'json("/offset").int().fallback(0.0) + json("/limit").int().fallback(0.0) < json("/total").int().fallback(0.0)'
  fields:
    id: 'self.ptr("/id").str()'
    title: |
      self.ptr("/attributes/title").get(pref("language")).str()
        .fallback(self.ptr("/attributes/title/en").str())
        .fallback("Unknown Title")
    cover_url:
      expr: |
        let $filename = self.ptr("/relationships").find("type", "cover_art").ptr("/attributes/fileName").str();
        if $filename != null
          then format("https://cdn.example.com/covers/{}/{}{}", self.ptr("/id").str(), $filename, pref("cover_size").fallback(".512.jpg"))
          else null
      optional: true
```

#### SearchEndpoint

```yaml
search:
  route: "/search"
  queries:
    q: $query$
    page: $page$
  container: ".grid.gap-3 > div"
  fields:
    id: 'self.first("a").attr("href").split("/").at(2)'
    title: 'self.first(".line-clamp-2").text()'
    cover_url:
      expr: 'self.first("img").attr("data-src")'
      optional: true
```

#### DetailsEndpoint

The details endpoint does not iterate over a container. Instead, it extracts fields directly from the page. Set `container` to `":root"` (HTML) or `""` (JSON root) to select the document itself.

```yaml
manga_details:
  route: "/manga/$manga_id$"
  container: ":root"
  fields:
    id: '"$manga_id$"'   # Literal passthrough of the function argument
    title: 'dom("h1.font-bold").text()'
    description:
      expr: 'dom("p.text-sm").text()'
      optional: true
    status: |
      dom("div.grid:nth-child(3) > div:nth-child(2) > div:nth-child(2)").text().trim().lower().lookup({
        "publishing": "ongoing",
        "finished": "completed",
        "on hiatus": "hiatus",
        "discontinued": "cancelled",
        "not yet published": "hiatus"
      }).fallback("unknown")
    tags:
      expr: 'self.select("div.mb-3 a.text-sm").map($item.text().trim()).filter($item.matches("[^\s]")).join(", ")'
      optional: true
    cover_url:
      expr: 'dom("img").attr("data-src")'
      optional: true
```

#### ChapterListEndpoint

```yaml
chapter_list:
  route: "/manga/$manga_id$"
  container: "div.grid a.border"
  fields:
    id: 'self.attr("href").split("/").at(2)'
    number: 'self.text().trim().split(" ").at(-1).parse_float().fallback(0.0)'
    title:
      expr: "null"
      optional: true
    volume:
      expr: "null"
      optional: true
    scanlator:
      expr: "null"
      optional: true
    date_uploaded:
      expr: "null"
      optional: true
    language: '"en"'
  has_next_page: false    # Static value, or a DSL expression evaluated on the document
  total_pages: 12         # Optional. Static u32 or a DSL expression evaluated on the
                          # document. Populates `total_pages` on the returned
                          # MangaList/ChapterList; omit it when the source does not
                          # report a count and the host will rely on `has_next_page`.
```

#### PagesEndpoint

```yaml
pages:
  route: "/chapters/$chapter_id$"
  container: "img.js-page"
  fields:
    index: "index()"
    url: 'self.attr("data-src")'
    transform:                       # Optional. Names an image transform.
      expr: '"lcg-tile-5x5-from-header"'
      optional: true
```

**`transform`:** an optional per-page field naming a transform from the host's
transform registry (`kani_core::transform`), the declarative equivalent of a
compiled extension setting `Page.transform`. The name is carried to the image
proxy, which resolves it against the upstream response headers and applies it if
it resolves; an unknown name, or one whose parameters are absent from the
response, is a passthrough. An empty value counts as absent.

### 3.3 Pagination

Kani uses a `(page, page_size)` pagination model. Source websites may paginate differently — for example, a site might always return exactly 32 items per request regardless of the `limit` parameter. The `pagination` section (optional, per-endpoint) delegates the offset algebra to the framework:

```yaml
pagination:
  native_page_size: 32    # How many items the source returns per chunk
  offset_param: "offset"  # Query parameter name for the chunk offset
  offset_type: item        # "item" (0, 32, 64, …) or "page" with a start index
```

When `pagination` is set on an endpoint, the framework calls `paginated-extract-html` instead of `extract-html`. It automatically fetches as many chunks as needed to fulfil the client's `page_size`, injects the correct `offset_param` value per chunk, and determines `has_next_page`. Extensions do not need to implement pagination loops.

**`offset_type` values:**

| Value | Description |
|-------|-------------|
| `item` | Offset param = absolute item count: 0, 32, 64, … |
| `page` | Offset param = page number. Defaults to 1-based. Use `page_start: 0` for 0-based. |
| `cursor` | Cursor-token pagination. Set `cursor_field` to the JSON Pointer of the next-page token in the response (e.g. `cursor_field: "/next_cursor"`). The host injects the token as `offset_param` on each subsequent request and stops when the field is absent or null. |

**`has_next_page` detection:** If the blueprint includes a `scalars` entry named `has_next_page`, its value from the last fetched chunk is used. Otherwise the framework falls back to: last chunk was full (≥ `native_page_size` items) → more pages available.

For endpoints where the source supports arbitrary page sizes (the client's `page_size` is passed directly), omit `pagination` and use `$page$` / `$page_size$` in `queries` as before.

### 3.4 Filters Section

Filters map directly to the `filter_list!` macro output:

```yaml
filters:
  - id: "genre:Action"
    name: "Action"
    type: checkbox

  - id: "genre:Adventure"
    name: "Adventure"
    type: checkbox

  - id: type
    name: Type
    type: select
    options:
      - name: All
        value: ""
      - name: Manga
        value: manga
      - name: Manhua
        value: manhua
    default:
      name: All
      value: ""

  - id: status
    name: Status
    type: select
    options:
      - name: All
        value: ""
      - name: Ongoing
        value: publishing
    default:
      name: All
      value: ""

  - id: genres
    name: Genres
    type: multiselect
    options_ref: genres    # Resolve options from a shared `option_sets` entry instead of inlining

  - id: year_range
    name: Year range
    type: date_range       # Or int_range. Requires min and max.
    min: 1900
    max: 2100
```

**Filter kinds:** `checkbox`, `select`, `multiselect`, `sort`, `text_input`, `int_range`, `date_range`. The WIT `filter-state` surface has no dedicated range variant yet, so `int_range`/`date_range` filters are emitted as `TextInput` in `filter_list!`; pair them with a `tuple_split` filter mapping (below) to submit the range as two query parameters from a single colon-delimited text value (e.g. `"1900:2100"`).

**`name_i18n`:** optional alternate i18n key for the filter's display name, carried through validation but not yet consumed by codegen.

**Option sets (`options_ref`):** instead of inlining `options:` on every filter or preference, declare a named, reusable list at the top level under `option_sets` and reference it via `options_ref`:

```yaml
option_sets:
  genres:
    - name: Action
      value: action
    - name: Romance
      value: romance
      nsfw: true            # Optional per-option NSFW flag

  tags:
    options_fetched_by:      # Lazily resolved by the host at filter-panel render time
      route: "/api/tags"
      type: json              # "html" (default) or "json"
      container: "/tags"      # JSON Pointer (json) or CSS selector (html)
      fields:                 # JSON Pointers (json) or "selector|attr" specs (html) — not DSL
        name: "/name"
        value: "/id"
        adult: "/adult"        # Any extra fields are available for nsfw_field
      nsfw_field: adult        # Optional. Options flagged by this field are dropped
      cache:
        ttl: 600               # Seconds, max 30 days
        key: tags-v1

filters:
  - id: genre
    name: Genre
    type: select
    options_ref: genres

  - id: tag
    name: Tag
    type: multiselect
    options_ref: tags
```

A `Static` option set (a plain YAML sequence) is resolved and inlined directly into the generated `filter_list!`/`preference_list!` call at codegen time.

A `Fetched` option set (`options_fetched_by`) declares a remote source for the options. Codegen emits an empty options list for the filter itself but also generates a `get_fetched_option_sets()` WIT export returning a JSON array of `FilterFetchDef` records. The host calls this at filter-panel render time, fetches each route using `SmartClient`, parses the response per `container` + `fields`, and injects the resolved options back into the returned `FilterList`. Results are cached in the host's `ext_cache` under the namespace `fetched_opts:{source_id}`, keyed by `cache.key` (or the option set's name when there is no `cache:` block). The TTL is `cache.ttl`: 3600 s if the block omits it, 300 s if there is no block, and `0` never expires (§4.1).

**`fields` format:**
- For HTML (`type: html`): values are CSS selectors yielding text. Attribute extraction uses `"selector|attribute"` (e.g., `"a|href"`); `"self"` or `"self|attr"` refers to the container element.
- For JSON (`type: json`): values are JSON Pointer paths (e.g., `/name`, `/meta/id`).

**NSFW filtering:** Set `nsfw_field` on an `options_fetched_by` block to the name of a field in the `fields` map. For HTML, options where that field's text is `"true"` or `"1"` are dropped; for JSON, options where it is the boolean `true` are dropped. They are dropped regardless of the source's or user's NSFW setting. The field name is embedded in the emitted `FilterFetchDef` JSON.

**Filter-to-query mapping:** When an endpoint receives filters, the codegen needs to know how to convert active filter values into query parameters. This is defined in the endpoint:

```yaml
search:
  route: "/search"
  queries:
    q: $query$
  filter_mapping:
    genre: genre          # Filter group "genre" maps to query param "genre"
    type: type            # Filter "type" maps to query param "type"
    status: status        # Filter "status" maps to query param "status"
    year_range:
      kind: tuple_split    # Splits a "from:to" TextInput value into two query params
      from_param: year_from
      to_param: year_to
  container: "..."
  fields: { ... }
```

**`filter_format` (optional, per-endpoint):** controls how filter values are serialized into query parameters when the default encoding doesn't match the source's API:

```yaml
search:
  route: "/search"
  filter_mapping:
    genres: genre
  filter_format:
    multiselect: bracket        # "default" (repeated param) | "bracket" (param[]) | "comma_separated" | "repeated"
    omit_empty: false            # Default: true. When false, emits an explicit query for an unchecked checkbox.
    bool_format: one_zero        # "true_false" (default) | "one_zero" | "yes_no"
    array_separator: "|"         # Separator used by "comma_separated". Default: ",". Must not be empty.
  container: "..."
  fields: { ... }
```

When `filter_format` is omitted, codegen output is byte-identical to the pre-`filter_format` emission (repeated query params for multiselect, `"true"`/`"false"` literals for checkboxes, no explicit unchecked-checkbox query).

### 3.5 Preferences Section

```yaml
preferences:
  - key: cover_size
    label: "Cover Size"
    kind: select
    options:
      - name: Small
        value: ".256.jpg"
      - name: Medium
        value: ".512.jpg"
      - name: Full
        value: ""
    default: ".512.jpg"
    description: "Set the size of manga covers when browsing."

  - key: api_key
    label: "API Key"
    kind: text
    default: ""
    secret: true
    description: "Enter your API key for authenticated access."

  - key: show_nsfw
    label: "Show NSFW Content"
    kind: toggle
    default: "false"

  - key: preferred_tags
    label: "Preferred Tags"
    kind: select
    options_ref: tags    # Resolve options from a top-level `option_sets` entry (see §3.4)
```

**`secret: true`** hides the value in the UI (a password field) and nowhere else. The extension
reads it like any other preference and may send it to any host it is allowed to contact (§7);
the settings page says so under the field.

### 3.6 Complete Schema Reference

<!-- schema sketch: not an executable example -->
```yaml
# Top-level fields
id: string                      # Required. Extension identifier.
name: string                    # Required. Display name.
version: string                 # Required. Semver.
base_url: string                # Required. Base URL.
language: string                # Optional. Default: "en".
nsfw: bool                      # Optional. Default: false.
unrestricted_http: bool         # Optional. Default: false.
schema_version: integer         # Optional. Default: current schema version.
min_kani_version: string        # Optional. Semver floor on the host version.
requires_capabilities: [string] # Optional. Host capability flags required to install.

# Extension metadata (optional)
metadata:
  icon: string                  # Optional. Base64-encoded PNG/WebP/SVG, ≤ 64KB decoded.
  rate_limit:
    rps: number                 # Optional. Default: 2.0. Must be > 0.
    burst: integer              # Optional. Default: 8.
    max_concurrent: integer     # Optional. Default: 4.
    max_hook_requests: integer  # Optional. Default: 3. Max hook-driven retries per request (§3.10).
  languages: [string]           # Optional.
  description: string           # Optional.
  sections:
    - id: string                 # Required, non-empty, unique within `sections`.
      name: string
      nsfw: bool                 # Optional. Default: false.

# Endpoints (all optional but at least one should be defined)
endpoints:
  popular:                      # -> get_popular_manga
    delegate_to: string         # Optional: delegate to another endpoint (e.g. "search")
    empty_without_filters: bool # Optional: return empty list when no filters are active
    route: string
    method: string              # Default: "GET"
    headers: map<string, string>
    queries: map<string, string>
    filter_mapping: map<string, FilterMappingEntry>
    filter_format: FilterFormatCfg # Optional. Controls filter -> query serialization.
    type: "html" | "json"       # Default: "html"
    container: string
    bindings: map<string, string>
    fields: map<string, FieldDef>
    scalars: map<string, FieldDef>
    has_next_page: bool | string # Default: true. Static or DSL expression.
    pagination: PaginationConfig

  search:                       # -> search_manga
    route: string
    method: string              # Default: "GET"
    headers: map<string, string>
    queries: map<string, string>
    filter_mapping: map<string, FilterMappingEntry>
    filter_format: FilterFormatCfg # Optional. Controls filter -> query serialization.
    type: "html" | "json"       # Default: "html"
    container: string
    bindings: map<string, string>
    fields: map<string, FieldDef>
    scalars: map<string, FieldDef>
    has_next_page: bool | string # Default: true. Static or DSL expression.
    pagination: PaginationConfig

  manga_details:                # -> get_manga_details
    route: string
    method: string
    headers: map<string, string>
    queries: map<string, string>
    type: "html" | "json"
    container: string           # Usually ":root" or ""
    bindings: map<string, string>
    fields: map<string, FieldDef>
    scalars: map<string, FieldDef>

  chapter_list:                 # -> get_chapter_list
    route: string
    method: string
    headers: map<string, string>
    queries: map<string, string>
    type: "html" | "json"
    container: string
    bindings: map<string, string>
    fields: map<string, FieldDef>
    scalars: map<string, FieldDef>
    has_next_page: bool | string

  pages:                        # -> get_pages
    route: string
    method: string
    headers: map<string, string>
    queries: map<string, string>
    type: "html" | "json"
    container: string
    bindings: map<string, string>
    fields: map<string, FieldDef>
    scalars: map<string, FieldDef>

# FieldDef is either:
#   - A bare string (DSL expression, required field)
#   - A map with:
#       expr: string            # DSL expression
#       optional: bool          # Default: false

# Filters
filters:
  - id: string
    name: string
    name_i18n: string           # Optional. Alternate i18n key for the display name.
    type: "checkbox" | "select" | "text_input" | "sort" | "multiselect" | "int_range" | "date_range"
    options:                    # For select/sort/multiselect types (inline)
      - name: string
        value: string
        nsfw: bool               # Optional. Default: false.
    options_ref: string         # Optional. Resolve options from `option_sets.<name>` instead of inlining.
    min: number                  # Required for int_range/date_range
    max: number                  # Required for int_range/date_range
    step: number                 # Optional, for int_range/date_range
    default:                    # Optional
      name: string
      value: string
    semantic: "author" | "artist" | "tag"  # Optional hint

# FilterMappingEntry is either:
#   - A bare string (query param name; "simple" mapping)
#   - A map with `kind: sort_pair` and asc_param/desc_param
#   - A map with `kind: tuple_split`, from_param, to_param (splits a "from:to" TextInput value)

# FilterFormatCfg (optional, per-endpoint)
filter_format:
  multiselect: "default" | "bracket" | "comma_separated" | "repeated"  # Default: "default"
  omit_empty: bool                # Default: true
  bool_format: "true_false" | "one_zero" | "yes_no"                     # Default: "true_false"
  array_separator: string         # Default: ",". Must not be empty.

# Option sets (top-level, optional)
option_sets:
  <name>:                         # Static: a plain sequence of options
    - name: string
      value: string
      nsfw: bool                   # Optional. Default: false.
  <name>:                         # Fetched: resolved lazily by the host at render time
    options_fetched_by:
      route: string
      type: "html" | "json"        # Default: "html"
      container: string
      fields: map<string, string>  # JSON Pointers (json) or "selector|attr" specs (html); not DSL
      cache:
        ttl: integer                # Seconds. Default: 3600. Max: 30 days.
        key: string

# Preferences
preferences:
  - key: string
    label: string
    kind: "toggle" | "select" | "text" | "multi_value_list"
    options:                    # For select kind (inline)
      - name: string
        value: string
    options_ref: string         # Optional. Resolve options from `option_sets.<name>` instead of inlining.
    default: string
    description: string         # Optional
    secret: bool                # Optional, for text kind. Default: false.

# Pagination (per-endpoint; omit for sources with flexible page sizes)
pagination:
  native_page_size: integer          # Source's fixed chunk size
  offset_param: string               # Query param name for offset/page/cursor
  offset_type: "item" | "page" | "cursor"
  page_start: integer                # For "page" type: starting page number (default: 1)
  cursor_field: string               # For "cursor" type: JSON Pointer to next-page token in response

# Scripting (top-level, optional; see §3.10)
scripts:
  pure:
    <name>: string                   # Rhai source defining `fn <name>(...)`; also shared with hooks

pre_request: string                  # Rhai hook body; runs before every source-level HTTP request
on_status:                           # Rhai hook bodies by status pattern
  "401" | "4xx" | "5xx" | "default": string

# Per-endpoint hooks (subset of above, on any endpoint block)
# endpoints.<name>:
#   pre_request: string              # Replaces source-level pre_request for this endpoint
#   on_status:
#     <pattern>: string
```

### 3.7 Multi-Source Factory

A single YAML file can act as a template that produces multiple extension WASM outputs via a top-level `factory:` block. This eliminates copy-paste when a content network hosts many regional or language-specific subdomains that share the same extraction logic.

```yaml
factory:
  sources:
    - id: my-source-en       # Required. Must be unique across all sources.
      name: My Source (EN)   # Required. Display name for this source.
      base_url: "https://en.example.com"
      language: en
      mihon_source_id: 123456789   # Optional.
      overrides:                   # Optional. Dot-path keyed values applied on top of the template.
        endpoints.search.route: "/en/search?q=$query$"

    - id: my-source-ja
      name: My Source (JA)
      base_url: "https://ja.example.com"
      language: ja
      overrides:
        endpoints.search.route: "/ja/search?q=$query$"
        endpoints.popular.route: "/ja/top"
```

**How it works:**

1. `kani-cli build my-source.yaml` detects the `.yaml` extension and enters factory mode.
2. The factory block is validated (`validate_factory`): sources must be non-empty; IDs must be non-empty, unique, and non-duplicate; `base_url` and `name` must be non-empty.
3. For each source entry, the template's YAML value tree is cloned and the source's named fields (`id`, `name`, `base_url`, `language`, `mihon_source_id`) are written as top-level overrides; then the dot-path `overrides` map is applied recursively.
4. The expanded YAML is validated as a standalone extension (all standard validation rules apply).
5. A Rust crate is generated to `kani-extensions/kani-{source.id}/` and then compiled to `wasm_sources/{source.id}.wasm`.

**Dot-path override semantics:** keys use `.` as a path separator. Each segment descends into a YAML mapping. If an intermediate key is absent, a new empty mapping is created. Leaf values replace whatever was there. Unknown paths produce a stderr warning but do not abort the build.

**Validation rules for `factory`:**
- `factory.sources` must not be empty.
- Each source's `id` must be non-empty and unique within the factory block.
- Each source's `name` and `base_url` must be non-empty.

### 3.8 Browser Payload Endpoints

Set `via: browser_payload` to load an endpoint in the solver's browser instead of using direct HTTP.
Code generation emits `capture_page_payload` with the configured script and timeout. The browser
runtime manages the solver session, `passPayload` injection, `AllowedHost` checks, and resource
limits.

Both backends extract the captured payload as a standard JSON endpoint: the interpreted backend
(§5.1) directly, and generated code by passing it to `extract::json`.

Auto-scroll defaults to `false` everywhere: browser endpoints, the hook `ctx.capture_page_payload`,
and a Rust extension's `v8_context::capture_page_payload`. A Rust extension opts in with
`capture_page_payload_configured(url, script, timeout_ms, true)`.

```yaml
browser_scripts:
  fetch_manga: |
    // Script injected into the solver's browser page.
    // Must call passPayload(jsonString) with the data to extract.
    fetch('/api/manga')
      .then(r => r.json())
      .then(data => passPayload(JSON.stringify(data)));

endpoints:
  manga_details:
    via: browser_payload          # Required: marks this as a browser endpoint.
    page_url: "https://example.com/manga/$manga_id$"  # Required. Loaded in the browser.
    script: fetch_manga           # Required. Must be declared in browser_scripts.
    timeout_ms: 15000             # Optional. Default: 30000.
    auto_scroll: true             # Optional. Default: false. Periodically scrolls the
                                  # page so lazy-loaded content is present before the
                                  # payload is captured. Browser endpoints only.
    container: ""                 # The payload is JSON, so address its root with a pointer.
    fields:
      id: 'self.ptr("/id").str()'
      title: 'self.ptr("/title").str()'
      status: 'self.ptr("/status").str()'
```

**`browser_scripts`:** top-level map from script name to JavaScript source. Each script is written to `src/scripts/<name>.js` in the generated crate and accessed via `static SCRIPT_<NAME>: &str = include_str!("scripts/<name>.js")`. Scripts that do not call `passPayload` produce a warning during validation.

**`page_url`:** the absolute URL to load. May use `$manga_id$` and `$chapter_id$` placeholders (substituted from endpoint function arguments).

**`auto_scroll`:** when true, the solver scrolls the page during the load so content behind an infinite scroll or a lazy-loading observer is rendered before `passPayload` runs. It costs wall-clock time against `timeout_ms`, so enable it only for endpoints that need it. It has no effect on a non-browser endpoint.

**`queries` and `filter_mapping`:** a browser endpoint issues no request of its own — the page is the request, and the site's scripts turn its query string into whatever API call the payload comes from. Both are therefore appended to `page_url` as query parameters (endpoint queries first, then mapped filters), giving browser endpoints the same filter surface as HTTP ones. Note that sites commonly read a repeated parameter as its *first* occurrence, so a name used in `queries` should not also be the target of a mapped filter.

**`route`:** ignored when `via: browser_payload` is set (produces a warning).

**Validation rules for `via: browser_payload`:**
- `page_url` must be present and non-empty.
- `script` must be present, non-empty, and declared in `browser_scripts`.
- `browser_scripts` entries must have non-empty names and non-empty source.
- Scripts that do not call `passPayload` produce a warning (not an error).

### 3.9 YAML Validation Rules

The `kani-cli validate` command checks:

1. **Required fields present:** `id`, `name`, `version`, `base_url` must all be set.
2. **ID format:** Must match `[a-z][a-z0-9-]*` (lowercase, starts with letter). The host enforces
   the same rule on a WASM extension's metadata id at install. Ids name artifact files, source rows
   and cache namespaces, so a separator such as `:` or `_` would let two extensions' namespaces
   collide.
3. **Version format:** Must be valid semver.
4. **Base URL format:** Must be a valid URL with scheme.
5. **DSL syntax:** All DSL strings must parse without errors.
6. **Field completeness:** For `manga_details`, the required fields are `id`, `title`, `status`. For `chapter_list`, the required fields are `id`, `number`, `language`. For `pages`, the required fields are `index`, `url`, and `transform` is optional.
7. **Variable references:** All `$variable$` references in routes and queries must correspond to available function arguments or preference keys.
8. **Preference references:** All `$pref:key$` references must correspond to a declared preference.
9. **Filter mapping:** All filter mapping keys must correspond to declared filter group IDs.
10. **No unused bindings:** Warn if a top-level binding is declared but never referenced.
11. **`filter_format`:** `array_separator` must not be empty.
12. **`options_ref`:** every filter or preference `options_ref` must resolve to a declared `option_sets` entry.
13. **Range filters:** `int_range`/`date_range` filters must declare both `min` and `max`.
14. **Option sets:** a `Fetched` (`options_fetched_by`) entry's `route` must not be empty; its `cache.key` must not be empty and `cache.ttl` must not exceed 30 days.
15. **`metadata.icon`:** must be valid base64, decode to ≤ 64KB, and match a recognized PNG/WebP/SVG signature.
16. **`metadata.rate_limit.rps`:** must be greater than 0.
17. **`metadata.sections`:** each entry's `id` must be non-empty and unique within `sections`.
18. **`schema_version`:** must not exceed the schema version this `kani-cli` supports.
19. **`min_kani_version`:** when present, must be a valid semver version string.

### 3.10 Scripting Hooks

Scripting hooks run sandboxed [Rhai](https://rhai.rs) scripts before a request (`pre_request:`) or
after a response with a matching status (`on_status:`). DSL expressions may call pure helpers
defined under `scripts.pure:` (see §1.5).

#### Hook locations

Hooks can be declared at two levels:

- **Source-level** — top-level `pre_request:` / `on_status:` keys; fire on every HTTP request made by this extension.
- **Per-endpoint** — the same keys inside an endpoint block (e.g. `endpoints.manga_details.pre_request:`); apply to that endpoint's requests and to its `then:` / `for_each:` sub-fetches. Per-endpoint hooks receive the same sandbox bindings.

#### Dispatch

Exactly one hook body runs per event. A per-endpoint hook **replaces** the source-level hook; the
two never both run.

Sub-fetches (`then:` / `for_each:` steps) carry an `endpoint_id` of the form
`"<parent_endpoint>/<merge_as>"` (e.g. `"manga_details/chapters"`). Hooks are looked up for the
full id first, then for the parent (the part before `/`), then at source level. No endpoint can be
named with a `/`, so in practice a sub-fetch runs its parent's hooks. A hook that must treat the
sub-fetch differently can branch on `req.endpoint_id`.

1. `pre_request`: the first hook found for the full id, the parent endpoint, or the source level,
   in that order; otherwise nothing.
2. The (possibly mutated) request is sent.
3. `on_status`: each map, in the same order, is searched for the exact status (`"401"`), then
   its class (`"4xx"`), then `"default"`. The first match runs. A per-endpoint map with no
   matching key falls through to the next level rather than suppressing it.

#### Sandbox bindings

Each hook body is a Rhai expression body (not a function declaration) evaluated with the following variables in scope:

| Variable | Type | Description |
|----------|------|-------------|
| `req` | `ScriptableRequest` | Mutable HTTP request. Read `req.url`, `req.method`, `req.endpoint_id`, `req.headers`, `req.queries`. Mutate with `req.url = s`, `req.set_header(k,v)`, `req.remove_header(k)`, `req.set_query(k,v)`, `req.push_query(k,v)`, `req.remove_query(k)`. |
| `ctx` | `ScriptableCtx` | Context: `ctx.pref(key)`, the cache methods below, and `ctx.capture_page_payload(page_url, script, timeout_ms[, auto_scroll])`, which loads `page_url` in the solver's browser with the named `browser_scripts` entry and returns the string it passes to `passPayload`. `auto_scroll` defaults to `false`, as on browser endpoints (§3.8). |
| `resp` | `ScriptableResponse` | Available in `on_status` only: `resp.status` (integer), `resp.headers` (map), and `resp.body`, which the hook may reassign. |

`set_query` replaces any existing parameter of that name; `push_query` appends, so a
source that expects a repeated key (`?tag=a&tag=b`) needs `push_query`.

The body must return a `HookAction` value:

| Constructor | Meaning |
|-------------|---------|
| `proceed()` | Continue with the (possibly mutated) request/response as-is. |
| `retry()` | Re-send the request immediately (counts against `max_hook_requests`). |
| `retry_after(seconds)` | Re-send after a delay (counts against `max_hook_requests`). |
| `fail(kind, reason)` | Abort with an `ExtensionError` of the named kind. |
| `refresh_auth(endpoint_id)` | Re-run the named endpoint's auth flow, then retry (counts against `max_hook_requests`). |

#### Cache in hook scripts

The cache methods are registered directly on `ctx`. There is no `ctx.cache` sub-object. Every
`namespace` must be declared in the extension's `cache:` block (§3.2); any other namespace is a
script error.

| Method | Description |
|--------|-------------|
| `ctx.cache_get(namespace, key)` | Retrieve a string value. Returns `()` if absent or expired; test with `== ()`. |
| `ctx.cache_put(namespace, key, value, ttl_seconds)` | Store a string value. The TTL is held to the namespace's declared `ttl`: `0` takes the declared value, a longer TTL is shortened to it, and a negative TTL removes the entry. The namespace's `max_entries` applies. |
| `ctx.cache_delete(namespace, key)` | Remove an entry. |

`namespace` is prefixed with the extension's own namespace before it reaches the
backend, so two sources using the same namespace string cannot read each other's
entries.

The host uses `get_or_insert` internally for caching auth tokens; scripts express the
same pattern with `cache_get` + `cache_put`.

#### Byte primitives in hook scripts

Rhai arrays are capped by `KANI_RHAI_MAX_ARRAY`, so payloads cross the boundary as an
opaque `Bytes` value rather than an array of integers. These are registered as free
functions, not methods.

| Function | Description |
|----------|-------------|
| `bytes_from_utf8(text)` | `Bytes` from a string's UTF-8 encoding. |
| `bytes_to_utf8(data)` | String from `Bytes`. Fails if the bytes are not valid UTF-8. |
| `bytes_from_base64url(text)` | `Bytes` from base64url **without padding**. Fails on invalid input. |
| `bytes_to_base64url(data)` | base64url without padding. |
| `bytes_len(data)` | Length in bytes. |
| `bytes_substitute(data, table, key, seed, inverse)` | One round of keyed substitution with output feedback (below). |

`bytes_substitute` computes `out[i] = table[data[i] ^ key[i % key.len] ^ prev]`, where
`prev` is the previous output byte and starts at `seed`. With `inverse: true` it runs the
round backwards, which requires `table` to be a permutation of `0..=255`. `table` and
`key` may be Rhai arrays of integers `0..=255`, or `Bytes` — the latter is the shape
`bytes_from_base64url` returns, so material harvested from a page needs no JSON parser
to reach this call.

This exists for sources that obfuscate page URLs or identifiers behind a substitution
table shipped in their own JavaScript. It is not a general cryptography facility.

#### Retry composition

`SmartClient` retries `429`, `502`, and `504` responses, including `Retry-After` handling, before
hooks run. An `on_status` hook therefore receives only the final response. Hook retries use a
separate limit, `metadata.rate_limit.max_hook_requests` (default: 3), for cases such as refreshing
credentials after a `401`. Do not retry exhausted `429` or `5xx` responses in hooks.

#### Sandbox limits

| Limit | Default | Override (env var) |
|-------|---------|-------------------|
| Max Rhai operations | 100,000 | `KANI_RHAI_MAX_OPS` |
| Max string length | 1 MB | `KANI_RHAI_MAX_STRING` |
| Max array length | 10,000 | `KANI_RHAI_MAX_ARRAY` |
| Max expression depth | 64 / 32 | — |
| Max call levels | 16 | — |

`eval` and module `import`/`export` are disabled. Closures and `FnPtr` are not available to scripts.

`RefreshAuth { endpoint_id }` dispatch invokes a named YAML endpoint (one of `popular`, `search`, `manga_details`, `chapter_list`, `pages`) from within a hook via `ValidatedExtension::endpoint_by_name`, then retries the original request. Scripts can also refresh auth imperatively via `ctx.cache_put` and return `retry()` when a dedicated endpoint isn't needed.

#### Example

```yaml
metadata:
  rate_limit:
    max_hook_requests: 2

cache:
  auth:
    ttl: 3600

pre_request: |
  let token = ctx.cache_get("auth", "token");
  if token != () {
    req.set_header("Authorization", "Bearer " + token);
  }
  proceed()

on_status:
  "401": |
    let new_token = ctx.cache_get("auth", "pending_token");
    ctx.cache_put("auth", "token", new_token, 3600);
    retry()
  "5xx": |
    retry_after(5)

endpoints:
  popular:
    pre_request: |
      req.set_header("X-Endpoint", "popular");
      proceed()
```

---

## 4. Extension Cache Interface

Extensions can store and retrieve values across invocations using the host-provided `cache` WIT
interface. Each extension has its own namespace, so one extension cannot read another's entries.

### 4.1 Operations

| WIT function | Guest wrapper (`kani_shared::host_abi::cache`) | Description |
|--------------|-----------------------------------------------|-------------|
| `get(key: string) -> option<list<u8>>` | `get(key: &str) -> Option<Vec<u8>>` | Retrieve a value. `None` if absent or expired. |
| `put(key: string, value: list<u8>, ttl-secs: u32)` | `put(key: &str, value: Vec<u8>, ttl_secs: u32)` | Store a value. A TTL of `0` means the entry never expires; it leaves only by capacity eviction or deletion. |
| `delete(key: string)` | `delete(key: &str)` | Remove one entry. |
| `clear()` | `clear()` | Remove every entry in this extension's namespace. |

Values are raw bytes; extensions encode structured values themselves (e.g. JSON). The wrappers
return nothing: a cache write that fails is dropped rather than failing the call.

**`get_or_insert`** is a host-side convenience on the `CacheBackend` trait that composes `get` +
`put`. It is not exposed over WIT; hook scripts express the same pattern with `ctx.cache_get` +
`ctx.cache_put` (§3.10).

### 4.2 Namespace and backend

All extension cache entries live in one SQLite table (`extension_cache`), keyed by namespace and
key, so they persist across restarts. An extension's own namespace (used by the WIT calls above)
is its id followed by `:`; each hook namespace it declares (§3.2) is that prefix followed by the
declared name. Fetched option sets (§3.4) are cached under the host namespace
`fetched_opts:{source_id}`. All of these are shared by every user of the source (§3.2).

**A version change clears the cache.** When an install, an update, a reload, or the startup scan
records a version different from the stored one, the host deletes every namespace beginning with
`"<id>:"` (the extension's own and its hooks') and the source's `fetched_opts:{source_id}`
namespace. Reinstalling the same version keeps the cache.

### 4.3 Capacity limits

Each namespace is capped at 4 MB and 4096 entries; a declared `max_entries` lowers the entry cap
for that namespace. When a write would exceed a cap, the entries closest to expiry are evicted
first. An extension has its own namespace plus at most 16 declared ones, so its total cache
storage is bounded at 68 MB.

### 4.4 Usage in Rust Extensions

```rust
use kani_shared::host_abi::cache;

// Store the fetched cover CDN base URL for 10 minutes
cache::put("cdn_base", cdn_url.as_bytes().to_vec(), 600);

// Retrieve on subsequent calls
if let Some(base) = cache::get("cdn_base") {
    // use cached bytes
}

// Invalidate on auth refresh
cache::delete("auth_token");
```

### 4.5 TTL and Pruning

Expired entries are never returned by `get`. A background job (`spawn_cache_prune`) deletes them
every 10 minutes, so they may occupy space until the next prune.

## 5. Runtime Backends

`SourceBackend` (`kani-app/src/source/mod.rs`) abstracts two interchangeable backends stored in
`SourceRegistry` (`DashMap<i64, Arc<ArcSwap<SourceBackend>>>`). Both provide the same asynchronous
dispatch interface.

| Backend | Artifact | Toolchain | Execution |
|---------|----------|-----------|-----------|
| `Wasm`  | `<name>.wasm` | `wasm32-unknown-unknown` → `wasm-opt` → component | leased WASM instance over the WIT boundary |
| `Yaml`  | `<name>.yaml` | none (parsed at load) | host-native blueprint evaluation, no WIT call |

A YAML source can run either way: interpreted as-is, or compiled to a WASM crate by
`kani-cli generate` / `build`. The two differ only where this table says so. Code generation
rejects a source that uses an interpreted-only feature (`reject_interpreted_only` in
`kani-cli/src/commands/generate.rs`) rather than producing a crate that ignores it. Add any newly
found difference here and to that check.

| Feature | Interpreted YAML | Generated WASM |
|---------|------------------|----------------|
| `for_each[].deduplicate_by` (§3.2) | Supported | Rejected at generation |

### 5.1 Interpreted YAML backend

`kani-yaml::parse_and_validate` compiles a `.yaml` extension's DSL strings into `Expr` trees within
a `ValidatedExtension`, which is then wrapped in `YamlSource`. Dispatch resolves the endpoint,
builds a `Blueprint` and `HostState`, and calls the same `extract_html` or `extract_json` evaluator
as the WASM backend without crossing WIT or FFI. Preferences are injected as `$pref:key`.
`browser_payload` endpoints pass V8 output through standard JSON blueprints. Fetched filter option
sets resolve at request time, and `RefreshAuth { endpoint_id }` hooks behave as in the compiled
backend. Both backends share `build_blueprint` and `build_blueprint_core` in `kani-yaml/src/lib.rs`.

### 5.2 Evaluator resource limits

The evaluator (`kani-core/src/evaluator/shared.rs`) enforces host-side caps (not author-overridable): `MAX_EVAL_ITERATIONS` (100 000), `MAX_EVAL_DEPTH` (50), `MAX_LIST_SIZE` (10 000), `MAX_STRING_LENGTH` (1 000 000). Exceeding a cap aborts evaluation with a limit error.

### 5.3 Selection and supersession

Sources live in a single directory (`wasm_storage_path`), one artifact per source. Installing or
updating a source in one format deletes its artifact in the other, and overwrites the previous
version in place: **no earlier version is kept**, so there is no automatic rollback. To go back,
reinstall the older version from its repository.

Both `<name>.yaml` and `<name>.wasm` exist only when an operator has placed them by hand. Then
**YAML wins**, the choice is logged, and the WASM file is left unused; deleting the YAML file and
restarting switches the source back to it.

A YAML source that fails at startup (validation, missing capabilities, `min_kani_version`,
`schema_version`) leaves the row `enabled = 0` with the reason stored in `sources.load_error`. A
WASM artifact that cannot be compiled is reported as a source-load degradation instead.

### 5.4 Hot-swap

`SourceRegistry::hot_swap` installs a new backend atomically via `ArcSwap`. For WASM it first drains in-flight leases (30 s default) before swapping the `InstancePre`; new leases during draining return `ExtensionError::source_updating()` (`ExtensionErrorKind::Updating`, WIT `source-updating`), a 2-second-retry-hinted transient error. YAML swaps are immediate (no leases).

## 6. Signed Distribution

Git-hosted repositories distribute extensions through a signed `index.json`. Ed25519 signatures
and SHA-256 digests protect provenance and integrity. Third-party repositories use trust on first
use (TOFU) key pinning.

### 6.1 Repository index (`index.json`)

```jsonc
{
  "name": "Example Repo",
  "maintainer_key": "<base64 Ed25519 public key>",   // signs index.json
  "extensions": [
    {
      "id": "example",
      "name": "Example",
      "version": "1.2.0",
      "format": "yaml",            // or "wasm"
      "sha256": "<hex>",           // of the artifact bytes
      "signature": "<base64>",     // author Ed25519 signature over the artifact
      "author_key": "<base64>",    // author public key
      "min_kani_version": "0.1.0", // optional
      "url": "extensions/example/1.2.0/extension.yaml",
      "description": "…",          // optional
      "language": "en",            // optional
      "nsfw": false                // optional
    }
  ]
}
```

`index.json` is accompanied by `index.json.sig` (the maintainer signature over the index bytes).

### 6.2 Trust model

- **TOFU.** Adding an unpinned repo returns HTTP `428` with the maintainer key fingerprint (`SHA256:…`). The operator verifies the fingerprint out-of-band and re-submits with `confirm_fingerprint` (or the `X-Confirm-Key-Fingerprint` header); the server pins the key only if the confirmed fingerprint matches the freshly-fetched key.
- **Key change after trust** returns HTTP `409` (`REPO_KEY_CHANGED`); re-trust requires an explicit re-add.
- **Blocked repos** (admin-managed `blocked_repos`, merged with a compile-time list) return HTTP `403`.

### 6.3 Install pipeline

`install_or_update_from_repo` (serialized per extension id by an install lock): locate the manifest entry → check `min_kani_version` → download the artifact through the SSRF-protected client with size caps (`MAX_INDEX_BYTES` 1 MiB, `MAX_ARTIFACT_BYTES` 10 MiB) → verify `sha256` → verify the author Ed25519 signature → check that the artifact's own `id` equals the index entry's `id` (and, on update, the updated source's name) → **only then** write the file (`save_yaml`/`save_wasm`, both path-traversal guarded) → upsert the `sources` row (`name` is UNIQUE) → `registry.insert` (new) or `registry.hot_swap` (update). A verification failure writes no file and makes no DB change.

Every fallible step that has no side effects (verification, YAML validation, WASM compilation and
instantiation, capability checks) runs before anything is written. Artifacts are written to a
staging file and renamed into place, so a crash cannot leave a truncated artifact. The source's
existing artifacts are read before the write; if writing the file, removing the other format, or
the row upsert then fails, they are put back exactly as they were and the install returns the
error. The registry is only touched after the row is committed, and `hot_swap` cannot fail: it
waits up to 30 s for in-flight calls (§5.4) and then swaps. Repo add/trust/install/update/remove and block/unblock are audit-logged.

**The file and the row are not one transaction.** The rename and the row upsert are separate
steps, so a crash between them leaves the new artifact on disk under the old row. The artifact
on disk is therefore authoritative. At startup every artifact's own metadata is read and its row's
`version`, `base_url` and `unrestricted_http` are rewritten to match; when the version changed,
the extension's cache is cleared as it would be on an update (§4). Reloading a source applies the
same rule. A file whose declared id differs from the source it is stored under is not loaded
under that source and is reported as a `source_load` degradation.

**Every install path runs this pipeline.** A manual install (`POST /rest/sources/yaml`,
`/yaml/fetch`, `/wasm`, `/wasm/fetch`) skips only the repository steps (index lookup, hash and
signature) and finds or creates the source row by the artifact's own id. Replacing a specific
source (`POST /rest/sources/{id}/wasm`, `/{id}/wasm/fetch`) requires the artifact to declare that
source's id. Reserved ids (`example`, `test-abi`), the artifact's own `min_kani_version`, id form
(§3.9), blueprint schema version (§2.4) and capability checks apply on every path; an artifact that fails any of them, or does not
compile, is a `400` and changes nothing.

`kani-cli check <file>` runs the same checks without a server, so a repository can refuse an
artifact before publishing it. It also compiles hooks on the engine they run on and reports any
call to a function that neither the extension's scripts nor Kani define, which Rhai otherwise
reports only when the call runs. It also reports a literal cache namespace a hook uses without
declaring it in `cache:` (§3.2), which the runtime refuses on the first call.

### 6.4 SSE events

`SourceInstalled`, `RepoRefreshed`, `UpdateAvailable` (emitted by `refresh_repo` when a repo version exceeds the installed version, by semver compare), and `SourceUpdating` (emitted at the start of an update) are broadcast for live frontend indicators.

### 6.5 Environment variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `KANI_OFFICIAL_REPO_URL` | empty (bootstrap disabled) | Official repo `index.json` URL, pinned on first run |
| `KANI_OFFICIAL_REPO_KEY` | baked-in `OFFICIAL_REPO_KEY` | Base64 Ed25519 maintainer key for the official repo |
| `KANI_SOURCE_INSTALL_ALLOWED` | `true` | When `false`/`0`, install, repo, update, and the legacy unsigned upload/fetch routes return `403` |

### 6.6 Author tooling (`kani-cli`)

- `kani-cli keygen` — generate an author keypair (`author.pub` / passphrase-encrypted `author.key`).
- `kani-cli publish` — validate, hash, sign an extension and upsert its `index.json` entry.
- `kani-cli repo init|add|list|verify` — manage a local repo; `verify` recomputes hashes + checks signatures (non-zero exit on failure, for CI).
- `kani-cli new <name>` scaffolds a YAML extension; `--rust` scaffolds a Rust/WASM crate instead.

## 7. Outbound Request Policy

Every request an extension causes, directly or through the host, passes two checks. They apply
to the first request and to every redirect hop.

1. **Host policy (`AllowedHost`).** A source may contact only its `base_url` host, matched
   exactly, unless it declares `unrestricted_http: true`. The check runs on the final request,
   after any `pre_request` hook has rewritten it.
2. **Forbidden addresses.** Private, loopback, link-local (including cloud metadata
   `169.254.169.254`), CGNAT, multicast, documentation and reserved ranges are refused whatever
   the host policy allows. For a hostname, the validating resolver filters the addresses it
   resolves to at connect time, so DNS rebinding cannot change the answer after the check. For an
   IP literal, the URL itself is checked before connecting.

**Local-network grants.** An administrator can allow one installed source to reach named private
hosts, for a self-hosted server such as Komga on the LAN: `PUT /rest/sources/{id}/local-hosts`
with entries of the form `host` or `host:port` (a port-less entry covers every port). Extensions
cannot grant themselves anything. A grant:

- exempts only the listed hosts, and only for that source: its requests, sub-fetches, hooks,
  option sets, cover and page images through the proxy, downloads and quality probes all use a
  client carrying the grant, and every other source's client is unchanged;
- resolves a granted name through the system resolver, so LAN names and `/etc/hosts` work, and
  pins the connection to the addresses it checked;
- never opens loopback (Kani itself), link-local (including `169.254.169.254`), unspecified or
  multicast addresses, whether named directly or reached through DNS;
- is still subject to the host policy: a restricted source reaches a granted host only if it is
  the source's `base_url` host.

Changing a grant reloads the source.

| Path | Host policy | Forbidden addresses |
|------|-------------|---------------------|
| Endpoint routes and pagination chunks | Yes | Yes |
| Sub-fetches (`then:` / `for_each:`, `Expr::Fetch`) | Yes | Yes |
| Hook-driven retries and rewritten `req.url` | Yes (checked after the hook) | Yes |
| WASM guest HTTP imports | Yes | Yes |
| Fetched option sets (§3.4) | Yes (`base_url` host) | Yes |
| Browser `page_url` (endpoint, WASM guest, hook `ctx.capture_page_payload`) | Yes | Yes |
| Page scripts and subresources inside the solver browser | No: pages load CDNs and challenge scripts | Yes, by the solver (below); captures are refused without it |
| Image proxy (covers, pages) | No: images may come from any CDN | Yes; the owning source's grant applies |
| Repository index and artifacts | Artifact must share the repo's host | Yes |

**Redirects** are held to both checks on every hop: a restricted source's request may not be
redirected off its host. A site that redirects to another host (commonly `example.com` →
`www.example.com`) must use the final host as its `base_url`, or declare `unrestricted_http`.

**The solver browser** is a separate process with its own network. Kani checks the page it asks
the solver to load; the solver (`flaresolverr-kani`) enforces the forbidden-address rule on
everything that page then fetches, by routing the browser through an egress-guard proxy that
dials only the address it checked. It advertises this as `kani.egress-guard/1` on `GET /`.

**Browser captures require the guard.** A capture runs extension-supplied JavaScript in the
solver's browser, so Kani refuses it (`solver_egress_guard_missing`) unless the solver
advertises `kani.egress-guard/1`; browser endpoints and hook `ctx.capture_page_payload` fail
rather than run under a weaker policy. Ordinary challenge solving, which loads the source's own
page and returns cookies, still works with a stock FlareSolverr, but its browser then operates
under the **weaker policy**: nothing stops that page's scripts reaching private addresses on the
solver's network. Kani raises a `solver_egress_guard` warning in Diagnostics while the configured
solver lacks the capability, reading its index at startup and whenever the solver setting
changes. A solver request that supplies its own upstream `proxy` is not covered by the guard. The host policy cannot apply inside the browser: real pages load CDNs, fonts and
challenge scripts from other hosts.

**Secret preferences** are readable by the extension that declares them and may be sent to any
host the table above allows (§3.5).
