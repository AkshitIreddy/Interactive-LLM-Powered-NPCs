# World, character, resource, and preference workspace audit

Date: 2026-09-05
Scope: `ProductWorkspaces.tsx` and `ProductPreferencesWorkspace.tsx` as reached from the current `ProductConsole` World, Voice & Models, Diagnostics, Session, and Settings & Guide routes.

Snapshot notice: “current” in the audit tables means the pre-edit September 5
source. The mounted workspaces now include the revised job hierarchy, data-only
content pack preview/activation, persistent character overrides, and private
character mouth-pack controls. Those controls retain native fail-closed gates;
they do not prove an active vision provider, live capture, or visual quality.
See [the reconciled review record](local-review-2026-09-05.md).

## Evidence method

This is a source functional audit completed before changing either component. It traces visible controls through the React bridge to registered Tauri commands and their Rust owners. It also compares the relevant behavior with immutable v1 revision `503ef3b64a921b6a11efa9e3e0432a0c3de3b619` through `git show`; no legacy notebook, pickle, generated script, credential, or model was executed.

The status words below mean:

- **Working**: a visible action reaches a native command and the command owns a checked state or mutation.
- **Working, bounded**: the action is real, but it deliberately stops before a broader capability that is unavailable or unsafe.
- **Presentation-only**: the value is supplied for display and has no mutation on this surface.
- **Misleading hierarchy**: the underlying data is real, but the layout or wording gives implementation evidence more weight than the user's task.
- **Unavailable by design**: the control is disabled with a sourced reason and does not pretend to work.

## Current 2.0 source audit

### World: game and process selection

| Visible element | Source and effect | Status | Required change |
| --- | --- | --- | --- |
| Game profile selector | Receives native bootstrap `NativeGameProfileSummary[]`; changing it updates `ProductConsole` state and reloads the game-scoped character inspection. | Working | Lead with the selected game's display name and concrete bundled status. Keep IDs and safety metadata secondary. |
| Discover running windows | `discover_game_targets` enumerates fresh top-level Windows processes and filters them through the selected profile's process rules. | Working | Rename to the shorter action “Find running game”; make its empty result actionable. |
| Offline single-player checkbox | Passed to `select_game_target` as an operator statement. The Rust owner explicitly keeps `capture_authorized: false`. | Working, bounded | Keep next to the bind action. Say “required to select” and avoid implying it proves anti-cheat or capture safety. |
| Candidate list and Select action | Each candidate carries PID, HWND, executable leaf/path digest, title, foreground state, and client size. Selection re-observes the process and creates a current-session binding. | Working | Make title/executable primary; put PID/HWND and digest-level evidence in an advanced disclosure. |
| Current selection | Read through `selected_game_target`; it is intentionally session-only and revalidated against a fresh observation. | Working | Present this as the current session, with one concise visual-capture blocker. |
| Clear binding | `clear_game_target` clears the manager state. | Working | Keep as a secondary action beside the selected target. |
| Verify ordinary game capture | Disabled because ordinary targets lack trusted offline/protection evidence. | Unavailable by design | Remove the disabled pseudo-action from the main path. State the blocker once in the selected-target status. |
| Select character in game | Starts the native, capture-excluded manual actor picker and polls its status; cancellation reaches a native command. | Working, bounded | Keep under an “In-game targeting” disclosure because it needs a selected target and is not the authored-character roster choice. |
| Source/provenance paragraphs | Repeat WebView/native boundaries, current-session scope, and safety limitations around every action. | Misleading hierarchy | Replace with one concise status near the affected action and one advanced “Technical evidence” disclosure. |

The bundled profile summary currently exposes ID, display name, catalog state, safety mode, wave, and default fallback. It does **not** expose full detection/build readiness, authored capability detail, or a profile-detail navigation action to this component. The redesigned surface may show the supplied summary, but it must not invent a full profile inspector.

### World: game-scoped character selection

