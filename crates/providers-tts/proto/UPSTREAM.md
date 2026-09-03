# NVIDIA Riva protobuf provenance

These files are copied without semantic modification from the MIT-licensed
`nvidia-riva/common` repository:

- tag: `r2.17.0`
- commit: `a7d342c`
- source path: `riva/proto`
- retrieved: 2026-08-30

Pinned SHA-256 values:

- `riva_tts.proto`: `1b9d4fe77999f8f7529e67d1efb71b8d817eeb93af2e39ce8deee54c54285a5a`
- `riva_audio.proto`: `7ab2939dfe9f5ed121eba1c6b86cad0cd561fa483acfc653762a8b8af0b2a123`
- `riva_common.proto`: `07d668e74d7c2c4e01fa5fee5323b9f6aff2f57f86f4f00d51656b9cb2a3a377`
- `LICENSE`: `75a6506ede3eccb41ac95de4428055d83d962d343867314753c9222b7744441f`

The vendored license adds only a final newline; the upstream byte checksum is
`9c0bb53336319c13d061a2246eea4f3f216bad31cdeb1fc9ff0fd8d8308dd543`.

The build uses a version-pinned, vendored `protoc` executable. It never fetches
protobuf definitions or code generators from the network.
