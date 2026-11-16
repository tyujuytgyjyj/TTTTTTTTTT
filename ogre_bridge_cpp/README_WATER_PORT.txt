APEX.drive — OGRE Next water and SSR integration
=================================================
Uses OGRE-Next HLMS PBS Refractive mode and the OGRE ScreenSpaceReflections
compositor. The official RefractionsWorkspace is retained as fallback.

Required OGRE Samples/Media directories:
- 2.0/scripts/materials/Common and Common/HLSL
- 2.0/scripts/materials/ScreenSpaceReflections and ScreenSpaceReflections/HLSL

Keep water_media beside CMakeLists.txt and build Release x64 with the same
OGRE-Next 3.0 SDK. The DLL path is expected to be <project>/build/Release.

Frame flow:
1. SSR GBuffer/reflections of opaque world objects.
2. Capture opaque scene in rttOpaque.
3. Refractive water uses captured colour + resolved depth in queue 200.
4. APEX HUD is drawn in queue 99 after water.
5. rttFinal is copied to the render window.

The water uses three generated 512x512 seamless normal scales, two animated
PBS detail-normal layers, IOR 1.333, screen-space refraction, three Gerstner
wave bands, Fresnel sky fallback, sun glints and animated shoreline foam.
Dawn, dusk and the sky system remain unchanged.
The full source repository was used read-only; no write was made there.
This source archive excludes build directories and stale DLL/OBJ binaries.
Build and in-game verification on Windows is still required.

APEX.drive — Sky, Sand and Road Surface Correction
===================================================

- Sky cloud coordinates use viewing-direction mapping, avoiding stretched radial streaks near the horizon.
- The 2K cumulus layer uses warped multi-scale billow forms, edge erosion and internal shading. The cirrus layer is thinner.
- Sand uses isotropic multi-scale grain, without directional sine bands, with reduced normal strength and tiling.
- Road asphalt also uses isotropic fine aggregate instead of directional waves, with a separate subtle road-normal family.
- Water rendering, vehicle physics and sunrise/sunset curves are unchanged.

Source and archive structure were checked. A local Windows/MSVC OGRE build and in-game visual test are still required.
