APEX.drive — Sky, Sand and Road Surface Correction
===================================================

- Sky cloud coordinates use viewing-direction mapping, avoiding stretched radial streaks near the horizon.
- The 2K cumulus layer uses warped multi-scale billow forms, edge erosion and internal shading. The cirrus layer is thinner.
- Sand uses isotropic multi-scale grain, without directional sine bands, with reduced normal strength and tiling.
- Road asphalt also uses isotropic fine aggregate instead of directional waves, with a separate subtle road-normal family.
- Water rendering, vehicle physics and sunrise/sunset curves are unchanged.

Source and archive structure were checked. A local Windows/MSVC OGRE build and in-game visual test are still required.
