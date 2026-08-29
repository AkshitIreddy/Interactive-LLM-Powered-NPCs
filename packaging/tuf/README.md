# TUF metadata boundary

No placeholder root metadata is committed because a fabricated or unsigned root would create false trust. Before Model Manager leaves fixture mode:

1. Generate root, targets, snapshot, and timestamp keys in an approved offline/online split.
2. Commit only reviewed public root metadata and key IDs.
3. Keep private keys outside the repository and CI logs.
4. Test expiry, threshold signatures, rollback, freeze, mix-and-match, wrong length/hash, and root rotation.
5. Keep the default catalog endpoint disabled until explicit release approval.

Tests may use ephemeral keys and a loopback fixture repository; fixture trust roots must be visibly marked and rejected by release builds.
