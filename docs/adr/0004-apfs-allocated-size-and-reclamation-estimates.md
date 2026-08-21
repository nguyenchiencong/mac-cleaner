# APFS Block Allocation and Reclamation Estimation

On macOS APFS, file sizes can be misleading due to extent sharing (clones), sparse files, local snapshots, and hardlinks. We decided to compute candidate sizes using allocated block counts (`st_blocks * 512`) with inode deduplication for hardlinks, while explicitly reporting values as estimates and caveats regarding APFS clones, snapshots, and Trash semantics. This provides users with realistic reclamation bounds without making unprovable exact-byte guarantees.
