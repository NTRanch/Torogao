/*
    Torogao headless CLI

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    Subcommands:
        torogao-cli inspect <project.inx> [--out file.json]
        torogao-cli validate <project.inx> [--out file.json]
        torogao-cli export-rig <project.inx> --out <dir>
        torogao-cli mcp [--stdio]      MCP server over stdio
        torogao-cli a2a [--bind <addr>] [--port <n>]
                                       A2A HTTP surface
        torogao-cli registry-dump      the tool registry as JSON

    The agent surfaces (mcp, a2a) delegate to the same tool
    registry the subcommands use; see docs/agents.md.

    Exit codes:
        0  success (validate: no findings)
        1  validate: findings present
        2  project could not be loaded
        3  usage error
        4  export failed after load
*/
module torogao.cli.app;

import inochi2d.torogao.bridge : torogaoInitHeadless;
import inochi2d.core : Puppet;
import inochi2d.fmt : inLoadPuppet;
import inochi2d.integration : inCurrentPuppetTextureSlots;
import torogao.a2a.server;
import torogao.cli.criteria;
import torogao.cli.inpfile;
import torogao.cli.inspect_;
import torogao.cli.puppetwalk;
import torogao.cli.rigexport;
import torogao.mcp.server;
import torogao.tools.projectops;
import torogao.tools.registry;
import std.json : JSONValue, JSONType;
import std.stdio : writeln, stderr;
import std.file : exists, mkdirRecurse, write;
import std.path : baseName, buildPath, stripExtension;

int main(string[] args) {
    // Agent-surface modes take no project argument.
    if (args.length >= 2 && args[1] == "mcp") {
        return runMcpServer(buildCoreRegistry());
    }
    if (args.length >= 2 && args[1] == "registry-dump") {
        writeln(buildCoreRegistry().dumpJson().toString());
        return 0;
    }
    if (args.length >= 2 && args[1] == "a2a") {
        string bind = "127.0.0.1";
        ushort port = a2aDefaultPort;
        for (size_t i = 2; i < args.length; i++) {
            if (args[i] == "--port" && i + 1 < args.length) {
                import std.conv : to, ConvException;
                try {
                    port = to!ushort(args[++i]);
                } catch (ConvException) {
                    stderr.writeln("a2a: --port needs a number");
                    return 3;
                }
            } else if (args[i] == "--bind" && i + 1 < args.length) {
                bind = args[++i];
            } else {
                stderr.writeln("a2a: unknown argument: ", args[i]);
                return 3;
            }
        }
        return runA2aServer(buildCoreRegistry(), bind, port);
    }
    if (args.length < 3) {
        stderr.writeln("usage: torogao-cli <inspect|validate|export-rig|mcp|a2a|registry-dump> <project.inx> [--out <path>]");
        return 3;
    }
    string command = args[1];
    string projectPath = args[2];
    string outPath;
    for (size_t i = 3; i + 1 < args.length + 1 && i < args.length; i++) {
        if (args[i] == "--out" && i + 1 < args.length) outPath = args[++i];
    }
    if (command != "inspect" && command != "validate" &&
        command != "export-rig") {
        stderr.writeln("unknown subcommand: ", command);
        return 3;
    }
    if (!exists(projectPath)) {
        stderr.writeln("project file not found: ", projectPath);
        return 2;
    }
    if (command == "export-rig" && outPath.length == 0) {
        stderr.writeln("export-rig requires --out <dir>");
        return 3;
    }

    torogaoInitHeadless();

    Puppet puppet;
    try {
        puppet = inLoadPuppet(projectPath);
    } catch (Exception ex) {
        JSONValue err;
        err["file"] = projectPath;
        err["result"] = "unloadable";
        err["error"] = ex.msg;
        writeln(err.toString());
        return 2;
    }

    auto walk = new PuppetWalk(puppet);
    auto payload = inpPayload(projectPath);
    size_t slotCount = inCurrentPuppetTextureSlots.length;

    switch (command) {
        case "inspect": {
            JSONValue doc;
            doc["file"] = projectPath;
            doc["census"] = census(puppet, walk);
            emit(doc, outPath);
            return 0;
        }
        case "validate": {
            auto findings = validatePuppet(puppet, walk, payload, slotCount);
            JSONValue doc;
            doc["file"] = projectPath;
            doc["result"] = findings.length == 0 ? "clean" : "findings";
            JSONValue[] list;
            foreach(f; findings) list ~= f.toJson();
            doc["findings"] = list;
            emit(doc, outPath);
            return findings.length == 0 ? 0 : 1;
        }
        case "export-rig": {
            auto exporter = new RigExporter(puppet, walk, slotCount);
            ExportResult result;
            try {
                result = exporter.run();
            } catch (Exception ex) {
                stderr.writeln("export failed: ", ex.msg);
                return 4;
            }
            mkdirRecurse(outPath);
            string base = projectPath.baseName.stripExtension;
            string modelPath = buildPath(outPath, base ~ ".model.json");
            string reportPath = buildPath(outPath, base ~ ".export-report.json");
            write(modelPath, result.model.toString() ~ "\n");
            JSONValue report;
            report["file"] = projectPath;
            report["model"] = modelPath;
            report["mapped"] = result.count("mapped");
            report["approximated"] = result.count("approximated");
            report["dropped"] = result.count("dropped");
            JSONValue[] list;
            foreach(e; result.entries) list ~= e.toJson();
            report["entries"] = list;
            write(reportPath, report.toString() ~ "\n");
            writeln(report.toString());
            return 0;
        }
        default:
            return 3;
    }
}

private void emit(JSONValue doc, string outPath) {
    string text = doc.toString();
    if (outPath.length > 0) write(outPath, text ~ "\n");
    writeln(text);
}
