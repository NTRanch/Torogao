/*
    Torogao foreign export: INP -> lumen-rig-core native model

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    Conversion taxonomy (per ANALYSIS §4a): every source construct
    is MAPPPED, APPROXIMATED, or DROPPED-WITH-REASON, and lands in
    the conversion report. The exporter mechanically accounts for
    every parameter binding: a binding no handler consumed is
    reported dropped at the end, so silent loss is impossible by
    construction.

    Orientation: INP space is y-down, lumen model space is y-up
    (SPEC §2.1). All baked positions flip y; rotation angles flip
    sign.
*/
module torogao.cli.rigexport;

import inochi2d.core;
import inochi2d.math;
import torogao.cli.puppetwalk;
import std.json : JSONValue, JSONType;
import std.format : format;
import std.math : PI, round;
import std.algorithm : sort, canFind;

struct ConvEntry {
    string verdict;   /// "mapped" | "approximated" | "dropped"
    string construct; /// what the source construct is
    string detail;    /// how it was handled, or why it was dropped

    JSONValue toJson() const {
        JSONValue j;
        j["verdict"] = verdict;
        j["construct"] = construct;
        j["detail"] = detail;
        return j;
    }
}

struct ExportResult {
    JSONValue model;
    ConvEntry[] entries;

    size_t count(string verdict) const {
        size_t n;
        foreach(e; entries) if (e.verdict == verdict) n++;
        return n;
    }
}

private JSONValue jv(double v) { return JSONValue(v); }
private JSONValue jv2(float x, float y) {
    JSONValue[] pair = [JSONValue(cast(double)x), JSONValue(cast(double)y)];
    return JSONValue(pair);
}
private JSONValue jv2f(float[2] p) { return jv2(p[0], p[1]); }

/// One lattice axis: a scalar parameter channel + its key values.
private struct AxisSrc {
    Parameter param;
    int axis;          /// 0 = x channel, 1 = y channel
    string scalarId;   /// lumen scalar parameter id
    float[] keys;      /// axis point values, source order
}

private bool strictlyIncreasing(float[] v) {
    foreach(i; 1..v.length) if (!(v[i] > v[i-1])) return false;
    return true;
}

final class RigExporter {
    Puppet puppet;
    PuppetWalk walk;
    size_t slotCount;
    ConvEntry[] entries;
    private bool[ParameterBinding] consumed;

    // Parameter mapping
    private string[Parameter] paramXId;
    private string[Parameter] paramYId;   // only for vec2 params
    private bool[Parameter] paramDropped;

    // Draw order ranks by node uuid
    private size_t[uint] drawRank;

    // Anchor reference points (lumen coords) by node uuid
    private float[2][uint] anchorPoint;

    this(Puppet puppet, PuppetWalk walk, size_t slotCount) {
        this.puppet = puppet;
        this.walk = walk;
        this.slotCount = slotCount;
    }

    private void note(string verdict, string construct, string detail) {
        entries ~= ConvEntry(verdict, construct, detail);
    }

    ExportResult run() {
        emitParameters();
        assignDrawRanks();
        auto deformers = emitDeformers();   // rotation + warp, sets rotId
        auto parts = emitParts();
        auto textures = emitTextures();
        auto meshes = emitMeshes();
        auto physics = emitPhysics();
        accountBindings();
        accountWholeConstructs();

        JSONValue model;
        model["format"] = "lumen-rig-model";
        model["format_version"] = 1;
        string modelName = puppet.meta.name.length > 0
            ? slugify(puppet.meta.name, 48) : "model";
        model["id"] = modelName;
        model["generator"] = "torogao-cli 0.1.0";
        model["parameters"] = paramArray;
        model["parts"] = parts;
        model["textures"] = textures;
        model["deformers"] = deformers;
        model["meshes"] = meshes;
        model["physics"] = physics;
        return ExportResult(model, entries);
    }

    private JSONValue paramArray;

