# Provider catalog

`v1/catalog.json` is the checked-in development catalog for provider routes,
local model-pack templates, privacy classes, lifecycle gates, and semantic voice
intents. It contains no API keys, bearer tokens, or executable model payloads.

The Rust loader in `crates/provider-catalog` rejects unknown JSON fields,
invalid cross-references, unsafe automatic fallbacks, and incomplete pack
templates. Production callers must use `CatalogDocument::load_with_trust` with
`TrustPolicy::RequireSigned`; the checked-in artifact is intentionally unsigned
and accepted only with `DevelopmentAllowUnsigned`.

## Signing contract

Sign exactly the bytes returned by `CatalogDocument::signing_bytes()` with an
Ed25519 release key. Add the detached Base64 signature and stable key ID to the
top-level `signatures` array. Signature verification is injected through the
`SignatureVerifier` trait so the runtime can use its audited crypto/key-rotation
implementation. A catalog revision must only increase.

## Discovery contract

Provider discovery happens outside this crate. Supply sanitized discovery
results to `Catalog::with_discovery`. Curated entries win on a
provider/upstream-ID/modality collision. Unknown entries are assigned a stable
content-derived ID, marked `experimental`, and prohibited from default or
automatic fallback routing until a later signed catalog qualifies them.

`$user_selected`, `$provider_default`, and `$user_imported_*` upstream IDs are
route slots, not provider model aliases. They ensure the catalog never silently
remaps a user to a changing hosted model.

Cohere is an optional hosted text route for Chat, Embed, and Rerank. Its
`usage_tiers` distinguish evaluation-only trial keys from production keys; the
application must not treat a working trial credential as production readiness.
The capability flags reflect Cohere's model-list endpoint, Chat SSE streaming,
and model-dependent structured outputs. Embed and Rerank are intentionally not
marked as streaming or automatic fallback targets. They are `catalog_only` and
cannot be selected because this repository implements Cohere Chat, not Cohere
Embed or Rerank adapters.

NVIDIA NIM is one multi-service hosted provider record spanning Chat,
embedding, rerank, ASR, and TTS metadata. Chat has a dedicated implemented
adapter, but remains experimental, explicitly selected, non-default, and never
an automatic fallback. The exact `nvidia/nemotron-3-embed-1b` embedding route
has implemented transport/runtime integration plus a successful synthetic
hosted endpoint probe; it remains experimental, explicit, non-default, and
non-fallback. Rerank transport/fixtures exist, but tested hosted routes were
unavailable, so rerank remains `catalog_only`. Nemotron Streaming ASR has a
fixed implemented NVCF/Riva gRPC adapter but remains non-selectable with live
audio qualification pending. Magpie TTS has green fixed-adapter tests and
authorized HTTP stock-voice/audio evidence, so it is implemented but remains
experimental/manual with a live gRPC streaming warning. It uses only discovered
stock voices and structurally forbids cloning. Other account-specific hosted
speech endpoints and service-specific protocol/function
metadata remain unqualified; catalog presence is not evidence of a free hosted
endpoint.

The NVIDIA Developer Program/API Catalog entry is strictly a one-account
experimentation route. Its free hosted endpoints are for prototyping,
development, testing, and evaluation, with dynamic model-specific limits, and
grant no production or commercial entitlement. A Developer API key may cover
API Catalog model endpoints available to that account, but availability is not
universal. Do not send confidential, controlled/sensitive, personal, or secret
game data. Ordinary session content is generally not stored after the session
unless a service-specific disclosure says otherwise. The same governing terms
also disclose product/model-improvement collection plus security/fraud/abuse
logging, and specially disclosed services may retain data. Exact endpoint/model
terms and licenses always apply. See NVIDIA's [NIM developer access](https://developer.nvidia.com/nim),
[API quickstart](https://docs.api.nvidia.com/nim/re/docs/api-quickstart),
[NIM FAQ](https://forums.developer.nvidia.com/t/nvidia-nim-faq/300317), and
[API Trial Terms](https://assets.ngc.nvidia.com/products/api-catalog/legal/NVIDIA%20API%20Trial%20Terms%20of%20Service.pdf).

The UI may recommend NIM as an experimentation convenience for users bringing
their own Developer API key because one account/key can cover multiple available
preview services. That recommendation never selects a model, changes a default,
authorizes egress, enables an unimplemented modality, or promises durable/free
production capacity.

## Route availability

Catalog presence is not runtime support. Every model route has one state:

- `implemented_adapter`: an adapter and fixture conformance tests exist; normal
  credential, lifecycle, live-qualification, and fallback policy still apply.
- `catalog_only`: descriptive/provider-discovery metadata with no runtime
  adapter. It is never selectable, stable, default, or an automatic fallback.
- `pack_candidate_unqualified`: a local candidate with no qualified installed
  pack. It is never selectable, stable, default, or an automatic fallback.
- `installed_qualified`: a model-manager promotion backed by a signed manifest,
  runtime self-test, explicit license approval, and benchmark report. Promotion
  does not automatically make the route a default or fallback.

The checked-in development catalog contains no `installed_qualified` entries.
`Catalog::with_qualified_install` creates that state only from complete evidence.
For hosted routes, `live_qualification: pending` or `endpoint_unavailable`
blocks selection even when the adapter itself is implemented. Synthetic probe
results never count as benchmark evidence.

Automatic fallback is disabled catalog-wide. Every route has
`automatic_fallback_candidate: false`, and validation rejects both an enabled
global fallback policy and any future automatic candidate. Named loadouts may
contain several selectable provider/model routes, but changing the active route
is a separate, explicit user action; catalog discovery, provider errors, and
resource degradation never authorize a provider or model switch.

## Model packs

Pack templates describe required integrity and provenance fields; they are not
download manifests. The model manager instantiates a template only after it has
an immutable source revision, SHA-256, byte size, runtime ABI, platform,
architecture, measured resource envelope, license terms, attribution, and a
passing self-test. Candidate downloads remain blocked until qualification.
The required manifest fields also include the catalog signature, explicit
license approval, and benchmark report.

## Voice intents

Voice intents are provider-neutral acoustic goals. Use
`Catalog::select_voice_intent(character_or_encounter_id, required_tags)` for
repeatable background-NPC assignment. Provider adapters may map the selected
intent to an available voice, but must not imitate performers or clone game
audio.