| Visible element | Source and effect | Status | Required change |
| --- | --- | --- | --- |
| Character roster | `character_database_catalog` loads characters from the selected `GameProfileV2` through the native resource catalog. | Working | Present the roster immediately after game selection, labeled with the selected game. |
| Inspect character | `character_database_inspection` resolves the requested/default/persisted ID and loads authored data plus delivered-memory records from the scoped native store. | Working | Treat clicking a roster row as inspection, distinct from choosing the character for turns. |
| Use for ordinary turns | `select_game_character` validates the character against the selected game and atomically persists a per-game selection. | Working | Rename to “Use this character” and surface selected state in the main heading. |
| Biography/personality/dialogue style | Comes from the authored profile. | Presentation-only | Keep a concise role and voice summary in the ready state; move long biography/style material under details. |
| Identity strategy | Comes from the authored profile and currently claims no automatic recognition for the browser fixture. | Presentation-only | Show the fallback in plain language. Put raw strategy names/evidence under advanced details. |
| Authored knowledge | Comes from the game profile with authority, spoiler tier, and provenance ID. | Presentation-only | Collapse by default because it is reference material, not a selection prerequisite. |
| Provenance | Comes from profile provenance records. | Presentation-only | Collapse by default and retain exact source/review fields without turning them into marketing claims. |
| Delivered memory | Queries only delivered records for the exact game/character scope. Fresh state is empty. | Working | Keep count in the main summary; show records under a disclosure. Never infer quests or relationships. |
| Memory backup/restore/delete/erasure | Reaches native SQLite lifecycle commands with explicit two-step confirmations, integrity checks, and receipts. Whole-store backup scope is real. | Working | Keep status and guarded actions visible so destructive scope is clear before interaction. Verbose receipts and backup inventory may use disclosures. Preserve precise confirmation language. |
| Browser Mara record | A hard-coded preview object, explicitly labeled as non-native and immutable. | Presentation-only | Preserve the honest preview boundary without letting its architecture copy dominate the page. |

The source audit also found a React lifecycle defect in the character loader. `CharacterDatabase` depended on the parent’s inline `onSelectionChange` callback, so inspecting a native character could repeatedly reload the catalog and update the parent. The workspace now keeps the latest callback in a ref and reloads only when the selected game or native availability changes.

### Session: unknown encounter lifecycle

The correction and merge controls reach native commands, are scoped to an active encounter, require an explicit confirmation, and return mutation receipts. This is a working but advanced recovery tool. It should remain subordinate to the current encounter and authored-character decision; native encounter IDs and receipt details belong in a disclosure.

### Voice & Models / Diagnostics: local resources

| Visible element | Source and effect | Status | Required change |
| --- | --- | --- | --- |
| Game VRAM/RAM reserve, ceilings, warm/unload windows, residency | Read and saved through native local-resource settings. Inputs are range checked in the UI and validated by the native owner. | Working | Keep the common game reserve and residency controls easy to reach. Put ceiling and lifecycle tuning in “Advanced resource policy.” |
| Device/resource observations | Native timestamped observations distinguish available from unavailable; missing values are not converted to zero. | Working | Show a short fit summary first. Move raw adapter, PID, per-process pressure, and all observation provenance into details. |
| Selected-loadout admission | Uses the exact native-selected signed pack identities, recollects telemetry, verifies measured envelopes, and persists a receipt only on success. | Working | Show one “Check this loadout” action and one concise result. Put receipt arithmetic and residency decisions in details. |
| Signed local runtime catalog | Native signed catalog and optional-pack lifecycle commands own identity, URL, size, hash, license acceptance, install/repair/remove, and cancellation. | Working, bounded | Keep installed/available packs visible. Collapse full runtime, license, measurement, and trust metadata under each pack. |
| This PC benchmark | Starts/cancels a bounded native benchmark and reads timestamped reports. | Working | Retain as a separate task; avoid making it a prerequisite unless admission reports that it is required. |
| Experimental visual signal pack | Explicit debug/local-review lifecycle; it is not a complete lip-sync model and cannot activate without measured admission. | Working, bounded | Hide the whole developer-only experiment under an advanced disclosure. It must not read like a normal product option. |

