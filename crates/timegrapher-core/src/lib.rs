//! Core signal processing for the timegrapher.
//!
//! The design measures every beat individually and keeps the result, so
//! that both live readings and long-horizon analysis (hours to days) read
//! from the same per-beat log. See `docs/` in the repository.

pub mod amplitude;
pub mod analysis;
pub mod analysis_take;
pub mod archive;
pub mod audio;
pub mod beats;
pub mod breaks;
pub mod calibre_store;
pub mod calibres;
pub mod capture;
pub mod clock;
pub mod clockstore;
pub mod confidence;
pub mod diagnose;
pub mod distribution;
pub mod dsp;
pub mod excerpt;
pub mod filter;
pub mod gappy;
pub mod histogram;
pub mod input_domain;
pub mod library;
pub mod live;
pub mod longrun;
pub mod longterm;
pub mod measure;
pub mod mixer;
pub mod palette;
pub mod periodicity;
pub mod power_reserve;
pub mod profile;
pub mod recorder;
pub mod recording_order;
pub mod scenarios;
pub mod session;
pub mod shape;
pub mod spectrum;
pub mod steadiness;
pub mod steady_views;
pub mod stream;
pub mod synth;
pub mod timefreq;
pub mod timing;
pub mod twostate;
pub mod update;
pub mod usbclock;
pub mod wheel_lines;

pub use analysis::{analyze, Analysis, AnalysisConfig, Summary};
pub use audio::{load, Audio};
