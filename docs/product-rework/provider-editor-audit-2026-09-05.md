# Provider editor audit — 2026-09-05

Scope: `apps/control/src/ProviderLoadoutEditor.tsx`, its native bridge, and the
Voice/onboarding route that hosts it. This audit was written before editing the
component.

Snapshot notice: the defect table remains the before-edit audit. The current
editor receives game/character context, publishes accepted native snapshots,
and exposes qualified Gemini, Groq, Mistral, OpenRouter, Cohere, Cartesia,
Deepgram, and Inworld choices with fixed routes. Cloudflare and generic
OpenAI-compatible choices remain unavailable. Route visibility does not prove a
physical audio turn. See [the reconciled review record](local-review-2026-09-05.md).

## Product job

The editor should answer one question: **what exact voice-and-mind route will
this game character use on the next turn?** A player must be able to inspect
and change LLM, speech recognition, speech output/stock voice, and memory
retrieval without reading six simultaneous implementation cards. Vision and
lip-sync remain optional and must say why they are unavailable. Credentials are
managed by the native account surface; this editor stores provider/model IDs
and resolves native inheritance.

## Current route flow and authority

| Interaction | Current authority | Classification | Finding |
| --- | --- | --- | --- |
| Load native loadouts | `provider_loadout_snapshot` → protected JSON document | Working | Browser state is correctly replaced by the native snapshot. |
| Create/clone/rename/update/delete | Native provider-loadout commands when installed; `localStorage` preview otherwise | Working, with UX flaws | The protected native path is real and failures do not masquerade as commits. Browser-only changes need a clear preview label. |
| Activate/deactivate a scope | Native activation document | Working | Activation is next-turn safe; an in-flight turn keeps its snapshot. |
| Resolve inheritance | `review_provider_loadout` | Working policy review | It validates active global → game → character resolution and offline policy. It does **not** contact a provider or test credentials. |
| Connect/check credentials | Separate provider-account commands | Working elsewhere | The editor has no path to that surface, so a failed/missing hosted route is difficult to repair in context. |
| NVIDIA Magpie stock voices | Authenticated native discovery with expiry and provenance | Working, restricted | Selection is correctly blocked without an unexpired discovered provider-stock voice and exact private-evaluation acknowledgement. |
| Generic stock voice ID | Free-text field | Partially operable | It persists an ID but offers no discovery, label, preview, or membership proof for non-NVIDIA providers. It must be described as an identifier, not a tested voice. |
| Local models/packs | Static catalog options | Mostly unavailable | Local keyword memory is real. Local LLM/STT/TTS and mouth packs are not selectable in this route catalog; the old copy incorrectly sends users to the obsolete Models page. |
| Fallback routes | Persisted manual-only authorization | Working | The six-role grid makes this recovery feature dominate the main task. It belongs behind progressive disclosure. |
| Privacy/cost/receipt evidence | Catalog disclosure plus native review receipt | Real metadata, overexposed | Policy facts are useful, but the permanent wall of terms, IDs, and receipt fields overwhelms the basic choice. |

## Concrete defects

1. `makeLoadout` supplies `cyberpunk-2077` for every new game override and
   `eclipse-harbor/mara-venn` for every character override. The editor calls it
   without current World context, so newly created scopes can target the wrong
   game or character. The native bridge also has legacy fallback IDs if a
   target is omitted.
2. Every role is expanded at once. The result is a long six-card technical wall
   before activation, recovery, or native evidence. It is especially poor in
   onboarding.
3. The header describes architecture rather than showing the effective target
   and the four essential choices. The useful route is not readable at a
   glance.
4. “Review active route” can be mistaken for a live provider test. Its own copy
   says credentials and network are untouched, but this distinction appears
   late.
5. Hosted-route repair is disconnected from the route that needs the account.
6. NVIDIA legal/policy fields, manual recovery, and low-level native receipt
   fields are always expanded. They are necessary evidence, not the primary
   workflow.
7. The editor exposes visual research beside the required conversation chain,
   although no lip-sync pack is selectable. The primary setup path should
   remain honestly complete with audio and subtitles.
8. Existing target labels are partly prettified from hard-coded IDs. The label
   is display metadata only; native scope IDs are the authority.

## Version 1 comparison

