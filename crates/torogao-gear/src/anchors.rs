//! The v1 canonical body map (AMENDMENT-T4 §2.4) and the rig
//! templates anchors resolve against.
//!
//! Resolution is exact-name only: an anchor the map does not
//! name, or one the target rig cannot present, refuses with
//! `TG-GEAR-ANCHOR-UNKNOWN`. There is no nearest-anchor
//! fallback — silently clamping a device to the wrong body
//! part is a correctness failure, not graceful degradation.
//!
//! The site positions below are AUTHORED placeholder-class
//! values on the 1024 rig-canvas convention, derived from the
//! cast rig template's assembly proportions (RIG-TEMPLATE §1).
//! They exist so the solve and the census have concrete sites;
//! keeper geometry is an Owner/lumen-lane act that changes
//! these numbers, not the resolution contract.

use std::collections::BTreeMap;

/// One named anchor of the v1 body map and the rig part it is
/// based on (§2.4, middle column).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyAnchorDef {
    /// Canonical anchor name, e.g. `body.nipple.left`.
    pub name: &'static str,
    /// Rig basis: the template part/anchor the site derives
    /// from.
    pub rig_basis: &'static str,
}

/// The v1 canonical body map, alphabetical by anchor name.
pub const BODY_MAP_V1: &[BodyAnchorDef] = &[
    BodyAnchorDef {
        name: "body.ankle.left",
        rig_basis: "BodyRoot",
    },
    BodyAnchorDef {
        name: "body.ankle.right",
        rig_basis: "BodyRoot",
    },
    BodyAnchorDef {
        name: "body.breast.left",
        rig_basis: "BreastAnchor",
    },
    BodyAnchorDef {
        name: "body.breast.right",
        rig_basis: "BreastAnchor",
    },
    BodyAnchorDef {
        name: "body.chest",
        rig_basis: "BodyRoot",
    },
    BodyAnchorDef {
        name: "body.face.left",
        rig_basis: "Head",
    },
    BodyAnchorDef {
        name: "body.face.right",
        rig_basis: "Head",
    },
    BodyAnchorDef {
        name: "body.hair",
        rig_basis: "HairAnchor",
    },
    BodyAnchorDef {
        name: "body.head",
        rig_basis: "HeadAnchor",
    },
    BodyAnchorDef {
        name: "body.mouth",
        rig_basis: "Mouth",
    },
    BodyAnchorDef {
        name: "body.neck",
        rig_basis: "HeadAnchor",
    },
    BodyAnchorDef {
        name: "body.nipple.left",
        rig_basis: "BreastAnchor",
    },
    BodyAnchorDef {
        name: "body.nipple.right",
        rig_basis: "BreastAnchor",
    },
    BodyAnchorDef {
        name: "body.pelvis",
        rig_basis: "BodyRoot",
    },
    BodyAnchorDef {
        name: "body.perineum",
        rig_basis: "BodyRoot",
    },
    BodyAnchorDef {
        name: "body.root",
        rig_basis: "BodyRoot",
    },
    BodyAnchorDef {
        name: "body.waist",
        rig_basis: "BodyRoot",
    },
    BodyAnchorDef {
        name: "body.wrist.left",
        rig_basis: "BodyRoot",
    },
    BodyAnchorDef {
        name: "body.wrist.right",
        rig_basis: "BodyRoot",
    },
];

/// Looks up a canonical anchor name in the v1 map.
pub fn body_anchor(name: &str) -> Option<&'static BodyAnchorDef> {
    BODY_MAP_V1.iter().find(|def| def.name == name)
}

/// Whether a name is a fixture anchor reference
/// (`fixture.<id>`).
pub fn is_fixture_anchor(name: &str) -> bool {
    name.starts_with("fixture.") && name.len() > "fixture.".len()
}

/// The fixture id a `fixture.<id>` anchor names.
pub fn fixture_anchor_id(name: &str) -> &str {
    name.strip_prefix("fixture.").unwrap_or("")
}

/// One resolved anchor site on a rig: where the anchor sits on
/// the 1024 canvas and how large its contact area is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorSite {
    /// Canvas x in pixels (1024 convention).
    pub x_px: f64,
    /// Canvas y in pixels (1024 convention).
    pub y_px: f64,
    /// Declared contact radius in pixels; attachment offsets
    /// are bounded by it (§2.4).
    pub contact_radius_px: f64,
}

