---
name: RustBot WASM plugins
description: The chosen extension model for the Rust rewrite of Heroku.
---

RustBot extensions should be built in Rust and loaded as WebAssembly modules; do not restore Python plugin compatibility.

**Why:** The user chose the recommended WASM option to keep the rewritten userbot in Rust while supporting separately developed modules.

**How to apply:** Keep the WASM runtime sandboxed. Add host capabilities only through an explicit, narrow plugin API.