    // ------------------------------------------------ parameters
    private void emitParameters() {
        JSONValue[] arr;
        bool[string] usedIds;
        foreach(param; puppet.parameters) {
            string base = suffixedParam(param);
            if (!(param.min.x < param.max.x) ||
                param.defaults.x < param.min.x ||
                param.defaults.x > param.max.x ||
                (param.isVec2 && (!(param.min.y < param.max.y) ||
                                  param.defaults.y < param.min.y ||
                                  param.defaults.y > param.max.y))) {
                paramDropped[param] = true;
                note("dropped", format!"parameter '%s'"(param.name),
                    "parameter range is degenerate or its default lies outside it; lumen parameters require min < max with the default inside (LRF-302/303)");
                continue;
            }
            string xid = base ~ "_x";
            if (xid in usedIds) xid = base ~ "_x2";
            usedIds[xid] = true;
            paramXId[param] = xid;
            JSONValue sx;
            sx["id"] = xid;
            sx["kind"] = "scalar";
            sx["min"] = jv(param.min.x);
            sx["max"] = jv(param.max.x);
            sx["default"] = jv(param.defaults.x);
            arr ~= sx;
            if (param.isVec2) {
                string yid = base ~ "_y";
                if (yid in usedIds) yid = base ~ "_y2";
                usedIds[yid] = true;
                paramYId[param] = yid;
                JSONValue sy;
                sy["id"] = yid;
                sy["kind"] = "scalar";
                sy["min"] = jv(param.min.y);
                sy["max"] = jv(param.max.y);
                sy["default"] = jv(param.defaults.y);
                arr ~= sy;
            }
            // Additive (the default) and Passthrough are the plain
            // channel behaviors; only the modes that change how
            // drivers compose are worth a report entry.
            if (param.mergeMode != ParamMergeMode.Additive &&
                param.mergeMode != ParamMergeMode.Passthrough) {
                note("approximated", format!"parameter '%s'"(param.name),
                    format!"merge mode '%s' is a runtime composition rule in INP; lumen v1 has no equivalent (combined parameters are explicit terms), so the parameter exports as plain scalar channel(s)"
                        (param.mergeMode));
            }
        }
        paramArray = JSONValue(arr);
    }

    private string suffixedParam(Parameter param) {
        return format!"%s_%u"(slugify(param.name, 44), param.uuid);
    }

    // ------------------------------------------------ draw order
    private void assignDrawRanks() {
        size_t rank = 0;
        void assignSubtreeParts(WNode w) {
            if (cast(Part)w.node) drawRank[w.uuid] = rank++;
            foreach(c; w.children) assignSubtreeParts(c);
        }
        foreach(rp; puppet.getRootParts()) {
            auto w = walk[rp.uuid];
            if (w is null) continue;
            if (cast(Composite)rp) assignSubtreeParts(w);
            else if (cast(Part)rp) drawRank[w.uuid] = rank++;
        }
        foreach(w; walk.order) {
            if (cast(Part)w.node && (w.uuid !in drawRank))
                drawRank[w.uuid] = rank++;
        }
    }

    // ------------------------------------------------ lattices
    /**
        Collects the axes a set of bindings spans, in first-use
        order. Returns false (with `reason`) when the bindings
        cannot form a lumen lattice (over 2 axes, or an axis whose
        keys are unusable).
    */
    private bool collectAxes(ParameterBinding[] bindings,
                             out AxisSrc[] axes, out string reason) {
        bool[string] seen;
        foreach(b; bindings) {
            auto param = paramOf(b);
            if (param is null) continue;
            int[] chans = param.isVec2 ? [0, 1] : [0];
            foreach(ch; chans) {
                string key = format!"%u/%d"(param.uuid, ch);
                if (key in seen) continue;
                seen[key] = true;
                AxisSrc ax;
                ax.param = param;
                ax.axis = ch;
                ax.scalarId = ch == 0 ? paramXId.get(param, null)
                                      : paramYId.get(param, null);
                ax.keys = param.axisPoints[ch].dup;
                axes ~= ax;
            }
        }
        if (axes.length == 0) { reason = "no usable parameter axes"; return false; }
        if (axes.length > 2) {
            reason = format!"bindings span %d parameter axes; a lumen lattice has at most 2"
                (axes.length);
            return false;
        }
        foreach(ax; axes) {
            if (ax.scalarId is null) {
                reason = "an axis parameter was dropped during parameter conversion";
                return false;
            }
            if (ax.keys.length < 2 || ax.keys.length > 32) {
                reason = format!"axis '%s' has %d keypoints; a lumen lattice axis needs 2..=32"
                    (ax.scalarId, ax.keys.length);
                return false;
            }
            if (!strictlyIncreasing(ax.keys)) {
                reason = format!"axis '%s' keypoints are not strictly increasing"
                    (ax.scalarId);
                return false;
            }
            float lo = ax.axis == 0 ? ax.param.min.x : ax.param.min.y;
            float hi = ax.axis == 0 ? ax.param.max.x : ax.param.max.y;
            foreach(k; ax.keys) {
                if (k < lo || k > hi) {
                    reason = format!"axis '%s' keypoint %g lies outside the parameter range"
                        (ax.scalarId, k);
                    return false;
                }
            }
        }
        return true;
    }

