# Native Trash Backend and Receipt-Driven Restoration

Trash moves on macOS must be reliable, handle multi-volume boundaries, and capture actual destination paths for restore operations. We decided to access Cocoa's `NSFileManager` via a `TrashBackend` abstraction that returns a structured `TrashReceipt` with the exact resulting location in `.Trash`. This decouples the engine from OS APIs, allows sandbox testing with a mock backend, and prevents path-guess errors during undo.
