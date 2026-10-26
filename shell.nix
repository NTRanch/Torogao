# Compatibility shim for non-flake nix entry points
# (`nix-shell`, older tooling). The flake is the canonical
# definition; this just re-exports its dev shell.
{ }:
let
  flake = builtins.getFlake (toString ./.);
in
flake.devShells.${builtins.currentSystem}.default
