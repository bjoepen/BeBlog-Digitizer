//! Inert, deterministic child-process simulator for the D4 guided UI test path.
//! It does not link SANE and only writes synthetic files in an app-reserved folder.

use std::{
    fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
};

pub fn run_from_args(args: &[std::ffi::OsString]) -> Result<(), String> {
    if args.len() != 6 || args[0] != "--prefix" || args[2] != "--frame" || args[4] != "--dpi" {
        return Err("invalid internal simulator arguments".into());
    }
    let prefix = PathBuf::from(&args[1]);
    let frame = args[3]
        .to_string_lossy()
        .parse::<u8>()
        .map_err(|_| "invalid frame")?;
    let dpi = args[5]
        .to_string_lossy()
        .parse::<u16>()
        .map_err(|_| "invalid dpi")?;
    if !(1..=4).contains(&frame) || !matches!(dpi, 600 | 2820) {
        return Err("unsupported synthetic profile".into());
    }
    let (width, height) = if dpi == 600 {
        (564_u32, 850_u32)
    } else {
        (120_u32, 80_u32)
    };
    let stride = width.checked_mul(3).ok_or("stride overflow")?;
    let raw_len = usize::try_from(stride.checked_mul(height).ok_or("raw size overflow")?)
        .map_err(|_| "raw size overflow")?;
    let raw = (0..raw_len)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();

    event("OPENED").map_err(|e| e.to_string())?;
    let mut input = String::new();
    io::stdin()
        .lock()
        .read_line(&mut input)
        .map_err(|e| e.to_string())?;
    if input.trim_end() != "CONTINUE" {
        event("ERROR\tstatus=2").ok();
        return Err("CONTINUE required".into());
    }
    event("HOLDER_DETECTED").map_err(|e| e.to_string())?;
    event(&format!("FRAME_POSITIONED\t{frame}")).map_err(|e| e.to_string())?;
    event(&format!(
        "PARAMETERS\t{dpi}\t{width}\t{height}\t{stride}\t8\tRGB\t{raw_len}"
    ))
    .map_err(|e| e.to_string())?;
    event("SCAN_STARTED").map_err(|e| e.to_string())?;

    write_artifact(&prefix, "raw", &raw).map_err(|e| e.to_string())?;
    let mut total = 0_usize;
    for chunk in raw.chunks(64 * 1024) {
        total += chunk.len();
        event(&format!("DATA_CHUNK\t{}\t{total}", chunk.len())).map_err(|e| e.to_string())?;
    }
    event(&format!("EOF\t{raw_len}")).map_err(|e| e.to_string())?;
    let tiff = make_tiff(&raw, width, height);
    write_artifact(&prefix, "tiff", &tiff).map_err(|e| e.to_string())?;
    let params = format!("device=simulator:hardware-free\nrequested_resolution_dpi={dpi}\nactual_resolution_dpi={dpi}\nresolution_dpi={dpi}\nformat=RGB\nchannels=3\nbytes_per_line={stride}\npixels_per_line={width}\nlines={height}\ndepth=8\nraw_bytes={raw_len}\nexpected_raw_bytes={raw_len}\nterminal_status=5\nframe={frame}\n");
    write_artifact(&prefix, "params.txt", params.as_bytes()).map_err(|e| e.to_string())?;
    let log = format!("sane_init -> 0 (Success)\nsane_open (only call) -> 0 (Success)\nframe descriptor range=1..6 quant=1\nsane_control_option(frame, SET={frame}) -> 0 (Success)\nsane_control_option(mode, SET='Color') -> 0 (Success)\nsane_control_option(resolution, SET={dpi}) -> 0 (Success)\nsane_control_option(resolution, GET={dpi}) -> 0 (Success), info=0x0\nparameters(pre): format=RGB last=1 bpl={stride} pixels={width} lines={height} depth=8\nsane_start (only call) -> 0 (Success)\nparameters(post): format=RGB last=1 bpl={stride} pixels={width} lines={height} depth=8\nsane_read -> 0 (Success), bytes={raw_len}\nsane_read -> 5 (End of file reached), bytes=0\ncapture ended: total={raw_len} terminal=5 (End of file reached)\nmetadata capture.params.txt -> written\nTIFF capture.tiff -> written\nsane_close\nsane_exit\nclient exit -> 0 (Success)\n");
    write_artifact(&prefix, "client.log", log.as_bytes()).map_err(|e| e.to_string())?;
    event("TIFF_WRITTEN").map_err(|e| e.to_string())?;
    event("SCANNER_CLOSING").map_err(|e| e.to_string())?;
    event("COMPLETE").map_err(|e| e.to_string())?;
    eprintln!("simulation only: synthetic artifacts generated; no scanner or SANE access");
    Ok(())
}

fn event(body: &str) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(out, "BEBLOG/1\t{body}")?;
    out.flush()
}
fn write_artifact(prefix: &Path, extension: &str, data: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(format!("{}.{}", prefix.display(), extension))?;
    file.write_all(data)?;
    file.sync_all()
}
fn make_tiff(raw: &[u8], width: u32, height: u32) -> Vec<u8> {
    let tags: [(u16, u16, u32, u32); 11] = [
        (256, 4, 1, width),
        (257, 4, 1, height),
        (258, 3, 3, 0),
        (259, 3, 1, 1),
        (262, 3, 1, 2),
        (273, 4, 1, 0),
        (277, 3, 1, 3),
        (278, 4, 1, height),
        (279, 4, 1, raw.len() as u32),
        (284, 3, 1, 1),
        (339, 3, 3, 0),
    ];
    let ifd = 8_usize;
    let bits_at = ifd + 2 + tags.len() * 12 + 4;
    let data_at = bits_at + 6;
    let format_at = data_at + raw.len();
    let mut bytes = vec![0; format_at + 6];
    bytes[..2].copy_from_slice(b"II");
    put16(&mut bytes, 2, 42);
    put32(&mut bytes, 4, ifd as u32);
    put16(&mut bytes, ifd, tags.len() as u16);
    for (i, (tag, kind, count, value)) in tags.iter().enumerate() {
        let at = ifd + 2 + i * 12;
        put16(&mut bytes, at, *tag);
        put16(&mut bytes, at + 2, *kind);
        put32(&mut bytes, at + 4, *count);
        let v = match tag {
            258 => bits_at as u32,
            273 => data_at as u32,
            339 => format_at as u32,
            _ => *value,
        };
        if *kind == 3 && *count == 1 {
            put16(&mut bytes, at + 8, v as u16)
        } else {
            put32(&mut bytes, at + 8, v)
        }
    }
    put32(&mut bytes, ifd + 2 + tags.len() * 12, 0);
    for at in [bits_at, bits_at + 2, bits_at + 4] {
        put16(&mut bytes, at, 8)
    }
    for at in [format_at, format_at + 2, format_at + 4] {
        put16(&mut bytes, at, 1)
    }
    bytes[data_at..data_at + raw.len()].copy_from_slice(raw);
    bytes
}
fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes())
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes())
}
