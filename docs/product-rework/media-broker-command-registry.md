# Media-broker command registry

This registry is the canonical ownership map for the authenticated native
media-broker control protocol. Command numbers are append-only. A reserved
number must not be reused even before its implementation is enabled by the
decoder. `ipc.hpp` contains a compile-time uniqueness assertion over the same
registry.

| IDs | Commands | Owner / status |
| --- | --- | --- |
| 1-11 | Health, target, PTT, legacy audio status, visual patching, cancellation, diagnostics, shutdown, capture evidence | Frozen core |
| 12-13 | Allocate/cancel one-use PCM playback | Frozen playback transport |
| 14-15 | Allocate/release visual source | Visual runtime |
| 16-17 | Allocate/release identity frame | Identity runtime |
| 18-20 | Enumerate/select/query audio output | Frozen playback output selection |
| 21-22 | Allocate/release identity reference import | Frozen identity runtime |
| 23 | Query trusted subtitle presentation context | Frozen native subtitle context |
| 24-26 | Enumerate/select/query audio input | Frozen native input selection |
| 27-28 | Allocate/cancel bounded authenticated PTT PCM input | Frozen native input transport v2 |
| 29 | Query causal visual audio envelope | Frozen read-only playback evidence |
| 30 | Query PTT activation state | Native-only arm/poll baseline evidence |

Adding a command requires an entry in all four authoritative wire surfaces:

1. `native/media-broker/include/npc/media_broker/ipc.hpp`
2. `native/media-broker/protocol/control.proto`
3. `native/media-broker/src/ipc.cpp`
4. the native-only Rust bridge in `apps/control/src-tauri/src/media_broker.rs`

The envelope decoder must reject reserved-but-unimplemented commands. A command
becomes available only when its request codec, response codec, broker dispatch,
portable tests, and Windows evidence test land together.
