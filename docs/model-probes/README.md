# Archived model probes

These two opt-in programs preserve the bounded synthetic probes discussed in
[measurement precision](../measurement-precision.md). They are examples, not
statistical acceptance tests: neither program asserts a target detection rate
or runs automatically as a model experiment in the test suite. They do not read
audio, access hardware or download data.

The original assessment baseline was
`009dcc6d54638ff4df51563941935ddc1c654a74`. The output files here are the archived
October 2026 observations, copied byte for byte. They are **not** results from a
new run against the current engine. The supplied original source and output
files did not preserve compiler versions, build flags, dependency/artifact
hashes or complete original invocation logs; these cannot be reconstructed from
the observations. The baseline identifies the assessed revision, not a verified
binary-to-source identity for the historical executable.

[provenance.json](provenance.json) records the original source/output SHA-256
hashes, copied-file hashes and the documentation baseline. The example sources
are the original programs with `rustfmt` formatting only. Seeds, series lengths,
recursions, missing-data masks, calls and output formats are unchanged. The
original compiled binary is not distributed.

From the repository root, compile without running the models:

```sh
cargo check --locked -p timegrapher-core --example precision-model-probe --example scientific-statistics-probe
```

To opt in to a new run using the current checkout and locked dependencies:

```sh
rustc --version --verbose
cargo --version
cargo run --release --locked -p timegrapher-core --example precision-model-probe > precision-model-current.txt
cargo run --release --locked -p timegrapher-core --example scientific-statistics-probe > scientific-statistics-current.csv
```

Keep new outputs separate from the archives. Record the Git revision, local
diff, compiler/target, command and `Cargo.lock` hash with them. These commands
reproduce the experiment definitions with the selected build; they cannot
recreate the missing historical build provenance. Floating-point, dependency
or engine changes may change results. Do not turn the archived counts into
mandatory tolerances or update them silently to match a new run.

- [precision-model-probe.rs](../../crates/timegrapher-core/examples/precision-model-probe.rs)
  directly probes the median SE model with stationary, unit-variance Gaussian
  AR(1) input. [Historical output](precision-model-probe-output.txt).
- [scientific-statistics-probe.rs](../../crates/timegrapher-core/examples/scientific-statistics-probe.rs)
  probes missing-grid FFT heights and the default period search on bounded
  synthetic null series. Its AR(1) recursion starts at zero with unscaled
  innovations, unlike the precision probe. [Historical output](scientific-statistics-probe.csv).

The results expose conditional model behaviour. They do not establish the
noise statistics, accuracy or presence/absence of faults in a real watch.
Measured drift and periodic watch variation remain meaningful observations;
this documentation does not reclassify them as measurement error or change
any estimator, threshold or eligibility policy.
