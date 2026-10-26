/*
    Torogao headless CLI — INP container access

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.
*/
module torogao.cli.inpfile;

import std.json : JSONValue, parseJSON;
import std.exception : enforce;

/**
    Extracts the JSON payload of an INP/INX file.

    The library's loader consumes the same payload; the CLI reads
    it separately because one validation check (bindings the
    loader silently drops) is only observable here. Returns a null
    JSONValue when the file is not an INP container.
*/
JSONValue inpPayload(string path) {
    import std.file : read;
    auto buffer = cast(ubyte[])read(path);
    enum magic = cast(ubyte[])"TRNSRTS\0";
    if (buffer.length < 12 || buffer[0..8] != magic)
        return JSONValue(null);
    import std.bitmanip : bigEndianToNative;
    ubyte[4] lenBytes = buffer[8..12];
    uint payloadLength = bigEndianToNative!uint(lenBytes);
    enforce(12 + payloadLength <= buffer.length,
        "INP payload length exceeds the file size");
    return parseJSON(cast(string)buffer[12..12 + payloadLength]);
}
