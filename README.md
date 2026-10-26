# Torogao

Torogao is a realtime 2D puppet rigging application, maintained by
**Nice Thoughts Ranch**. It is a downstream fork of
[Inochi Creator](https://github.com/Inochi2D/inochi-creator) and the
[Inochi2D](https://github.com/Inochi2D/inochi2d) library, built on the
upstream v0_8 editor line and library v0.8.7.

Torogao is developed independently. It is not affiliated with or
endorsed by the Inochi2D Project, and support requests for Torogao
builds belong in this repository, not upstream.

## License and attribution

Torogao is distributed under the BSD 2-Clause License. The upstream
license texts and copyright notices are preserved intact:

- `creator/LICENSE` — editor (Copyright © 2020, Inochi2D Project)
- `inochi2d/LICENSE` — library (Copyright © 2020, Inochi2D Project)
- `third-party/i2d-imgui/LICENSE` — Dear ImGui bindings
- `creator/CONTRIBUTORS.md` — upstream contributor credits

## Layout

| Path | Contents |
|---|---|
| `creator/` | The editor application (dub package `torogao`) |
| `inochi2d/` | The core puppet library, pinned at v0.8.7 |
| `third-party/i2d-imgui/` | i2d-imgui 0.8.1, vendored (see below) |

Upstream history is preserved in this repository under
`refs/upstream-creator/*` and `refs/upstream-lib/*`.

## Building

Requirements: LDC >= 1.40 and dub.

```
git submodule update --init --recursive
cd inochi2d && dub add-local . 0.8.7 && cd ..
cd third-party/i2d-imgui && dub add-local . 0.8.1 && cd ../../..
cd creator && dub build
```

The default dub configuration is `torogao`, derived from upstream's
`barebones` configuration: a plain Linux executable build with no
branding or demo nagscreen. The library is pinned exactly at v0.8.7
by `creator/dub.selections.json` (path-resolved to `../inochi2d`),
and i2d-imgui is path-resolved to the vendored copy.

### Hermetic native dependencies

i2d-imgui's native dependency build (`deps/CMakeLists.txt`) pulls
its git submodules (SDL, GLFW, glbinding, freetype, cimgui) from
GitHub at build time via CMake FetchContent, unless a submodule
directory already contains a `.git` checkout. Those five submodules
are therefore registered in this repository at the exact commits
recorded by upstream i2d-imgui v0.8.1, and cimgui's own `imgui`
submodule rides inside the cimgui pin. After the one-time submodule
fetch above, the build performs no network access.

One deliberate deviation: upstream floats its SDL submodule on the
`SDL2` branch; this repository pins SDL to the commit upstream
recorded (031912c4, the release-2.28.2 commit) for reproducibility.
On Linux, SDL and FreeType come from the system; the submodules are
the Windows/macOS source path.

## Versioning

Version files (`creator/source/creator/ver.d`,
`inochi2d/source/inochi2d/ver.d`) are checked in and regenerated at
release time only, via the `update-version` dub configuration in
each package. Normal builds never run the generator and never touch
the network for versioning.
