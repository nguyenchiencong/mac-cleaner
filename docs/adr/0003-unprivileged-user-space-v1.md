# Unprivileged User-Space Operation with Transparent Diagnostics

Accessing root and TCC-protected directories requires Full Disk Access or administrative privileges, creating security friction and potential system instability. We decided that v1 will operate entirely in unprivileged user-space, focusing on user developer caches, project build artifacts, and user caches, while transparently reporting any skipped paths and permission constraints. This captures the vast majority of reclaimable storage safely without requiring elevated permissions or compromising user trust.