### Settings: product preferences

| Visible element | Source and effect | Status | Required change |
| --- | --- | --- | --- |
| Global/game/character scope | Native preference manager resolves exact inheritance and writes a revision-checked JSON document atomically. | Working | Turn scope into the first clear choice and show the human-readable selected game/character beside it. |
| Execution and performance presets | Persisted by the native manager. The current `ProductConsole` consumes execution/performance after a snapshot callback; this does not activate a provider route or admit a model. | Working, bounded | Label as defaults. Keep route/admission caveat in one short note. |
| Input, subtitles, verbosity, response length, creativity | Persisted with inheritance. The current console consumes input mode and subtitles; other values appear in effective configuration and may depend on runtime support. | Working, bounded | Group “Conversation” and “Presentation” separately. Do not present all fields as equally immediate. |
| Memory, emotion, vision, webcam presence | Persisted intent. Webcam is expressly consent intent and opens no camera. | Working, bounded | Put optional capabilities in an advanced disclosure, with unavailable consequences stated once. |
| Overlay override | Persisted and projected as `presentation.overlayEnabled`. No non-test consumer of `effective.overlay` was found in the current control/runtime source. | **Persisted only; runtime effect unproven** | Do not imply that saving it immediately shows an overlay. Keep the operable saved override, label it “Overlay request,” and state that native presentation still fails closed until a trusted target/context is available. Runtime consumption needs a separate backend change. |
| Effective data egress | Derived by the native preference resolver from execution preset and capability intent. It is a snapshot, not proof that a provider turn enforced the route. | Presentation-only | Collapse under “Privacy and routing details.” Use “allowed for selected route” rather than network-safe marketing language. |
| Route/resource/egress proofs | Real snapshot metadata. | Presentation-only | Move into one technical-details disclosure with a compact route, admission, and game-image summary. Keep the persisted schema/revision state as a short saved-document note. |
| Save/reset/refresh | Native save and reset are revision checked; reset requires a second explicit click. | Working | Use direct names: “Save defaults,” “Remove overrides,” “Reload.” Keep native error text actionable. |

### Settings: subtitle typography and overlay presentation

| Visible element | Source and effect | Status | Required change |
| --- | --- | --- | --- |
| Bundled subtitle style | Native `SubtitlePreferenceManager` validates the selected style. On turn dispatch, `commands.rs` resolves the scoped renderer authority and passes it to the runtime. | Working and consumed | Present this as “Subtitle look” and show that it includes typography. Use readable labels for `cinematic_glass` and `accessibility_high_contrast`. |
| Font behavior | The style selects bundled font roles; the roles resolve to validated Windows/system fallback chains. Arbitrary font-family input is not in the supported schema. | Working through style | Make the effective body/speaker font roles visible near the style control. Do not offer an unsupported free-form font picker. |
| Safe area, text scale, backplate, opacity | Scoped, revision-checked overrides; resolved renderer parameters are pinned into the next turn. | Working and consumed | Keep as the operable overlay appearance controls. Use sliders or bounded numeric inputs with explicit effective values. |
| Font/license disclosure | Validated role/fallback/license metadata from bundled manifests; system lookup remains a candidate, not installation proof. | Presentation-only | Keep under “Font fallback and licenses.” |
| Renderer/migration proof | Native authority metadata and supported-field list. | Presentation-only | Move into technical details. |

## V1 comparison relevant to these workspaces

V1 had no coherent desktop workspace hierarchy. The notebook and helper modules selected a game through folder conventions, searched every known-character image directory with DeepFace/Facenet512, loaded character/public Chroma stores and mutable JSON conversation files, and ran per-character `voice.py` or SadTalker subprocesses. The useful product ideas were game-scoped known characters, per-character lore/style/voice, push-to-talk, and an audio-only fallback.

The 2.0 native catalog, explicit game/character IDs, versioned provenance, delivered-only memory, OS-owned selection state, signed pack identities, and scoped preference inheritance are materially better foundations. V1's single-frame identity search, pickle/Chroma deserialization, plaintext credential rotation, generated Python execution, demographics, file-loop orchestration, and frozen full-face video path must stay retired.

