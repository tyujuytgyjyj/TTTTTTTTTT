#!/usr/bin/env python3
"""Patch the user's ogre_bridge.cpp in-place after backing it up.

Run from the folder containing ogre_bridge.cpp:
    python apply_cpp_bridge_patch.py
The script is deliberately fail-closed: if an expected source pattern is missing or
ambiguous, it stops rather than making a partial/guessy edit.
"""
from pathlib import Path
import re
import sys

path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("ogre_bridge.cpp")
if not path.exists():
    raise SystemExit(f"Bridge source not found: {path.resolve()}")
text = path.read_text(encoding="utf-8-sig")
original = text
if "ogre_set_texture_filter" in text and "ogre_set_mesh_material" in text and "pointSampler" in text:
    print(f"Bridge already contains the APEX patch: {path.resolve()}")
    raise SystemExit(0)

def sub_once(pattern: str, replacement: str, label: str, flags=re.MULTILINE):
    global text
    text2, n = re.subn(pattern, replacement, text, count=1, flags=flags)
    if n != 1:
        raise SystemExit(f"Patch stopped: expected exactly one match for {label}, found {n}. No file written.")
    text = text2

# Fix a compile error in the pasted source: std::vector needs its element type.
sub_once(r"const std::vector\s*&\s*verts", "const std::vector<BridgeVertex3D>& verts", "populateManualObject vertex vector type")

# Reject invalid texture sizes/context before allocating an Ogre TextureGpu.
sub_once(
    r"OgreContext\* ctx = \(OgreContext\*\)raw_ctx;\s*uint32_t id = ctx->next_tex_id\+\+;",
    "if (!raw_ctx || width == 0 || height == 0 || width > 8192 || height > 8192) return 1;\n"
    "OgreContext* ctx = (OgreContext*)raw_ctx;\n"
    "uint32_t id = ctx->next_tex_id++;",
    "texture create argument validation")

# Texture metadata stores the intended sampler mode.
sub_once(
    r"struct TexInfo\s*\{\s*Ogre::TextureGpu\* gpu = nullptr;\s*bool has_cutout_alpha = false;\s*\};",
    "struct TexInfo {\n    Ogre::TextureGpu* gpu = nullptr;\n    bool has_cutout_alpha = false;\n    bool linear_filter = true;\n};",
    "TexInfo definition")
sub_once(
    r"const Ogre::HlmsSamplerblock\* anisoSampler = nullptr;",
    "const Ogre::HlmsSamplerblock* anisoSampler = nullptr;\n    const Ogre::HlmsSamplerblock* pointSampler = nullptr;",
    "sampler fields")

# The actual filtering selection is honored when the datablock is created.
sub_once(
    r"ctx->textures\[id\]\s*=\s*\{\s*tex,\s*cutout\s*\};",
    "ctx->textures[id] = { tex, cutout, linear_filter };",
    "texture metadata assignment")
sub_once(
    r"db->setTexture\(Ogre::PBSM_DIFFUSE,\s*texIt->second\.gpu,\s*ctx->anisoSampler\);",
    "db->setTexture(Ogre::PBSM_DIFFUSE, texIt->second.gpu,\n            texIt->second.linear_filter ? ctx->anisoSampler : ctx->pointSampler);",
    "PBS texture sampler")
sub_once(
    r"db->setTexture\(0,\s*texIt->second\.gpu\);",
    "db->setTexture(0, texIt->second.gpu,\n            texIt->second.linear_filter ? ctx->anisoSampler : ctx->pointSampler);",
    "Unlit texture sampler")

# Set up a nearest sampler separately; the anisotropic sampler is kept for linear textures.
sub_once(
    r"ctx->anisoSampler\s*=\s*ctx->root->getHlmsManager\(\)->getSamplerblock\(sb\);",
    "ctx->anisoSampler = ctx->root->getHlmsManager()->getSamplerblock(sb);\n"
    "Ogre::HlmsSamplerblock pointSb;\n"
    "pointSb.mMinFilter = Ogre::FO_POINT;\n"
    "pointSb.mMagFilter = Ogre::FO_POINT;\n"
    "pointSb.mMipFilter = Ogre::FO_POINT;\n"
    "pointSb.setAddressingMode(Ogre::TAM_WRAP);\n"
    "ctx->pointSampler = ctx->root->getHlmsManager()->getSamplerblock(pointSb);",
    "point sampler creation")

