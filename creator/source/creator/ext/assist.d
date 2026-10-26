/*
    Torogao agent assist

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    The editor's built-in assist talks to Torogao's agent tool
    surface exactly the way an external agent does: it spawns
    `torogao-cli mcp` as a stdio child, speaks MCP/JSON-RPC to
    it, and shows the result in a dialog. There is no private
    back door — anything the assist can do, an external agent
    can do through the same tools, under the same gates.

    v1 action: validate the open project (Tools menu). The
    exchange runs on a worker thread with a hard deadline so
    the UI never blocks on the child; incAssistPoll() (called
    once per frame from the app loop) surfaces the result.
*/
module creator.ext.assist;

import creator : incProjectPath;
import creator.core : incSetStatus;
import creator.widgets.dialog : incDialog, DialogLevel;
import core.thread : Thread;
import core.time : dur;
import std.json : JSONValue, JSONType, parseJSON;
import std.process : ProcessPipes, environment, kill, pipeProcess,
    Redirect, wait;
import std.string : splitLines, toStringz;

private {
    __gshared bool assistBusy;
    __gshared bool assistDone;
    __gshared bool assistExchangeFinished;
    __gshared string assistTitle;
    __gshared string assistBody;
    __gshared DialogLevel assistLevel;

    enum assistDeadlineSeconds = 30;
    enum assistMaxResponseBytes = 4 * 1024 * 1024;
}

/// Tools > Agent > "Validate Project via Agent..."
void incAssistValidateProject() {
    if (assistBusy) return;

    string projectPath = incProjectPath();
    if (projectPath.length == 0) {
        incDialog("Agent Assist".toStringz,
            "No project file is associated with this session yet.\n\n" ~
            "Open or save a project first, then run validation again.",
            DialogLevel.Warning);
        return;
    }
    string cliPath = findAgentCli();
    if (cliPath.length == 0) {
        incDialog("Agent Assist".toStringz,
            "Could not locate the torogao-cli tool.\n\n" ~
            "The assist talks to Torogao's agent surface through " ~
            "torogao-cli (the headless tool in tools/torogao-cli). " ~
            "Build it, put it on the PATH, or point the TOROGAO_CLI " ~
            "environment variable at it.",
            DialogLevel.Error);
        return;
    }

    assistBusy = true;
    assistDone = false;
    assistExchangeFinished = false;
    incSetStatus("Validating project via the agent surface...");
    auto worker = new Thread(() => assistExchange(cliPath, projectPath));
    worker.isDaemon = true;
    worker.start();
}

/// Called once per frame by the app loop; shows a finished result.
void incAssistPoll() {
    if (!assistDone) return;
    assistDone = false;
    assistBusy = false;
    incDialog(assistTitle.toStringz, assistBody, assistLevel);
}

/**
    Locates the torogao-cli binary: the TOROGAO_CLI environment
    variable first, then beside the editor executable, then the
    in-tree build location relative to it, then the PATH (the
    bare name, which spawn resolves). Returns "" when no local
    candidate exists and PATH lookup is the only hope — the
    caller treats that as "not found" only when spawn fails;
    here we return the bare name as a last resort candidate.
*/
private string findAgentCli() {
    import std.file : exists;
    import std.path : buildPath, dirName;

    if (auto fromEnv = environment.get("TOROGAO_CLI")) {
        if (fromEnv.length > 0 && exists(fromEnv)) return fromEnv;
    }
    string exeDir;
    version (linux) {
        import core.sys.posix.unistd : readlink;
        char[4096] buf;
        auto n = readlink("/proc/self/exe", buf.ptr, buf.length);
        if (n > 0) exeDir = dirName(cast(string) buf[0 .. cast(size_t) n].dup);
    }
    if (exeDir.length > 0) {
        string sibling = buildPath(exeDir, "torogao-cli");
        if (exists(sibling)) return sibling;
        string inTree = buildPath(exeDir, "..", "..", "tools",
            "torogao-cli", "out", "torogao-cli");
        if (exists(inTree)) return inTree;
    }
    return "torogao-cli"; // last resort: PATH resolution at spawn
}

