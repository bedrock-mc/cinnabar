# Personal inventory close response

The server-initiated `ContainerClosePacket` branch invokes the local-player
server-close path with the container type. The response branch instead calls the
local item-stack manager without passing or comparing the response’s window id or type.

Screen close removes the pending screen when the retained screen queue
has more than its base entry. The response payload is not a screen-identity
correlation token.

Cinnabar formerly required a response's id/type to match its personal window.
An otherwise valid response with another type left the window closing; its
timeout then disabled further personal opens for that session. A response now
settles an already transport-admitted personal close independently of those
payload fields. Unsolicited responses during opening/open states or before
transport admission do not close anything. Typed server-initiated closes keep
their identity checks and cursor reconciliation.

Regression coverage runs production keyboard handling with actual `E`/Escape
messages, production inventory ingress, transport admission and three repeated
open/close cycles. It exercises inventory, generic and odd well-formed response
payloads without claiming those are an official BDS's response shapes. It also
checks that a late timeout cannot poison a settled close. The October 2 UTC
offline vanilla-BDS run performs three real E/Escape close/reopen cycles in one
focus interval after the offhand and drop/pickup transactions. The inspected
local `2026-10-02_01.01.27.png` macOS/Metal Retina scale-2 frame shows the
subsequent open and correct final inventory/cursor state.
This is a live functional witness, not full screen-lifecycle parity.