## Replacement information architecture

### World workspace

```text
Choose a game
├── selected bundled profile summary
└── game selector

Connect this session
├── Find running game
├── eligible windows or one actionable empty/error state
├── offline single-player confirmation
└── selected process + Clear
    └── Technical evidence (PID/HWND/hash/safety boundary)

Choose who answers
├── game-scoped roster
├── selected character, role, voice, memory count
├── Use this character
└── Character details
    ├── biography and dialogue style
    ├── identity fallback
    ├── authored knowledge
    ├── provenance
    ├── delivered memory
    └── Manage local memory

In-game targeting (advanced)
└── start/cancel native actor picker
```

### Settings workspace

```text
Apply to: Global | This game | This character

Conversation defaults
├── execution / performance
├── input / interruption
├── response length / verbosity / creativity
└── Save / remove overrides / reload

In-game presentation
├── subtitles enabled
├── overlay request
├── subtitle look (style includes font roles)
├── safe area / text scale / backplate / opacity
└── Save subtitle look / remove overrides

Optional capabilities (advanced)
└── memory / emotion / vision / webcam consent

Privacy and routing details (advanced)
├── effective egress
├── route and resource snapshot
└── effective configuration and migration provenance
```

### Resource workspace

```text
Can this PC run the selected local loadout?
├── ready / blocked / needs benchmark
├── game reserve + residency preference
├── Check this loadout
└── Admission evidence (advanced)

Installed and available local packs
├── pack state + normal lifecycle action
└── Runtime/license/measurement details (advanced)

Advanced resource policy
├── soft ceilings / warm / unload
├── raw telemetry
└── experimental review pack
```

## Acceptance map for this edit

1. The World path reads in the order game → running process → character, and each section has a clear heading and one primary action.
2. A fresh native state gives one actionable empty state: select a profile, start the game, then choose **Find running game**. Browser preview clearly says native discovery is unavailable.
3. The selected process card names the game window first. PID/HWND and safety evidence remain available under a keyboard-accessible disclosure.
4. Character rows remain strictly scoped to the selected game. Inspecting a row never silently changes the persisted turn character; **Use this character** does.
5. The character ready state shows role, voice, identity fallback, and delivered-memory count without requiring raw provenance to be open.
6. Authored knowledge, provenance, delivered records, actor-picker evidence, raw resource telemetry, admission arithmetic, experimental pack evidence, and action explanations are closed by default in semantic `<details>` groups. Guarded local-memory actions stay visible with explicit scope and confirmation language.
7. Product and subtitle scope changes reload native state without stale snapshots. Save/reset errors remain announced with `role="alert"`; successful mutations use `role="status"`.
8. The saved overlay field is labeled as a request and does not claim immediate presentation. Subtitle style, safe area, text scale, backplate, and opacity remain operable and are identified as next-turn native renderer inputs.
9. Subtitle styles have human-readable names and expose their effective font roles. No unsupported font download or arbitrary family picker appears.
10. The browser state does not render native-only controls as working, and disabled actions retain concise reasons.
11. Existing native command names and request shapes do not change in this UI-only slice.
12. Focused React tests cover discovery/binding, persisted character choice, preference save/reset, subtitle style/font behavior, and collapsed advanced disclosures. A headless native-state integration pass checks that the character workspace settles without a render loop. Typecheck passes.
13. Headless rendered verification covers World and Settings at wide and narrow viewports with no horizontal overflow. Technical disclosures are checked both closed and open.

## Out-of-scope blockers that must remain visible to the full product rework

- Ordinary commercial-game capture remains blocked because the current selected-target owner has no trusted offline/protection evidence.
- The profile summary supplied here is not a full profile detail model.
- The overlay preference has no proven runtime consumer in the audited source.
- Arbitrary font-family overrides are unsupported; bundled style selection is the real typography control.
- Identity enrollment remains unavailable until a signed, measured identity pack is admitted.
- The experimental visual-signal pack is not a qualified lip-sync solution.
