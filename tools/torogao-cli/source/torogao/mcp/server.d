/*
    Torogao MCP server mode

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    `torogao-cli mcp` serves the tool registry over stdio as an
    MCP server: newline-delimited JSON-RPC 2.0 frames, hand-rolled
    in the bounded style — a hard frame size cap, typed protocol
    errors, and no input (malformed, oversized, unknown) that can
    crash or hang the server. Every tool call delegates to the
    one tool registry; this module is transport only.

    Error model:
      - Transport/protocol problems (unparseable frame, frame
        over the cap, unknown method, argument problems an MCP
        client caused: unknown tool, disabled tool, malformed
        arguments) are JSON-RPC error responses with the stable
        machine code in error.data.code.
      - Tool execution failures (missing project, unloadable
        project, failed export) are normal tool results with
        isError set, so an agent sees them as tool outcomes.
*/
module torogao.mcp.server;

import torogao.tools.registry;
import std.json : JSONValue, JSONType, parseJSON;
import std.stdio : File, stdin, stdout;

/// Hard cap on one input frame, in bytes (1 MiB).
enum maxFrameBytes = 1_048_576;

/// MCP protocol version this server answers with.
enum mcpProtocolVersion = "2024-11-05";

/**
    Reads one newline-terminated frame at a time from a file
    descriptor, enforcing the size cap while reading: an
    over-cap frame is consumed through its newline (so the
    stream resyncs) and reported via `oversized` instead of
    being buffered whole.

    The descriptor is read with POSIX read() directly:
    std.stdio's rawRead fills its whole buffer before
    returning, which would stall a pipe until EOF.
*/
private final class FrameReader {
    private int fd;
    private ubyte[] pending;
    private bool eof;

    this(File input) {
        import std.stdio : fileno;
        this.fd = input.fileno();
    }

    /// False at end of stream (with no partial frame delivered).
    bool readFrame(out string frame, out bool oversized) {
        frame = null;
        oversized = false;
        bool overflow = false;
        size_t scanned = 0;
        while (true) {
            // Look for a newline in what we already hold.
            foreach (i, b; pending[scanned .. $]) {
                if (b == '\n') {
                    size_t end = scanned + i;
                    if (overflow || end > maxFrameBytes) {
                        oversized = true;
                    } else {
                        frame = cast(string) pending[0 .. end].dup;
                    }
                    pending = pending[end + 1 .. $];
                    return true;
                }
            }
            scanned = pending.length;
            if (scanned > maxFrameBytes) overflow = true;
            if (overflow) {
                // Discard buffered bytes; only resync matters now.
                pending = pending[$ .. $];
                scanned = 0;
            }
            if (eof && pending.length == 0) return false;
            if (eof) {
                // Final frame without a trailing newline.
                if (!overflow) frame = cast(string) pending.dup;
                pending = pending[$ .. $];
                oversized = overflow;
                return true;
            }
            import core.sys.posix.unistd : read;
            ubyte[8192] chunk;
            auto n = read(fd, chunk.ptr, chunk.length);
            if (n <= 0) {
                eof = true;
            } else {
                pending ~= chunk[0 .. cast(size_t) n];
            }
        }
    }
}

private JSONValue rpcError(JSONValue id, int code, string dataCode,
                           string message) {
    JSONValue response;
    response["jsonrpc"] = "2.0";
    response["id"] = id;
    JSONValue error;
    error["code"] = code;
    error["message"] = message;
    JSONValue data;
    data["code"] = dataCode;
    error["data"] = data;
    response["error"] = error;
    return response;
}

private JSONValue rpcResult(JSONValue id, JSONValue result) {
    JSONValue response;
    response["jsonrpc"] = "2.0";
    response["id"] = id;
    response["result"] = result;
    return response;
}

/// MCP tool description, generated from the registry spec.
private JSONValue toolJson(const ToolSpec spec) {
    JSONValue tool;
    tool["name"] = spec.name;
    tool["title"] = spec.title;
    tool["description"] = spec.description;
    tool["inputSchema"] = spec.inputSchema;
    JSONValue annotations;
    annotations["readOnlyHint"] = spec.posture == ToolPosture.readOnly;
    annotations["destructiveHint"] = false;
    annotations["idempotentHint"] = spec.posture != ToolPosture.exportOnly;
    annotations["openWorldHint"] = false;
    annotations["torogao:namespace"] = spec.namespace_;
    annotations["torogao:posture"] = postureLabel(spec.posture);
    if (spec.posture == ToolPosture.declaredDisabled)
        annotations["torogao:gate"] = spec.gate;
    tool["annotations"] = annotations;
    return tool;
}

