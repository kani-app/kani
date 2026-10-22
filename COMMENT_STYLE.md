# Comment Style

Use comments only when names, types, structure, tests, or developer documentation cannot express
the required information.

## Classify before editing

Every comment should have one outcome:

- **Keep** an essential local contract or machine directive.
- **Encode** the intent in names, types, assertions, tests, or a structural check.
- **Extract** durable engineering knowledge into the engineering-constraints register.
- **Delete** narration, repetition, history, or inaccurate detail.

## Comments that belong in code

Keep comments only for:

- Safety, security, concurrency, lifetime, or ownership requirements that are not apparent from
  the code.
- Public contracts whose edge cases are not represented by the signature or type system.
- External workarounds that must remain adjacent to the affected operation.
- Tool directives, generated-file markers, lint justifications, doctests, CLI help, and type-bearing
  JSDoc.
- Short category labels inside a long flat declarative registry when the language provides no
  structural grouping and the labels materially help audit membership or ordering.

State the current constraint and its consequence. Do not narrate the implementation, recount its
history, address the reader, or restate names. Prefer one sentence. Place explanations above the
smallest relevant construct and reserve trailing comments for directives.

Ordinary prose comments may use at most three physical lines and 100 columns. Contract docs and
machine-required forms may be longer when their format requires it.

## Declaration documentation

Documentation coverage is semantic, not numerical. A declaration needs documentation when a
caller or implementer must know a contract that its name, signature, visibility, and surrounding
type do not communicate. Do not add comments merely because a neighbouring declaration has one.

Document:

- Public cross-crate types, traits, and functions that form a supported API, unless the complete
  contract is self-evident from a conventional name and signature.
- Trait methods, WIT operations, REST-facing types, and exported JavaScript APIs when callers or
  implementers need behavioral, error, lifecycle, ordering, mutation, or compatibility details.
- Modules whose purpose, boundary, or relationship to adjacent modules is not evident from the
  module name and layout.
- Private or crate-local declarations only when they carry a non-obvious safety, security,
  concurrency, ownership, lifecycle, performance, external-system, or cross-component invariant.
- Units, bounds, defaults, sentinel values, partial-result behavior, and side effects when the type
  system does not encode them.

Do not require documentation for:

- Every function, method, type, field, enum variant, module, or JavaScript export.
- Tests and fixtures whose names and assertions state their contract.
- Conventional constructors, accessors, conversions, delegation methods, and collection helpers
  whose behavior is fully described by their signatures.
- Private implementation steps with no caller-visible or maintainer-critical constraint.
- Declarations generated from another authoritative schema or interface; document the source
  contract instead.

Type-level documentation should explain the abstraction and its invariants. Function and method
documentation should explain caller-visible behavior rather than restating the action named by the
function. Document errors, panics, safety requirements, and side effects only when they are possible
and non-obvious.

### Common scenarios

Use these decisions during coverage reviews. **Required** means the declaration is incomplete
without documentation. **Conditional** means document only the listed non-obvious contract.
**Omit** means a comment normally reduces signal.

