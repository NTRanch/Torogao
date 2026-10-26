/*
    Torogao headless bridge

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.
*/
module inochi2d.torogao.bridge;

import inochi2d.core : initRenderer;

/**
    Initializes the inochi2d library for headless (renderless) use.

    This module deliberately lives inside the `inochi2d` package
    hierarchy: the library's node-type registration entry point
    (`initRenderer`) is `package(inochi2d)`, so no module outside
    that hierarchy can perform the registration a puppet load
    requires. In the renderless configuration every GL call inside
    `initRenderer` is compiled out; what remains is viewport state,
    node-type registration, and the parameter factory — exactly
    what a headless load needs.
*/
void torogaoInitHeadless() {
    initRenderer();
}
