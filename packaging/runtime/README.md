# Runtime staging layout

`layout.json` is the packaging contract, not a directory to pre-populate in Git. Immutable application files and mutable per-user state must resolve to separate roots at install time.

Model Manager downloads into a unique bounded staging directory, validates metadata, byte size, hashes, archive paths, runtime ABI, license acceptance, and self-tests, then atomically activates a version directory. Failed or interrupted operations leave the active version untouched. Shared packs are reference-counted before removal.

Provider secrets never enter this layout; only opaque Windows Credential Manager target names may be persisted in settings.
