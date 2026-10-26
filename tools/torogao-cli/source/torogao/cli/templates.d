/*
    Torogao rig templates: listing and instantiation

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    Templates are data (format torogao-template/1) in the
    repository's templates/ directory. A "puppet" template is
    a payload-shaped skeleton (meta / physics / parameters /
    node tree, in the payload's own JSON shapes — no second
    binding language). A "physics-chain" preset attaches a
    nested pendulum chain to a named anchor node of the base
    puppet and adds the parameters the chain drives.

    Instantiation is a documented conversion (templates/
    README.md): each preset stage becomes a segment part, a
    SimplePhysics node and a rotation node nested exactly
    like a hand-authored chain; the preset's fields map to
    SimplePhysics fields as angleDamping = 1 - easeOfSwinging
    and outputScale = scale / 90. Symbolic ids in the
    template become fresh uuids from a deterministic counter,
    and every reference (physics param uuid, binding node
    uuid) is remapped through the same allocation, so
    instantiating the same inputs twice yields byte-identical
    output. The result is a standard INP file (one texture
    slot holding a 1x1 placeholder PNG) that the normal
    validate/export gates consume.
*/
module torogao.cli.templates;

import std.algorithm : canFind, map, sort;
import std.array : array;
import std.base64 : Base64;
import std.conv : to;
import std.file : dirEntries, readText, SpanMode, write;
import std.json : JSONType, JSONValue, parseJSON;
import std.path : extension;

import torogao.cli.datadirs : existingDataDirs, findDataFile;

/// One listed template.
struct TemplateInfo {
    string name;
    string kind;
    string title;
    string description;
    string path;
}

/// List all templates visible in the search directories
/// (first occurrence of a name wins).
TemplateInfo[] listTemplates() {
    TemplateInfo[] result;
    bool[string] seen;
    foreach (dir; existingDataDirs("TOROGAO_TEMPLATES_DIR", "templates")) {
        string[] files;
        foreach (entry; dirEntries(dir, SpanMode.shallow))
            if (entry.isFile && extension(entry.name) == ".json")
                files ~= entry.name;
        files.sort;
        foreach (file; files) {
            JSONValue doc;
            try doc = parseJSON(readText(file));
            catch (Exception) continue;
            if (doc.type != JSONType.object) continue;
            if (doc.getStr("format") != "torogao-template/1") continue;
            string name = doc.getStr("name");
            if (name.length == 0 || (name in seen)) continue;
            seen[name] = true;
            result ~= TemplateInfo(name, doc.getStr("kind"),
                doc.getStr("title"), doc.getStr("description"), file);
        }
    }
    return result;
}

private JSONValue loadTemplate(string name, out string error) {
    string path = findDataFile("TOROGAO_TEMPLATES_DIR", "templates",
        name ~ ".json");
    if (path.length == 0) {
        error = "template '" ~ name ~ "' not found";
        return JSONValue.init;
    }
    try return parseJSON(readText(path));
    catch (Exception e) {
        error = "template '" ~ name ~ "' is unreadable: " ~ e.msg;
        return JSONValue.init;
    }
}

private string getStr(JSONValue v, string key) {
    if (v.type != JSONType.object) return "";
    auto p = key in v;
    if (p is null || p.type != JSONType.string) return "";
    return p.str;
}

private double getNum(JSONValue v, string key, double fallback = 0) {
    if (v.type != JSONType.object) return fallback;
    auto p = key in v;
    if (p is null) return fallback;
    if (p.type == JSONType.integer) return cast(double) p.integer;
    if (p.type == JSONType.uinteger) return cast(double) p.uinteger;
    if (p.type == JSONType.float_) return p.floating;
    return fallback;
}

private double chosenOf(JSONValue setting) {
    return getNum(setting, "chosen");
}

/// Depth-first search for a node by name; on success path
/// holds the child indices from the tree root (empty when
/// the root itself matches).
private bool findNodePath(JSONValue node, string name, out size_t[] path) {
    if (node.getStr("name") == name) {
        path = [];
        return true;
    }
    if ("children" in node) {
        auto kids = node["children"].array;
        foreach (i, kid; kids) {
            size_t[] sub;
            if (findNodePath(kid, name, sub)) {
                path = [i] ~ sub;
                return true;
            }
        }
    }
    return false;
}

private JSONValue* navigate(JSONValue* node, size_t[] path) {
    foreach (idx; path)
        node = &((*node)["children"].array[idx]);
    return node;
}

private bool hasParam(JSONValue payload, string name) {
    foreach (p; payload["param"].array)
        if (p.getStr("name") == name) return true;
    return false;
}

private size_t countNodes(JSONValue node) {
    size_t n = 1;
    if ("children" in node)
        foreach (kid; node["children"].array) n += countNodes(kid);
    return n;
}

private JSONValue jsonTransform(double x, double y) {
    JSONValue t;
    t["trans"] = [JSONValue(x), JSONValue(y), JSONValue(0.0)];
    t["rot"] = [JSONValue(0.0), JSONValue(0.0), JSONValue(0.0)];
    t["scale"] = [JSONValue(1.0), JSONValue(1.0)];
    return t;
}

