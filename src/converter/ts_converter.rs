/// TS → MP4 converter.
///
/// Uses ffmpeg for reliable TS-to-MP4 remuxing (copy streams, no re-encoding).
use std::path::Path;
use std::process::Command;

pub fn convert(src: &Path, dst: &Path) -> Result<(), String> {
  let output = Command::new("ffmpeg")
    .args([
      "-y",
      "-i",
      src.to_str().ok_or("Invalid source path")?,
      "-c",
      "copy",
      dst.to_str().ok_or("Invalid destination path")?,
    ])
    .output()
    .map_err(|e| format!("Failed to run ffmpeg: {e}. Is ffmpeg installed?"))?;

  if output.status.success() {
    Ok(())
  } else {
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("ffmpeg failed: {stderr}"))
  }
}
