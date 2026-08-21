# Explicit Configuration Lifecycle

User workspace roots, exclusions, and scanner preferences must be persistent and reproducible across scan, plan, and apply phases without unexpected side effects. We decided to use a TOML configuration file at standard paths, which is never created implicitly during read-only commands and must be initialized explicitly via `config init`. This prevents surprise filesystem writes during inspection while establishing a deterministic configuration precedence hierarchy.
