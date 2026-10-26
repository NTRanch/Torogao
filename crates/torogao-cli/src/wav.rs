//! WAV reading/writing (24-bit PCM write; 16/24/32-bit PCM and
//! float32 read), ported from the D `torogao.audio.wav` module.

use std::fmt::Write as _;

pub struct WavData {
    pub sample_rate: u32,
    /// One sample vector per channel.
    pub channels: Vec<Vec<f64>>,
}

impl WavData {
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }
}

pub fn write_wav24(channels: &[Vec<f64>], sample_rate: u32) -> Vec<u8> {
    let n_channels = channels.len();
    let frames = if n_channels == 0 {
        0
    } else {
        channels[0].len()
    };
    let data_bytes = (frames * n_channels * 3) as u32;
    let mut out: Vec<u8> = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(n_channels as u16).to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * n_channels as u32 * 3).to_le_bytes());
    out.extend_from_slice(&(n_channels as u16 * 3).to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for i in 0..frames {
        for ch in channels {
            let x = ch[i].clamp(-1.0, 1.0);
            let v = (x * 8_388_607.0) as i32;
            out.push((v & 0xff) as u8);
            out.push(((v >> 8) & 0xff) as u8);
            out.push(((v >> 16) & 0xff) as u8);
        }
    }
    out
}

fn tag_at(bytes: &[u8], off: usize) -> String {
    let mut s = String::new();
    for k in 0..4 {
        let _ = write!(s, "{}", bytes[off + k] as char);
    }
    s
}

fn u16le(bytes: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([bytes[off], bytes[off + 1]])
}

fn u32le(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
}

pub fn parse_wav(bytes: &[u8]) -> Result<WavData, String> {
    if bytes.len() < 44 {
        return Err("not a WAV file (too small)".to_string());
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".to_string());
    }
    let mut format_tag: u16 = 0;
    let mut n_channels: u16 = 0;
    let mut sample_rate: u32 = 0;
    let mut bits: u16 = 0;
    let mut data: &[u8] = &[];
    let mut off = 12usize;
    while off + 8 <= bytes.len() {
        let tag = tag_at(bytes, off);
        let size = u32le(bytes, off + 4) as usize;
        let body = off + 8;
        if body + size > bytes.len() {
            return Err(format!("truncated WAV chunk '{tag}'"));
        }
        if tag == "fmt " {
            if size < 16 {
                return Err("short fmt chunk".to_string());
            }
            format_tag = u16le(bytes, body);
            n_channels = u16le(bytes, body + 2);
            sample_rate = u32le(bytes, body + 4);
            bits = u16le(bytes, body + 14);
        } else if tag == "data" {
            data = &bytes[body..body + size];
        }
        off = body + size + (size & 1);
    }
    if format_tag == 0 {
        return Err("WAV has no fmt chunk".to_string());
    }
    if data.is_empty() {
        return Err("WAV has no data chunk".to_string());
    }
    if !(1..=2).contains(&n_channels) {
        return Err("only mono or stereo WAV is supported".to_string());
    }
    if sample_rate == 0 {
        return Err("WAV has no sample rate".to_string());
    }
    let bytes_per_sample: usize = match format_tag {
        1 => match bits {
            16 => 2,
            24 => 3,
            32 => 4,
            _ => return Err("unsupported PCM bit depth".to_string()),
        },
        3 => {
            if bits != 32 {
                return Err("unsupported float depth".to_string());
            }
            4
        }
        _ => return Err("unsupported WAV format tag".to_string()),
    };
    let frames = data.len() / (bytes_per_sample * n_channels as usize);
    let mut channels: Vec<Vec<f64>> = (0..n_channels as usize)
        .map(|_| Vec::with_capacity(frames))
        .collect();
    for i in 0..frames {
        for (c, channel) in channels.iter_mut().enumerate() {
            let base = (i * n_channels as usize + c) * bytes_per_sample;
            let v: f64 = match (format_tag, bits) {
                (1, 16) => i16::from_le_bytes([data[base], data[base + 1]]) as f64 / 32768.0,
                (1, 24) => {
                    let mut s = (data[base] as i32)
                        | ((data[base + 1] as i32) << 8)
                        | ((data[base + 2] as i32) << 16);
                    if s & 0x80_0000 != 0 {
                        s -= 0x100_0000;
                    }
                    s as f64 / 8_388_608.0
                }
                (1, 32) => {
                    i32::from_le_bytes([data[base], data[base + 1], data[base + 2], data[base + 3]])
                        as f64
                        / 2_147_483_648.0
                }
                _ => {
                    f32::from_le_bytes([data[base], data[base + 1], data[base + 2], data[base + 3]])
                        as f64
                }
            };
            channel.push(v);
        }
    }
    Ok(WavData {
        sample_rate,
        channels,
    })
}

pub fn read_wav_file(path: &str) -> Result<WavData, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    parse_wav(&bytes)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}
