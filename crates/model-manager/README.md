# Model Manager security boundary

`model-manager` owns signed model-pack metadata, download integrity, safe staging,
self-test authorization, atomic activation, rollback, repair, and removal.

## Attested self-tests

Production activation cannot be authorized by a caller-supplied boolean or output digest.
The manager issues an OS-random, short-lived `SelfTestChallengeV1` bound to:

- pack ID and immutable revision;
- manifest digest and signed-catalog payload digest/version;
- exact staging transaction and installed-content tree digest;
- runtime ABI and approved backend;
- test-suite ID and immutable suite revision;
- nonce, issue time, and deadline.

A trusted runtime implements `SelfTestAttestationVerifier` and authenticates the returned
`SelfTestAttestationV1`. The manager validates every binding again, durably consumes a replay
key, and creates a crate-private `ActivationAuthorization`. Immediately before its same-volume
rename, the filesystem backend re-hashes the staged tree and requires the exact attested
binding. Mismatch, expiry, replay, unknown runner, invalid proof, or post-test modification
fails closed.

`MockAttestedExecutorVerifier` is available only to crate tests or the explicit
`test-support` feature. It is deterministic and intentionally not cryptographically secure.
This crate does **not** provide or claim a production model runner, runner key provisioning,
or runner sandbox. The trusted runtime must supply those components.

## Windows path safety

Windows file creation retains exclusive `CreateFileW` handles for the protected directory
and every ancestor below it while the target file is open. Reparse-point attributes and volume
identity are checked from live handles, and the post-open canonical target must remain beneath
the protected root. Unprepared download directories and any unsupported safe-open condition
fail closed. Atomic state replacement uses a protected parent handle and write-through rename.

Archive extraction uses the same protected creation primitive after a complete metadata
preflight. ZIP/TAR links, devices, traversal, alternate data streams, reserved device names,
case-fold collisions, expansion bombs, and file/directory collisions are rejected before any
member is written.

## Testing

Tests use local mock HTTP only. No model is executed, no live artifact is downloaded, and no
production runner identity is bundled.
