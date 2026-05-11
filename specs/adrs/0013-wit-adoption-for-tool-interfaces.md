# ADR 0013: WIT Adoption for Tool Interfaces

## Status

Accepted

## Context

ADR 0012 introduces WASM host imports for self-improving agents. The host↔guest contract could be defined as raw wasmtime function imports (ad-hoc signatures) or using WebAssembly Interface Types (WIT), the Bytecode Alliance standard for component interfaces.

Raw imports are simpler initially but create maintenance debt: every language targeting AgentZero's WASM sandbox needs hand-written glue code, and the import signatures are undocumented outside Rust source.

WIT provides a language-neutral interface definition that generates bindings for Rust, Go, JS, C, and other languages via `wit-bindgen`. Zed's extension model uses WIT successfully.

## Decision

Adopt WIT for the `az:host` interface between AgentZero (host) and WASM tools (guest).

### Interface definition

```wit
package az:host@0.1.0;

interface filesystem {
    read-file: func(path: string) -> result<string, string>;
    write-file: func(path: string, content: string) -> result<bool, string>;
}

interface logging {
    log: func(message: string);
}

interface secrets {
    get-secret: func(handle: string) -> result<string, string>;
}

interface tools {
    register-tool: func(name: string, schema-json: string, wasm-bytes: list<u8>) -> result<bool, string>;
}

world tool {
    import az:host/filesystem;
    import az:host/logging;
    import az:host/secrets;
    export run: func() -> result<string, string>;
}
```

### Implementation path

1. Define `.wit` files in `crates/agentzero-sandbox/wit/`.
2. Use `wasmtime::component::Linker` to bind host functions.
3. Each host function delegates to the policy engine before executing.
4. Guest tools compiled against the WIT world can be written in any language with `wit-bindgen`.

### Versioning

The WIT package is versioned (`@0.1.0`). Breaking changes increment the minor version during pre-1.0 development. Published tools declare which WIT version they target.

## Consequences

### Positive

- Tools composable across languages (Rust, Go, JS, C target the same contract).
- `wit-bindgen` auto-generates glue code for guest languages.
- Aligns with WASM Component Model ecosystem and Warg registry standards.
- Interface is documented and machine-readable (not buried in Rust source).
- Future-proofs for component composition.

### Negative

- Adds `wit-bindgen` and component model dependencies.
- Slightly more complex than raw function imports.
- Component model tooling is still maturing.

### Migration

- Phase 1: Define WIT, implement host side with `wasmtime::component::Linker`.
- Phase 2: Provide `wit-bindgen` templates for guest tool authors.
- Phase 3: Migrate existing WASM skills to component model (backward compat via adapter).
