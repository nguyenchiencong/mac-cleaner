# Multi-Layer Sandbox Verification and Safety Gates

A filesystem reclamation tool must prove that it never deletes unapproved paths or acts on drifted files before running on live disks. We decided to test the engine using isolated temporary directory trees with simulated project mutations, fake trash backends, and fault injection, while keeping real macOS Trash tests isolated and opt-in. This verifies all core safety invariants (revalidation, non-following of symlinks, collision-safe restore) deterministically with zero host system risk.
