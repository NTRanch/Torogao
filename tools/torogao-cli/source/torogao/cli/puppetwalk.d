/*
    Torogao headless CLI — puppet traversal and id assignment

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.
*/
module torogao.cli.puppetwalk;

import inochi2d.core;
import inochi2d.math;
import std.format : format;

/**
    One node of the walked puppet tree, with the lumen-rig ids the
    exporter assigns to it. Id assignment is a pure function of the
    node (name slug + INP uuid), so every Torogao tool derives the
    same ids for the same project.
*/
final class WNode {
    Node node;
    WNode parent;
    WNode[] children;

    /// Lumen part id for this node ("root" for the root node).
    string partId;
    /// Lumen mesh id, set when this node exports a mesh (Parts).
    string meshId;
    /// Lumen warp deformer id, set for grid MeshGroups.
    string warpId;
    /// Lumen rotation deformer id, set when parameter bindings
    /// rotate this node (assigned by the exporter).
    string rotId;

    /// World matrix at rest state (no parameter offsets applied).
    mat4 world;

    string name() { return node.name; }
    uint uuid() { return node.uuid; }
    string typeId() { return node.typeId; }

    /// World-space position of this node's origin, lumen orientation
    /// (y flipped relative to INP space).
    float[2] worldPos() {
        vec4 p = world * vec4(0, 0, 0, 1);
        return [p.x, -p.y];
    }

    /// A rest-state local point carried to lumen world space:
    /// INP local point (relative to the mesh origin) in, lumen
    /// [x, y] out.
    float[2] toWorld(vec2 local, vec2 origin) {
        vec4 p = world * vec4(local.x - origin.x, local.y - origin.y, 0, 1);
        return [p.x, -p.y];
    }

    /// A local-space direction/delta carried to lumen world space
    /// (linear part only, y flipped).
    float[2] deltaToWorld(vec2 delta) {
        vec4 p = world * vec4(delta.x, delta.y, 0, 0);
        return [p.x, -p.y];
    }
}

/**
    Reduces a display name to the lumen id alphabet
    ([A-Za-z0-9_-]); every other byte becomes '_'. Deterministic.
*/
string slugify(string name, size_t maxLen) {
    string out_;
    foreach(char c; name) {
        if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
            (c >= '0' && c <= '9') || c == '_' || c == '-')
            out_ ~= c;
        else
            out_ ~= '_';
    }
    // Collapse runs of '_' produced by replaced characters.
    string collapsed;
    bool lastUnderscore = true; // also trims a leading run
    foreach(char c; out_) {
        if (c == '_') {
            if (!lastUnderscore) collapsed ~= c;
            lastUnderscore = true;
        } else {
            collapsed ~= c;
            lastUnderscore = false;
        }
    }
    while (collapsed.length > 0 && collapsed[$-1] == '_')
        collapsed = collapsed[0..$-1];
    if (collapsed.length == 0) collapsed = "node";
    if (collapsed.length > maxLen) collapsed = collapsed[0..maxLen];
    return collapsed;
}

string suffixed(string prefix, string name, uint uuid, size_t slugLen) {
    return format!"%s%s_%u"(prefix, slugify(name, slugLen), uuid);
}

/**
    Determines whether a mesh is a regular grid, and its cell
    dimensions. The grid is derived from the vertex lattice itself
    (row-major, x fastest), NOT from MeshData.gridAxes: the
    library's own quad-mesh generator writes a malformed axis
    array, so the axes cannot be trusted. Vertices must match the
    lattice exactly.
*/
bool gridDims(MeshData mesh, out size_t cols, out size_t rows) {
    import std.algorithm : sort, uniq;
    import std.array : array;
    if (mesh.vertices.length < 4) return false;
    float[] xs, ys;
    foreach(v; mesh.vertices) { xs ~= v.x; ys ~= v.y; }
    xs = xs.sort.uniq.array;
    ys = ys.sort.uniq.array;
    if (xs.length < 2 || ys.length < 2) return false;
    if (mesh.vertices.length != xs.length * ys.length) return false;
    foreach(k, v; mesh.vertices) {
        if (v.x != xs[k % xs.length] || v.y != ys[k / xs.length])
            return false;
    }
    cols = xs.length - 1;
    rows = ys.length - 1;
    return true;
}

/**
    Walks a loaded puppet once, assigning ids and capturing rest
    world matrices. Shared by inspect, validate, and export-rig so
    the three subcommands can never disagree about a project's
    structure.
*/
final class PuppetWalk {
    Puppet puppet;
    WNode root;
    /// Pre-order traversal, root first.
    WNode[] order;
    WNode[uint] byUuid;

    this(Puppet puppet) {
        this.puppet = puppet;
        root = build(puppet.root, null, true);
    }

    private WNode build(Node node, WNode parent, bool isRoot = false) {
        auto w = new WNode;
        w.node = node;
        w.parent = parent;
        w.partId = isRoot ? "root" : suffixed("", node.name, node.uuid, 48);
        if (cast(Part)node)
            w.meshId = suffixed("mesh_", node.name, node.uuid, 44);
        if (auto mg = cast(MeshGroup)node) {
            size_t cols, rows;
            if (gridDims(mg.getMesh(), cols, rows))
                w.warpId = suffixed("warp_", node.name, node.uuid, 44);
        }
        w.world = node.transform.matrix;
        order ~= w;
        byUuid[node.uuid] = w;
        foreach(child; node.children)
            w.children ~= build(child, w);
        return w;
    }

    WNode opIndex(uint uuid) {
        auto p = uuid in byUuid;
        return p is null ? null : *p;
    }

    /// Nearest strict ancestor carrying a lumen deformer (warp or
    /// rotation), or null.
    WNode nearestDeformerAncestor(WNode w) {
        for (auto p = w.parent; p !is null; p = p.parent) {
            if (p.warpId.length > 0 || p.rotId.length > 0)
                return p;
        }
        return null;
    }

    /// The lumen deformer id of a walk node that carries one.
    static string deformerIdOf(WNode w) {
        if (w is null) return null;
        return w.warpId.length > 0 ? w.warpId : w.rotId;
    }

    /// Slash-joined node path ("/Root/Head/Face") for reports.
    static string pathOf(WNode w) {
        string path;
        for (auto p = w; p !is null; p = p.parent)
            path = "/" ~ p.name ~ path;
        return path;
    }
}
