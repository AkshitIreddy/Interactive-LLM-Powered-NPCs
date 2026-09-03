# Model-pack metadata

`npc.model-pack/v2` is the single distributable manifest schema for language
models, speech recognition, speech synthesis, embeddings, vision signals, and
lip-sync. `model-pack-manifest.schema.json` is strict: every object rejects
unknown properties, archive artifacts name their exact format (`zip`, `tar`,
`tar_gz`, or `tar_bz2`), and role-specific facts live only in the matching
closed extension object.

The generic boundary always records:

- immutable source and runtime revisions, exact artifact byte lengths and
  SHA-256 values, destinations, archive extraction metadata, and self-test pins;
- platform/runtime/backend compatibility and hardware planning disclosures;
- component-level license, notice, redistribution, acceptance, and stock-voice
  bindings;
- explicit install/repair/removal gates and threshold-catalog trust policy;
- a required `npc.measured-resource-envelope/v1` qualification contract with at
  least 20 samples, current-device binding, signatures, and distinct p99 reload;
- an admission state and allowed residency modes. Unknowns always fail closed.

Manifest RAM, VRAM, and load values are planning disclosures only. They never
authorize activation. A pack still needs a threshold-signed catalog entry,
attested self-test, exact target-PID telemetry, a signed current-device measured
envelope, and whole-loadout admission. `null` means unknown; it is not zero.
CPU-only policy may use measured zero VRAM only in the signed envelope.

The stable `model-pack-manifest.example.json` is a non-installable shape fixture
using `.invalid` URLs. Runtime code consumes it through
`parse_and_normalize_model_pack_manifest`; it must not deserialize v2 directly
as the legacy core type.

## Migration

The Rust normalizer deterministically converts generic `npc.model-pack/v1` to
v2 while forcing `blocked_pending_measurement` and preserving former resource
values as non-admissible planning data. It never guesses artifact roles,
runtime entrypoints, network policy, license components, voices, or role facts.
The two historical role schemas require named, one-time adapters:

| Historical schema | Adapter |
|---|---|
| `npc.local-model-pack/v2` | `moonshine_local_model_v2_to_npc_model_pack_v2` |
| `npc.local-tts-pack/v1` | `local_tts_v1_to_npc_model_pack_v2` |

Converted real manifests must validate against the JSON schema and the Rust
normalizer before their raw SHA-256 is frozen. A previous benchmark bound to an
older manifest digest cannot qualify the converted revision.

## Catalog bundle

`model-catalog-root-v1.json` pins a threshold of Ed25519 public keys.
`model-catalog-v1.json` contains two payloads, each covered by threshold signatures:

1. the compatibility catalog consumed by the planner; and
2. a source inventory binding every distributed raw v2 digest, canonical v2
   digest, normalized core digest, admission/license state, and the exact set of
   qualified envelope report bindings.

An empty `qualified_envelopes` list is deliberate when no real signed
current-device qualification exists. Catalog inclusion does not make that pack
admissible. The checked root is explicitly
`automated_local_review_bootstrap`: both ephemeral keys were controlled by one
automated local-review run, so no organizational signer independence is
claimed. It sets `productionTrust: false`, requires rotation before release,
and forbids promotion/publication. For an actual release, regenerate offline
with independently controlled release signers:

```text
NPC_MODEL_CATALOG_SIGNERS=key-a=<64-hex-seed>,key-b=<64-hex-seed> \
  cargo run -p model-manager --example generate_release_catalog -- \
  packaging/model-packs packaging/model-packs <version> <generated-unix> <expires-unix>
```

The generator sorts identities, validates every real JSON document as canonical
v2, and emits deterministic bytes for the same inputs. Private signer seeds are
never stored in the repository. The catalog lifetime is capped at 31 days and
must be renewed with a monotonically increasing version. Tests reject missing
metadata, signature/content tampering, rollback, and inventory/catalog mismatch.

The base installer contains no large model. Do not infer pack redistribution
permission from the core application's MIT license, and do not add game audio,
performer clones, or research-only weights to a distributable pack.
