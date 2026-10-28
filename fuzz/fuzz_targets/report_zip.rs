//! A report bundle's ZIP file, read as `timegrapher bundle` reads one
//! (`archive::read_zip`).
#![no_main]
use libfuzzer_sys::fuzz_target;

#[path = "common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let file = common::scratch("report_zip").join("bundle.zip");
    std::fs::write(&file, data).unwrap();
    let _ = timegrapher_core::archive::read_zip(&file);
});
