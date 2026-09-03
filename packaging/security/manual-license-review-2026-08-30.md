# Manual lock-license reconciliation — 2026-08-30

This is the human-readable evidence for the 67 components that entered the four-lock merged inventory during the 2.0 integration. Each conclusion was read from the exact cached crate release's normalized `Cargo.toml` license field; project crates were checked against `license.workspace = true` and the root MIT workspace license. No automated guess or package-name heuristic was used to assign these entries.

The shared locks subsequently added `pkg:cargo/npc-product-benchmark@2.0.0-alpha.1`; its `license.workspace = true` conclusion was separately reviewed as MIT. A later model-pack archive integration added `pkg:cargo/bzip2@0.5.2` and `pkg:cargo/bzip2-sys@0.1.13+1.0.8`; both exact cached releases declare `MIT OR Apache-2.0` (`MIT/Apache-2.0` in the sys crate's upstream spelling). Therefore the live result is 964/964, while the original remediation set below remains exactly 67.

| SPDX conclusion | Manually reviewed exact purls |
|---|---|
| `MIT` | `pkg:cargo/alsa-sys@0.3.1`; `pkg:cargo/axum-core@0.4.5`; `pkg:cargo/axum@0.7.9`; `pkg:cargo/coreaudio-sys@0.2.18`; `pkg:cargo/data-encoding@2.11.1`; `pkg:cargo/h2@0.4.19`; `pkg:cargo/nom@7.1.3`; `pkg:cargo/npc-character-db@2.0.0-alpha.1`; `pkg:cargo/npc-identity-engine@2.0.0-alpha.1`; `pkg:cargo/npc-subtitle-engine@2.0.0-alpha.1`; `pkg:cargo/npc-system-telemetry@2.0.0-alpha.1`; `pkg:cargo/protoc-bin-vendored-linux-aarch_64@3.2.0`; `pkg:cargo/protoc-bin-vendored-linux-ppcle_64@3.2.0`; `pkg:cargo/protoc-bin-vendored-linux-s390_64@3.2.0`; `pkg:cargo/protoc-bin-vendored-linux-x86_32@3.2.0`; `pkg:cargo/protoc-bin-vendored-linux-x86_64@3.2.0`; `pkg:cargo/protoc-bin-vendored-macos-aarch_64@3.2.0`; `pkg:cargo/protoc-bin-vendored-macos-x86_64@3.2.0`; `pkg:cargo/protoc-bin-vendored-win32@3.2.0`; `pkg:cargo/protoc-bin-vendored@3.2.0`; `pkg:cargo/tokio-stream@0.1.19`; `pkg:cargo/tokio-tungstenite@0.27.0`; `pkg:cargo/tonic-build@0.12.3`; `pkg:cargo/tonic@0.12.3`; `pkg:cargo/tower@0.4.13` |
| `Apache-2.0` | `pkg:cargo/clang-sys@1.9.1`; `pkg:cargo/cpal@0.15.3`; `pkg:cargo/oboe-sys@0.6.1`; `pkg:cargo/oboe@0.6.1`; `pkg:cargo/prost-build@0.13.5`; `pkg:cargo/prost-types@0.13.5` |
| `BSD-3-Clause` | `pkg:cargo/bindgen@0.72.1`; `pkg:cargo/sha1_smol@1.0.1` |
| `ISC` | `pkg:cargo/libloading@0.8.9` |
| `CDLA-Permissive-2.0` | `pkg:cargo/webpki-roots@0.26.11` |
| `Apache-2.0 OR MIT` | `pkg:cargo/alsa@0.9.1`; `pkg:cargo/cexpr@0.6.0`; `pkg:cargo/minimal-lexical@0.2.1`; `pkg:cargo/pin-project-internal@1.1.13`; `pkg:cargo/pin-project@1.1.13` |
| `MIT OR Apache-2.0` | `pkg:cargo/coreaudio-rs@0.11.3`; `pkg:cargo/dasp_sample@0.11.0`; `pkg:cargo/fixedbitset@0.5.7`; `pkg:cargo/httpdate@1.0.3`; `pkg:cargo/hyper-timeout@0.5.2`; `pkg:cargo/itertools@0.13.0`; `pkg:cargo/jobserver@0.1.35`; `pkg:cargo/multimap@0.10.1`; `pkg:cargo/ndk-context@0.1.1`; `pkg:cargo/ndk-sys@0.5.0+25.2.9519653`; `pkg:cargo/ndk@0.8.0`; `pkg:cargo/num-derive@0.4.2`; `pkg:cargo/petgraph@0.7.1`; `pkg:cargo/prettyplease@0.2.37`; `pkg:cargo/rand@0.8.8`; `pkg:cargo/rand_chacha@0.3.1`; `pkg:cargo/sha1@0.10.7`; `pkg:cargo/shlex@1.3.0`; `pkg:cargo/socket2@0.5.10`; `pkg:cargo/tungstenite@0.27.0`; `pkg:cargo/utf-8@0.7.6`; `pkg:cargo/windows-core@0.54.0`; `pkg:cargo/windows-result@0.1.2`; `pkg:cargo/windows@0.54.0` |
| `MIT AND BSD-3-Clause` | `pkg:cargo/matchit@0.7.3` |
| `BSD-2-Clause OR MIT OR Apache-2.0` | `pkg:cargo/mach2@0.4.3` |
| `Apache-2.0 OR ISC OR MIT` | `pkg:cargo/rustls-pemfile@2.2.0` |

The exact machine-consumed conclusions are the entries in `dependency-licenses.json`; this document is review evidence and must not be parsed as a replacement ledger.
