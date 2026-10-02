#![no_main]

use c2pa_http::link::{extract, locate_all};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    let _ = locate_all([data]);
    let _ = extract([data]);
});