private void assistExchange(string cliPath, string projectPath) {
    string body = "The agent surface did not answer.";
    DialogLevel level = DialogLevel.Error;
    try {
        auto pipes = pipeProcess([cliPath, "mcp"],
            Redirect.stdin | Redirect.stdout | Redirect.stderr);

        // Watchdog: the exchange has a hard deadline.
        auto watchdog = new Thread(() {
            Thread.sleep(dur!"seconds"(assistDeadlineSeconds));
            if (!assistExchangeFinished) {
                try kill(pipes.pid);
                catch (Exception) { }
            }
        });
        watchdog.isDaemon = true;
        watchdog.start();

        import std.json : JSONValue;
        JSONValue init;
        init["jsonrpc"] = "2.0";
        init["id"] = 1;
        init["method"] = "initialize";
        JSONValue initParams;
        initParams["protocolVersion"] = "2024-11-05";
        initParams["capabilities"] = JSONValue.emptyObject;
        JSONValue clientInfo;
        clientInfo["name"] = "torogao-editor";
        clientInfo["version"] = "1.5.0";
        initParams["clientInfo"] = clientInfo;
        init["params"] = initParams;
        pipes.stdin.writeln(init.toString());

        JSONValue call;
        call["jsonrpc"] = "2.0";
        call["id"] = 2;
        call["method"] = "tools/call";
        JSONValue callParams;
        callParams["name"] = "validate";
        JSONValue arguments;
        arguments["project"] = projectPath;
        callParams["arguments"] = arguments;
        call["params"] = callParams;
        pipes.stdin.writeln(call.toString());
        pipes.stdin.flush();
        pipes.stdin.close();

        // Read the child's stdout to EOF with POSIX reads
        // (std.stdio rawRead would stall the pipe; see the
        // MCP server's FrameReader for the same lesson).
        import core.sys.posix.unistd : read;
        import std.stdio : fileno;
        int fd = pipes.stdout.fileno();
        ubyte[] data;
        ubyte[8192] chunk;
        while (true) {
            auto n = read(fd, chunk.ptr, chunk.length);
            if (n <= 0) break;
            data ~= chunk[0 .. cast(size_t) n];
            if (data.length > assistMaxResponseBytes) break;
        }
        wait(pipes.pid);
        assistExchangeFinished = true;

        JSONValue callResponse;
        foreach (line; splitLines(cast(string) data)) {
            if (line.length == 0) continue;
            JSONValue msg;
            try msg = parseJSON(line);
            catch (Exception) continue;
            if (msg.type != JSONType.object) continue;
            if (auto id = "id" in msg)
                if (id.type == JSONType.integer && id.integer == 2)
                    callResponse = msg;
        }
        if (callResponse.type == JSONType.null_) {
            body = "The agent surface answered without a validation " ~
                "result. The torogao-cli tool may be out of date.";
        } else {
            body = summarize(callResponse, level);
        }
    } catch (Exception ex) {
        assistExchangeFinished = true;
        body = "Could not run the agent surface: " ~ ex.msg;
        level = DialogLevel.Error;
    }
    assistTitle = "Agent Assist";
    assistBody = body;
    assistLevel = level;
    assistDone = true;
}

/// Renders the tools/call response as dialog text.
private string summarize(JSONValue callResponse, out DialogLevel level) {
    import std.format : format;

    if (auto err = "error" in callResponse) {
        level = DialogLevel.Error;
        string message = "unknown error";
        string code = "";
        if (auto m = "message" in *err) message = m.str;
        if (auto d = "data" in *err)
            if (auto c = "code" in *d) code = c.str;
        return format!"The agent surface returned an error (%s):\n\n%s"(
            code, message);
    }
    auto result = "result" in callResponse;
    if (result is null) {
        level = DialogLevel.Error;
        return "The agent surface returned a malformed result.";
    }
    if (auto isErr = "isError" in *result) {
        if ((isErr.type == JSONType.true_
                || isErr.type == JSONType.false_) && isErr.boolean) {
            level = DialogLevel.Error;
            string text = "";
            if (auto content = "content" in *result)
                if (content.type == JSONType.array
                        && content.array.length > 0)
                    if (auto t = "text" in content.array[0])
                        text = t.str;
            string code = "tool_error";
            string message = text;
            try {
                auto parsed = parseJSON(text);
                if (auto e = "error" in parsed) {
                    if (auto c = "code" in *e) code = c.str;
                    if (auto m = "message" in *e) message = m.str;
                }
            } catch (Exception) { }
            return format!"Validation could not run (%s):\n\n%s"(
                code, message);
        }
    }
    auto payload = "structuredContent" in *result;
    if (payload is null) {
        level = DialogLevel.Error;
        return "The agent surface returned no structured result.";
    }
    string file = "";
    if (auto f = "file" in *payload) file = f.str;
    string verdict = "";
    if (auto r = "result" in *payload) verdict = r.str;
    if (verdict == "clean") {
        level = DialogLevel.Info;
        return "Validation clean — no findings.\n\nProject: " ~ file ~
            "\n\nThis check ran through Torogao's agent tool surface:" ~
            " the same validate tool external agents call over MCP" ~
            " and A2A.";
    }
    level = DialogLevel.Warning;
    string text = "Validation findings";
    size_t count = 0;
    string lines;
    if (auto findings = "findings" in *payload) {
        if (findings.type == JSONType.array) {
            count = findings.array.length;
            foreach (finding; findings.array) {
                string code, subject, message;
                if (auto c = "code" in finding) code = c.str;
                if (auto s = "subject" in finding) subject = s.str;
                if (auto m = "message" in finding) message = m.str;
                lines ~= format!"\n\n[%s] %s\n%s"(code, subject, message);
            }
        }
    }
    return format!"%s (%d):\n\nProject: %s%s"(
        text, count, file, lines);
}
