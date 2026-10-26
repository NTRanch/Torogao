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
`incOpenWindow`): SDL has already established the window's real
subsystem via `SDL_GetWindowWMInfo` before `loadOpenGL()` runs.
When it reports Wayland, the process environment is aligned
(`XDG_SESSION_TYPE=wayland`) so the loader takes the EGL path.
When SDL reports X11/XWayland, nothing is changed and the GLX
path is used as before.

Workaround for older builds: launch with
`XDG_SESSION_TYPE=wayland` in the environment.

## Separate finding: crash loop after an unclean exit

Unrelated to Wayland (reproduces identically under X11): if a
previous run exited uncleanly, its project lockfile
(`<config>/autosaves/<project>/creator-project.lock`) survives.
The next launch with that project as an argument defers the
load to the restore-save window, leaving the active project's
puppet null — and the toolbar dereferences it
(`creator/widgets/toolbar.d`, `incActivePuppet().enableDrivers`)
and segfaults instead of showing the restore window. The same
null-puppet dereference pattern appears across the UI panels,
so this needs a UI-wide audit rather than a one-site guard; it
is recorded here instead of being patched piecemeal.
Workaround: delete the stale `creator-project.lock` file.