private JSONValue jsonNode(long uuid, string name, string type,
                           double x, double y) {
    JSONValue n;
    n["type"] = type;
    n["uuid"] = uuid;
    n["name"] = name;
    n["enabled"] = true;
    n["zsort"] = 0;
    n["transform"] = jsonTransform(x, y);
    n["lockToRoot"] = false;
    return n;
}

private JSONValue jsonPart(long uuid, string name, double x, double y,
                           double halfW, double halfH) {
    JSONValue n = jsonNode(uuid, name, "Part", x, y);
    JSONValue mesh;
    mesh["verts"] = [-halfW, -halfH, halfW, -halfH,
                     -halfW, halfH, halfW, halfH].map!(JSONValue).array;
    mesh["uvs"] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0]
        .map!(JSONValue).array;
    mesh["indices"] = [0, 1, 2, 2, 1, 3];
    mesh["origin"] = [0.0, 0.0];
    n["mesh"] = mesh;
    n["textures"] = [JSONValue(0), JSONValue(4294967295),
                     JSONValue(4294967295)];
    n["blend_mode"] = "Normal";
    n["opacity"] = 1.0;
    n["mask_threshold"] = 0.5;
    n["tint"] = [1.0, 1.0, 1.0];
    n["screenTint"] = [0.0, 0.0, 0.0];
    n["emissionStrength"] = 1.0;
    return n;
}

private JSONValue jsonPhysics(long uuid, string name, long paramUuid,
                              double length, double angleDamping,
                              double outputScale) {
    JSONValue n = jsonNode(uuid, name, "SimplePhysics", 0, 0);
    n["param"] = paramUuid;
    n["model_type"] = "Pendulum";
    n["map_mode"] = "AngleLength";
    n["gravity"] = 1.0;
    n["length"] = length;
    n["frequency"] = 1.0;
    n["angle_damping"] = angleDamping;
    n["length_damping"] = 0.5;
    n["output_scale"] = [outputScale, outputScale];
    n["local_only"] = false;
    return n;
}

private JSONValue rotationBinding(long nodeUuid) {
    JSONValue b;
    b["node"] = nodeUuid;
    b["param_name"] = "transform.r.z";
    b["values"] = [[JSONValue(-0.6)], [JSONValue(0.0)], [JSONValue(0.6)]];
    b["isSet"] = [[true], [true], [true]];
    return b;
}

private JSONValue drivenParam(long uuid, string name, long nodeUuid) {
    JSONValue p;
    p["uuid"] = uuid;
    p["name"] = name;
    p["is_vec2"] = false;
    p["min"] = [-1.0, 0.0];
    p["max"] = [1.0, 0.0];
    p["defaults"] = [0.0, 0.0];
    p["axis_points"] = [[JSONValue(-1.0), JSONValue(0.0), JSONValue(1.0)],
                        [JSONValue(0.0)]];
    p["bindings"] = [rotationBinding(nodeUuid)];
    return p;
}

/// The 1x1 transparent placeholder PNG (base64).
private immutable placeholderPngB64 =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk" ~
    "YPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

private void putBE(ref ubyte[] out_, uint v) {
    out_ ~= cast(ubyte)(v >> 24);
    out_ ~= cast(ubyte)(v >> 16);
    out_ ~= cast(ubyte)(v >> 8);
    out_ ~= cast(ubyte) v;
}

/// Pack a payload JSON and texture slots into INP bytes.
ubyte[] packInp(JSONValue payload, in ubyte[][] textures) {
    ubyte[] out_;
    auto body_ = cast(ubyte[]) payload.toString();
    out_ ~= cast(ubyte[]) "TRNSRTS\0";
    putBE(out_, cast(uint) body_.length);
    out_ ~= body_;
    out_ ~= cast(ubyte[]) "TEX_SECT";
    putBE(out_, cast(uint) textures.length);
    foreach (tex; textures) {
        putBE(out_, cast(uint) tex.length);
        out_ ~= cast(ubyte) 0;
        out_ ~= tex;
    }
    return out_;
}

