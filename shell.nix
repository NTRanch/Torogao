# Compatibility shim for non-flake nix entry points.
# The flake is the canonical definition; this just re-exports
# its dev shell.
#
# Known quirk (verified on nix 2.35.4 + the pinned nixpkgs):
# legacy `nix-shell` rejects mkShell derivations that carry an
# `env` attribute overlapping stdenv's own, failing with
# "The `env` attribute set cannot contain ... NIX_CFLAGS_COMPILE".
# `nix develop` — the supported entry point — is unaffected and
# is what the build is gated on.
{ }:
let
  flake = builtins.getFlake (toString ./.);
in
flake.devShells.${builtins.currentSystem}.default
