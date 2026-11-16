APEX.drive — GPU Sky and PBR Ground Surfaces
==============================================

Clouds:
- Two high-resolution 2048x1024 tileable alpha maps are generated at startup:
  cumulus and high-altitude cirrus.
- They are sampled in transparent OGRE Unlit passes after the atmospheric sky
  gradient. Their texture details are evaluated by the GPU rather than only
  interpolated from the sky mesh's vertex colours.
- Dawn/dusk gradient curves, sun disc, stars and moon are retained.

Ground and roads:
- Six seamless 512x512 procedural tangent-space normal maps are generated for
  sand, dirt/gravel, grass terrain, terrain asphalt, roads and grass blades.
- OGRE PBS detail normal layers add fine surface relief while the baked albedo
  remains unchanged.
- Terrain families use the following vertex spec tags:
  10 sand, 11 dirt/gravel, 12 grass, 13 asphalt, 14 roads, 15 grass blades.

Physics:
- Surface ID 4 is sand. It deliberately reuses the existing dirt/gravel tyre grip
  and rolling resistance; no vehicle physics parameters were changed.

Water:
- Water rendering, water.rs and the SSR/refraction compositor are intentionally
  unchanged in this upgrade.

Limitations:
- Clouds are textured GPU sky overlays, not volumetric ray-marched clouds.
- PBR details are procedural normals over the existing albedo, not new authored
  4K material/albedo packs.
- The source/archives are structurally checked but were not built with the user's
  local MSVC/OGRE SDK or visually tested in the running game.
