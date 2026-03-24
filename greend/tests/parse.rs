use std::path::PathBuf;
use std::str::FromStr;

use greend::subprocess::macos::parse_sample;

#[test]
fn test_parse_sample() {
    let sample_input = std::fs::read_to_string(PathBuf::from_str("../fike.xml").unwrap()).unwrap();
    let sample = parse_sample(sample_input.as_bytes()).unwrap();
    println!("{sample:?}");
}
