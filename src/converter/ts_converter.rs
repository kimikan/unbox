/// TS  MP4 converter.
///
/// Pure Rust remuxer:
/// - Demux: `mpeg2ts` (TS packets â†? PES packets).
/// - Mux: `muxide` (pure-Rust MP4 muxer with proper H.264/H.265 support).
///
/// Handles both AVC (H.264) and HEVC (H.265) elementary streams. Audio is
/// currently ignored DSSAD TS clips are video-only.
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::process::Command;

use mpeg2ts::es::StreamType;
use mpeg2ts::pes::{PesPacketReader, ReadPesPacket};
use mpeg2ts::ts::{Pid, ReadTsPacket, TsPacketReader, TsPayload};
use muxide::api::{MuxerBuilder, VideoCodec};

#[allow(dead_code)]
pub fn convert_cmd(src: &Path, dst: &Path) -> Result<(), String> {
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

/// PES-derived video access unit.
struct Frame {
  pts_90khz: u64,
  data: Vec<u8>, // Annex-B bytes
  is_keyframe: bool,
  has_param_sets: bool,
}

/// Pure Rust TS MP4 remuxer.
pub fn convert(src: &Path, dst: &Path) -> Result<(), String> {
  // 1. Scan PMT to determine video PID + codec.
  let (video_pid, codec) = scan_video_track(src)?;

  // 2. Read PES for the video PID, classify each frame.
  let src_file = File::open(src).map_err(|e| format!("Open source: {e}"))?;
  let ts_reader = TsPacketReader::new(BufReader::new(src_file));
  let mut pes_reader = PesPacketReader::new(ts_reader);

  // Cached parameter sets  used to prepend to the first keyframe if the
  // stream doesn't inline them alongside the IDR.
  let mut cached_params: Vec<Vec<u8>> = Vec::new();
  let mut frames: Vec<Frame> = Vec::new();

  while let Some(pes) = pes_reader
    .read_pes_packet()
    .map_err(|e| format!("Read PES: {e}"))?
  {
    if pes.header.stream_id.as_u8() < 0xE0 || pes.header.stream_id.as_u8() > 0xEF {
      continue;
    }
    // We can't tell the PID from PES header alone; PesPacketReader hides it.
    // Fortunately in TS files with a single video track the video PID equals
    // the only PID producing video-range stream IDs. If there are multiple,
    // we'd need to walk raw TS packets deferred.
    let _ = video_pid; // reserved for future multi-PID handling

    let Some(pts_ts) = pes.header.pts else {
      continue;
    };
    let pts_90khz = pts_ts.as_u64();

    let nals = split_annex_b(&pes.data);
    if nals.is_empty() {
      continue;
    }

    let mut is_keyframe = false;
    let mut has_ps = false;
    for nal in &nals {
      if nal.is_empty() {
        continue;
      }
      match codec {
        VideoCodec::H264 => {
          let t = nal[0] & 0x1F;
          if t == 5 {
            is_keyframe = true;
          }
          if t == 7 || t == 8 {
            // SPS or PPS
            has_ps = true;
            remember_param(&mut cached_params, nal);
          }
        }
        VideoCodec::H265 => {
          if nal.len() < 2 {
            continue;
          }
          let t = (nal[0] >> 1) & 0x3F;
          if (16..=21).contains(&t) {
            is_keyframe = true;
          }
          if (32..=34).contains(&t) {
            // VPS, SPS, or PPS
            has_ps = true;
            remember_param(&mut cached_params, nal);
          }
        }
        _ => {}
      }
    }

    // Skip everything until we hit the first keyframe.
    if frames.is_empty() && !is_keyframe {
      continue;
    }

    // If this is the first keyframe and it doesn't inline parameter sets,
    // prepend the cached ones so muxide can build the codec config.
    let data = if frames.is_empty() && !has_ps && !cached_params.is_empty() {
      let mut buf = Vec::with_capacity(pes.data.len() + 32);
      for ps in &cached_params {
        buf.extend_from_slice(&[0, 0, 0, 1]);
        buf.extend_from_slice(ps);
      }
      buf.extend_from_slice(&pes.data);
      buf
    } else {
      pes.data.clone()
    };

    frames.push(Frame {
      pts_90khz,
      data,
      is_keyframe,
      has_param_sets: has_ps,
    });
  }

  if frames.is_empty() {
    return Err("No video keyframes found in stream".to_string());
  }

  // 3. Normalize PTS: subtract the first PTS so the output starts at t=0.
  //    Muxide requires strictly increasing, non-negative timestamps.
  frames.sort_by_key(|f| f.pts_90khz);
  let base_pts = frames[0].pts_90khz;

  // Deduplicate exact-duplicate PTS to satisfy the strictly-increasing rule.
  frames.dedup_by_key(|f| f.pts_90khz);

  // Sniff dimensions and rough framerate from the frames we have.
  let framerate = estimate_framerate(&frames);
  let (width, height) = detect_dimensions(codec, &frames).unwrap_or((1920, 1080));

  // 4. Create muxer and write frames.
  let dst_file = File::create(dst).map_err(|e| format!("Create destination: {e}"))?;
  let dst_writer = BufWriter::new(dst_file);
  let mut muxer = MuxerBuilder::new(dst_writer)
    .video(codec, width, height, framerate)
    .build()
    .map_err(|e| format!("Build MP4 muxer: {e}"))?;

  for f in &frames {
    let pts_sec = (f.pts_90khz.wrapping_sub(base_pts) as f64) / 90_000.0;
    muxer
      .write_video(pts_sec, &f.data, f.is_keyframe)
      .map_err(|e| format!("Write frame: {e}"))?;
  }

  muxer
    .finish()
    .map_err(|e| format!("Finalize MP4: {e}"))?;
  Ok(())
}

/// First-pass scan: walk TS packets, find PMT, return (video_pid, codec).
fn scan_video_track(src: &Path) -> Result<(Pid, VideoCodec), String> {
  let f = File::open(src).map_err(|e| format!("Open source: {e}"))?;
  let mut reader = TsPacketReader::new(BufReader::new(f));
  while let Some(packet) = reader.read_ts_packet().map_err(|e| format!("Scan PMT: {e}"))? {
    let Some(TsPayload::Pmt(pmt)) = packet.payload else {
      continue;
    };
    for es in &pmt.es_info {
      match es.stream_type {
        StreamType::H264 => return Ok((es.elementary_pid, VideoCodec::H264)),
        StreamType::H265 => return Ok((es.elementary_pid, VideoCodec::H265)),
        _ => {}
      }
    }
  }
  Err("No H.264 or H.265 video track found in TS".to_string())
}

fn remember_param(store: &mut Vec<Vec<u8>>, nal: &[u8]) {
  if !store.iter().any(|p| p.as_slice() == nal) {
    store.push(nal.to_vec());
  }
}

/// Split an H.264/H.265 Annex-B byte stream into NAL unit bodies (start
/// codes stripped).
fn split_annex_b(data: &[u8]) -> Vec<&[u8]> {
  let mut nals = Vec::new();
  let mut start: Option<usize> = None;
  let mut i = 0;
  while i < data.len() {
    let sc_len = if i + 4 <= data.len()
      && data[i] == 0
      && data[i + 1] == 0
      && data[i + 2] == 0
      && data[i + 3] == 1
    {
      4
    } else if i + 3 <= data.len() && data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
      3
    } else {
      0
    };
    if sc_len > 0 {
      if let Some(s) = start {
        nals.push(trim_trailing_zeros(&data[s..i]));
      }
      start = Some(i + sc_len);
      i += sc_len;
    } else {
      i += 1;
    }
  }
  if let Some(s) = start {
    nals.push(trim_trailing_zeros(&data[s..]));
  }
  nals
}

