/*
    Torogao vendor-extension data (INP EXT section)

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    Torogao's project-level data (rating/character metadata,
    expression states, stage variants, blend-shape parameters,
    and the consumer layer) lives in the INP container's EXT
    section under one vendor section name. The format defines
    that section for exactly this purpose: the core library
    loads every EXT section into Puppet.extData untouched, and
    re-saving a project from the editor preserves the sections
    it does not know. Nothing here extends the payload JSON,
    whose shape belongs to the core format.

    Reading goes through the loaded puppet (Puppet.extData).
    Writing a NEW project file with an updated Torogao section
    is byte surgery on the container only: magic, payload, and
    texture section are copied verbatim, other vendors'
    sections are preserved in order, and only the Torogao
    section is replaced or appended. Writing is export-class:
    callers always name a new output path.
*/
module torogao.cli.extdata;

import std.json : JSONValue, JSONType, parseJSON;

/// The Torogao vendor section name in the EXT section.
enum torogaoExtSection = "io.ntranch.torogao";

/// Extension document format identifier (the section's JSON).
enum torogaoExtFormat = "torogao-ext/1";

private void putBE(ref ubyte[] out_, uint v) {
    out_ ~= cast(ubyte)(v >> 24);
    out_ ~= cast(ubyte)(v >> 16);
    out_ ~= cast(ubyte)(v >> 8);
    out_ ~= cast(ubyte) v;
}

private uint getBE(in ubyte[] b, size_t at) {
    return (cast(uint) b[at] << 24) | (cast(uint) b[at + 1] << 16)
        | (cast(uint) b[at + 2] << 8) | cast(uint) b[at + 3];
}

/// A parsed EXT section: name plus raw JSON bytes.
struct ExtSection {
    string name;
    ubyte[] data;
}

/**
    Splits an INP file's bytes into the prefix (everything up
    to and including the texture section) and its EXT
    sections. Returns false with `error` on a malformed file.
*/
bool splitInp(in ubyte[] bytes, out ubyte[] prefix,
              out ExtSection[] sections, out string error) {
    enum magic = cast(ubyte[]) "TRNSRTS\0";
    enum texMagic = cast(ubyte[]) "TEX_SECT";
    enum extMagic = cast(ubyte[]) "EXT_SECT";
    if (bytes.length < 12 || bytes[0 .. 8] != magic) {
        error = "not an INP container (bad magic)";
        return false;
    }
    size_t at = 8;
    uint payloadLen = getBE(bytes, at);
    at += 4 + payloadLen;
    if (at + 8 > bytes.length || bytes[at .. at + 8] != texMagic) {
        error = "INP texture section not found where expected";
        return false;
    }
    at += 8;
    if (at + 4 > bytes.length) {
        error = "INP texture section is truncated";
        return false;
    }
    uint slotCount = getBE(bytes, at);
    at += 4;
    foreach (i; 0 .. slotCount) {
        if (at + 5 > bytes.length) {
            error = "INP texture slot is truncated";
            return false;
        }
        uint len = getBE(bytes, at);
        at += 4 + 1 + len;
    }
    if (at > bytes.length) {
        error = "INP texture section overruns the file";
        return false;
    }
    prefix = bytes[0 .. at].dup;
    if (at == bytes.length) return true; // no EXT section
    if (at + 8 > bytes.length || bytes[at .. at + 8] != extMagic) {
        error = "unexpected trailing data after the texture section";
        return false;
    }
    at += 8;
    if (at + 4 > bytes.length) {
        error = "INP EXT section is truncated";
        return false;
    }
    uint count = getBE(bytes, at);
    at += 4;
    foreach (i; 0 .. count) {
        if (at + 4 > bytes.length) {
            error = "INP EXT section entry is truncated";
            return false;
        }
        uint nameLen = getBE(bytes, at);
        at += 4;
        if (at + nameLen + 4 > bytes.length) {
            error = "INP EXT section name is truncated";
            return false;
        }
        ExtSection s;
        s.name = cast(string) bytes[at .. at + nameLen].dup;
        at += nameLen;
        uint dataLen = getBE(bytes, at);
        at += 4;
        if (at + dataLen > bytes.length) {
            error = "INP EXT section payload is truncated";
            return false;
        }
        s.data = bytes[at .. at + dataLen].dup;
        at += dataLen;
        sections ~= s;
    }
    return true;
}

/**
    Writes a new INP file at outPath: the source's prefix
    verbatim, plus an EXT section holding every source section
    (in order) with the Torogao section replaced by `ext`
    (or appended when the source has none).
*/
bool writeProjectWithExt(string inPath, string outPath, JSONValue ext,
                         out string error) {
    import std.file : read, write;
    ubyte[] bytes;
    try bytes = cast(ubyte[]) read(inPath);
    catch (Exception e) {
        error = "project unreadable: " ~ e.msg;
        return false;
    }
    ubyte[] prefix;
    ExtSection[] sections;
    if (!splitInp(bytes, prefix, sections, error)) return false;

    auto extBytes = cast(ubyte[]) ext.toString();
    ExtSection[] kept;
    bool replaced = false;
    foreach (s; sections) {
        if (s.name == torogaoExtSection) {
            if (!replaced) {
                ExtSection mine;
                mine.name = torogaoExtSection;
                mine.data = extBytes;
                kept ~= mine;
                replaced = true;
            }
            // A duplicate Torogao section is dropped: the
            // section is single by contract.
        } else {
            kept ~= s;
        }
    }
    if (!replaced) {
        ExtSection mine;
        mine.name = torogaoExtSection;
        mine.data = extBytes;
        kept ~= mine;
    }

    ubyte[] out_ = prefix;
    if (kept.length > 0) {
        out_ ~= cast(ubyte[]) "EXT_SECT";
        putBE(out_, cast(uint) kept.length);
        foreach (s; kept) {
            putBE(out_, cast(uint) s.name.length);
            out_ ~= cast(ubyte[]) s.name;
            putBE(out_, cast(uint) s.data.length);
            out_ ~= s.data;
        }
    }
    try write(outPath, out_);
    catch (Exception e) {
        error = "could not write project: " ~ e.msg;
        return false;
    }
    return true;
}

/**
    The Torogao extension document of a loaded puppet, parsed
    from its EXT section. Returns a null JSONValue when the
    project carries no Torogao section (or an unparseable one;
    `malformed` is then set so callers can fail loudly).
*/
JSONValue torogaoExtOf(const(ubyte[][string]) extData, out bool malformed) {
    malformed = false;
    auto raw = torogaoExtSection in extData;
    if (raw is null || raw.length == 0) return JSONValue(null);
    try return parseJSON(cast(string) *raw);
    catch (Exception) {
        malformed = true;
        return JSONValue(null);
    }
}
