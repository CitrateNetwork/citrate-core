--------------------------- MODULE MCDeviceLinkShare ---------------------------
(* Model for DeviceLinkShare: three members (m3 is the adversary), three devices *)
(* (d1 and d2 owned by m1, d3 owned by m2), at most MaxNet relay messages.       *)
EXTENDS DeviceLinkShare
CONSTANTS m1, m2, m3, d1, d2, d3
OwnerMC == (d1 :> m1) @@ (d2 :> m1) @@ (d3 :> m2)
=============================================================================