fn trim_trailing_zeros(mut s: &[u8]) -> &[u8] {
  while let Some((&last, rest)) = s.split_last() {
    if last == 0 {
      s = rest;
    } else {
      break;
    }
  }
  s
}

/// Estimate framerate from PTS deltas. Falls back to 30 fps.
fn estimate_framerate(frames: &[Frame]) -> f64 {
  if frames.len() < 2 {
    return 30.0;
  }
  let dt = frames[1].pts_90khz.saturating_sub(frames[0].pts_90khz);
  if dt == 0 {
    30.0
  } else {
    90_000.0 / dt as f64
  }
}

/// Try to parse dimensions from an SPS NAL found in the first frame.
fn detect_dimensions(codec: VideoCodec, frames: &[Frame]) -> Option<(u32, u32)> {
  let first = frames.first()?;
  for nal in split_annex_b(&first.data) {
    if nal.is_empty() {
      continue;
    }
    match codec {
      VideoCodec::H264 if (nal[0] & 0x1F) == 7 => {
        if let Some((w, h)) = parse_h264_sps_resolution(nal) {
          return Some((w as u32, h as u32));
        }
      }
      VideoCodec::H265 if nal.len() >= 2 && ((nal[0] >> 1) & 0x3F) == 33 => {
        if let Some((w, h)) = parse_hevc_sps_resolution(nal) {
          return Some((w as u32, h as u32));
        }
      }
      _ => {}
    }
  }
  None
}