    /// Grid indices into binding `b`'s value grid for lattice point
    /// (i, j) over `axes`.
    private void bindingIndices(ParameterBinding b, AxisSrc[] axes,
                                size_t i, size_t j, out size_t bx, out size_t by) {
        bx = 0; by = 0;
        auto param = paramOf(b);
        foreach(ai, ax; axes) {
            size_t idx = ai == 0 ? i : j;
            if (ax.param is param) {
                if (ax.axis == 0) bx = idx;
                else by = idx;
            }
        }
    }

    // ------------------------------------------------ deformers
    private JSONValue emitDeformers() {
        JSONValue[] arr;
        // Rotation deformers first: warp parents may reference them.
        foreach(w; walk.order) {
            auto rot = buildRotation(w);
            if (rot.type != JSONType.null_) arr ~= rot;
        }
        foreach(w; walk.order) {
            if (w.warpId.length == 0) continue;
            auto warp = buildWarp(w);
            if (warp.type != JSONType.null_) arr ~= warp;
        }
        // MeshGroups without a grid cannot be lumen warps.
        foreach(w; walk.order) {
            auto mg = cast(MeshGroup)w.node;
            if (mg is null || w.warpId.length > 0) continue;
            note("dropped", format!"MeshGroup '%s'"(PuppetWalk.pathOf(w)),
                "the MeshGroup's mesh is not a regular grid; its barycentric triangle warp has no lumen v1 equivalent (lumen warps are grids)");
        }
        return JSONValue(arr);
    }

    /// Bindings of `param` targeting node `node` with property
    /// `prop`, across all parameters.
    private ParameterBinding[] channelBindings(Node node, string prop) {
        ParameterBinding[] out_;
        foreach(param; puppet.parameters) {
            if (param in paramDropped) continue;
            foreach(b; param.bindings) {
                if (b.getNode() is node && b.getName() == prop &&
                    cast(ValueParameterBinding)b)
                    out_ ~= b;
            }
        }
        return out_;
    }

    private JSONValue buildRotation(WNode w) {
        auto rz = channelBindings(w.node, "transform.r.z");
        auto sx = channelBindings(w.node, "transform.s.x");
        auto sy = channelBindings(w.node, "transform.s.y");
        if (rz.length == 0 && sx.length == 0 && sy.length == 0)
            return JSONValue(null);
        auto all = rz ~ sx ~ sy;
        AxisSrc[] axes; string reason;
        if (!collectAxes(all, axes, reason)) {
            foreach(b; all) {
                consumed[b] = true;
                note("dropped", bindingConstruct(b),
                    "rotation binding cannot form a lumen lattice: " ~ reason);
            }
            return JSONValue(null);
        }
        w.rotId = suffixed("rot_", w.name, w.uuid, 44);
        anchorPoint[w.uuid] = w.worldPos();

        float baseRzDeg = -(w.node.localTransform.rotation.z * 180.0f / PI);
        float baseSx = w.node.localTransform.scale.x;
        float baseSy = w.node.localTransform.scale.y;

        JSONValue[] forms;
        size_t nx = axes[0].keys.length;
        size_t ny = axes.length > 1 ? axes[1].keys.length : 1;
        foreach(i; 0..nx) {
            foreach(j; 0..ny) {
                float angle = baseRzDeg;
                float scaleX = baseSx, scaleY = baseSy;
                foreach(b; rz) {
                    size_t bx, by; bindingIndices(b, axes, i, j, bx, by);
                    angle -= valueAt(b, bx, by) * 180.0f / PI;
                }
                foreach(b; sx) {
                    size_t bx, by; bindingIndices(b, axes, i, j, bx, by);
                    scaleX *= valueAt(b, bx, by);
                }
                foreach(b; sy) {
                    size_t bx, by; bindingIndices(b, axes, i, j, bx, by);
                    scaleY *= valueAt(b, bx, by);
                }
                JSONValue f;
                JSONValue[] at = [JSONValue(i)];
                if (axes.length > 1) at ~= JSONValue(j);
                f["at"] = at;
                f["angle_degrees"] = jv(angle);
                f["scale"] = jv2(scaleX, scaleY);
                forms ~= f;
            }
        }
        foreach(b; all) consumed[b] = true;
        note("approximated", format!"rotation of node '%s'"(PuppetWalk.pathOf(w)),
            "node transform rotation/scale bindings become a lumen rotation deformer; angles are absolute world-space degrees (sign-flipped for the y-up space) and rest vertices are baked with the full rest transform, so runtime evaluation semantics differ from INP's local-transform composition");

        JSONValue d;
        d["id"] = w.rotId;
        d["kind"] = "rotation";
        auto parent = PuppetWalk.deformerIdOf(walk.nearestDeformerAncestor(w));
        if (parent !is null) d["parent"] = parent;
        d["origin"] = jv2f(w.worldPos());
        d["base_angle_degrees"] = jv(baseRzDeg);
        d["keyforms"] = latticeJson(axes, forms);
        return d;
    }