# Avoid disabling vertical synchronization by default. This is frame pacing, not a cure for geometry bugs.
sub_once(r'params\["vsync"\]\s*=\s*"false"\s*;', 'params["vsync"] = "true";\n    params["vsync_interval"] = "1";', "vsync setting")

# Validate inputs before copying raw FFI ranges.
sub_once(
    r"if \(vcount == 0 \|\| icount < 3\) return 0;",
    "if (!ctx || !verts || !indices || vcount == 0 || icount < 3) return 0;",
    "mesh upload pointer validation")
sub_once(
    r"OgreContext\* ctx = \(OgreContext\*\)raw_ctx;\s*auto it = ctx->meshes\.find\(mesh_id\);\s*if \(it == ctx->meshes\.end\(\) \|\| vcount == 0 \|\| icount < 3\) return;",
    "if (!raw_ctx || !verts || !indices || vcount == 0 || icount < 3) return;\n"
    "OgreContext* ctx = (OgreContext*)raw_ctx;\n"
    "auto it = ctx->meshes.find(mesh_id);\n"
    "if (it == ctx->meshes.end()) return;",
    "mesh update pointer validation")

# Reject null texture updates before dereferencing the context pointer.
sub_once(
    r"OgreContext\* ctx = \(OgreContext\*\)raw_ctx;\s*auto it = ctx->textures\.find\(tex_id\);\s*if \(it == ctx->textures\.end\(\) \|\| !rgba\) return;",
    "if (!raw_ctx || !rgba || width == 0 || height == 0) return;\n"
    "OgreContext* ctx = (OgreContext*)raw_ctx;\n"
    "auto it = ctx->textures.find(tex_id);\n"
    "if (it == ctx->textures.end()) return;",
    "texture update pointer validation")

# Validate each triangle index in ManualObject. A corrupt/invalid source index is skipped, not sent to Ogre.
sub_once(
    r"for \(size_t i = 0; i \+ 2 < indices\.size\(\); i \+= 3\) \{\s*mo->triangle\(indices\[i\], indices\[i \+ 1\], indices\[i \+ 2\]\);\s*\}",
    "for (size_t i = 0; i + 2 < indices.size(); i += 3) {\n"
    "    if (indices[i] >= verts.size() || indices[i + 1] >= verts.size() || indices[i + 2] >= verts.size()) continue;\n"
    "    mo->triangle(indices[i], indices[i + 1], indices[i + 2]);\n"
    "}",
    "manual object triangle bounds")

# Day/night lighting: remove the artificial 10% minimum sun and derive ambient independently of fog tint.
sub_once(
    r"float day = std::fmin\(1\.0f, std::fmax\(0\.10f, \(sun_elev \+ 0\.12f\) \* 2\.2f\)\);\s*ctx->sunLight->setPowerScale\(Ogre::Math::PI \* \(0\.35f \+ 1\.45f \* day\)\);\s*ctx->skyRimLight->setPowerScale\(Ogre::Math::PI \* \(0\.15f \+ 0\.40f \* day\)\);\s*ctx->sceneMgr->setAmbientLight\([\s\S]*?Ogre::Vector3::UNIT_Y\);",
    "float day = std::fmin(1.0f, std::fmax(0.0f, (sun_elev + 0.12f) / 0.42f));\n"
    "float twilight = std::fmax(0.0f, 1.0f - std::fabs(sun_elev) / 0.22f);\n"
    "ctx->sunLight->setPowerScale(Ogre::Math::PI * (0.015f + 1.55f * day + 0.20f * twilight));\n"
    "ctx->skyRimLight->setPowerScale(Ogre::Math::PI * (0.06f + 0.22f * day + 0.08f * twilight));\n"
    "const Ogre::ColourValue ambientTop(0.012f + 0.30f * day + 0.06f * twilight, 0.018f + 0.34f * day + 0.035f * twilight, 0.045f + 0.42f * day);\n"
    "const Ogre::ColourValue ambientBottom(0.006f + 0.12f * day, 0.009f + 0.13f * day, 0.020f + 0.16f * day);\n"
    "ctx->sceneMgr->setAmbientLight(ambientTop, ambientBottom, Ogre::Vector3::UNIT_Y);",
    "day/night light power and ambient")

