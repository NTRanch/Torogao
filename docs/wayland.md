# Native Wayland support

Torogao runs natively under Wayland (verified on Hyprland with
Mesa: OpenGL 4.6 Core Profile via EGL, project load included).
No XWayland fallback is required.

## Historical failure: "No valid OpenGL context was found!"

Launching under Wayland from an environment whose
`XDG_SESSION_TYPE` was not `wayland` — typical for SSH launches,
wrapper scripts, and service managers — aborted startup with:

```
object.Exception@source/creator/core/package.d: No valid OpenGL context was found!
```

Root cause: the OpenGL loader (bindbc-opengl) chooses between
its EGL and GLX proc-address paths by reading the
`XDG_SESSION_TYPE` environment variable, which describes the
*login session*, not this process. With any other value it takes
the GLX path, finds no current GLX context behind SDL's EGL
context, and reports `GLSupport.noContext` — even though
`SDL_GL_CreateContext` succeeded.

Fix (in `creator/source/creator/core/package.d`,
`incOpenWindow`): before `loadOpenGL()` runs, the window's real
subsystem is established from SDL itself —
`SDL_GetWindowWMInfo`, or, where the SDL2 compatibility layer
does not report the subsystem through it, the current video
driver name (`SDL_GetCurrentVideoDriver`). When either reports
Wayland, the process environment is aligned
(`XDG_SESSION_TYPE=wayland`) so the loader takes the EGL path.
When SDL reports X11/XWayland, nothing is changed and the GLX
path is used as before.

Workaround for older builds: launch with
`XDG_SESSION_TYPE=wayland` in the environment.

## Separate finding: segfault when a project argument fails to load

Unrelated to Wayland (reproduces identically under X11): when
the editor is launched with a project argument and loading it
fails for any reason — the file is missing, or a stale lockfile
from an unclean exit (`<config>/autosaves/<project>/creator-project.lock`)
defers the load to the restore-save window — `incOpenProject`
returns before any project object is created, leaving the
global active project null. The first UI frame then
dereferences it (`creator/widgets/toolbar.d`,
`incActivePuppet().enableDrivers`, fault at offset 0x10) and
the editor segfaults instead of showing the error dialog or
the restore window. The same unguarded dereference pattern
appears across the UI panels, so this needs a UI-wide audit
rather than a one-site guard; it is recorded here instead of
being patched piecemeal. Workarounds: pass an existing file,
and delete a stale `creator-project.lock` after an unclean
exit.
