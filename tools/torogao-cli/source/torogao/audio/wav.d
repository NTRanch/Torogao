/*
    Torogao audio: minimal WAV reader/writer

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    The writer emits the house master shape: 48 kHz,
    24-bit PCM, RIFF. The reader accepts PCM 16/24/32-bit
    integer and 32-bit float WAV, one or two channels —
    the shapes masters and stems arrive in — and returns
    per-channel double samples in [-1, 1]. Everything else
    is a typed error, never a guess.
*/
module torogao.audio.wav;

import std.bitmanip : littleEndianToNative, nativeToLittleEndian;
import std.file : read, write;

/// A decoded WAV file.
struct WavData {
    uint sampleRate;
    double[][] channels;  /// channels[ch][sample]

    size_t frames() const {
        return channels.length ? channels[0].length : 0;
    }
    double durationSeconds() const {
        return sampleRate ? cast(double) frames() / sampleRate : 0;
    }
}

private void putTag(ref ubyte[] out_, string tag) {
    out_ ~= cast(ubyte[]) tag;
}

private void putU32(ref ubyte[] out_, uint v) {
    out_ ~= nativeToLittleEndian(v);
}

private void putU16(ref ubyte[] out_, ushort v) {
    out_ ~= nativeToLittleEndian(v);
}

/// Serialize channels as a 48 kHz 24-bit PCM WAV.
/// Samples are clamped to [-1, 1] on write.
ubyte[] writeWav24(in double[][] channels, uint sampleRate = 48000) {
    ushort ch = cast(ushort) channels.length;
    size_t frames = channels.length ? channels[0].length : 0;
    uint dataBytes = cast(uint)(frames * ch * 3);
    ubyte[] out_;
    putTag(out_, "RIFF");
    putU32(out_, 36 + dataBytes);
    putTag(out_, "WAVE");
    putTag(out_, "fmt ");
    putU32(out_, 16);
    putU16(out_, 1);                    // PCM
    putU16(out_, ch);
    putU32(out_, sampleRate);
    putU32(out_, sampleRate * ch * 3);  // byte rate
    putU16(out_, cast(ushort)(ch * 3)); // block align
    putU16(out_, 24);                   // bits per sample
    putTag(out_, "data");
    putU32(out_, dataBytes);
    foreach (f; 0 .. frames) {
        foreach (c; 0 .. ch) {
            double x = channels[c][f];
            if (x > 1.0) x = 1.0;
            if (x < -1.0) x = -1.0;
            int v = cast(int)(x * 8388607.0);
            out_ ~= cast(ubyte)(v & 0xFF);
            out_ ~= cast(ubyte)((v >> 8) & 0xFF);
            out_ ~= cast(ubyte)((v >> 16) & 0xFF);
        }
    }
    return out_;
}

/// Write channels to a WAV file on disk.
void writeWavFile(string path, in double[][] channels,
                  uint sampleRate = 48000) {
    write(path, writeWav24(channels, sampleRate));
}

private uint u32at(in ubyte[] b, size_t at) {
    ubyte[4] tmp = b[at .. at + 4];
    return littleEndianToNative!uint(tmp);
}

private ushort u16at(in ubyte[] b, size_t at) {
    ubyte[2] tmp = b[at .. at + 2];
    return littleEndianToNative!ushort(tmp);
}

/// Decode WAV bytes. Throws Exception with a plain message
/// on any shape this reader does not support.
WavData parseWav(in ubyte[] bytes) {
    if (bytes.length < 44)
        throw new Exception("not a WAV file (too small)");
    if (bytes[0 .. 4] != "RIFF" || bytes[8 .. 12] != "WAVE")
        throw new Exception("not a RIFF/WAVE file");
    ushort format = 0, channels = 0, bits = 0;
    uint rate = 0;
    const(ubyte)[] data;
    size_t at = 12;
    while (at + 8 <= bytes.length) {
        string tag = cast(string) bytes[at .. at + 4];
        uint size = u32at(bytes, at + 4);
        size_t body = at + 8;
        if (body + size > bytes.length)
            throw new Exception("truncated WAV chunk '" ~ tag ~ "'");
        if (tag == "fmt ") {
            if (size < 16) throw new Exception("short fmt chunk");
            format = u16at(bytes, body);
            channels = u16at(bytes, body + 2);
            rate = u32at(bytes, body + 4);
            bits = u16at(bytes, body + 14);
        } else if (tag == "data") {
            data = bytes[body .. body + size];
        }
        at = body + size + (size & 1);  // chunks are word-aligned
    }
    if (format == 0) throw new Exception("WAV has no fmt chunk");
    if (data.length == 0) throw new Exception("WAV has no data chunk");
    if (channels < 1 || channels > 2)
        throw new Exception("only mono or stereo WAV is supported");
    if (rate == 0) throw new Exception("WAV has no sample rate");

    size_t bytesPerSample;
    if (format == 1) {          // PCM integer
        if (bits != 16 && bits != 24 && bits != 32)
            throw new Exception("unsupported PCM bit depth");
        bytesPerSample = bits / 8;
    } else if (format == 3) {   // IEEE float
        if (bits != 32) throw new Exception("unsupported float depth");
        bytesPerSample = 4;
    } else {
        throw new Exception("unsupported WAV format tag");
    }
    size_t frameBytes = bytesPerSample * channels;
    size_t frames = data.length / frameBytes;
    auto result = WavData(rate,
        new double[][](channels, frames));
    foreach (f; 0 .. frames) {
        foreach (c; 0 .. channels) {
            size_t off = f * frameBytes + c * bytesPerSample;
            double v;
            if (format == 3) {
                ubyte[4] tmp = data[off .. off + 4];
                uint u = littleEndianToNative!uint(tmp);
                v = *cast(float*)&u;
            } else if (bits == 16) {
                ubyte[2] tmp = data[off .. off + 2];
                v = littleEndianToNative!short(tmp) / 32768.0;
            } else if (bits == 24) {
                int s = data[off] | (data[off + 1] << 8) |
                        (data[off + 2] << 16);
                if (s & 0x800000) s -= 0x1000000;
                v = s / 8388608.0;
            } else {
                ubyte[4] tmp = data[off .. off + 4];
                v = littleEndianToNative!int(tmp) / 2147483648.0;
            }
            result.channels[c][f] = v;
        }
    }
    return result;
}

/// Read and decode a WAV file from disk.
WavData readWavFile(string path) {
    return parseWav(cast(ubyte[]) read(path));
}
