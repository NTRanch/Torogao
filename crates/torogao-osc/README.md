# torogao-osc

The VRChat-dialect OSC parameter surface for Torogao (ruling D-752):
a bounded OSC 1.0 codec, a declared typed surface over
`/avatar/parameters/*`, `/avatar/change`, `/input/*`, and
`/chatbox/*`, and a localhost-first UDP transport whose
rejections are counted, never panicked on. Zero dependencies;
`#![forbid(unsafe_code)]`.

## Dialect contract

The shapes are VRChat's published documentation (read 2026-11-03;
pages updated 2025-11-13):

- https://docs.vrchat.com/docs/osc-overview — ports (UDP 9000 in /
  9001 out, localhost) and OSCQuery discovery.
- https://docs.vrchat.com/docs/osc-avatar-parameters —
  `/avatar/parameters/<name>` with types Int / Bool / Float only;
  `/avatar/change` carries the avatar ID as one string; custom
  parameters read-write, reserved parameters read-only (modelled
  here as declaration direction).
- https://docs.vrchat.com/docs/osc-as-input-controller — axes take
  a float from -1 to 1 and must reset to 0; buttons take int 1
  (pressed) / 0 (released), bool also accepted; the documented
  axis/button name lists are `dialect::INPUT_AXES` /
  `dialect::INPUT_BUTTONS`.
- The wire form is the OSC 1.0 specification (address + type-tag
  string + big-endian arguments, 4-byte alignment).

OSCQuery discovery (the survey's §5 sketch mentioned it) is not
implemented: it is an HTTP+mDNS discovery layer, not part of the
parameter surface, and no consumer needs it yet. It can land as a
separate module if a discovery consumer appears.

## Transport choice: why not rosc

Evaluated 2026-11-03 against the surface's stated requirements.
rosc (0.11.4, crates.io) is dual-licensed MIT / Apache-2.0 —
licence fine. It fails three requirements:

1. **Named decode bounds before allocation.** rosc's decoder
   (docs.rs source, `src/decoder.rs`) enforces no caps on packet
   size, argument count, or address/string length. This estate's
   wire codecs bound all four by named constants.
2. **Adversarial input can never crash the process.** rosc
   decodes bundles by unbounded recursion
   (`decode_bundle` → `read_bundle_element` → `decode_packet`);
   a crafted datagram nesting bundles ~20 bytes per level can
   exhaust the stack. The dialect never sends bundles, so the
   attack surface is pure liability.
3. **Strict datagram semantics.** rosc's `decode_udp` returns
   trailing bytes as a successful remainder; this surface's
   contract is one message per datagram, consumed exactly.

So the codec is hand-rolled to the same bounded pattern as
torogao-vmc's `osc` module (the two remain independent
implementations; consistency of discipline, not shared code).

## Network discipline

- Default bind `127.0.0.1:9000`, default peer `127.0.0.1:9001`.
- A non-loopback bind exists only as an explicitly written
  `OscConfig.bind_addr`; there is no widening flag.
- The receive loop's queue is bounded (`EVENT_QUEUE_CAPACITY =
  256`); overflow drops and counts as `RejectKind::QueueFull`.
- Malformed, oversized, or truncated datagrams drop and count;
  the loop never blocks and never panics on wire content.
