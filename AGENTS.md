# Build storage

Use the shared Cargo target directory configured in .cargo/config.toml (E:\temp\InteractiveNPCs\cargo-target). Do not create task-specific, agent-specific, or nested Cargo caches. Concurrent Cargo builds should share Cargo's locking or run sequentially. Override the target directory only when isolation is necessary, and remove that disposable cache after the experiment.

Keep large generated artifacts outside the source tree under E:\temp\InteractiveNPCs. Reuse build outputs; do not retain duplicate intermediate frame sequences or obsolete portable packages after their replacement is verified. Preserve source assets, current review packages, and final evidence.

Keep verification headless. Never launch the flashing red/blue native smoke-test window.
