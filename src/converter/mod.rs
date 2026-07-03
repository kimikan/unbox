pub mod ts_converter;
pub mod txt_converter;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub use txt_converter::CrcStats;

/// Result of a single file conversion.
#[derive(Debug, Clone)]
pub struct ConvertResult {
  pub src: PathBuf,
  pub dst: PathBuf,
  pub ok: bool,
  pub message: String,
  pub crc_stats: Option<CrcStats>,
}

/// Count convertible files (txt + ts) recursively.
pub fn count_files(dir: &Path) -> usize {
  let mut count = 0;
  count_files_recursive(dir, &mut count);
  count
}

fn count_files_recursive(dir: &Path, count: &mut usize) {
  let entries = match std::fs::read_dir(dir) {
    Ok(e) => e,
    Err(_) => return,
  };
  for entry in entries.flatten() {
    let path = entry.path();
    if path.is_dir() {
      count_files_recursive(&path, count);
    } else {
      let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
      if ext == "txt" || ext == "ts" {
        *count += 1;
      }
    }
  }
}

/// Convert all files incrementally, pushing each result into `results` as it completes.
pub fn convert_directory_incremental(input_dir: &Path, results: &Arc<Mutex<Vec<ConvertResult>>>) {
  let dir_name = input_dir
    .file_name()
    .unwrap_or_default()
    .to_string_lossy()
    .to_string();

  let output_root = input_dir
    .parent()
    .unwrap_or(Path::new("."))
    .join(format!("{}_result", dir_name));

  collect_and_convert_incremental(input_dir, &output_root, input_dir, results);
}

fn collect_and_convert_incremental(
  base: &Path,
  output_root: &Path,
  current: &Path,
  results: &Arc<Mutex<Vec<ConvertResult>>>,
) {
  let entries = match std::fs::read_dir(current) {
    Ok(e) => e,
    Err(e) => {
      let r = ConvertResult {
        src: current.to_path_buf(),
        dst: PathBuf::new(),
        ok: false,
        message: format!("Failed to read directory: {e}"),
        crc_stats: None,
      };
      results.lock().unwrap().push(r);
      return;
    }
  };

  for entry in entries.flatten() {
    let path = entry.path();
    if path.is_dir() {
      collect_and_convert_incremental(base, output_root, &path, results);
      continue;
    }

    let relative = path.strip_prefix(base).unwrap_or(&path);
    let ext = path
      .extension()
      .unwrap_or_default()
      .to_string_lossy()
      .to_lowercase();

    let result = match ext.as_str() {
      "txt" => {
        let dst = output_root.join(relative).with_extension("json");
        convert_one_txt(&path, &dst)
      }
      "ts" => {
        let dst = output_root.join(relative).with_extension("mp4");
        convert_one_ts(&path, &dst)
      }
      _ => continue,
    };

    results.lock().unwrap().push(result);
  }
}

fn convert_one_txt(src: &Path, dst: &Path) -> ConvertResult {
  if let Some(parent) = dst.parent() {
    let _ = std::fs::create_dir_all(parent);
  }
  match txt_converter::convert(src, dst) {
    Ok(stats) => {
      let msg = format!(
        "OK — {} msgs, CRC {}/{} passed",
        stats.total, stats.passed, stats.total
      );
      ConvertResult {
        src: src.to_path_buf(),
        dst: dst.to_path_buf(),
        ok: true,
        message: msg,
        crc_stats: Some(stats),
      }
    }
    Err(e) => ConvertResult {
      src: src.to_path_buf(),
      dst: dst.to_path_buf(),
      ok: false,
      message: format!("Error: {e}"),
      crc_stats: None,
    },
  }
}

fn convert_one_ts(src: &Path, dst: &Path) -> ConvertResult {
  if let Some(parent) = dst.parent() {
    let _ = std::fs::create_dir_all(parent);
  }
  match ts_converter::convert(src, dst) {
    Ok(()) => ConvertResult {
      src: src.to_path_buf(),
      dst: dst.to_path_buf(),
      ok: true,
      message: "OK".to_string(),
      crc_stats: None,
    },
    Err(e) => ConvertResult {
      src: src.to_path_buf(),
      dst: dst.to_path_buf(),
      ok: false,
      message: format!("Error: {e}"),
      crc_stats: None,
    },
  }
}
