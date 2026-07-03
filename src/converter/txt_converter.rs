/// TXT -> JSON converter with CRC32 verification.
///
/// Each line of the input looks like:
/// ```text
/// { timestamp: 947..., len: 170, crc: 424..., data: {ChassisReport { ... }} }
/// ```
///
/// Output JSON per line:
/// ```json
/// { "timestamp": 947..., "len": 170, "crc": 424..., "crc_valid": true, "data": "<base64>" }
/// ```
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use regex::Regex;
use serde::Serialize;

#[derive(Serialize)]
struct Message {
  timestamp: u64,
  len: u64,
  crc: u64,
  crc_valid: bool,
  data: String,
}

/// CRC32 stats returned from conversion.
#[derive(Debug, Clone)]
pub struct CrcStats {
  pub total: usize,
  pub passed: usize,
  pub failed: usize,
}

/// CRC32 matching the C++ implementation:
/// ```c++
/// uint32_t crc = 0xFFFFFFFF;
/// for (size_t i = 0; i < len; ++i) {
///     crc ^= data[i];
///     for (int j = 0; j < 8; ++j)
///         crc = (crc >> 1) ^ (0xEDB88320 & (-(crc & 1)));
/// }
/// return ~crc;
/// ```
fn crc32(data: &[u8]) -> u32 {
  let mut crc: u32 = 0xFFFFFFFF;
  for &byte in data {
    crc ^= byte as u32;
    for _ in 0..8 {
      let mask = (crc & 1).wrapping_neg(); // -(crc & 1) as unsigned
      crc = (crc >> 1) ^ (0xEDB88320 & mask);
    }
  }
  !crc
}

pub fn convert(src: &Path, dst: &Path) -> Result<CrcStats, String> {
  let file = fs::File::open(src).map_err(|e| e.to_string())?;
  let reader = BufReader::new(file);

  let re =
    Regex::new(r"\{\s*timestamp:\s*(\d+)\s*,\s*len:\s*(\d+)\s*,\s*crc:\s*(\d+)\s*,\s*data:\s*\{")
      .unwrap();

  let mut messages: Vec<Message> = Vec::new();
  let mut stats = CrcStats {
    total: 0,
    passed: 0,
    failed: 0,
  };

  for line in reader.lines() {
    let line = line.map_err(|e| e.to_string())?;
    let line = line.trim();
    if line.is_empty() {
      continue;
    }

    let caps = match re.captures(line) {
      Some(c) => c,
      None => continue,
    };

    let timestamp: u64 = caps[1].parse().unwrap_or(0);
    let len: u64 = caps[2].parse().unwrap_or(0);
    let expected_crc: u64 = caps[3].parse().unwrap_or(0);

    // Extract raw data: content after `data: {`, take `len` bytes
    let data_match = caps.get(0).unwrap();
    let data_start = data_match.end();

    let raw_data = if data_start < line.len() {
      let remaining = &line[data_start..];
      let take = (len as usize).min(remaining.len());
      &remaining[..take]
    } else {
      ""
    };

    let raw_bytes = raw_data.as_bytes();

    // CRC32 verification
    let computed_crc = crc32(raw_bytes) as u64;
    let crc_valid = computed_crc == expected_crc;

    stats.total += 1;
    if crc_valid {
      stats.passed += 1;
    } else {
      stats.failed += 1;
    }

    let encoded = BASE64.encode(raw_bytes);

    messages.push(Message {
      timestamp,
      len,
      crc: expected_crc,
      crc_valid,
      data: encoded,
    });
  }

  let json = serde_json::to_string_pretty(&messages).map_err(|e| e.to_string())?;
  let mut out = fs::File::create(dst).map_err(|e| e.to_string())?;
  out.write_all(json.as_bytes()).map_err(|e| e.to_string())?;

  Ok(stats)
}
