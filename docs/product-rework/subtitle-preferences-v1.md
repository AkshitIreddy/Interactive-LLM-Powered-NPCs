# Native subtitle preferences v1

## Product boundary

The native authority is `npc_subtitle_engine::SubtitlePreferenceManager`.
Control and the WebView must not create a second preference store or add fields
to the persisted JSON. The manager validates the bundled subtitle style, font,
and license manifests before it reads or mutates preferences.

The only persisted override fields are `safeAreaDp`, `textScale`,
`backplateEnabled`, and `opacity`. They map respectively to the layout/fallback
safe margin, DirectWrite body/speaker sizes, native backplate toggle, and native
renderer global alpha. Unsupported font, color, HDR, animation, download, and
arbitrary style fields are omitted from response DTOs and rejected on input.

## Persistence and inheritance

The file is `<native config directory>/subtitle-preferences-v1.json` with
`schemaVersion: 1`, a monotonic optimistic-concurrency `revision`, and unique
global/game/character scope entries. Resolution order is global, game, then
character. Each effective value reports one of these exact sources:

- `bundledManifestDefault` for the manifest's default style selection;
- `bundledStyle` with the exact style ID for a style-owned value;
- `rendererDefault` for native defaults such as 1.0 opacity/scale; or
- `persistedScope` with the exact global/game/character scope.

Writes use a same-directory temporary file, flush and sync its bytes, and then
atomically replace the destination. Schema v0 global preferences migrate once
to a global v1 scope with an incremented revision. Corrupt, oversized,
symlinked, or unknown future documents fail closed and are not overwritten.

## Control registration handoff

Tauri should own one manager in native application state and expose three thin
commands without revalidating or reshaping the DTOs:

| Suggested command | Manager call | Request DTO |
| --- | --- | --- |
| `read_subtitle_preferences` | `SubtitlePreferenceManager::read` | `ReadSubtitlePreferencesRequestV1` |
| `save_subtitle_preferences` | `SubtitlePreferenceManager::save` | `SaveSubtitlePreferencesRequestV1` |
| `reset_subtitle_preferences` | `SubtitlePreferenceManager::reset` | `ResetSubtitlePreferencesRequestV1` |

The settings UI should render `availableStyles`, `supportedOverrideFields`,
`effective.*.source`, and `assets.fontRoles`. A font entry marked
`systemLookupRequired` is a fallback candidate, not proof it is installed.
Reset requires both the current revision and explicit user confirmation. An
empty save is rejected so it cannot bypass reset confirmation.

The runtime adapter should call `resolve_for_renderer` and pass only its
validated `SubtitleStyle` plus `global_opacity` into the existing native
shaping/renderer boundary. Physical-pixel font limits must still be checked
after trusted DPI conversion; persistence never authorizes a render outside
the presenter's bounds.
