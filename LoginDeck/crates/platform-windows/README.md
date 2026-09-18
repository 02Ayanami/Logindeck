# LoginDeck Windows platform

This directory is reserved for the future Windows implementation of LoginDeck.

It is intentionally not a Cargo workspace member yet. Windows-specific code should be added and
verified on a Windows development machine before it is wired into `crates/platform-runtime` or
the workspace build.

When implementation begins, keep the same boundary as `platform-macos`:

- implement the platform-neutral contracts from `autologin-core`;
- keep Windows APIs and dependencies in this crate;
- expose the stable facade through `platform-runtime`;
- add target-native tests and a Windows verification workflow only after a real Windows build
  succeeds.

The shared frontend, desktop command layer, native host, and domain logic remain in their current
locations and should not be duplicated here.