    private JSONValue buildWarp(WNode w) {
        auto mg = cast(MeshGroup)w.node;
        auto mesh = mg.getMesh();
        size_t cols, rows;
        if (!gridDims(mesh, cols, rows) || cols > 32 || rows > 32) {
            note("dropped", format!"MeshGroup '%s'"(PuppetWalk.pathOf(w)),
                format!"mesh vertices (%d) do not form a regular grid within the lumen warp limits (1..=32 cells per side); the deformer and its bindings were dropped"
                    (mesh.vertices.length));
            foreach(defB; deformBindingsOf(w.node)) consumed[defB] = true;
            w.warpId = "";
            return JSONValue(null);
        }
        anchorPoint[w.uuid] = w.worldPos();

        JSONValue[] rest;
        foreach(v; mesh.vertices) rest ~= jv2f(w.toWorld(v, mesh.origin));

        // Deformation bindings on the MeshGroup itself.
        ParameterBinding[] defs = deformBindingsOf(w.node);
        JSONValue keyforms;
        if (defs.length > 0) {
            AxisSrc[] axes; string reason;
            if (!collectAxes(defs, axes, reason)) {
                foreach(b; defs) {
                    consumed[b] = true;
                    note("dropped", bindingConstruct(b),
                        "warp deformation cannot form a lumen lattice: " ~ reason);
                }
            } else {
                JSONValue[] forms;
                size_t nx = axes[0].keys.length;
                size_t ny = axes.length > 1 ? axes[1].keys.length : 1;
                foreach(i; 0..nx) {
                    foreach(j; 0..ny) {
                        JSONValue[] positions;
                        foreach(vi, v; mesh.vertices) {
                            auto p = w.toWorld(v, mesh.origin);
                            foreach(b; defs) {
                                size_t bx, by;
                                bindingIndices(b, axes, i, j, bx, by);
                                auto off = deformOffsetAt(b, bx, by, vi);
                                auto d = w.deltaToWorld(off);
                                p[0] += d[0]; p[1] += d[1];
                            }
                            positions ~= jv2f(p);
                        }
                        JSONValue f;
                        JSONValue[] at = [JSONValue(i)];
                        if (axes.length > 1) at ~= JSONValue(j);
                        f["at"] = at;
                        f["positions"] = positions;
                        forms ~= f;
                    }
                }
                foreach(b; defs) consumed[b] = true;
                keyforms = latticeJson(axes, forms);
            }
        }
        note("mapped", format!"MeshGroup '%s'"(PuppetWalk.pathOf(w)),
            "grid MeshGroup becomes a lumen warp deformer; children deform through the deformer chain (INP applies the same mesh barycentrically — grid case only)");

        JSONValue d;
        d["id"] = w.warpId;
        d["kind"] = "warp";
        string parent = w.rotId.length > 0 ? w.rotId
            : PuppetWalk.deformerIdOf(walk.nearestDeformerAncestor(w));
        if (parent !is null) d["parent"] = parent;
        d["columns"] = cols;
        d["rows"] = rows;
        d["rest_positions"] = rest;
        if (keyforms.type != JSONType.null_) d["keyforms"] = keyforms;
        return d;
    }

    private JSONValue latticeJson(AxisSrc[] axes, JSONValue[] forms) {
        JSONValue lat;
        JSONValue[] axisIds;
        JSONValue[] keys;
        foreach(ax; axes) {
            axisIds ~= JSONValue(ax.scalarId);
            JSONValue[] ks;
            foreach(k; ax.keys) ks ~= jv(k);
            keys ~= JSONValue(ks);
        }
        lat["axes"] = axisIds;
        lat["keys"] = keys;
        lat["forms"] = forms;
        return lat;
    }

