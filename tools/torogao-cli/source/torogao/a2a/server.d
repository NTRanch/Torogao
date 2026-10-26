/*
    Torogao A2A surface

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    `torogao-cli a2a [--bind 127.0.0.1] [--port 8377]` publishes
    the agent surface other agents commission work from:

      GET  /.well-known/agent-card.json   the Agent Card; its
                                          skills are generated
                                          from the tool registry
      POST /                              JSON-RPC task methods:
                                          message/send, tasks/get,
                                          tasks/cancel

    Tasks delegate to the same tool registry the MCP server
    serves — this module is transport only, like the MCP one.
    A message/send carries one data part of the form
    {"tool": "<name>", "arguments": {...}}; the task completes
    synchronously and its artifact is the tool's payload (or
    its typed error, as a failed task).

    Transport note: the HTTP layer is a small bounded server
    over std.socket (Phobos) rather than a web framework — the
    surface is two routes and one request shape, requests and
    headers are size-capped, and it keeps the headless tool's
    dependency footprint at zero additions.
*/
module torogao.a2a.server;

import torogao.tools.registry;
import std.json : JSONValue, JSONType, parseJSON;
import std.socket;
import core.time : dur;
import std.conv : to;
import std.format : format;

/// Default port for the A2A surface.
enum a2aDefaultPort = 8377;

private enum maxHeaderBytes = 64 * 1024;
private enum maxBodyBytes = 1_048_576;

/// Builds the Agent Card JSON from the registry.
JSONValue buildAgentCard(ToolRegistry registry, string baseUrl) {
    JSONValue card;
    card["name"] = "Torogao";
    card["description"] =
        "Torogao puppet tooling for agents: inspect, validate, " ~
        "and convert realtime 2D puppet projects. Every skill " ~
        "delegates to the one tool registry; tools that would " ~
        "write into a project are listed as disabled skills " ~
        "with their gate named.";
    card["url"] = baseUrl;
    card["version"] = torogaoToolsVersion;
    JSONValue provider;
    provider["organization"] = "Nice Thoughts Ranch";
    card["provider"] = provider;
    JSONValue capabilities;
    capabilities["streaming"] = false;
    capabilities["pushNotifications"] = false;
    capabilities["stateTransitionHistory"] = false;
    card["capabilities"] = capabilities;
    card["defaultInputModes"] = [JSONValue("application/json")];
    card["defaultOutputModes"] = [JSONValue("application/json")];
    JSONValue[] skills;
    foreach (spec; registry.list()) {
        JSONValue skill;
        skill["id"] = spec.name;
        skill["name"] = spec.title;
        skill["description"] = spec.description;
        skill["tags"] = [JSONValue(spec.namespace_),
                          JSONValue(postureLabel(spec.posture))];
        skill["inputModes"] = [JSONValue("application/json")];
        skill["outputModes"] = [JSONValue("application/json")];
        skills ~= skill;
    }
    card["skills"] = skills;
    return card;
}

private struct TaskRecord {
    JSONValue task; // the task object as served
}

/// Minimal JSON-RPC + routing layer over one accepted socket.
private final class A2aServer {
    private ToolRegistry registry;
    private string baseUrl;
    private TaskRecord[string] tasks;
    private size_t taskCounter;