# Guard invalid transform values; negative scale remains allowed for mirrored geometry.
sub_once(
    r"OgreContext\* ctx = \(OgreContext\*\)raw_ctx;\s*auto it = ctx->meshes\.find\(mesh_id\);\s*if \(it == ctx->meshes\.end\(\) \|\| it->second\.cpu_indices\.size\(\) < 3\) return;",
    "if (!raw_ctx) return;\n"
    "OgreContext* ctx = (OgreContext*)raw_ctx;\n"
    "auto it = ctx->meshes.find(mesh_id);\n"
    "if (it == ctx->meshes.end() || it->second.cpu_indices.size() < 3) return;\n"
    "if (!std::isfinite(px) || !std::isfinite(py) || !std::isfinite(pz) ||\n"
    "    !std::isfinite(qx) || !std::isfinite(qy) || !std::isfinite(qz) || !std::isfinite(qw) ||\n"
    "    !std::isfinite(sx) || !std::isfinite(sy) || !std::isfinite(sz) ||\n"
    "    std::fabs(sx) < 1.0e-6f || std::fabs(sy) < 1.0e-6f || std::fabs(sz) < 1.0e-6f) return;",
    "mesh draw transform validation")

# Add FFI implementations immediately after texture update and mesh vertex update.
texture_fn = r'''
BRIDGE_API void ogre_set_texture_filter(void* raw_ctx, uint32_t tex_id, bool linear_filter) {
    if (!raw_ctx) return;
    OgreContext* ctx = (OgreContext*)raw_ctx;
    auto it = ctx->textures.find(tex_id);
    if (it == ctx->textures.end()) return;
    // Filter mode is applied to datablocks created after this call. The Rust wrapper
    // calls this as soon as a texture is created, before its first draw/use.
    it->second.linear_filter = linear_filter;
}
'''
sub_once(
    r"(BRIDGE_API void ogre_update_texture\([\s\S]*?\n\})\s*\n\s*// -{5,}",
    r"\1\n" + texture_fn + "\n// ---------------------------------------------------------------------------",
    "insert texture filter FFI",
    flags=0)

mesh_fn = r'''
BRIDGE_API void ogre_set_mesh_material(void* raw_ctx, uint32_t mesh_id, uint32_t tex_id, bool is_glass) {
    if (!raw_ctx) return;
    OgreContext* ctx = (OgreContext*)raw_ctx;
    auto it = ctx->meshes.find(mesh_id);
    if (it == ctx->meshes.end()) return;
    CachedMesh& cm = it->second;
    cm.tex_id = tex_id == 0 ? 1 : tex_id;
    cm.is_glass = is_glass;
}
'''
sub_once(
    r"(BRIDGE_API void ogre_update_mesh_vertices\([\s\S]*?\n\})\s*\n\s*// -{5,}",
    r"\1\n" + mesh_fn + "\n// ---------------------------------------------------------------------------",
    "insert mesh material FFI",
    flags=0)

if text == original:
    raise SystemExit("No changes were made; the source may already be patched or does not match the supplied version.")
backup = path.with_suffix(path.suffix + ".before_apex_patch.bak")
if not backup.exists():
    backup.write_text(original, encoding="utf-8")
path.write_text(text, encoding="utf-8")
print(f"Patched {path.resolve()}")
print(f"Backup:  {backup.resolve()}")
print("NOTE: texture filter selection is intended to be set before the first datablock is created.")
