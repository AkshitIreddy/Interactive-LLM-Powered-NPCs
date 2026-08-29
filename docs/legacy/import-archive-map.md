# Reversible legacy import and archive map

## Policy

Migration is quarantine-first and non-destructive. The importer reads from an immutable export of the audited revision, writes a new staging directory/database, emits a manifest and validation report, and never edits the legacy source. Activation is a separate atomic step. Re-running the same importer version with the same inputs must produce the same normalized output and evidence manifest.

No legacy Python module is imported. No pickle, Chroma binary/parquet index, notebook output, generated program, SadTalker environment, or credential file is deserialized or executed.

## Source map

| Legacy source | Content | Action | Destination/evidence | Rollback |
| --- | --- | --- | --- | --- |
| Git revision `503ef3b…` | Complete historical baseline | Preserve immutable Git reference/archive | Archive manifest records revision and file hashes | Delete only the new import; original Git object remains. |
| `Cyberpunk_2077/world.txt` | World description | Quarantine, provenance review, normalize text if accepted | `GameProfileV2` canon/lore source with source hash and reviewer status | Deactivate imported profile version. |
| `Cyberpunk_2077/public_info.txt` | Public lore | Same as world text | Scoped lore records with provenance and spoiler level | Revert profile activation pointer. |
| Character `bio.txt` | Biography/persona prose | Review originality, factuality, spoilers and license; transform if accepted | Character content records with stable profile/character IDs | Restore prior profile version. |
| `character_knowledge.txt` | Character-specific facts | Review and split into atomic provenance-bearing entries | Character knowledge records, never a binary index | Rebuild derived search indexes or deactivate batch. |
| `pre_conversation.json` | Style examples | Validate JSON, sender/content, copyright and safety; deduplicate | Curated style examples with deterministic ordering | Remove import batch by ID. |
| `conversation.json` | Mutable prototype dialogue | Exclude by default; opt-in personal import only | Immutable turns marked `legacy_unverified`, never canon | Delete opt-in batch; raw archive remains. |
| Default `name.txt`, `voice.txt`, `timestamp.txt`, generated `bio.txt` | One mutable background identity | Do not ship/import automatically | Optional user-export report only | No activation occurs. |
| Character image JPEGs/default face | Game-derived/reference faces | Exclude pending rights/provenance review | User-local identity evidence only when permitted | Delete local evidence record and derived embeddings. |
| `representations_facenet512.pkl` | Derived face embeddings/pickle | Reject without opening | Report `REJECTED_UNSAFE_DERIVED` with hash | Nothing activated. |
| Chroma `.pkl`, `.bin`, `.parquet` | Derived retrieval store | Reject without opening | Report and rebuild from accepted source text using pinned v2 model | Drop derived v2 index namespace and rebuild. |
| Per-character `voice/voice.py` | Executable TTS wrapper | Archive only; never import | Manually map non-secret voice intent to provider-neutral traits if rights allow | Remove mapping record. |
| `default_voices/*.json` | Edge voice catalog snapshot | Archive only; do not trust as current | Query/curate provider catalog at runtime; retain no demographic inference | Revert curated catalog version. |
| `apikeys.json` | Plaintext secrets | Never copy or log; require external revocation/rotation | Credential setup asks user for fresh value and stores only OS credential reference | Delete credential entry through app settings. |
| Root notebooks and `functions/` | Prototype runtime | Historical archive only | Audit and deterministic behavior fixtures | V2 build never references archive. |
| `miscellaneous/Single Monitor/**` | PrintWindow experiment | Historical archive only | Root-cause test requirements | V2 build never references archive. |
| `SadTalker/**` | Vendored third-party runtime | Historical archive only; exclude from product | Attribution/history record, no runtime/model activation | V2 build never references archive. |
| `temp/`, `video_temp/`, `nul`, notebook outputs | Ephemeral or accidental output | Ignore/reject | Import report lists ignored paths | Nothing activated. |
| `Games/Cyberpunk_2077/**` | Near-duplicate game tree | Compare by normalized path/hash; never merge silently | Conflict report; canonical selection requires explicit rule/review | Import batch remains inactive on conflict. |

## Import phases

1. **Snapshot:** record source revision, importer version, UTC timestamp, every input path, byte size and SHA-256. Secret paths are recorded by classification only; their contents and hashes are not exported because even a hash can become sensitive evidence.
2. **Classify:** allowlist UTF-8 text/JSON paths and reject executable, archive, model, pickle, database/index and media categories by default.
3. **Parse safely:** enforce file and aggregate size limits, reject symlinks/path traversal, decode strictly, use data parsers only, validate schemas, and never resolve a path outside the snapshot root.
4. **Normalize:** preserve raw source in the immutable archive, normalize line endings/Unicode only in staging, assign stable IDs, and split lore into explicitly scoped records.
5. **Provenance review:** require `original`, `licensed`, `user_private`, or `rejected` with source/attribution. Unknown is not distributable.
6. **Validate:** run profile schema, referential integrity, spoiler-scope, prompt-injection/content, duplication and deterministic replay checks.
7. **Dry-run report:** emit accepted/rejected/conflicting records and a complete destination diff without changing the active profile/database.
8. **Atomic activation:** commit the new profile version and import batch in one transaction, then switch a version pointer. Derived FTS/vector indexes are rebuilt after authoritative data commits.
9. **Rollback:** switch back to the prior profile version and delete only rows/derived namespaces owned by the import batch. Preserve the audit report.

## Duplicate-tree conflict rule

Neither `Cyberpunk_2077/` nor `Games/Cyberpunk_2077/` wins merely because of its path. For every allowlisted relative path:

- identical normalized content becomes one candidate with both source paths in provenance;
- content differing only by line endings becomes one normalized candidate with both raw hashes;
- substantive differences block that record and produce a side-by-side conflict;
- reviewers may select one version or author a new reconciled version, with the decision recorded.

## Import manifest minimum fields

```text
manifest_version
source_revision
importer_version
batch_id
created_at_utc
source_path_classifications[]
accepted_records[] {source_path, source_sha256, destination_id, transform_version}
rejected_records[] {source_path, classification, reason}
conflicts[] {logical_id, candidates[]}
provenance_decisions[]
validation_results[]
previous_active_profile_version
new_profile_version
```

The manifest contains no credentials, raw webcam frames, secret hashes, or unredacted personal transcript by default.