/// Instantiate a puppet template plus physics-chain presets
/// into an INP file at outPath. Returns a summary document;
/// on failure error is set and the summary is null.
JSONValue instantiateTemplate(string templateName, string[] presetNames,
                              string outPath, out string error) {
    auto base = loadTemplate(templateName, error);
    if (error.length > 0) return JSONValue.init;
    if (base.getStr("kind") != "puppet") {
        error = "template '" ~ templateName ~ "' is not a puppet template";
        return JSONValue.init;
    }

    JSONValue payload;
    payload["meta"] = base["meta"];
    payload["physics"] = base["physics"];
    payload["param"] = base["parameters"];
    JSONValue tree = base["nodes"];  // mutated per preset, assigned at the end

    long nextUuid = 9001;
    long alloc() { return nextUuid++; }

    string[] applied;
    foreach (presetName; presetNames) {
        if (canFind(applied, presetName)) {
            error = "preset '" ~ presetName ~ "' applied twice";
            return JSONValue.init;
        }
        auto preset = loadTemplate(presetName, error);
        if (error.length > 0) return JSONValue.init;
        if (preset.getStr("kind") != "physics-chain") {
            error = "template '" ~ presetName ~
                "' is not a physics-chain preset";
            return JSONValue.init;
        }
        string attachName = preset.getStr("attachTo");
        size_t[] attachPath;
        if (!findNodePath(tree, attachName, attachPath)) {
            error = "preset '" ~ presetName ~ "': anchor node '" ~
                attachName ~ "' not found in template '" ~
                templateName ~ "'";
            return JSONValue.init;
        }
        foreach (inp; preset["inputs"].array) {
            string pname = inp.getStr("parameter");
            if (!hasParam(payload, pname)) {
                error = "preset '" ~ presetName ~ "': input parameter '" ~
                    pname ~ "' is not present in template '" ~
                    templateName ~ "'";
                return JSONValue.init;
            }
        }
        auto stages = preset["stages"].array;
        auto outputs = preset["outputs"].array;
        if (stages.length == 0 || outputs.length != stages.length) {
            error = "preset '" ~ presetName ~
                "': outputs must cover every stage exactly once";
            return JSONValue.init;
        }

        // Allocate the chain: per stage a segment part, a
        // physics node, and (except after the last stage) a
        // rotation node and a segment anchor below it.
        struct StageIds { long part; long phys; long rot; long anchor; }
        StageIds[] ids;
        foreach (i, stage; stages) {
            StageIds id;
            id.part = alloc();
            id.phys = alloc();
            if (i + 1 < stages.length) {
                id.rot = alloc();
                id.anchor = alloc();
            }
            ids ~= id;
        }
        long[] paramUuids;
        foreach (o; outputs) {
            string pname = o.getStr("parameter");
            if (hasParam(payload, pname)) {
                error = "parameter '" ~ pname ~ "' from preset '" ~
                    presetName ~ "' already exists in the puppet";
                return JSONValue.init;
            }
            paramUuids ~= alloc();
        }

        // Build the fragment bottom-up: the tail stage first,
        // wrapped by the previous stage's rotation/anchor.
        JSONValue* cursor = null;  // node whose children get the next wrap
        JSONValue tailHolder;
        foreach_reverse (i, stage; stages) {
            double segLen = getNum(stage, "segmentLength", 30);
            // Find this stage's output.
            JSONValue outp;
            bool found = false;
            foreach (o; outputs)
                if (cast(size_t) getNum(o, "stage", -1) == i + 1) {
                    outp = o; found = true;
                }
            if (!found) {
                error = "preset '" ~ presetName ~ "': no output for " ~
                    "stage " ~ to!string(i + 1);
                return JSONValue.init;
            }
            double scale = getNum(outp["scale"], "value", 45);
            double ease = chosenOf(stage["easeOfSwinging"]);
            auto part = jsonPart(ids[i].part,
                presetName ~ "Seg" ~ to!string(i + 1),
                0, segLen / 2, 18, segLen / 2);
            auto phys = jsonPhysics(ids[i].phys,
                presetName ~ "Phys" ~ to!string(i + 1),
                paramUuids[i], segLen, 1.0 - ease, scale / 90.0);

            JSONValue[] kids;
            kids ~= part;
            kids ~= phys;
            if (cursor !is null) {
                auto rot = jsonNode(ids[i].rot,
                    presetName ~ "Rot" ~ to!string(i + 1),
                    "Node", 0, 0);
                auto anchor = jsonNode(ids[i].anchor,
                    presetName ~ "SegAnchor" ~ to!string(i + 2),
                    "Node", 0, segLen);
                anchor["children"] = (*cursor)["children"];
                rot["children"] = [anchor];
                kids ~= rot;
            }
            tailHolder = JSONValue.init;
            tailHolder["children"] = kids;
            cursor = &tailHolder;
        }
        auto attach = navigate(&tree, attachPath);
        JSONValue[] attachKids;
        if ("children" in *attach)
            attachKids = (*attach)["children"].array;
        attachKids ~= (*cursor)["children"].array;
        (*attach)["children"] = attachKids;

        // Driven parameters bind their stage's rotation node,
        // or the tip segment part for the last stage.
        auto paramsArr = payload["param"].array;
        foreach (i, o; outputs) {
            long target = (i + 1 < stages.length)
                ? ids[i].rot : ids[i].part;
            paramsArr ~= drivenParam(paramUuids[i],
                o.getStr("parameter"), target);
        }
        payload["param"] = paramsArr;
        applied ~= presetName;
    }

    payload["nodes"] = tree;
    auto png = Base64.decode(placeholderPngB64);
    write(outPath, packInp(payload, [png]));

    JSONValue summary;
    summary["template"] = templateName;
    summary["presets"] = applied;
    summary["out"] = outPath;
    summary["parameters"] = payload["param"].array.length;
    summary["nodes"] = countNodes(tree);
    return summary;
}
