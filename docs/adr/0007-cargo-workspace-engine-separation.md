# Cargo Workspace Architecture and Core Engine Separation

We need strict separation between the safety-critical cleanup engine and presentation layers (CLI now, TUI later). We decided to structure the project as a two-crate Cargo workspace (`mac-cleaner-core` and `mac-cleaner-cli`), keeping traversal, risk evaluation, sizing, and journaling strictly independent of terminal UI, parsing, and formatting concerns. This guarantees that automation, TUI, and CLI share identical safety guarantees and domain models without interface leakage.
