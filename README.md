# 📦 Unbox — DSSAD Data Converter

A clean, lightweight desktop tool for converting DSSAD data files, built with **Rust + egui**.

![Rust](https://img.shields.io/badge/Rust-2021-orange)
![License](https://img.shields.io/badge/License-MIT-blue)

## Features

- 🗂️ **Directory-based batch conversion** — select a directory via GUI file dialog, all files are recursively converted
- 📝 **TXT → JSON** — each line parsed into structured JSON, `data` field base64-encoded
- 🎬 **TS → MP4** — MPEG-TS remuxed to MP4 via ffmpeg (stream copy, no re-encoding)
- 🌙☀️ **Dark / Light theme toggle** — one-click theme switching
- 📊 **Results dashboard** — see conversion status for every file at a glance

## Prerequisites

- **Rust** ≥ 1.70
- **ffmpeg** installed and available in `$PATH` (for TS → MP4 conversion)
- Linux: `libgtk-3-dev` or equivalent for the native file dialog

## Build & Run

```bash
cargo build --release
./target/release/unbox
```

Or run directly in development mode:

```bash
cargo run
```

## Usage

1. Launch the application
2. Click **📂 Open Directory** to select a DSSAD data directory (e.g. `dssad_data/`)
3. Conversion starts automatically — results appear in the central panel
4. Output is written to `<directory_name>_result/` next to the input directory, preserving the subdirectory structure

### Input Directory Structure Example

```
dssad_data/
├── RSK/
│   ├── VIN_CN-DSSAD_20000105_053651_1_1_3.txt
│   └── VIN_CN-DSSAD_20000105_053651_1_1_3.ts
├── LCK/
│   ├── VIN_CN-DSSAD_20000105_053055_1_1_1.txt
│   └── VIN_CN-DSSAD_20000105_053055_1_1_1.ts
├── TSD/
│   └── VIN_CN-DSSAD_20000105_054205_1_2_28.txt
└── ULK/
    ├── VIN_CN-DSSAD_20000105_053627_1_1_2.txt
    └── VIN_CN-DSSAD_20000105_053627_1_1_2.ts
```

### Output Directory Structure

```
dssad_data_result/
├── RSK/
│   ├── VIN_CN-DSSAD_20000105_053651_1_1_3.json   ← converted from .txt
│   └── VIN_CN-DSSAD_20000105_053651_1_1_3.mp4    ← converted from .ts
├── LCK/
│   ├── VIN_CN-DSSAD_20000105_053055_1_1_1.json
│   └── VIN_CN-DSSAD_20000105_053055_1_1_1.mp4
├── TSD/
│   └── VIN_CN-DSSAD_20000105_054205_1_2_28.json
└── ULK/
    ├── VIN_CN-DSSAD_20000105_053627_1_1_2.json
    └── VIN_CN-DSSAD_20000105_053627_1_1_2.mp4
```

## Conversion Details

### TXT → JSON

Each line in the input TXT file has the format:

```
{ timestamp: 947021796826019136, len: 170, crc: 4248255101, data: {ChassisReport { speed: 0.000000, ... }} }
```

Converted to JSON:

```json
[
  {
    "timestamp": 947021796826019136,
    "len": 170,
    "crc": 4248255101,
    "data": "Q2hhc3Npc1JlcG9ydCB7IHNwZWVkOiAwLjAw..."
  }
]
```

- `timestamp`, `len`, `crc` are extracted as numbers
- `data` content (the substring after `data: {`, length = `len`) is **base64-encoded**

### TS → MP4

Uses `ffmpeg -c copy` for lossless stream remuxing — fast and no quality loss.

## Project Structure

```
src/
├── main.rs                 # Entry point, window setup
├── app.rs                  # egui application logic & UI
├── theme.rs                # Dark/Light theme management
└── converter/
    ├── mod.rs              # Directory traversal & orchestration
    ├── txt_converter.rs    # TXT → JSON conversion logic
    └── ts_converter.rs     # TS → MP4 conversion (ffmpeg)
```

## Tech Stack

| Component      | Crate / Tool         |
|----------------|----------------------|
| GUI Framework  | `eframe` + `egui`    |
| File Dialog    | `egui-file-dialog`   |
| JSON           | `serde` + `serde_json` |
| Base64         | `base64`             |
| Text Parsing   | `regex`              |
| Video Remux    | `ffmpeg` (external)  |

## License

MIT
