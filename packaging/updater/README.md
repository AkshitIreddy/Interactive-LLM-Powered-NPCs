# Updater is intentionally inactive

`local-feed.example.json` has no endpoints or trust key and cannot activate an updater. It exists so repair, rollback, and installer tests can target a loopback fixture in an isolated test environment later.

Do not add a production endpoint, updater public key, signing secret, CI publication step, or release automation without explicit user approval and a reviewed key-rotation/rollback design.