/**
    Handles one frame. Returns true (and fills `response`) when
    a response must be written; notifications return false.
*/
private bool handleFrame(string frame, ToolRegistry registry,
                         out JSONValue response) {
    JSONValue msg;
    try {
        msg = parseJSON(frame);
    } catch (Exception ex) {
        response = rpcError(JSONValue(null), -32700, "parse_error",
            "frame is not valid JSON: " ~ ex.msg);
        return true;
    }
    if (msg.type != JSONType.object) {
        response = rpcError(JSONValue(null), -32600, "invalid_request",
            "frame must be a JSON object");
        return true;
    }
    auto methodVal = "method" in msg;
    if (methodVal is null || methodVal.type != JSONType.string) {
        response = rpcError(JSONValue(null), -32600, "invalid_request",
            "frame has no string method");
        return true;
    }
    string method = methodVal.str;
    auto idVal = "id" in msg;
    bool isNotification = idVal is null || idVal.type == JSONType.null_;
    JSONValue id = isNotification ? JSONValue(null) : *idVal;

    switch (method) {
        case "initialize": {
            JSONValue result;
            result["protocolVersion"] = mcpProtocolVersion;
            JSONValue caps;
            JSONValue tools;
            tools["listChanged"] = false;
            caps["tools"] = tools;
            result["capabilities"] = caps;
            JSONValue info;
            info["name"] = "torogao";
            info["title"] = "Torogao";
            info["version"] = torogaoToolsVersion;
            result["serverInfo"] = info;
            response = rpcResult(id, result);
            return true;
        }
        case "ping": {
            response = rpcResult(id, JSONValue.emptyObject);
            return true;
        }
        case "tools/list": {
            JSONValue[] tools;
            foreach (spec; registry.list()) tools ~= toolJson(spec);
            JSONValue result;
            result["tools"] = tools;
            response = rpcResult(id, result);
            return true;
        }
        case "tools/call": {
            auto paramsVal = "params" in msg;
            if (paramsVal is null || paramsVal.type != JSONType.object) {
                response = rpcError(id, -32602, "invalid_arguments",
                    "tools/call requires a params object");
                return true;
            }
            auto nameVal = "name" in *paramsVal;
            if (nameVal is null || nameVal.type != JSONType.string) {
                response = rpcError(id, -32602, "invalid_arguments",
                    "tools/call requires a tool name");
                return true;
            }
            JSONValue arguments = JSONValue.emptyObject;
            if (auto argsVal = "arguments" in *paramsVal) {
                if (argsVal.type != JSONType.object) {
                    response = rpcError(id, -32602, "invalid_arguments",
                        "tool arguments must be a JSON object");
                    return true;
                }
                arguments = *argsVal;
            }
            auto result = registry.call(nameVal.str, arguments);
            if (result.isError) {
                // Client-caused problems are protocol errors.
                if (result.errorCode == "unknown_tool"
                        || result.errorCode == "tool_disabled"
                        || result.errorCode == "invalid_arguments") {
                    response = rpcError(id, -32602, result.errorCode,
                        result.errorMessage);
                    return true;
                }
                // Everything else is a tool outcome.
                JSONValue toolResult;
                JSONValue content;
                content["type"] = "text";
                content["text"] = result.errorJson().toString();
                toolResult["content"] = [content];
                toolResult["isError"] = true;
                response = rpcResult(id, toolResult);
                return true;
            }
            JSONValue toolResult;
            JSONValue content;
            content["type"] = "text";
            content["text"] = result.payload.toString();
            toolResult["content"] = [content];
            toolResult["structuredContent"] = result.payload;
            toolResult["isError"] = false;
            response = rpcResult(id, toolResult);
            return true;
        }
        default: {
            if (isNotification) return false; // notifications: ignore
            if (method.length > 12 && method[0 .. 12] == "notifications")
                return false;
            response = rpcError(id, -32601, "method_not_found",
                "unknown method: " ~ method);
            return true;
        }
    }
}

/// Runs the MCP server on stdio until the input reaches EOF.
int runMcpServer(ToolRegistry registry) {
    auto reader = new FrameReader(stdin);
    string frame;
    bool oversized;
    while (reader.readFrame(frame, oversized)) {
        if (oversized) {
            import std.format : format;
            auto response = rpcError(JSONValue(null), -32600,
                "frame_too_large",
                format!"frame exceeds the %d-byte cap"(maxFrameBytes));
            stdout.writeln(response.toString());
            stdout.flush();
            continue;
        }
        if (frame.length == 0) continue; // blank lines are ignored
        JSONValue response;
        if (handleFrame(frame, registry, response)) {
            stdout.writeln(response.toString());
            stdout.flush();
        }
    }
    return 0;
}