The immutable v1 code at `503ef3b64a921b6a11efa9e3e0432a0c3de3b619`
hard-coded Cohere generation/embeddings, Google speech recognition, and
character-specific Edge TTS scripts. Known characters therefore retained a
stable voice, while a background character wrote a randomly selected Edge voice
to `voice.txt` for a ten-minute encounter window. The directness was useful:
the selected character implied a real prompt, memory store, and voice. The
implementation was unsafe and inflexible: plaintext rotating keys, dynamically
generated `temp.py`, subprocess speech scripts, provider coupling, untrusted
pickle/Chroma state, and demographic-driven background voice selection.

The 2.0 native loadout document is the correct replacement mechanism because it
keeps scoped inheritance, provider-neutral roles, credential references, and
next-turn snapshots. The editor should recover v1's direct character → voice
relationship by accepting the selected game/character context and showing the
effective voice prominently; it should not recover v1's code execution or
demographic inference.

## Replacement interaction

- Accept current game and character IDs/labels as props. Global editing always
  works. Game/character creation is disabled with a plain reason when its
  required context is absent.
- Show one role at a time with ordinary pressed buttons. The role rail carries
  provider/model summaries so the whole route remains scannable. Default to
  LLM, STT, or TTS as requested by the host.
- Keep the essential chain (LLM, STT, TTS, embeddings) first. Put vision and
  lip-sync in an optional group with honest disabled/unqualified states.
- Add an optional host callback that opens the native account surface for the
  selected hosted provider. Label native route review as validation, with a
  visible “no network request” boundary.
- Publish accepted native loadout snapshots to the host through a callback so
  the Session signal rail, push-to-talk readiness, and onboarding summary use
  the same native authority. Do not copy native documents into browser storage.
- Collapse manual recovery, NVIDIA terms/evidence, provider disclosures, and
  the full native receipt. Surface only the actionable summary until expanded.
- Preserve all native CRUD, activation, stock-voice discovery, exact terms
  acknowledgement, fallback authorization, and immutable in-flight behavior.

## Acceptance map

| State/action | Required evidence |
| --- | --- |
| Global route | All four essential roles are reachable by keyboard and their selected provider/model is visible without expanding six cards. |
| Scoped creation | Supplied game/character IDs reach the native create payload; no unrelated hard-coded game or character appears. Missing context blocks only the unavailable scope. |
| Hosted account handoff | The selected provider ID is passed to the host callback; no credential value enters the WebView. |
| Stock voice | NVIDIA remains blocked until native discovery; generic voice ID is labeled unverified until the provider path proves it. |
| Native validation | Review receipt states active leaf, inheritance, configured roles, offline policy, and explicitly `no provider contact`/`credentials not checked`. |
| Local/optional routes | Unavailable packs have a disabled reason and no install/download claim. FTS remains presented as local keyword retrieval, not embeddings. |
| Onboarding mode | Same native editor is usable in a shorter embedded mode; advanced policy and receipt evidence are collapsed. |
| Failure | Native mutation failure leaves the selected loadout inactive/uncommitted and gives one actionable message. |

## Executable route boundary rechecked during implementation

- LLM at inspection: runtime constructors existed for OpenAI, Anthropic, Gemini,
  Groq, Cohere, and NVIDIA NIM. Follow-up source adds fixed-origin Mistral and
  OpenRouter routes. The separately cataloged generic OpenAI-compatible endpoint
  has no trusted runtime constructor and is not offered here.
- STT: the complete selected push-to-talk bridge currently accepts only
  AssemblyAI `u3-rt-pro` (Universal-3 Pro Streaming). Other STT catalog entries
  remain visible as unavailable evidence so an inherited stale route can be
  repaired; they cannot be newly selected.
- TTS at inspection: ordinary execution supported ElevenLabs and isolated
  private NVIDIA Magpie evaluation. Follow-up source adds fixed-origin Cartesia,
  Deepgram Aura, and Inworld construction and selectable stock voices; the
  Cartesia component chain is measured, while physical output remains open.
- Retrieval: NVIDIA Nemotron Embed 1B is the executable hosted embedding route.
  SQLite FTS5 is the real local keyword fallback and is explicitly described as
  having no embeddings. Cohere embedding metadata remains disabled.
- Vision and lip-sync: no current product turn executes a qualified live route.
  Both default to Off; research candidates remain unavailable and cannot be
  activated from the editor.
