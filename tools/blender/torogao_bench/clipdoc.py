"""clipdoc — torogao-motion-clip/2 document handling for the
Torogao bench. No Blender imports.

The bench authors *working* documents. Canonicalisation and
freezing belong to the estate gate tool (`motion-clip-check`,
crates/torogao-format): the bench never writes a freeze block,
and a document is only ever called frozen after that tool has
stamped it. What lives here is the document discipline the
converter needs:

- rig identity: the lowercase hex SHA-256 of the hierarchy
  definition file's bytes (the definition of record for Lana,
  lana-reaction-hierarchy.json, hashes to the rig value her
  frozen clips carry);
- the op log: entries appended in order, `seq` from 0 by ones,
  each naming actor (human | converter | agent), session, op;
- channel assembly: duplicate targets refused, keys sorted by
  frame, the final key's interp forced to `step` (a key's
  interp names the segment it opens; the last key opens none),
  tangents present exactly where the segment structure obliges
  them (out_tangent iff this key's segment is cubic,
  in_tangent iff the previous segment is cubic);
- the round-trip stash rule: a clip imported to an action
  carries its source document with it; on export, if the
  regenerated channels are identical to the stashed document's,
  the stashed document is emitted verbatim — op log and all —
  so clip -> action -> clip is byte-identical after the gate
  tool canonicalises both sides. Any edit breaks the equality
  and the export becomes a new revision with new op entries.
"""

import hashlib
import json

FORMAT = "torogao-motion-clip/2"

KEY_TYPES = ("keyframe", "breakdown", "moving_hold", "extreme")


class ClipError(Exception):
    """A named converter/document refusal."""


def rig_identity(hierarchy_path):
    """The clip `rig` value for a hierarchy definition file:
    lowercase hex SHA-256 of the file's bytes."""
    with open(hierarchy_path, "rb") as handle:
        return hashlib.sha256(handle.read()).hexdigest()


def new_document(name, rig, rate, start, end, loop=None):
    return {
        "format": FORMAT,
        "name": name,
        "rig": rig,
        "rate": {"num": rate[0], "den": rate[1]},
        "range": {"start": start, "end": end},
        "loop": ({"start": loop[0], "end": loop[1]} if loop else None),
        "channels": [],
        "shaders": [],
        "op_log": [],
    }


def append_op(doc, actor, session, op):
    # The format caps an op at 128 bytes (TG-CLIP-FIELD-TOO-
    # LONG); provenance lines are written to fit, and anything
    # still over is cut at a byte boundary with an ellipsis
    # marker so the document the bench writes always validates.
    encoded = op.encode("utf-8")
    if len(encoded) > 128:
        op = encoded[:125].decode("utf-8", "ignore") + "..."
    doc["op_log"].append({
        "seq": len(doc["op_log"]),
        "actor": actor,
        "session": session,
        "op": op,
    })


def add_channel(doc, kind, name, prop, keys):
    """Append one channel. `keys` are dicts with frame, value,
    interp and optional in_tangent / out_tangent / key_type /
    easing / pose_ref. Enforces the document discipline the
    format validator will check again at freeze time; failing
    here names the problem in bench terms."""
    target = {"kind": kind, "name": name, "property": prop}
    for existing in doc["channels"]:
        if existing["target"] == target:
            raise ClipError(
                "duplicate channel target {}:{}:{}".format(kind, name, prop))
    keys = sorted(keys, key=lambda k: k["frame"])
    for prev, cur in zip(keys, keys[1:]):
        if cur["frame"] <= prev["frame"]:
            raise ClipError(
                "channel {}:{}:{} has non-increasing key frames".format(
                    kind, name, prop))
    for i, key in enumerate(keys):
        is_last = i == len(keys) - 1
        if is_last:
            key["interp"] = "step"
        opens_cubic = (not is_last) and key["interp"] == "cubic"
        prev_cubic = i > 0 and keys[i - 1]["interp"] == "cubic"
        if opens_cubic and "out_tangent" not in key:
            raise ClipError(
                "channel {}:{}:{} key {} opens a cubic segment "
                "without an out_tangent".format(kind, name, prop,
                                                key["frame"]))
        if not opens_cubic:
            key.pop("out_tangent", None)
        if prev_cubic and "in_tangent" not in key:
            raise ClipError(
                "channel {}:{}:{} key {} closes a cubic segment "
                "without an in_tangent".format(kind, name, prop,
                                               key["frame"]))
        if not prev_cubic:
            key.pop("in_tangent", None)
        if "key_type" in key and key["key_type"] not in KEY_TYPES:
            raise ClipError(
                "channel {}:{}:{} key {} has unknown key_type "
                "{!r}".format(kind, name, prop, key["frame"],
                              key["key_type"]))
    doc["channels"].append({"target": target, "keys": keys})


def channels_equal(a, b):
    """Deep equality of two documents' channel lists (the stash
    rule's test). Numbers compare as parsed JSON numbers."""
    return a["channels"] == b["channels"]


def document_equal(a, b):
    """Deep equality of the freeze-relevant document body
    (everything but the freeze block, which a working document
    never carries)."""
    keys = ("format", "name", "rig", "rate", "range", "loop",
            "channels", "shaders")
    return all(a.get(k) == b.get(k) for k in keys)


def load_document(path):
    with open(path, "r", encoding="utf-8") as handle:
        doc = json.load(handle)
    if doc.get("format") != FORMAT:
        raise ClipError("{} is not a {} document".format(path, FORMAT))
    return doc


def write_document(doc, path):
    """Write a working document as plain JSON. The gate tool
    canonicalises at freeze time; key order here follows the
    golden documents' field order for readability."""
    ordered = {
        "format": doc["format"],
        "name": doc["name"],
        "rig": doc["rig"],
        "rate": doc["rate"],
        "range": doc["range"],
        "loop": doc.get("loop"),
        "channels": doc["channels"],
        "shaders": doc.get("shaders", []),
        "op_log": doc["op_log"],
    }
    if "freeze" in doc:
        ordered["freeze"] = doc["freeze"]
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(ordered, handle, indent=2, ensure_ascii=False)
        handle.write("\n")
    return path