/// Reverse H.264/HEVC emulation prevention: drop 0x03 following two zero
/// bytes.
fn ebsp_to_rbsp(ebsp: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(ebsp.len());
  let mut i = 0;
  while i < ebsp.len() {
    if i + 2 < ebsp.len() && ebsp[i] == 0 && ebsp[i + 1] == 0 && ebsp[i + 2] == 0x03 {
      out.push(0);
      out.push(0);
      i += 3;
    } else {
      out.push(ebsp[i]);
      i += 1;
    }
  }
  out
}

/// Minimal H.264 SPS parser (width, height).
fn parse_h264_sps_resolution(sps: &[u8]) -> Option<(u16, u16)> {
  if sps.len() < 4 {
    return None;
  }
  let rbsp = ebsp_to_rbsp(&sps[1..]);
  let mut r = BitReader::new(&rbsp);
  let profile_idc = r.read_bits(8)?;
  let _ = r.read_bits(8)?;
  let _ = r.read_bits(8)?;
  let _ = r.read_ue()?; // sps_id

  let chroma_format_idc = if matches!(
    profile_idc,
    100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
  ) {
    let cfi = r.read_ue()?;
    if cfi == 3 {
      r.read_bit()?;
    }
    r.read_ue()?;
    r.read_ue()?;
    r.read_bit()?;
    if r.read_bit()? == 1 {
      let n = if cfi == 3 { 12 } else { 8 };
      for i in 0..n {
        if r.read_bit()? == 1 {
          let size = if i < 6 { 16 } else { 64 };
          let mut last_scale = 8i32;
          let mut next_scale = 8i32;
          for _ in 0..size {
            if next_scale != 0 {
              let delta = r.read_se()?;
              next_scale = (last_scale + delta).rem_euclid(256);
            }
            if next_scale != 0 {
              last_scale = next_scale;
            }
          }
        }
      }
    }
    cfi
  } else {
    1
  };

  let _ = r.read_ue()?; // log2_max_frame_num
  let pic_order_cnt_type = r.read_ue()?;
  if pic_order_cnt_type == 0 {
    r.read_ue()?;
  } else if pic_order_cnt_type == 1 {
    r.read_bit()?;
    r.read_se()?;
    r.read_se()?;
    let n = r.read_ue()?;
    for _ in 0..n {
      r.read_se()?;
    }
  }
  r.read_ue()?; // max_num_ref_frames
  r.read_bit()?; // gaps_in_frame_num_value_allowed
  let pw = r.read_ue()?;
  let ph = r.read_ue()?;
  let frame_mbs_only = r.read_bit()?;
  if frame_mbs_only == 0 {
    r.read_bit()?;
  }
  r.read_bit()?; // direct_8x8_inference
  let cropping = r.read_bit()?;
  let (mut cl, mut cr, mut ct, mut cb) = (0u32, 0u32, 0u32, 0u32);
  if cropping == 1 {
    cl = r.read_ue()?;
    cr = r.read_ue()?;
    ct = r.read_ue()?;
    cb = r.read_ue()?;
  }
  let width_mb = (pw + 1) * 16;
  let height_mb = (2 - frame_mbs_only) * (ph + 1) * 16;
  let (sw, sh) = match chroma_format_idc {
    1 => (2, 2),
    2 => (2, 1),
    3 => (1, 1),
    _ => (1, 1),
  };
  Some((
    (width_mb.saturating_sub(sw * (cl + cr))) as u16,
    (height_mb.saturating_sub(sh * (ct + cb))) as u16,
  ))
}

