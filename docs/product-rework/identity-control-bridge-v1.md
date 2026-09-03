# Native CPU identity control bridge V1

Status: implemented below the WebView boundary; product activation remains
fail-closed until Model Manager admits the exact signed/measured model pack and
the user explicitly authorizes its activation mode.

## Broker contract

- Command `16` (`AllocateIdentityFrame`) is separate from frozen GPU command
  `14`. It copies one bounded rectangle from the exact latest advancing WGC
  BGRA8 frame into a broker-owned CPU mapping.
- Command `17` (`ReleaseIdentityFrame`) destroys the broker's mapping handle by
  exact worker PID, lease ID, and nonce. Target clear/change, generation cancel,
  broker restart, and parent death revoke all live identity mappings.
- Queue depth is exactly one. Crops are at most 8192 pixels per edge and 64 MiB,
  and the source frame must be no more than 120 ms old. The worker operation is
  bounded to 500 ms by the Rust controller.
- The mapping name is exactly
  `Local\\npc.identity.<sha256(lease_id || NUL || lease_nonce)>`. The DACL grants
  read access only to the current user SID; the broker writes through its
  creator handle and the worker opens the mapping read-only. `Global\\` and
  caller-selected mapping names are not representable.
- The response binds the mapping digest to capture session, selected PID/HWND/
  executable, device generation, geometry epoch, frame sequence, source QPC,
  wall-clock capture time, exact crop/source extent, advancing-frame evidence,
  overlay exclusion, and protected/online/anti-cheat safety state.

## Worker and authority boundary

`identity_runtime::IdentityControlBridge` builds the worker's bounded WGC frame
request directly from the native lease. Mapping name and nonce are redacted in
Debug output, are not Serde/WebView types, and are released after every terminal
worker result. Worker observations remain `untrusted_worker_observations`; the
controller rejects any target/frame/QPC/geometry/digest/safety mismatch before
constructing `TrustedWgcIdentityFrameV1`, then delegates to the frozen
`TrustedWgcEvidenceAdapterV1`.

The worker transport must prove both a consumed launch-nonce handshake and a
kill-on-parent-close Job Object binding. Cancellation advances exactly one
worker generation; a worker that cannot acknowledge it is terminated and
restarted. Backpressure never queues a second frame.

## Product registration handoff

The Tauri crate registers the native-only module but exposes no command to the
WebView. Product activation must supply a concrete `IdentityWorkerTransport`
only after all of these are true:

1. Model Manager verifies the canonical V2 manifest and all artifact digests.
2. The requested activation is explicitly authorized (`private_evaluation`) or
   has a pinned `verified_catalog_admission_sha256` (`qualified_catalog`).
3. The hidden child completes its launch-nonce handshake and is assigned to the
   control process's kill-on-close Job Object before model load.
4. Target/profile safety admits local WGC identity and the broker proves overlay
   capture exclusion. Console-isolated, unknown, blocked, protected-online, and
   anti-cheat states cannot allocate an identity frame.

Until registration completes, identity inference is unavailable rather than a
fixture, heuristic, or browser-preview success.

## Native acceptance seam

`npc_media_broker_windows_identity_smoke_tests` is a dedicated command 16/17
product seam, separate from the broader residual/WGC lifecycle test. It launches
the broker and a kill-on-parent-close hidden worker, uses a secondary-monitor
no-activate WGC fixture, rejects wrong worker identity and out-of-source crops,
has the attested worker open the mapping read-only, verifies the exact byte SHA
and source evidence, and proves release, expiry, and generation cancellation
make the capability impossible to reopen. The executable itself never loads an
AI model.

## Private/original reference import

Command IDs 21 (`AllocateIdentityReferenceImport`) and 22
(`ReleaseIdentityReferenceImport`) are now registered as a separate native
picker/import capability. They do not reuse command 16 and do not accept a file
path, encoded bytes, raw pixels, mapping name, lease nonce, or import timestamp
from the WebView.

The broker opens the Windows system picker for PNG/JPEG only, reads at most
32 MiB of encoded data, decodes one frame through WIC to tight BGRA8, and bounds
the normalized image to 8192 pixels per edge and 64 MiB. The selected path and
encoded bytes remain inside the picker helper and are discarded; no EXIF is
retained. The normalized bytes are copied into the same current-user,
read-only-worker `Local\\npc.identity.*` mapping scheme as command 16, with a
two-second lease. The response binds worker PID/creation/executable, selected
target PID/HWND/executable, capture session, device and geometry generations,
native picker-consent token, game, character, subject, reference, rights,
source-file digest, and normalized-pixel digest.

The controller accepts exactly one of these provenance modes:

- `user_private`: explicit consent, a bounded owner ID, local-only, and no
  original-work license;
- `original_synthetic`: a nonempty bounded original-work license, local-only,
  and no private owner ID.

Character and subject IDs must match. Every terminal worker result, error,
timeout, cancellation, target change, broker restart, and parent death releases
the mapping. Command 22 is single-consume: a second or mismatched release is a
typed invalid-payload failure.

`IdentityGalleryStore` persists the strict `PortableReferenceImportV1` result
only after the identity engine revalidates its exact model, detector,
preprocessing, tensor digest, source digest, game scope, and rights. Galleries
live in the protected application configuration directory at
`identity-galleries/<game-profile>.identity-gallery.v1.json`, are limited to
8 MiB, reject links/reparse points, use atomic replacement, and reopen only
under the exact pinned qualification. Each successful persisted mutation
advances a monotonic gallery generation distinct from the schema version. The
final receipt exposes scope, generation, reference count, and digests but never
the embedding tensor or mapping capability.

The Tauri request DTO is bounded and path-free, but product activation still
fails closed: `LocalResourceManager` must first return an exact signed/measured
CPU identity-pack admission and the user must explicitly activate it. The
current canonical pack is not admitted, so the command must report unavailable
rather than launch the worker or present a false enrollment affordance.
