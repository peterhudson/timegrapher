//! Core signal processing for the timegrapher.
//!
//! The design measures every beat individually and keeps the result, so
//! that both live readings and long-horizon analysis (hours to days) read
//! from the same per-beat log. See `docs/` in the repository.

pub mod amplitude;
pub mod analysis;
pub mod audio;
pub mod beats;
pub mod capture;
pub mod clock;
pub mod clockstore;
pub mod diagnose;
pub mod dsp;
pub mod filter;
pub mod histogram;
pub mod live;
pub mod longrun;
pub mod longterm;
pub mod mixer;
pub mod periodicity;
pub mod profile;
pub mod recorder;
pub mod session;
pub mod shape;
pub mod steadiness;
pub mod stream;
pub mod synth;
pub mod timing;
pub mod twostate;

pub use analysis::{analyze, Analysis, AnalysisConfig, Summary};
pub use audio::{load, Audio};