/// Minimal HEVC SPS parser (width, height). Handles the header up to
/// `pic_width_in_luma_samples` / `pic_height_in_luma_samples` and the
/// conformance window crop.
fn parse_hevc_sps_resolution(sps: &[u8]) -> Option<(u16, u16)> {
  if sps.len() < 3 {
    return None;
  }
  // Skip two-byte HEVC NAL header.
  let rbsp = ebsp_to_rbsp(&sps[2..]);
  let mut r = BitReader::new(&rbsp);

  let _sps_video_parameter_set_id = r.read_bits(4)?;
  let sps_max_sub_layers_minus1 = r.read_bits(3)?;
  let _sps_temporal_id_nesting_flag = r.read_bit()?;

  // profile_tier_level(1, sps_max_sub_layers_minus1)
  skip_profile_tier_level(&mut r, sps_max_sub_layers_minus1)?;

  let _sps_seq_parameter_set_id = r.read_ue()?;
  let chroma_format_idc = r.read_ue()?;
  if chroma_format_idc == 3 {
    r.read_bit()?; // separate_colour_plane_flag
  }
  let pic_width = r.read_ue()?;
  let pic_height = r.read_ue()?;
  let conformance_window_flag = r.read_bit()?;
  let (mut cl, mut cr, mut ct, mut cb) = (0u32, 0u32, 0u32, 0u32);
  if conformance_window_flag == 1 {
    cl = r.read_ue()?;
    cr = r.read_ue()?;
    ct = r.read_ue()?;
    cb = r.read_ue()?;
  }
  let (sub_w, sub_h) = match chroma_format_idc {
    1 => (2, 2),
    2 => (2, 1),
    3 => (1, 1),
    _ => (1, 1),
  };
  Some((
    pic_width.saturating_sub(sub_w * (cl + cr)) as u16,
    pic_height.saturating_sub(sub_h * (ct + cb)) as u16,
  ))
}

fn skip_profile_tier_level(r: &mut BitReader<'_>, max_sub_layers_minus1: u32) -> Option<()> {
  // general_profile_space(2) | tier_flag(1) | profile_idc(5)
  r.read_bits(8)?;
  // general_profile_compatibility_flag[32]
  r.read_bits(32)?;
  // progressive/interlaced/non_packed/frame_only + 43 zero bits + 1 bit reserved
  r.read_bits(4)?;
  r.read_bits(32)?;
  r.read_bits(11)?;
  r.read_bits(1)?;
  // level_idc(8)
  r.read_bits(8)?;

  let n = max_sub_layers_minus1 as usize;
  let mut profile_present = vec![false; n];
  let mut level_present = vec![false; n];
  for i in 0..n {
    profile_present[i] = r.read_bit()? != 0;
    level_present[i] = r.read_bit()? != 0;
  }
  if n > 0 {
    // Alignment: 2 * (8 - n) reserved zero bits, per spec (rounded up to
    // the next byte boundary â€? that's 8*(8-n) bits when n<8).
    for _ in n..8 {
      r.read_bits(2)?;
    }
  }
  for i in 0..n {
    if profile_present[i] {
      r.read_bits(8)?;
      r.read_bits(32)?;
      r.read_bits(4)?;
      r.read_bits(32)?;
      r.read_bits(11)?;
      r.read_bits(1)?;
    }
    if level_present[i] {
      r.read_bits(8)?;
    }
  }
  Some(())
}

/// Bit-level reader for H.264/HEVC RBSP (Exp-Golomb + fixed-width fields).
struct BitReader<'a> {
  data: &'a [u8],
  pos: usize,
}

impl<'a> BitReader<'a> {
  fn new(data: &'a [u8]) -> Self {
    Self { data, pos: 0 }
  }

  fn read_bit(&mut self) -> Option<u32> {
    let byte = *self.data.get(self.pos / 8)?;
    let bit = (byte >> (7 - (self.pos % 8))) & 1;
    self.pos += 1;
    Some(bit as u32)
  }

  fn read_bits(&mut self, n: usize) -> Option<u32> {
    let mut v = 0u32;
    for _ in 0..n {
      v = (v << 1) | self.read_bit()?;
    }
    Some(v)
  }

  fn read_ue(&mut self) -> Option<u32> {
    let mut zeros = 0;
    while self.read_bit()? == 0 {
      zeros += 1;
      if zeros > 32 {
        return None;
      }
    }
    if zeros == 0 {
      return Some(0);
    }
    let suffix = self.read_bits(zeros)?;
    Some((1u32 << zeros) - 1 + suffix)
  }

  fn read_se(&mut self) -> Option<i32> {
    let v = self.read_ue()?;
    Some(if v & 1 == 1 {
      ((v + 1) >> 1) as i32
    } else {
      -((v >> 1) as i32)
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::PathBuf;

  #[test]
  fn convert_sample_ts() {
    let src = PathBuf::from(
      "./dssad_data/RSK/VIN_CN-DSSAD_20000105_053651_1_1_3.ts",
    );
    if !src.exists() {
      eprintln!("skipping  sample file missing");
      return;
    }
    let dst = PathBuf::from("/tmp/test_output.mp4");
    convert(&src, &dst).expect("convert failed");
    let size = std::fs::metadata(&dst).unwrap().len();
    assert!(size > 0, "output should be non-empty");
    println!("output size: {size} bytes");
  }
}
