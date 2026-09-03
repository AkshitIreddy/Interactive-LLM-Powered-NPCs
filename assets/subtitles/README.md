# Subtitle assets

This directory intentionally contains metadata only. It does **not** contain, fetch, install, or
redistribute font binaries. `fonts.v1.json` names platform font families in fallback order and the
renderer must resolve them through DirectWrite, CoreText, or Fontconfig. If a named family is not
installed, resolution continues to the next entry and finally the platform `sans-serif` family.

`styles.v1.json`, `fonts.v1.json`, and `licenses.v1.json` are original project assets licensed under
MIT. The machine-readable license file separately records the usage and redistribution status of
every referenced system family. Adding a bundled font requires adding its exact file hash, copyright,
SPDX expression, source URL, and redistribution status before the font can be packaged.

