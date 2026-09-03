# WebView2 policy

The NSIS package embeds Microsoft's exact x64 Evergreen Standalone Installer so a
clean Windows 10/11 installation does not need network access. The WebView carries
only the control UI; continuous PCM and video remain in native processes.

All three Tauri configs use `skip`. This is intentional: the locked Tauri CLI's
built-in `offlineInstaller` route still performs an unconditional HEAD against a
mutable Microsoft fwlink before it consults its cache. `scripts/dev.ps1 setup`
warms the exact immutable resolved URL into the ignored cache declared in
`packaging/security/installer-toolchain-provenance.json`; `setup -Offline` verifies
the already-warmed bytes. Packaging never downloads it.

Before NSIS compilation, `scripts/package.ps1` requires the exact 258,438,352-byte
file, SHA-256, file/product version, and valid reviewed Microsoft Authenticode
signer. It then generates an ignored hook that binds that path and the committed
hook source. Missing, tampered, unsigned, wrong-version, or wrong-signer input
aborts before bundling. The hook installs those bytes only when WebView2 is absent
and removes the temporary copy after use.

This pins the WebView input, but does not claim byte-identical NSIS output across
independent builds. Native compiler/linker and NSIS output timestamps still make
the local installer non-byte-reproducible; exact toolchain and final artifact
hashes remain mandatory evidence.

The manual clean-machine-style lifecycle check is implemented by
`scripts/installer-smoke.ps1`. It is intentionally excluded from push and
pull-request CI because it performs a real current-user install/uninstall. The
only GitHub workflow that invokes it is the explicit `workflow_dispatch` package
smoke job, and that workflow does not upload or publish the installer.