/// The content stamps a target character/scene must carry for
/// gear to validate against it (§2.7 `TG-GEAR-CONSENT-STAMP`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentStamps {
    /// The character/scene is stamped adult.
    pub adult: bool,
    /// The character/scene is stamped consent-forward.
    pub consent_forward: bool,
}

impl ContentStamps {
    /// The stamps every estate cast target carries.
    pub const GOVERNED: Self = Self {
        adult: true,
        consent_forward: true,
    };

    /// Whether both required stamps are present.
    pub fn is_complete(&self) -> bool {
        self.adult && self.consent_forward
    }
}

/// A rig template anchors resolve against: the cast character,
/// her anchor sites, her governed expression names (Addition
/// G), and her content stamps.
#[derive(Clone, Debug, PartialEq)]
pub struct RigTemplate {
    /// Character label, e.g. `Prima`.
    pub character: String,
    /// Resolved sites by canonical anchor name.
    pub sites: BTreeMap<String, AnchorSite>,
    /// Governed expression names this character's mapping
    /// resolves (§2.8 expression hooks must name one of these).
    pub governed_expressions: Vec<String>,
    /// Content stamps of the target.
    pub stamps: ContentStamps,
}

/// The governed expression names shared by the cast's Addition
/// G mappings (band names from §6.5 plus the adult pack's
/// governed `desire`).
pub const GOVERNED_EXPRESSIONS: &[&str] = &[
    "afterglow",
    "aroused",
    "building",
    "default",
    "desire",
    "near",
    "peak",
];

fn site(x_px: f64, y_px: f64, contact_radius_px: f64) -> AnchorSite {
    AnchorSite {
        x_px,
        y_px,
        contact_radius_px,
    }
}

fn cast_template(character: &str, dx: f64, dy: f64) -> RigTemplate {
    // Assembly proportions from RIG-TEMPLATE §1 on the 1024
    // canvas, shifted per character so the three templates are
    // genuinely distinct geometry, not three copies.
    let rows: &[(&str, f64, f64, f64)] = &[
        ("body.ankle.left", 430.0, 980.0, 44.0),
        ("body.ankle.right", 594.0, 980.0, 44.0),
        ("body.breast.left", 430.0, 600.0, 84.0),
        ("body.breast.right", 594.0, 600.0, 84.0),
        ("body.chest", 512.0, 560.0, 96.0),
        ("body.face.left", 432.0, 330.0, 62.0),
        ("body.face.right", 592.0, 330.0, 62.0),
        ("body.hair", 512.0, 180.0, 110.0),
        ("body.head", 512.0, 300.0, 120.0),
        ("body.mouth", 512.0, 368.0, 36.0),
        ("body.neck", 512.0, 468.0, 48.0),
        ("body.nipple.left", 430.0, 618.0, 30.0),
        ("body.nipple.right", 594.0, 618.0, 30.0),
        ("body.pelvis", 512.0, 800.0, 88.0),
        ("body.perineum", 512.0, 862.0, 40.0),
        ("body.root", 512.0, 512.0, 128.0),
        ("body.waist", 512.0, 700.0, 80.0),
        ("body.wrist.left", 250.0, 720.0, 40.0),
        ("body.wrist.right", 774.0, 720.0, 40.0),
    ];
    let sites = rows
        .iter()
        .map(|(name, x, y, r)| (name.to_string(), site(x + dx, y + dy, *r)))
        .collect();
    RigTemplate {
        character: character.to_string(),
        sites,
        governed_expressions: GOVERNED_EXPRESSIONS.iter().map(|s| s.to_string()).collect(),
        stamps: ContentStamps::GOVERNED,
    }
}

impl RigTemplate {
    /// Prima's template (Playroom host rig).
    pub fn prima() -> Self {
        cast_template("Prima", 0.0, 0.0)
    }

    /// Yui's template.
    pub fn yui() -> Self {
        cast_template("Yui", 6.0, -4.0)
    }

    /// Alma's template.
    pub fn alma() -> Self {
        cast_template("Alma", -5.0, 7.0)
    }

    /// The three cast templates, alphabetical by character.
    pub fn cast() -> [Self; 3] {
        [Self::alma(), Self::prima(), Self::yui()]
    }

    /// Resolves one canonical anchor name to its site on this
    /// rig, or `None` when this rig does not present it.
    pub fn resolve(&self, anchor: &str) -> Option<AnchorSite> {
        self.sites.get(anchor).copied()
    }
}