    // ------------------------------------------------ binding access
    private Parameter paramOf(ParameterBinding b) {
        if (auto f = cast(ParameterBindingImpl!float)b) return f.parameter;
        if (auto d = cast(ParameterBindingImpl!Deformation)b) return d.parameter;
        return null;
    }

    private float valueAt(ParameterBinding b, size_t bx, size_t by) {
        auto impl = cast(ParameterBindingImpl!float)b;
        if (impl is null || bx >= impl.values.length ||
            by >= impl.values[bx].length) return 0;
        return impl.values[bx][by];
    }

    private vec2 deformOffsetAt(ParameterBinding b, size_t bx, size_t by,
                                size_t vertexIndex) {
        auto impl = cast(ParameterBindingImpl!Deformation)b;
        if (impl is null || bx >= impl.values.length ||
            by >= impl.values[bx].length) return vec2(0, 0);
        // Take the cell by pointer: copying a Deformation runs its
        // destructor, which frees the offset buffer (numem vector),
        // so reading a copy's .data afterwards is use-after-free.
        Deformation* cell = &impl.values[bx][by];
        if (vertexIndex >= cell.vertexOffsets.size()) return vec2(0, 0);
        return cell.vertexOffsets.data[vertexIndex];
    }

    private ParameterBinding[] deformBindingsOf(Node node) {
        ParameterBinding[] out_;
        foreach(param; puppet.parameters) {
            if (param in paramDropped) continue;
            foreach(b; param.bindings) {
                if (b.getNode() is node &&
                    cast(DeformationParameterBinding)b) {
                    b.reInterpolate();
                    out_ ~= b;
                }
            }
        }
        return out_;
    }

    private string bindingConstruct(ParameterBinding b) {
        auto param = paramOf(b);
        Node target = b.getNode();
        return format!"binding '%s' -> '%s'.%s"(
            param is null ? "?" : param.name,
            target is null ? "?" : PuppetWalk.pathOf(walk[target.uuid]),
            b.getName());
    }

    // ------------------------------------------------ parts
    private JSONValue emitParts() {
        JSONValue[] arr;
        foreach(w; walk.order) {
            JSONValue p;
            p["id"] = w.partId;
            if (w.parent !is null) p["parent"] = w.parent.partId;
            arr ~= p;
            anchorPoint[w.uuid] = w.worldPos();
            if (w.node.lockToRoot) {
                note("approximated", format!"node '%s'"(PuppetWalk.pathOf(w)),
                    "lockToRoot is baked into the rest transform; the lock semantics are not carried");
            }
            if (auto comp = cast(Composite)w.node) {
                note("approximated", format!"Composite '%s'"(PuppetWalk.pathOf(w)),
                    format!"composite group is flattened: children reparent to the composite's parent part, its opacity is folded into descendant mesh opacity, and its blend mode (%s) is dropped (render-to-texture groups have no lumen v1 equivalent)"
                        (comp.blendingMode));
            }
        }
        return JSONValue(arr);
    }

    /// Product of Composite ancestors' opacity for a node.
    private float compositeOpacityFactor(WNode w) {
        float factor = 1;
        for (auto p = w.parent; p !is null; p = p.parent) {
            if (auto comp = cast(Composite)p.node)
                factor *= comp.opacity;
        }
        return factor;
    }

    // ------------------------------------------------ textures
    private string textureIdForSlot(size_t slot) {
        return format!"tex_%d"(slot);
    }

    private JSONValue emitTextures() {
        import inochi2d.integration : inCurrentPuppetTextureSlots;
        JSONValue[] arr;
        foreach(i, blob; inCurrentPuppetTextureSlots) {
            string ext = blob.tag == 0 ? "png" : blob.tag == 1 ? "tga"
                : blob.tag == 2 ? "bc7" : "bin";
            JSONValue t;
            t["id"] = textureIdForSlot(i);
            t["source"] = format!"textures/slot-%d.%s"(i, ext);
            arr ~= t;
        }
        if (arr.length > 0) {
            note("mapped", "embedded texture slots",
                "textures are referenced by generated slot paths (textures/slot-<n>.<fmt>); payloads stay embedded in the source INP and are not extracted by this converter");
        }
        return JSONValue(arr);
    }

