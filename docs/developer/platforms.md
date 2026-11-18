# Platforms

The rules any new platform follows, decided by Peter on 2026-11-13 ([#1585](https://github.com/peterhudson/timegrapher/issues/1585)). Today the app runs on Linux, macOS and Windows; no other platform has been chosen.

## 1. Full bench parity

A platform is supported with every bench function or not at all, as the app, CLI and MCP already are with each other.

> "I'm not interested in putting out a crippled or lesser version of the software on any of these platforms. A strong principal of what we've built so far is that everything that's available in the app is available on the CLI and through the MCP. [...] If we opt to support a platform it means that we support every (bench) function on that platform."

## 2. Touch-first apps for phones and tablets

A phone or tablet version gets its own interface, designed for touch from the start, not the desktop layout made smaller.

> "For mobile/tablet, I envision a completely new UX that's designed around touch first. The UX we have now for desktop is a UX for desktop. [...] The touchscreen versions of our app will be their own UX designed from the ground up as native touch interfaces."

## 3. When

- **2026-11-13:** the web version and the touch apps were both parked until after v1.1 ([decision](https://github.com/peterhudson/timegrapher/issues/1585#issuecomment-6860273162)).
- **2026-11-18:** "mobile doesn't belong in 1.3" ([Peter](https://github.com/peterhudson/timegrapher/issues/1585#issuecomment-6973220145)).

## Still open

- **Which platform comes first,** if any.
- **Where a web version keeps its takes:** the user's own folders, or a sign-in with analysis-only takes on a server. Peter: "Let's put off the decision about where a web version would store its takes until we decide to actually make a web version." ([#1585](https://github.com/peterhudson/timegrapher/issues/1585#issuecomment-6860281211))

## What was found (2026-11-13, [findings](https://github.com/peterhudson/timegrapher/issues/1585#issuecomment-6860273162))

- Core builds unchanged for wasm32, Android and iOS.
- Live analysis in headless Chromium ran at 34× real time, against 45× natively, with matching readings.
- Whole-take analysis needs a one-thread mode or browser threads (`stream.rs`, `cores.rs`).
- Rough sizes, in PRs: a web version with every bench function, 25 to 40; server storage, 20 or more; a first touch app, 40 to 60, after a design round.
