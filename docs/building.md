# Building from source

The supported source-build compiler floor is **Rust 1.90.0** for all four
workspace packages, using the checked-in `Cargo.lock`. This is a support policy
exercised by CI, not proof that each package requires every feature of that
compiler or cannot compile with an older compiler. Older compilers and arbitrary
new dependency resolutions are not covered by this policy.

## Packages and features

| Package | Build target and features | Linux requirements |
| --- | --- | --- |
| `timegrapher-core` | Library; no default features. Optional `capture` and `update-check`. | `capture` needs ALSA development headers (`libasound2-dev` on Ubuntu). |
| `timegrapher-cli` | Binary `timegrapher`; defaults enable `live` and `update-check`, forwarding to core capture/update support. | Default build needs ALSA development headers. `--no-default-features` omits sound-device and update support, retaining file analysis. |
| `timegrapher-app` | Binary `timegrapher-app`; core capture/update support is enabled by its dependency, with eframe's desktop backends. | ALSA development headers and a desktop environment for interactive use. |
| `timegrapher-mcp` | Binary `timegrapher-mcp`; core library without sound-device features by itself. It invokes a separate CLI for measurements. | Server-only compilation does not enable capture; building its CLI sibling with defaults does. Actual chart display needs an MCP Apps host. |

From the repository root:

```sh
cargo build --release --locked -p timegrapher-cli
cargo build --release --locked -p timegrapher-cli --no-default-features
cargo build --release --locked -p timegrapher-app
cargo build --release --locked -p timegrapher-mcp
```

Use the command matching the package/features needed; these are alternatives,
not a required sequence. A workspace build can unify core features requested by
other selected packages. A headless CLI build does not remove capture from a
simultaneously selected desktop app. On Linux, install `libasound2-dev` before a
capture-enabled build; the CI/release workflows show the tested runner setup.
On macOS and Windows, capture uses the native audio backends. Compiler success
does not establish device permissions or hardware compatibility.

## Compiler checks and dependency updates

The existing Linux platform CI job selects **1.90.0** for formatting, clippy's
all-target checks and the workspace release-profile tests with locked
dependencies/default features. This includes the app's source and test targets;
it does not launch an interactive desktop. The Windows and macOS platform jobs
continue using current stable Rust. The floor check adds no CI job and does
not change the seven-job release gate. These checks establish the
configurations they execute, not every optional feature, cross target or older compiler on every operating system.
Release packaging continues using current stable and records its build output;
the supported-floor check is a separate part of the candidate's required CI.

The old workspace declaration was 1.80, the MCP declaration was 1.88, and the app
did not inherit a compiler requirement. Those declarations did not describe the
current locked graph. For example, locked `indexmap` 2.14.2 (used through core's
`zip`) and `clap` 4.6.7 declare Rust 1.85. MCP's enabled rmcp `server` feature
requires `uuid` 1.27.0, which declares 1.89, despite rmcp 3.5.1 declaring 1.88.
The GUI's egui/eframe 0.33.3 declare 1.88; app code also uses newer standard-library
APIs than the old workspace declaration. Dependency declarations are evidence
of constraints, not a substitute for compiling the actual selected graph.
The uniform supported floor avoids claiming separate untested minima per package.

Keep `Cargo.lock` changes reviewable. Use `--locked` in verification and do not
change dependency versions just to make an unsupported old compiler pass. If an
update needs a higher supported compiler, review the workspace declaration,
Linux CI pin and these instructions together. A graph-resolution/setup failure
(such as an unavailable dependency in offline mode) is not a demonstrated
compiler incompatibility. A failed supported-compiler build needs its actual
compiler/dependency diagnostic before changing this policy.

## What packaging checks leave for a person

The [release procedure](install.md#making-a-release) gates the exact candidate
on CI and exercises extracted CLI/MCP binaries with a deterministic synthetic
recording and stdio calls. This checks delivered-program operability, not new
physical accuracy or statistical calibration. The
[archived model probes](model-probes/README.md) concern their stated mathematical
models; reproducing them does not validate a microphone or calibrate a watch.

Before publishing, a person still needs to check the downloaded package on the
intended host: native GUI first launch and permissions, a real input device,
and chart mounting/live updates in the actual MCP Apps host. Simulated bridge
tests and command-line package smoke do not establish those outcomes. Record
host/OS, package identity, device/permissions and what was observed, rather than
marking these checks complete from a headless CI run.
