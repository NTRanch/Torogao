/*
    Torogao headless CLI

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    Subcommands:
        torogao-cli inspect <project.inx> [--out file.json]
        torogao-cli validate <project.inx> [--out file.json]
        torogao-cli export-rig <project.inx> --out <dir>
        torogao-cli physics-preview <project.inx> --track <track.json>
                    [--fps <n>] [--out curves.json]
        torogao-cli list-templates     list rig templates + presets
        torogao-cli instantiate-template <name> [--preset <p>]...
                    --out <file.inx>   build a puppet from a template
        torogao-cli audio-render <recipe.json> --out <dir>
        torogao-cli audio-validate <recipe.json>
        torogao-cli audio-bake <file.wav> --recipe <bake.json>
                    --out <curves.json>
        torogao-cli viseme-check <project.inx> [--contract <file>]
        torogao-cli viseme-bake <track.json> [--contract <file>]
                    --out <curves.json>
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
import torogao.audio.bake;
import torogao.audio.proc;
import torogao.audio.wav;
import torogao.cli.creator;
import torogao.cli.drivers;
import torogao.cli.extdata;
import torogao.cli.packs;
import torogao.cli.writer;
import torogao.cli.physicspreview;
import torogao.cli.puppetwalk;
import torogao.cli.rigexport;
import torogao.cli.states;
import torogao.cli.templates;
import torogao.cli.viseme;
import torogao.curves;
import torogao.mcp.server;
import torogao.tools.projectops;
import torogao.tools.registry;
import std.conv : to, ConvException;
import std.digest.sha : sha256Of;
import std.format : format;
import std.json : JSONValue, JSONType, parseJSON;
import std.stdio : writeln, stderr;
import std.file : exists, mkdirRecurse, readText, write;
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
    if (args.length >= 2 && args[1] == "list-templates") {
        JSONValue doc;
        JSONValue[] list;
        foreach (t; listTemplates()) {
            JSONValue e;
            e["name"] = t.name;
            e["kind"] = t.kind;
            e["title"] = t.title;
            e["description"] = t.description;
            list ~= e;
        }
        doc["templates"] = list;
        writeln(doc.toString());
        return 0;
    }
    if (args.length >= 2 && args[1] == "instantiate-template") {
        if (args.length < 3) {
            stderr.writeln("usage: torogao-cli instantiate-template " ~
                "<name> [--preset <p>]... --out <file.inx>");
            return 3;
        }
        string name = args[2];
        string[] presets;
        string instOut;
        for (size_t i = 3; i < args.length; i++) {
            if (args[i] == "--preset" && i + 1 < args.length)
                presets ~= args[++i];
            else if (args[i] == "--out" && i + 1 < args.length)
                instOut = args[++i];
            else {
                stderr.writeln("instantiate-template: unknown " ~
                    "argument: ", args[i]);
                return 3;
            }
        }
        if (instOut.length == 0) {
            stderr.writeln("instantiate-template requires --out <file>");
            return 3;
        }
        string error;
        JSONValue summary;
        try summary = instantiateTemplate(name, presets, instOut, error);
        catch (Exception ex) {
            JSONValue doc;
            doc["template"] = name;
            doc["result"] = "error";
            doc["error"] = ex.msg;
            writeln(doc.toString());
            return 3;
        }
        if (error.length > 0) {
            JSONValue doc;
            doc["template"] = name;
            doc["result"] = "error";
            doc["error"] = error;
            writeln(doc.toString());
            return 3;
        }
        writeln(summary.toString());
        return 0;
    }
    if (args.length >= 2 && args[1] == "audio-validate") {
        if (args.length < 3) {
            stderr.writeln("usage: torogao-cli audio-validate <recipe.json>");
            return 3;
        }
        JSONValue recipe;
        try recipe = parseJSON(readText(args[2]));
        catch (Exception ex) {
            stderr.writeln("recipe unreadable: ", ex.msg);
            return 3;
        }
        auto findings = validateRecipe(recipe);
        JSONValue doc;
        doc["file"] = args[2];
        bool hasError = false;
        JSONValue[] list;
        foreach (f; findings) {
            list ~= f.toJson();
            if (f.severity == "error") hasError = true;
        }
        doc["result"] = hasError ? "findings" : "clean";
        doc["findings"] = list;
        writeln(doc.toString());
        return hasError ? 1 : 0;
    }
    if (args.length >= 2 && args[1] == "audio-render") {
        if (args.length < 3) {
            stderr.writeln("usage: torogao-cli audio-render " ~
                "<recipe.json> --out <dir>");
            return 3;
        }
        string outDir;
        for (size_t i = 3; i < args.length; i++)
            if (args[i] == "--out" && i + 1 < args.length)
                outDir = args[++i];
        if (outDir.length == 0) {
            stderr.writeln("audio-render requires --out <dir>");
            return 3;
        }
        string recipeText;
        JSONValue recipe;
        try {
            recipeText = readText(args[2]);
            recipe = parseJSON(recipeText);
        } catch (Exception ex) {
            stderr.writeln("recipe unreadable: ", ex.msg);
            return 3;
        }
        mkdirRecurse(outDir);
        auto result = renderRecipe(recipe, recipeText, outDir);
        if (result.error.length > 0) {
            JSONValue doc;
            doc["file"] = args[2];
            doc["result"] = "error";
            doc["error"] = result.error;
            writeln(doc.toString());
            return 3;
        }
        writeln(result.manifest.toString());
        return 0;
    }
    if (args.length >= 2 && args[1] == "audio-bake") {
        if (args.length < 3) {
            stderr.writeln("usage: torogao-cli audio-bake <file.wav> " ~
                "--recipe <bake.json> --out <curves.json>");
            return 3;
        }
        string bakeRecipePath, bakeOut;
        for (size_t i = 3; i < args.length; i++) {
            if (args[i] == "--recipe" && i + 1 < args.length)
                bakeRecipePath = args[++i];
            else if (args[i] == "--out" && i + 1 < args.length)
                bakeOut = args[++i];
        }
        if (bakeRecipePath.length == 0 || bakeOut.length == 0) {
            stderr.writeln("audio-bake requires --recipe and --out");
            return 3;
        }
        JSONValue bakeRecipe;
        try bakeRecipe = parseJSON(readText(bakeRecipePath));
        catch (Exception ex) {
            stderr.writeln("bake recipe unreadable: ", ex.msg);
            return 3;
        }
        WavData wav;
        try wav = readWavFile(args[2]);
        catch (Exception ex) {
            stderr.writeln("WAV unreadable: ", ex.msg);
            return 3;
        }
        CurveSet curves;
        string bakeError;
        if (!bakeEnvelope(wav, bakeRecipe, args[2], curves, bakeError)) {
            JSONValue doc;
            doc["file"] = args[2];
            doc["result"] = "error";
            doc["error"] = bakeError;
            writeln(doc.toString());
            return 3;
        }
        write(bakeOut, curves.toJson() ~ "\n");
        JSONValue doc;
        doc["file"] = args[2];
        doc["curves"] = bakeOut;
        doc["steps"] = curves.steps();
        doc["series"] = curves.series.length;
        writeln(doc.toString());
        return 0;
    }
    if (args.length >= 2 && args[1] == "viseme-check") {
        if (args.length < 3) {
            stderr.writeln("usage: torogao-cli viseme-check " ~
                "<project.inx> [--contract <file>]");
            return 3;
        }
        string contractPath;
        for (size_t i = 3; i < args.length; i++)
            if (args[i] == "--contract" && i + 1 < args.length)
                contractPath = args[++i];
        string cErr;
        auto contract_ = loadVisemeContract(contractPath, cErr);
        if (cErr.length > 0) {
            stderr.writeln(cErr);
            return 3;
        }
        if (!exists(args[2])) {
            stderr.writeln("project file not found: ", args[2]);
            return 2;
        }
        torogaoInitHeadless();
        Puppet puppet;
        try puppet = inLoadPuppet(args[2]);
        catch (Exception ex) {
            JSONValue doc;
            doc["file"] = args[2];
            doc["result"] = "unloadable";
            doc["error"] = ex.msg;
            writeln(doc.toString());
            return 2;
        }
        auto findings = checkVisemes(puppet, contract_);
        JSONValue doc;
        doc["file"] = args[2];
        doc["contract"] = contract_.setName;
        doc["covered"] = coveredParameters(puppet, contract_);
        doc["result"] = findings.length > 0 ? "findings" : "clean";
        JSONValue[] list;
        foreach (f; findings) list ~= f.toJson();
        doc["findings"] = list;
        writeln(doc.toString());
        return findings.length > 0 ? 1 : 0;
    }
    if (args.length >= 2 && args[1] == "viseme-bake") {
        if (args.length < 3) {
            stderr.writeln("usage: torogao-cli viseme-bake " ~
                "<track.json> [--contract <file>] --out <curves.json>");
            return 3;
        }
        string contractPath, bakeOut;
        for (size_t i = 3; i < args.length; i++) {
            if (args[i] == "--contract" && i + 1 < args.length)
                contractPath = args[++i];
            else if (args[i] == "--out" && i + 1 < args.length)
                bakeOut = args[++i];
        }
        if (bakeOut.length == 0) {
            stderr.writeln("viseme-bake requires --out <curves.json>");
            return 3;
        }
        string cErr;
        auto contract_ = loadVisemeContract(contractPath, cErr);
        if (cErr.length > 0) {
            stderr.writeln(cErr);
            return 3;
        }
        JSONValue track;
        try track = parseJSON(readText(args[2]));
        catch (Exception ex) {
            stderr.writeln("track unreadable: ", ex.msg);
            return 3;
        }
        CurveSet curves;
        string bakeError;
        if (!bakeVisemes(track, contract_, curves, bakeError)) {
            JSONValue doc;
            doc["file"] = args[2];
            doc["result"] = "error";
            doc["error"] = bakeError;
            writeln(doc.toString());
            return 3;
        }
        write(bakeOut, curves.toJson() ~ "\n");
        JSONValue doc;
        doc["track"] = args[2];
        doc["curves"] = bakeOut;
        doc["steps"] = curves.steps();
        doc["series"] = curves.series.length;
        writeln(doc.toString());
        return 0;
    }
    if (args.length >= 2 && args[1] == "pack-info") {
        if (args.length < 3) {
            stderr.writeln("usage: torogao-cli pack-info <pack>");
            return 3;
        }
        string packError;
        auto pack = loadPack(args[2], packError);
        if (packError.length > 0) {
            stderr.writeln(packError);
            return 3;
        }
        JSONValue doc;
        doc["pack"] = pack;
        doc["packs"] = packNames();
        writeln(doc.toString());
        return 0;
    }
    if (args.length >= 2 && args[1] == "sfx-list") {
        string sfxError;
        auto index = loadSfxIndex(sfxError);
        if (sfxError.length > 0) {
            stderr.writeln(sfxError);
            return 3;
        }
        writeln(index.toString());
        return 0;
    }
    if (args.length >= 2 && args[1] == "voice-check") {
        if (args.length < 3) {
            stderr.writeln("usage: torogao-cli voice-check <record.json>");
            return 3;
        }
        string loadError;
        auto record = loadJsonFile(args[2], loadError);
        if (loadError.length > 0) {
            stderr.writeln(loadError);
            return 3;
        }
        auto findings = validateVoice(record);
        JSONValue doc;
        doc["file"] = args[2];
        doc["result"] = findings.length == 0 ? "clean" : "findings";
        JSONValue[] list;
        foreach (f; findings) list ~= f.toJson();
        doc["findings"] = list;
        writeln(doc.toString());
        return findings.length == 0 ? 0 : 1;
    }
    if (args.length >= 2 && args[1] == "create-character") {
        string identityPath, ccPack, ccOut;
        string[] ccPresets;
        for (size_t i = 2; i < args.length; i++) {
            if (args[i] == "--identity" && i + 1 < args.length)
                identityPath = args[++i];
            else if (args[i] == "--pack" && i + 1 < args.length)
                ccPack = args[++i];
            else if (args[i] == "--preset" && i + 1 < args.length)
                ccPresets ~= args[++i];
            else if (args[i] == "--out" && i + 1 < args.length)
                ccOut = args[++i];
            else {
                stderr.writeln("create-character: unknown argument: ",
                    args[i]);
                return 3;
            }
        }
        if (identityPath.length == 0 || ccPack.length == 0
                || ccOut.length == 0) {
            stderr.writeln("usage: torogao-cli create-character "
                ~ "--identity <record.json> --pack <name> "
                ~ "[--preset <p>]... --out <file.inx>");
            return 3;
        }
        string packError;
        auto pack = loadPack(ccPack, packError);
        if (packError.length > 0) {
            stderr.writeln(packError);
            return 3;
        }
        string loadError;
        auto identity = loadJsonFile(identityPath, loadError);
        if (loadError.length > 0) {
            stderr.writeln(loadError);
            return 3;
        }
        CreatorProblem[] problems;
        string createError;
        auto summary = createCharacter(identity, pack, ccPresets, ccOut,
            problems, createError);
        JSONValue doc;
        if (problems.length > 0) {
            doc["result"] = "refused";
            JSONValue[] list;
            foreach (p; problems) list ~= p.toJson();
            doc["problems"] = list;
            writeln(doc.toString());
            return 1;
        }
        if (createError.length > 0) {
            doc["result"] = "error";
            doc["error"] = createError;
            writeln(doc.toString());
            return 3;
        }
        writeln(summary.toString());
        return 0;
    }
    if (args.length >= 2 && args[1] == "writer-scaffold") {
        string charPath, wPack, wKink, wOut;
        int wStage = -1;
        string[] wSfx;
        for (size_t i = 2; i < args.length; i++) {
            if (args[i] == "--character" && i + 1 < args.length)
                charPath = args[++i];
            else if (args[i] == "--pack" && i + 1 < args.length)
                wPack = args[++i];
            else if (args[i] == "--kink" && i + 1 < args.length)
                wKink = args[++i];
            else if (args[i] == "--sfx" && i + 1 < args.length)
                wSfx ~= args[++i];
            else if (args[i] == "--out" && i + 1 < args.length)
                wOut = args[++i];
            else if (args[i] == "--stage" && i + 1 < args.length) {
                try wStage = to!int(args[++i]);
                catch (ConvException) {
                    stderr.writeln("--stage needs a number");
                    return 3;
                }
            } else {
                stderr.writeln("writer-scaffold: unknown argument: ",
                    args[i]);
                return 3;
            }
        }
        if (charPath.length == 0 || wPack.length == 0
                || wKink.length == 0 || wStage < 0) {
            stderr.writeln("usage: torogao-cli writer-scaffold "
                ~ "--character <record.json> --pack <name> --kink <k> "
                ~ "--stage <n> [--sfx <id>]... [--out <file.json>]");
            return 3;
        }
        string packError;
        auto pack = loadPack(wPack, packError);
        if (packError.length > 0) {
            stderr.writeln(packError);
            return 3;
        }
        string loadError;
        auto character = loadJsonFile(charPath, loadError);
        if (loadError.length > 0) {
            stderr.writeln(loadError);
            return 3;
        }
        JSONValue scaffold, gap;
        buildScaffold(character, pack, wKink, wStage, wSfx, scaffold, gap);
        JSONValue doc = gap.type == JSONType.object ? gap : scaffold;
        if (wOut.length > 0) write(wOut, doc.toString() ~ "\n");
        writeln(doc.toString());
        return gap.type == JSONType.object ? 1 : 0;
    }
    if (args.length >= 2 && args[1] == "hitareas-author") {
        if (args.length < 3) {
            stderr.writeln("usage: torogao-cli hitareas-author "
                ~ "<project.inx> --spec <areas.json> --out <new.inx>");
            return 3;
        }
        string specPath, haOut;
        for (size_t i = 3; i < args.length; i++) {
            if (args[i] == "--spec" && i + 1 < args.length)
                specPath = args[++i];
            else if (args[i] == "--out" && i + 1 < args.length)
                haOut = args[++i];
            else {
                stderr.writeln("hitareas-author: unknown argument: ",
                    args[i]);
                return 3;
            }
        }
        if (specPath.length == 0 || haOut.length == 0) {
            stderr.writeln("hitareas-author requires --spec and --out");
            return 3;
        }
        if (!exists(args[2])) {
            stderr.writeln("project file not found: ", args[2]);
            return 2;
        }
        string loadError;
        auto spec = loadJsonFile(specPath, loadError);
        if (loadError.length > 0) {
            stderr.writeln(loadError);
            return 3;
        }
        torogaoInitHeadless();
        Puppet haPuppet;
        try haPuppet = inLoadPuppet(args[2]);
        catch (Exception ex) {
            stderr.writeln("project unloadable: ", ex.msg);
            return 2;
        }
        auto haWalk = new PuppetWalk(haPuppet);
        auto view = extOf(haPuppet);
        JSONValue ext = view.present ? view.doc : JSONValue.init;
        if (ext.type != JSONType.object) {
            ext["format"] = torogaoExtFormat;
        }
        JSONValue consumer = jget(ext, "consumer");
        if (consumer.type != JSONType.object) consumer = JSONValue.init;
        auto areas = jget(spec, "hitAreas");
        if (areas.type != JSONType.array) areas = spec;
        if (areas.type != JSONType.array) {
            stderr.writeln("hit-area spec must be an array or an object "
                ~ "with a hitAreas array");
            return 3;
        }
        consumer["hitAreas"] = areas;
        ext["consumer"] = consumer;
        // Validate before writing: an unknown part or a bad
        // shape refuses the write with the typed findings.
        JSONValue tmpExt = ext;
        auto probe = validateHitAreasOnly(tmpExt, haPuppet, haWalk);
        if (probe.length > 0) {
            JSONValue doc;
            doc["result"] = "refused";
            JSONValue[] list;
            foreach (f; probe) list ~= f.toJson();
            doc["findings"] = list;
            writeln(doc.toString());
            return 1;
        }
        string writeError;
        if (!writeProjectWithExt(args[2], haOut, ext, writeError)) {
            stderr.writeln(writeError);
            return 3;
        }
        JSONValue doc;
        doc["result"] = "authored";
        doc["out"] = haOut;
        doc["hitAreas"] = areas.array.length;
        writeln(doc.toString());
        return 0;
    }
    if (args.length < 3) {
        stderr.writeln("usage: torogao-cli <inspect|validate|export-rig|physics-preview|list-templates|instantiate-template|mcp|a2a|registry-dump> ...");
        return 3;
    }
    string command = args[1];
    string projectPath = args[2];
    string outPath;
    string trackPath;
    string stateName;
    string variantId;
    string driversPath;
    string packName;
    string setId;
    int fpsOverride = 0;
    for (size_t i = 3; i < args.length; i++) {
        if (args[i] == "--out" && i + 1 < args.length) {
            outPath = args[++i];
        } else if (args[i] == "--track" && i + 1 < args.length) {
            trackPath = args[++i];
        } else if (args[i] == "--state" && i + 1 < args.length) {
            stateName = args[++i];
        } else if (args[i] == "--variant" && i + 1 < args.length) {
            variantId = args[++i];
        } else if (args[i] == "--drivers" && i + 1 < args.length) {
            driversPath = args[++i];
        } else if (args[i] == "--pack" && i + 1 < args.length) {
            packName = args[++i];
        } else if (args[i] == "--set" && i + 1 < args.length) {
            setId = args[++i];
        } else if (args[i] == "--fps" && i + 1 < args.length) {
            try {
                fpsOverride = to!int(args[++i]);
            } catch (ConvException) {
                stderr.writeln("--fps needs a number");
                return 3;
            }
        }
    }
    if (command != "inspect" && command != "validate" &&
        command != "export-rig" && command != "physics-preview" &&
        command != "list-states" && command != "rating-check" &&
        command != "expression-check" && command != "tracking-check" &&
        command != "hitareas-list") {
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
    if (command == "physics-preview" && trackPath.length == 0) {
        stderr.writeln("physics-preview requires --track <track.json>");
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
            auto extView = extOf(puppet);
            auto summary = extSummary(extView.doc);
            summary["glueRecords"] = countGlueKeys(payload)
                + (extView.present
                    ? countGlueKeys(jget(extView.doc, "foreign")) : 0);
            doc["extension"] = summary;
            emit(doc, outPath);
            return 0;
        }
        case "validate": {
            auto findings = validatePuppet(puppet, walk, payload, slotCount)
                ~ validateExt(puppet, walk, payload);
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
            auto extView = extOf(puppet);
            string stampReason;
            if (extView.present
                    && ratingBlocksExport(extView.doc, stampReason)) {
                JSONValue doc;
                doc["file"] = projectPath;
                doc["result"] = "refused";
                doc["code"] = "TG-RATING-LANE-MISMATCH";
                doc["error"] = stampReason;
                writeln(doc.toString());
                return 4;
            }
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
            string stampPath = buildPath(outPath, base ~ ".stamp.json");
            write(modelPath, result.model.toString() ~ "\n");
            auto stamp = ratingStamp(extView.doc);
            JSONValue stampDoc;
            stampDoc["format"] = "torogao-rating-stamp/1";
            stampDoc["file"] = projectPath;
            stampDoc["model"] = modelPath;
            stampDoc["stamp"] = stamp;
            write(stampPath, stampDoc.toString() ~ "\n");
            JSONValue report;
            report["file"] = projectPath;
            report["model"] = modelPath;
            report["ratingStamp"] = stamp;
            report["stampFile"] = stampPath;
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
        case "list-states": {
            auto extView = extOf(puppet);
            JSONValue doc;
            doc["file"] = projectPath;
            doc["extension"] = extSummary(extView.doc);
            emit(doc, outPath);
            return 0;
        }
        case "rating-check": {
            auto extView = extOf(puppet);
            JSONValue doc;
            doc["file"] = projectPath;
            doc["stamp"] = ratingStamp(extView.doc);
            string reason;
            bool blocked = extView.present
                && ratingBlocksExport(extView.doc, reason);
            doc["result"] = blocked ? "mismatch" : "ok";
            if (blocked) {
                doc["code"] = "TG-RATING-LANE-MISMATCH";
                doc["error"] = reason;
            }
            emit(doc, outPath);
            return blocked ? 1 : 0;
        }
        case "expression-check": {
            if (packName.length == 0) {
                stderr.writeln("expression-check requires --pack <name>");
                return 3;
            }
            string packError;
            auto pack = loadPack(packName, packError);
            if (packError.length > 0) {
                stderr.writeln(packError);
                return 3;
            }
            auto extView = extOf(puppet);
            auto missing = expressionMissing(extView.doc, pack);
            JSONValue doc;
            doc["file"] = projectPath;
            doc["pack"] = packName;
            doc["missing"] = missing;
            doc["result"] = missing.length == 0 ? "covered" : "findings";
            if (missing.length > 0) doc["code"] = TG_EXPR_MISSING;
            emit(doc, outPath);
            return missing.length == 0 ? 0 : 1;
        }
        case "tracking-check": {
            if (packName.length == 0 || setId.length == 0) {
                stderr.writeln("tracking-check requires --pack <name> "
                    ~ "--set <id>");
                return 3;
            }
            string packError;
            auto pack = loadPack(packName, packError);
            if (packError.length > 0) {
                stderr.writeln(packError);
                return 3;
            }
            auto set = packTrackingSet(pack, setId);
            if (set.type != JSONType.object) {
                stderr.writeln("pack '" ~ packName ~ "' has no tracking "
                    ~ "set '" ~ setId ~ "'");
                return 3;
            }
            string[] missing, rangeBad;
            trackingCoverage(puppet, set, missing, rangeBad);
            JSONValue doc;
            doc["file"] = projectPath;
            doc["pack"] = packName;
            doc["set"] = setId;
            doc["completeness"] = packStr(set, "completeness");
            doc["missing"] = missing;
            doc["rangeViolations"] = rangeBad;
            bool ok = missing.length == 0 && rangeBad.length == 0;
            doc["result"] = ok ? "covered" : "findings";
            if (missing.length > 0) doc["code"] = TG_TRACK_MISSING;
            else if (rangeBad.length > 0) doc["code"] = TG_TRACK_RANGE;
            emit(doc, outPath);
            return ok ? 0 : 1;
        }
        case "hitareas-list": {
            auto extView = extOf(puppet);
            JSONValue doc;
            doc["file"] = projectPath;
            doc["hitAreas"] = extView.present
                ? jget(jget(extView.doc, "consumer"), "hitAreas")
                : JSONValue(null);
            emit(doc, outPath);
            return 0;
        }
        case "physics-preview": {
            JSONValue track;
            try {
                track = parseJSON(readText(trackPath));
            } catch (Exception ex) {
                stderr.writeln("input track unreadable: ", ex.msg);
                return 3;
            }
            PreviewOptions options;
            options.ext = extOf(puppet).doc;
            options.stateName = stateName;
            options.variantId = variantId;
            if (driversPath.length > 0) {
                JSONValue driverDoc;
                try driverDoc = parseJSON(readText(driversPath));
                catch (Exception ex) {
                    stderr.writeln("driver set unreadable: ", ex.msg);
                    return 3;
                }
                string driverError;
                if (!parseDriverConfig(driverDoc, options.drivers,
                        driverError)) {
                    stderr.writeln(driverError);
                    return 3;
                }
                options.hasDrivers = true;
            }
            string previewError;
            auto curves = runPhysicsPreview(puppet, track, fpsOverride,
                projectPath, options, previewError);
            if (previewError !is null) {
                JSONValue doc;
                doc["file"] = projectPath;
                doc["result"] = "error";
                doc["error"] = previewError;
                writeln(doc.toString());
                return 3;
            }
            string text = curves.toJson();
            if (outPath.length > 0) {
                write(outPath, text ~ "\n");
                JSONValue doc;
                doc["file"] = projectPath;
                doc["curves"] = outPath;
                doc["steps"] = curves.steps();
                doc["series"] = curves.series.length;
                doc["sha256"] = format("%(%02x%)",
                    sha256Of(cast(ubyte[]) (text ~ "\n")));
                writeln(doc.toString());
            } else {
                writeln(text);
            }
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
