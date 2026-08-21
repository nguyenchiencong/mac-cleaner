# CLI-First Architecture and Rust Core

We are building a storage reclamation tool for macOS where high performance, memory safety, and scriptability are essential. We decided to implement the core engine in Rust with a decoupled CLI-first architecture (`scan`, `plan`, `apply`, `restore`), treating the interactive TUI as an optional consumer of the same engine. This provides single-binary portability with zero runtime overhead, strong type-state safety around paths and actions, and full composability in automated workflows.
