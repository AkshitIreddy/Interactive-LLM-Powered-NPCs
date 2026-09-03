# Command 31 Control integration

`MANUAL_ACTOR_PICKER = 31` is a native-only begin/poll/cancel transaction. The
Control Rust backend must add it to its append-only command registry and codec;
the WebView bridge must not accept or return the native DTO directly.

The backend begin request is:

- `action = begin`, a unique safe `request_id`, and a 500-15000 ms timeout;
- the exact current `source_device_generation`, `source_geometry_epoch`,
  `source_frame_sequence`, and `source_frame_qpc` from the qualified native
  visual authority;
- 1-64 detected candidates containing typed `actor_id`, `track_id`,
  `track_epoch`, and normalized source-frame bounds.

Poll and cancel carry only the action and exact request ID. An authenticated
global cancellation-generation change, target clear, or target replacement
also cancels the native picker.

The terminal `ManualActorPickerReceipt` may be consumed only by the native
actor-lock bridge. It returns status plus typed selected identity and exact
session/target/WGC provenance. It intentionally contains no image bytes,
candidate bounds, or click coordinate. `candidate_set_sha256` binds the receipt
to the private native candidate set. `clicked_qpc` is the consumed matching
button-up timestamp, and `single_hardware_pointer_click` attests that one
hardware-origin down/up pair used the same button, pointer kind, and detected
ROI with no extra pointer or button. The actor-lock bridge should single-consume
the `(receipt_nonce_high, receipt_nonce_low)` pair and require `selected`, all
four safety booleans (including `single_hardware_pointer_click`), both
WebView-withholding booleans, and exact current
session/cancellation/target/device/geometry bindings before issuing authority.

The presentation-safe WebView surface should expose only start/cancel intent and
a coarse state such as `waiting`, `selected`, `cancelled`, or `unavailable`.
Never serialize the command-31 candidates or native receipt through Tauri event
payloads, invoke results, frontend state, logs, diagnostics, or support bundles.