| Scenario | Decision | What the documentation must add |
| --- | --- | --- |
| Crate root | Required for reusable/library crates | Purpose, boundary, and any crate-wide invariant or usage constraint. Binary-only entry points may omit it. |
| Module | Conditional | Its responsibility or boundary when the name and parent layout do not make those clear. Do not inventory the module's contents. |
| Public domain type | Required | The abstraction, valid states, and invariants consumers must preserve. A transparent newtype with completely conventional semantics may omit it. |
| Private or crate-local type | Conditional | Only a maintainer-critical invariant, lifecycle, concurrency model, or external constraint. |
| Trait or interface | Required | What implementers promise, who calls it, and any object-safety, threading, lifecycle, or compatibility contract. |
| Trait or interface member | Conditional | Ordering, defaults, errors, side effects, idempotency, or semantics not established by the parent contract and signature. |
| Public function or method | Conditional | Caller-visible behavior not encoded by its name and types, especially mutation, partial results, blocking, retries, caching, authorization, or external I/O. |
| Private function or method | Omit by default | Add documentation only for a safety, security, ownership, concurrency, protocol, or cross-component invariant. Prefer a better name or extracted function over narration. |
| Constructor or factory | Conditional | Defaults, validation, resource acquisition, registration, ownership transfer, or required follow-up actions. Omit "creates a new X." |
| Getter, setter, conversion, or delegation method | Omit by default | Document only lossy conversion, normalization, validation, caching, side effects, or behavior differing from the conventional name. |
| Async function | Conditional | Cancellation safety, spawned work, locks or resources held across suspension, timeout/retry behavior, and whether work survives the returned future. Do not say only that it is asynchronous. |
| Iterator, stream, or pagination API | Conditional | Ordering, laziness, termination, duplication, partial failure, cursor stability, and ownership of yielded values. |
| Callback, hook, or event handler | Conditional | Invocation timing, ordering, reentrancy, allowed side effects, and failure propagation. UI event handlers with evident local behavior normally omit documentation. |
| REST route handler | Omit by default | OpenAPI, request/response types, permission declarations, and tests are authoritative. Add local documentation only for behavior those cannot express, such as streaming or compatibility semantics. |
| Service method | Conditional | Transaction boundaries, consistency guarantees, authorization assumptions, emitted events, idempotency, and durable side effects. Do not paraphrase the method name. |
| Error type | Required at type level when public | The error boundary and how callers should classify or handle it. Self-evident wrapper variants need no individual prose. |
| Constant or static | Conditional | Units, derivation rule, protocol meaning, safety constraint, or why consumers must share the exact value. Omit restatements of a descriptive name. |
| Type alias | Conditional | The semantic boundary or narrowed contract when it is more than spelling convenience. Conventional `Result` aliases may omit it. |
| Macro | Required when intended for reuse | Accepted forms, expansion contract, evaluation behavior, hygiene limitations, and a short example when invocation is not evident. |
| Large declarative registry | Conditional | Short domain labels may partition a long flat list when no language construct can do so. Do not add prose, repeat item names, or label ordinary implementation sections. |
| `unsafe` item or block | Required | A `# Safety` contract for public unsafe APIs and a `SAFETY:` justification at each unsafe operation or block. State the invariant, not confidence. |
| Lint suppression or tool escape hatch | Required | Why the rule does not apply and the narrow condition that keeps the exception valid. |
| External workaround | Required locally while active | The upstream system/version, required constraint, and removal or revalidation condition. Put investigation history elsewhere. |
| Test | Omit by default | Express intent in the test name, setup helpers, and assertion messages. Document only fixture protocol, concurrency coordination, or a scenario that cannot be named clearly. |
| Test helper or fixture | Conditional | Hidden setup guarantees, ownership, timing, cleanup, or protocol behavior relied on by multiple tests. |
| Generated declaration or vendored file | Omit | Add one machine-readable generated/vendor marker where appropriate and document the generator or source schema. Never hand-maintain per-item docs. |

### Rust data declarations

- Struct and enum type documentation follows the public/private type rules above.
- Fields and variants follow the semantic rule below; visibility alone does not require prose.
- Tuple fields need documentation when their position carries meaning not represented by a domain
  type. Prefer a named struct when several positions need explanation.
- Generic parameters and lifetimes need explanation only when they impose a relationship callers
  would not infer from the bounds and signature.
- `impl` blocks do not need headings. Put shared invariants on the type and member-specific
  contracts on the relevant method.
- Derive and serde attributes are executable documentation. Do not restate them unless their
  compatibility consequence is surprising.

### WIT and boundary schemas

Document each public WIT interface, resource, and world at the boundary level. Document operations,
records, fields, and variants only where guest and host authors need semantics beyond the WIT type:
handle ownership, explicit release, units, pagination, ordering, error encoding, compatibility, or
async/stream behavior. Keep the authoritative contract in WIT and avoid duplicating it on generated
Rust bindings.