    // ------------------------------------------------ meshes
    private JSONValue emitMeshes() {
        JSONValue[] arr;
        bool[string] droppedMeshIds;
        foreach(w; walk.order) {
            auto part = cast(Part)w.node;
            if (part is null) continue;
            auto mesh = part.getMesh();
            string path = PuppetWalk.pathOf(w);

            string dropReason;
            if (mesh.vertices.length == 0)
                dropReason = "part has no vertices";
            else if (mesh.vertices.length > 65536)
                dropReason = format!"part has %d vertices; lumen allows at most 65536 per mesh"
                    (mesh.vertices.length);
            else if (mesh.indices.length < 3 || mesh.indices.length % 3 != 0)
                dropReason = format!"index list (%d) is not a triangle list of at least 3"
                    (mesh.indices.length);
            else if (mesh.uvs.length != mesh.vertices.length)
                dropReason = format!"uv count (%d) does not match vertex count (%d)"
                    (mesh.uvs.length, mesh.vertices.length);
            else if (part.textureIds.length == 0)
                dropReason = "part references no texture; a lumen mesh requires a texture reference";
            else if (part.textureIds[0] >= slotCount)
                dropReason = format!"part's texture slot %d does not exist (%d slots embedded)"
                    (part.textureIds[0], slotCount);
            if (dropReason.length > 0) {
                droppedMeshIds[w.meshId] = true;
                note("dropped", format!"Part '%s'"(path), dropReason);
                continue;
            }

            foreach(extra; part.textureIds[1..$]) {
                note("dropped", format!"Part '%s' secondary texture"(path),
                    format!"texture slot %d beyond the first: lumen v1 meshes reference exactly one texture (emissive/bump slots have no home)"
                        (extra));
            }

            JSONValue[] vertices;
            double cx = 0, cy = 0;
            foreach(v; mesh.vertices) {
                auto p = w.toWorld(v, mesh.origin);
                vertices ~= jv2f(p);
                cx += p[0]; cy += p[1];
            }
            anchorPoint[w.uuid] = [cast(float)(cx / mesh.vertices.length),
                                   cast(float)(cy / mesh.vertices.length)];

            JSONValue[] uvs;
            foreach(uv; mesh.uvs) uvs ~= jv2(uv.x, uv.y);
            JSONValue[] indices;
            foreach(ix; mesh.indices) indices ~= JSONValue(cast(uint)ix);

            float opacity = part.opacity * compositeOpacityFactor(w);

            JSONValue m;
            m["id"] = w.meshId;
            m["part"] = w.partId;
            m["texture"] = textureIdForSlot(part.textureIds[0]);
            string deformer = w.rotId.length > 0 ? w.rotId
                : PuppetWalk.deformerIdOf(walk.nearestDeformerAncestor(w));
            if (deformer !is null) m["deformer"] = deformer;
            m["vertices"] = vertices;
            m["uvs"] = uvs;
            m["indices"] = indices;
            m["opacity"] = jv(opacity);
            m["blend_mode"] = blendModeFor(part, path);
            m["draw_order"] = cast(int)drawRank.get(w.uuid, 0);
            m["rendered"] = w.node.renderEnabled;

            // Clipping masks
            JSONValue[] maskIds;
            foreach(mask; part.masks) {
                auto srcW = mask.maskSrc is null ? null
                    : walk[mask.maskSrc.uuid];
                if (mask.mode == MaskingMode.DodgeMask) {
                    note("dropped", format!"Part '%s' dodge mask"(path),
                        "dodge masks are inverted; the lumen v1 mask relation has no inversion");
                    continue;
                }
                if (srcW is null || srcW.meshId.length == 0 ||
                    (srcW.meshId in droppedMeshIds) !is null) {
                    note("dropped", format!"Part '%s' mask"(path),
                        "mask source does not export as a lumen mesh");
                    continue;
                }
                maskIds ~= JSONValue(srcW.meshId);
            }
            if (maskIds.length > 0) m["clipping_mask_ids"] = maskIds;

            // Keyforms from deformation bindings
            auto defs = deformBindingsOf(w.node);
            if (defs.length > 0) {
                AxisSrc[] axes; string reason;
                if (!collectAxes(defs, axes, reason)) {
                    foreach(b; defs) {
                        consumed[b] = true;
                        note("dropped", bindingConstruct(b),
                            "mesh deformation cannot form a lumen lattice: " ~ reason);
                    }
                } else {
                    JSONValue[] forms;
                    size_t nx = axes[0].keys.length;
                    size_t ny = axes.length > 1 ? axes[1].keys.length : 1;
                    foreach(i; 0..nx) {
                        foreach(j; 0..ny) {
                            JSONValue[] verts;
                            foreach(vi, v; mesh.vertices) {
                                auto p = w.toWorld(v, mesh.origin);
                                foreach(b; defs) {
                                    size_t bx, by;
                                    bindingIndices(b, axes, i, j, bx, by);
                                    auto d = w.deltaToWorld(
                                        deformOffsetAt(b, bx, by, vi));
                                    p[0] += d[0]; p[1] += d[1];
                                }
                                verts ~= jv2f(p);
                            }
                            JSONValue f;
                            JSONValue[] at = [JSONValue(i)];
                            if (axes.length > 1) at ~= JSONValue(j);
                            f["at"] = at;
                            f["vertices"] = verts;
                            forms ~= f;
                        }
                    }
                    foreach(b; defs) consumed[b] = true;
                    m["keyforms"] = latticeJson(axes, forms);
                }
            }

            // Non-representable part shading state
            if (part.tint != vec3(1, 1, 1))
                note("dropped", format!"Part '%s' tint"(path),
                    "lumen v1 meshes carry no tint");
            if (part.screenTint != vec3(0, 0, 0))
                note("dropped", format!"Part '%s' screen tint"(path),
                    "lumen v1 meshes carry no screen tint");
            if (part.emissionStrength != 1)
                note("dropped", format!"Part '%s' emission strength"(path),
                    "lumen v1 meshes carry no emission channel");

            arr ~= m;
        }
        return JSONValue(arr);
    }

