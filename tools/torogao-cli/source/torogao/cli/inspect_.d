/*
    Torogao headless CLI — project census (inspect)

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.
*/
module torogao.cli.inspect_;

import inochi2d.core;
import inochi2d.integration : inCurrentPuppetTextureSlots, TextureBlob;
import torogao.cli.puppetwalk;
import std.json : JSONValue;
import std.format : format;

private JSONValue jv2(float x, float y) {
    JSONValue[] pair = [JSONValue(cast(double)x), JSONValue(cast(double)y)];
    return JSONValue(pair);
}

private string textureFormatName(ubyte tag) {
    switch (tag) {
        case 0: return "png";
        case 1: return "tga";
        case 2: return "bc7";
        default: return "unknown";
    }
}

/// Builds the structured census for a loaded puppet.
JSONValue census(Puppet puppet, PuppetWalk walk) {
    JSONValue root;
    JSONValue meta;
    meta["name"] = puppet.meta.name;
    meta["version"] = puppet.meta.version_;
    meta["artist"] = puppet.meta.artist;
    meta["rigger"] = puppet.meta.rigger;
    root["meta"] = meta;

    // -- nodes ------------------------------------------------------
    JSONValue[] nodeList;
    size_t[string] byType;
    size_t partCount, drawableCount, deformerCount, driverCount;
    foreach(w; walk.order) {
        byType[w.typeId] = byType.get(w.typeId, 0) + 1;
        if (cast(Part)w.node) partCount++;
        if (cast(Drawable)w.node) drawableCount++;
        if (cast(MeshGroup)w.node) deformerCount++;
        if (cast(Driver)w.node) driverCount++;
        JSONValue n;
        n["uuid"] = w.uuid;
        n["name"] = w.name;
        n["type"] = w.typeId;
        n["parent"] = w.parent is null ? JSONValue(null)
                                       : JSONValue(w.parent.uuid);
        n["children"] = w.children.length;
        n["zsort"] = cast(double)w.node.relZSort;
        n["enabled"] = w.node.enabled;
        nodeList ~= n;
    }
    JSONValue nodes;
    nodes["total"] = walk.order.length;
    JSONValue typeCounts;
    foreach(type, count; byType) typeCounts[type] = count;
    nodes["by_type"] = typeCounts;
    nodes["list"] = nodeList;
    root["nodes"] = nodes;

    // -- parameters ---------------------------------------------------
    JSONValue[] paramList;
    size_t bindingTotal;
    foreach(param; puppet.parameters) {
        JSONValue p;
        p["uuid"] = param.uuid;
        p["name"] = param.name;
        p["is_vec2"] = param.isVec2;
        p["min"] = jv2(param.min.x, param.min.y);
        p["max"] = jv2(param.max.x, param.max.y);
        p["defaults"] = jv2(param.defaults.x, param.defaults.y);
        JSONValue[] axisCounts = [JSONValue(param.axisPointCount(0)),
                                  JSONValue(param.axisPointCount(1))];
        p["axis_points"] = axisCounts;
        JSONValue[] bindings;
        foreach(binding; param.bindings) {
            bindingTotal++;
            JSONValue b;
            b["node"] = binding.getNodeUUID();
            Node target = binding.getNode();
            b["node_name"] = target is null ? JSONValue(null)
                                            : JSONValue(target.name);
            b["param_name"] = binding.getName();
            b["kind"] = cast(DeformationParameterBinding)binding
                ? "deformation" : "value";
            b["set_count"] = binding.getSetCount();
            bindings ~= b;
        }
        p["bindings"] = bindings;
        paramList ~= p;
    }
    JSONValue params;
    params["total"] = puppet.parameters.length;
    params["bindings_total"] = bindingTotal;
    params["list"] = paramList;
    root["parameters"] = params;

    // -- textures -------------------------------------------------------
    JSONValue[] slotList;
    size_t totalBytes;
    foreach(i, blob; inCurrentPuppetTextureSlots) {
        totalBytes += blob.data.length;
        JSONValue s;
        s["index"] = i;
        s["format"] = textureFormatName(blob.tag);
        s["bytes"] = blob.data.length;
        slotList ~= s;
    }
    JSONValue textures;
    textures["slots"] = slotList.length;
    textures["total_bytes"] = totalBytes;
    textures["list"] = slotList;
    root["textures"] = textures;

    // -- drivers / physics ----------------------------------------------
    JSONValue[] driverList;
    foreach(driver; puppet.getDrivers()) {
        JSONValue d;
        d["node"] = driver.name;
        d["type"] = driver.typeId;
        if (auto phys = cast(SimplePhysics)driver) {
            Parameter driven = phys.param;
            d["drives"] = driven is null ? JSONValue(null)
                                         : JSONValue(driven.name);
            d["model"] = format!"%s"(phys.modelType_);
            d["map_mode"] = format!"%s"(phys.mapMode);
        }
        driverList ~= d;
    }
    JSONValue drivers;
    drivers["total"] = driverList.length;
    drivers["list"] = driverList;
    root["drivers"] = drivers;

    // -- animations -------------------------------------------------------
    JSONValue[] animList;
    foreach(name, ref anim; puppet.getAnimations()) {
        size_t keyframes;
        foreach(ref lane; anim.lanes) keyframes += lane.frames.length;
        JSONValue a;
        a["name"] = name;
        a["lanes"] = anim.lanes.length;
        a["keyframes"] = keyframes;
        a["length"] = anim.length;
        a["timestep"] = cast(double)anim.timestep;
        a["additive"] = anim.additive;
        animList ~= a;
    }
    JSONValue animations;
    animations["total"] = animList.length;
    animations["list"] = animList;
    root["animations"] = animations;

    root["automation"] = puppet.automation.length;
    JSONValue[] extNames;
    foreach(name, _; puppet.extData) extNames ~= JSONValue(name);
    root["ext_sections"] = extNames;

    JSONValue counts;
    counts["parts"] = partCount;
    counts["drawables"] = drawableCount;
    counts["meshgroup_deformers"] = deformerCount;
    root["counts"] = counts;

    return root;
}
