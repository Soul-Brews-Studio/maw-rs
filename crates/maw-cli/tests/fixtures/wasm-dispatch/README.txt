import-bearing.wasm is a byte-for-byte copy of the committed Extism reference module
crates/maw-plugin-manifest/tests/fixtures/wasm-parity/triggers/plugin.wasm
(AssemblyScript 0.27.31 + extism as-pdk 1.0.0). It imports host functions
(the extism env imports) yet makes ZERO maw host calls on the empty-arg path, so it
runs deterministically on the shipping Extism runtime without a seeded host.
Regenerate: cp from the wasm-parity/triggers fixture.
