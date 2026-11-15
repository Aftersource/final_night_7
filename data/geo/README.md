# Geodata

`PB2002_boundaries.json`: plate boundaries (fault lines), a GeoJSON FeatureCollection of LineStrings in [lon, lat]
degrees. It's identical to the client's copy (final_night_7_unreal docs/data/, sha256 42b3e087…), so the server and the
client use the same lines.

**Credit (required, ODC-BY 1.0):** "Plate boundaries: Bird, P. (2002), PB2002; GeoJSON by Hugo Ahlenius, Nordpil
(ODC-BY 1.0)."

Use: a mega power cast near a fault line has a stronger effect than one cast far from it (rules, see the DM's rulebook).

`land_mask_1deg.bin`: the land mask (8,100 bytes: 360 × 180 one-degree cells, one bit each, least significant bit
first; row 0 = 90N..89N, column 0 = 180W; bit set = land). Generated from Natural Earth 1:110m land (public domain);
the same bits as the client's `FNLandMask.h` (final_night_7_unreal Source/FinalNight/Private/). Use: the Earth
simulator's base terrain (docs/EARTH_SIM.md). There is no elevation data in either repository: the simulator's coarse
heightmap is generated (land mask + distance to the coast + distance to the PB2002 plate boundaries + hashed noise).
