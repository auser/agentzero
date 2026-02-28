# agentzero-tools

Security-sensitive local tool implementations shared by the runtime:

- `read_file`
- `write_file` (strict mode, default disabled)
- `shell`

This crate owns tool policies and the typed `ToolSecurityPolicy` used by higher layers.
Integrations that require external processes/protocols (for example MCP bridge or plugin process adapters) remain outside this crate.
