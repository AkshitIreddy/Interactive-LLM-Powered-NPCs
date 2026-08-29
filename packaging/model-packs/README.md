# Model pack boundary

The base installer contains no large model. Each real pack must have a schema-valid manifest, immutable upstream revision, exact artifact sizes and SHA-256 hashes, compatible runtime ABI/backend information, measured resource estimates, complete license and attribution terms, and a bounded self-test.

The example manifest is deliberately non-installable and uses the reserved `.invalid` domain. It demonstrates shape only; it is not catalog metadata.

Catalog and pack metadata will be authenticated with reviewed TUF roots. Do not infer redistribution permission from the core application's MIT license, and do not add game audio, performer clones, or research-only weights to a distributable pack.