    this(ToolRegistry registry, string baseUrl) {
        this.registry = registry;
        this.baseUrl = baseUrl;
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

    private JSONValue newTask(string contextId) {
        JSONValue task;
        task["id"] = format!"task-%d"(++taskCounter);
        task["contextId"] = contextId;
        JSONValue status;
        status["state"] = "working";
        task["status"] = status;
        task["artifacts"] = JSONValue.emptyArray;
        return task;
    }

    private JSONValue handleRpc(JSONValue msg) {
        auto idVal = "id" in msg;
        JSONValue id = idVal is null ? JSONValue(null) : *idVal;
        auto methodVal = "method" in msg;
        if (methodVal is null || methodVal.type != JSONType.string)
            return rpcError(id, -32600, "invalid_request",
                "request has no string method");
        auto paramsVal = "params" in msg;
        JSONValue params = paramsVal is null
            ? JSONValue.emptyObject : *paramsVal;

        switch (methodVal.str) {
            case "message/send": {
                JSONValue message;
                if (auto m = "message" in params) message = *m;
                else return rpcError(id, -32602, "invalid_arguments",
                    "message/send requires params.message");
                // Find the data part naming tool + arguments.
                string toolName;
                JSONValue arguments = JSONValue.emptyObject;
                bool found;
                if (auto parts = "parts" in message) {
                    if (parts.type == JSONType.array) {
                        foreach (part; parts.array) {
                            if (part.type != JSONType.object) continue;
                            auto kind = "kind" in part;
                            auto data = "data" in part;
                            if (kind !is null && data !is null
                                    && kind.type == JSONType.string
                                    && kind.str == "data"
                                    && data.type == JSONType.object) {
                                if (auto t = "tool" in *data) {
                                    if (t.type == JSONType.string) {
                                        toolName = t.str;
                                        found = true;
                                        if (auto a = "arguments" in *data)
                                            arguments = *a;
                                    }
                                }
                            }
                        }
                    }
                }
                if (!found)
                    return rpcError(id, -32602, "invalid_arguments",
                        "message must carry a data part " ~
                        "{\"tool\": ..., \"arguments\": {...}}");
                string contextId = "torogao";
                if (auto c = "contextId" in message)
                    if (c.type == JSONType.string) contextId = c.str;
                auto task = newTask(contextId);
                auto result = registry.call(toolName, arguments);
                JSONValue artifact;
                artifact["artifactId"] = task["id"].str ~ "-0";
                artifact["name"] = toolName;
                JSONValue part;
                part["kind"] = "data";
                if (result.isError) {
                    part["data"] = result.errorJson();
                    task["status"]["state"] = "failed";
                } else {
                    part["data"] = result.payload;
                    task["status"]["state"] = "completed";
                }
                artifact["parts"] = [part];
                task["artifacts"] = [artifact];
                tasks[task["id"].str] = TaskRecord(task);
                return rpcResult(id, task);
            }
            case "tasks/get": {
                auto tid = "id" in params;
                if (tid is null || tid.type != JSONType.string)
                    return rpcError(id, -32602, "invalid_arguments",
                        "tasks/get requires params.id");
                auto rec = tid.str in tasks;
                if (rec is null)
                    return rpcError(id, -32001, "task_not_found",
                        "no task with id " ~ tid.str);
                return rpcResult(id, rec.task);
            }
            case "tasks/cancel": {
                auto tid = "id" in params;
                if (tid is null || tid.type != JSONType.string)
                    return rpcError(id, -32602, "invalid_arguments",
                        "tasks/cancel requires params.id");
                auto rec = tid.str in tasks;
                if (rec is null)
                    return rpcError(id, -32001, "task_not_found",
                        "no task with id " ~ tid.str);
                // Tasks here complete synchronously; a terminal
                // task is returned as-is, anything else cancels.
                auto state = rec.task["status"]["state"].str;
                if (state != "completed" && state != "failed"
                        && state != "canceled") {
                    rec.task["status"]["state"] = "canceled";
                    tasks[tid.str] = *rec;
                }
                return rpcResult(id, rec.task);
            }
            default:
                return rpcError(id, -32601, "method_not_found",
                    "unknown method: " ~ methodVal.str);
        }
    }

    /// Serves one accepted connection to completion.
    void serve(Socket conn) {
        conn.setOption(SocketOptionLevel.SOCKET,
            SocketOption.RCVTIMEO, dur!"seconds"(30));
        ubyte[] buffer;
        ubyte[8192] chunk;
        size_t headerEnd = size_t.max;
        while (headerEnd == size_t.max) {
            auto n = conn.receive(chunk[]);
            if (n <= 0) return;
            buffer ~= chunk[0 .. cast(size_t) n];
            if (buffer.length > maxHeaderBytes) {
                respond(conn, 431, "application/json",
                    `{"error":"headers too large"}`);
                return;
            }
            size_t scanLimit = buffer.length >= 4 ? buffer.length - 3 : 0;
            foreach (i; 0 .. scanLimit) {
                if (buffer[i .. i + 4] == cast(ubyte[]) "\r\n\r\n") {
                    headerEnd = i + 4;
                    break;
                }
            }
        }
        string head = cast(string) buffer[0 .. headerEnd - 4];
        ubyte[] body = buffer[headerEnd .. $];
        // Parse the request line + the headers we act on.
        import std.string : split, splitLines, indexOf, strip, toLower;
        auto lines = splitLines(head);
        if (lines.length == 0) return;
        auto requestParts = lines[0].split(' ');
        if (requestParts.length < 2) return;
        string method = requestParts[0];
        string path = requestParts[1];
        size_t contentLength = 0;
        foreach (line; lines[1 .. $]) {
            auto colon = line.indexOf(':');
            if (colon < 0) continue;
            if (line[0 .. cast(size_t) colon].strip().toLower()
                    == "content-length") {
                try contentLength = to!size_t(
                    line[cast(size_t) colon + 1 .. $].strip());
                catch (Exception) contentLength = 0;
            }
        }
        if (contentLength > maxBodyBytes) {
            respond(conn, 413, "application/json",
                `{"error":"body too large"}`);
            return;
        }
        while (body.length < contentLength) {
            auto n = conn.receive(chunk[]);
            if (n <= 0) return;
            body ~= chunk[0 .. cast(size_t) n];
        }

        if (method == "GET" && path == "/.well-known/agent-card.json") {
            respond(conn, 200, "application/json",
                buildAgentCard(registry, baseUrl).toString());
            return;
        }
        if (method == "POST" && (path == "/" || path == "/rpc")) {
            JSONValue msg;
            try {
                msg = parseJSON(cast(string) body);
            } catch (Exception ex) {
                respond(conn, 400, "application/json",
                    rpcError(JSONValue(null), -32700, "parse_error",
                        ex.msg).toString());
                return;
            }
            if (msg.type != JSONType.object) {
                respond(conn, 400, "application/json",
                    rpcError(JSONValue(null), -32600,
                        "invalid_request",
                        "body must be a JSON object").toString());
                return;
            }
            respond(conn, 200, "application/json",
                handleRpc(msg).toString());
            return;
        }
        respond(conn, 404, "application/json",
            `{"error":"not found"}`);
    }

    private void respond(Socket conn, int status, string contentType,
                         string body) {
        string reason;
        switch (status) {
            case 200: reason = "OK"; break;
            case 400: reason = "Bad Request"; break;
            case 404: reason = "Not Found"; break;
            case 413: reason = "Content Too Large"; break;
            case 431: reason = "Request Header Fields Too Large"; break;
            default:  reason = "Status"; break;
        }
        string head = "HTTP/1.1 " ~ to!string(status) ~ " " ~ reason
            ~ "\r\nContent-Type: " ~ contentType
            ~ "\r\nContent-Length: " ~ to!string(body.length)
            ~ "\r\nConnection: close\r\n\r\n";
        conn.send(cast(ubyte[]) head);
        conn.send(cast(ubyte[]) body);
    }
}

/// Runs the A2A HTTP surface until the process is stopped.
int runA2aServer(ToolRegistry registry, string bindAddress,
                 ushort port) {
    string baseUrl = format!"http://%s:%d/"(bindAddress, port);
    auto server = new A2aServer(registry, baseUrl);
    auto listener = new TcpSocket();
    listener.setOption(SocketOptionLevel.SOCKET,
        SocketOption.REUSEADDR, true);
    listener.bind(new InternetAddress(bindAddress, port));
    listener.listen(16);
    import std.stdio : stderr;
    stderr.writefln("torogao a2a: listening on %s", baseUrl);
    while (true) {
        Socket conn;
        try {
            conn = listener.accept();
        } catch (Exception) {
            continue;
        }
        try {
            server.serve(conn);
        } catch (Exception) {
            // A broken client never takes the surface down.
        }
        conn.close();
    }
}