REST/OpenAPI schemas, YAML schemas, protobuf, and other boundary models follow the same rule. Schema
descriptions should explain wire semantics and validation constraints. Internal mirrors and generated
representations should point to or derive from the authoritative schema rather than repeat it.

### JavaScript and frontend code

- Shared exported utilities, hooks, state primitives, and components need JSDoc when consumers need
  parameter shapes, return types, lifecycle, mutation, cleanup, rendering, or error behavior not
  supplied by the code or type tooling.
- Local page functions and event handlers omit JSDoc when their names and call sites are sufficient.
- Component documentation explains its reusable contract, not its markup. Props that are
  self-evident need no individual `@property`; callbacks, ownership, controlled state, and mutually
  dependent props do.
- Hooks document subscription and cleanup behavior, stable versus changing return identities, and
  effects visible outside the component. Do not restate the hook's name.
- JSDoc used solely for TypeScript checking is retained as machine-bearing documentation even when
  the runtime behavior is obvious.
- CSS comments are not declaration documentation. Keep only cascade, browser, accessibility,
  token, or runtime coupling constraints that selectors and custom-property names cannot express.

### Configuration, workflows, scripts, and SQL

- Configuration keys normally rely on names, schema validation, and user-facing documentation.
  Comment only non-obvious units, precedence, platform constraints, or coupled values.
- Workflow steps and scripts should be named clearly. Comment only shell portability, security,
  external-tool, artifact-lifecycle, or cross-job data-flow constraints.
- New SQL migrations use comments only for a non-obvious data transformation or invariant that SQL
  cannot state. Table and column comments that merely repeat identifiers are omitted.
- Applied migrations remain checksum-frozen as described below; their legacy comments are not a
  model for new migrations.

### Fields and variants

Do not comment every struct field or enum variant. A comment such as "the manga title" above a
field named `title` is repetition, not documentation. Exhaustive field comments also conceal the
few fields whose semantics are genuinely surprising.

Document an individual field or variant when consumers need information not encoded by its name
and type, including:

- Units, ranges, coordinate systems, or whether a bound is inclusive.
- The meaning of `None`, zero, an empty collection, or another sentinel.
- Normalization, canonicalization, provenance, or whether a value is measured, configured, or
  derived.
- Ownership, retention, secrecy, serialization, compatibility, or external protocol requirements.
- A relationship or invariant involving another field.

Prefer a type-level invariant when it applies to several fields. If nearly every field needs a
separate explanation, first consider stronger names, domain types, or a smaller data model. Keep
field documentation only when those changes would not encode the contract adequately.

### Consistency

Consistency means applying these criteria uniformly, not making adjacent declarations look
symmetrical. Within a family of equivalent public declarations, use the same documentation shape
and level of detail. An intentionally undocumented self-evident member may sit beside a documented
member with additional constraints.

## Knowledge that belongs outside code

Measurements, toolchain behavior, repeated failure signatures, ordering traps, and architectural
or operational decisions belong in the
[engineering-constraints register](docs/docs/developer/engineering-constraints.md). Record the
evidence, affected versions, consequence, enforcement, and revalidation trigger there. Preserve
the behavior in a test or check when practical.

Historical context belongs in issues, commits, release notes, or decision records. It should not
be reconstructed in a source comment unless it remains part of the current contract.

## Required forms

- `SAFETY:` states the invariant that makes an unsafe operation valid.
- `TODO(#123):` and related debt markers reference a tracked issue.
- `audit-ignore` and similar directives include the shortest useful justification accepted by the
  tool.
- JSDoc defines types or non-obvious contracts; it does not paraphrase the function body.
- Test intent belongs first in the test name and assertion messages.
- Applied SQL migrations are checksum-frozen. Preserve an accurate legacy comment in place unless
  the change also provides and tests an explicit checksum-compatibility transition.