    private string blendModeFor(Part part, string path) {
        switch (part.blendingMode) {
            case BlendMode.Normal:      return "normal";
            case BlendMode.Multiply:    return "multiply";
            case BlendMode.Screen:      return "screen";
            case BlendMode.Overlay:     return "overlay";
            case BlendMode.Darken:      return "darken";
            case BlendMode.Lighten:     return "lighten";
            case BlendMode.ColorDodge:  return "color_dodge";
            case BlendMode.ColorBurn:   return "color_burn";
            case BlendMode.HardLight:   return "hard_light";
            case BlendMode.SoftLight:   return "soft_light";
            case BlendMode.Difference:  return "difference";
            case BlendMode.Exclusion:   return "exclusion";
            case BlendMode.LinearDodge: return "linear_dodge";
            case BlendMode.AddGlow:
                note("approximated", format!"Part '%s' blend mode"(path),
                    "AddGlow exports as linear_dodge; the glow falloff is not carried");
                return "linear_dodge";
            default:
                note("dropped", format!"Part '%s' blend mode"(path),
                    format!"blend mode %s has no lumen v1 equivalent; the mesh exports as normal"
                        (part.blendingMode));
                return "normal";
        }
    }

    // ------------------------------------------------ physics
    private JSONValue emitPhysics() {
        JSONValue[] arr;
        bool[string] writtenOutputs;
        foreach(w; walk.order) {
            auto phys = cast(SimplePhysics)w.node;
            if (phys is null) continue;
            string path = PuppetWalk.pathOf(w);
            Parameter driven = phys.param;

            // Anchor: nearest strict ancestor that exports as a
            // deformer or a mesh.
            WNode anchorW;
            string anchorKind;
            for (auto p = w.parent; p !is null; p = p.parent) {
                if (p.warpId.length > 0 || p.rotId.length > 0) {
                    anchorW = p; anchorKind = "deformer"; break;
                }
                if (p.meshId.length > 0 && cast(Part)p.node) {
                    anchorW = p; anchorKind = "mesh"; break;
                }
            }
            if (anchorW is null) {
                note("dropped", format!"SimplePhysics '%s'"(path),
                    "no ancestor exports as a lumen deformer or mesh, so the strand has no anchor");
                continue;
            }
            if (!(phys.length > 0)) {
                note("dropped", format!"SimplePhysics '%s'"(path),
                    "pendulum length is not positive");
                continue;
            }
            if (phys.angleDamping < 0 || phys.angleDamping >= 1) {
                note("dropped", format!"SimplePhysics '%s'"(path),
                    format!"angle damping %g lies outside the lumen particle damping range [0, 1)"
                        (phys.angleDamping));
                continue;
            }
            double gravityY = -(cast(double)puppet.physics.gravity *
                                puppet.physics.pixelsPerMeter * phys.gravity);
            if (gravityY < -1e5 || gravityY > 1e5) {
                note("dropped", format!"SimplePhysics '%s'"(path),
                    "effective gravity exceeds the lumen field bound (1e5)");
                continue;
            }

            JSONValue particle;
            particle["length"] = jv(phys.length);
            particle["radius"] = jv(0);
            particle["mobility"] = jv(phys.modelType_ ==
                PhysicsModel.SpringPendulum ? 0.5 : 0.0);
            particle["delay"] = jv(0);
            particle["damping"] = jv(phys.angleDamping);
            JSONValue[] particles = [particle];

            JSONValue strand;
            strand["id"] = "strand";
            JSONValue anchor;
            anchor["kind"] = anchorKind;
            anchor["id"] = anchorKind == "deformer"
                ? PuppetWalk.deformerIdOf(anchorW) : anchorW.meshId;
            auto myPos = w.worldPos();
            auto refPoint = anchorPoint.get(anchorW.uuid, anchorW.worldPos());
            anchor["offset"] = jv2(myPos[0] - refPoint[0], myPos[1] - refPoint[1]);
            strand["anchor"] = anchor;
            strand["particles"] = particles;

            string approxDetail = format!"SimplePhysics (%s, map mode %s) becomes a one-particle lumen strand; INP integrates a rigid/spring pendulum in anchor space while lumen integrates a Verlet chain — trajectories will differ"
                (phys.modelType_, phys.mapMode);
            if (driven !is null && !(driven in paramDropped)) {
                bool useY = phys.mapMode == ParamMapMode.LengthAngle ||
                            phys.mapMode == ParamMapMode.YX;
                string outId = useY ? paramYId.get(driven, null)
                                     : paramXId.get(driven, null);
                if (outId !is null && (outId in writtenOutputs) is null) {
                    writtenOutputs[outId] = true;
                    float scale = (useY ? phys.outputScale.y
                                        : phys.outputScale.x) / 180.0f;
                    JSONValue output;
                    output["parameter"] = outId;
                    output["scale"] = jv(scale);
                    output["offset"] = jv(0);
                    strand["output"] = output;
                } else if (outId !is null) {
                    approxDetail ~= "; the strand's output was dropped because another strand already writes that parameter (lumen one-writer rule)";
                }
            }
            note("approximated", format!"SimplePhysics '%s'"(path), approxDetail);

            JSONValue group;
            group["id"] = suffixed("phys_", w.name, w.uuid, 44);
            group["gravity"] = jv2(0, cast(float)gravityY);
            JSONValue[] noInputs; group["inputs"] = noInputs;
            JSONValue[] strands = [strand];
            group["strands"] = strands;
            arr ~= group;
        }
        return JSONValue(arr);
    }

