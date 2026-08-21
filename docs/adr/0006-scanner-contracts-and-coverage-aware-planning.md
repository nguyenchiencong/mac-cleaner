# Unified Scanner Trait and Coverage-Aware Planning

Different scanners must provide consistent diagnostics, risk classifications, and candidate metadata without fragmenting safety rules. We decided to use a unified compile-time `Scanner` trait delegating traversal and sizing to shared engine services, where degraded coverage (due to permission errors) prevents automatic candidate selection in plans. This guarantees uniform safety invariants across all scanners and prevents stale-decision mistakes caused by unreadable subtrees.