    // ------------------------------------------------ accounting
    /// Every binding no handler consumed is reported dropped.
    /// This is what makes silent loss impossible by construction.
    private void accountBindings() {
        foreach(param; puppet.parameters) {
            foreach(b; param.bindings) {
                if ((b in consumed) !is null) continue;
                consumed[b] = true;
                string channel = b.getName();
                string reason;
                if (cast(DeformationParameterBinding)b)
                    reason = "deformation binding was not consumed by any exporter path";
                else if (channel.length >= 10 && channel[0..10] == "transform.")
                    reason = "node transform channels have no lumen v1 representation (parts carry no transforms); only transform.r.z / s.x / s.y are consumed, as rotation deformers";
                else
                    reason = format!"property channel '%s' has no lumen v1 representation"(channel);
                note("dropped", bindingConstruct(b), reason);
            }
        }
    }

    private void accountWholeConstructs() {
        foreach(name, ref anim; puppet.getAnimations()) {
            size_t keyframes;
            foreach(ref lane; anim.lanes) keyframes += lane.frames.length;
            note("dropped", format!"animation '%s'"(name),
                format!"%d lane(s), %d keyframes: lumen motion assets are a separate asset kind (SPEC section 4) with no parser in lumen-rig-core R1; animations are not part of the model asset"
                    (anim.lanes.length, keyframes));
        }
        foreach(auto_; puppet.automation) {
            note("dropped", format!"automation '%s'"(auto_.typeId),
                "procedural parameter drivers have no lumen v1 representation");
        }
        foreach(name, _; puppet.extData) {
            note("dropped", format!"vendor EXT section '%s'"(name),
                "vendor extension data is not carried into the lumen model");
        }
    }
}
