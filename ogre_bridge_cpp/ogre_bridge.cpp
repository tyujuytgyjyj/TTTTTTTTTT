// ============================================================================
// ogre_bridge.cpp  --  نسخة الخطوة 2 من 3 (إضاءة Forza + TRS + Alpha Test)
// تم إصلاح: error C3861 'skyMix' identifier not found (إضافة forward declaration)
// ============================================================================
#include "ogre_bridge.h"

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <windowsx.h>

#include <vector>
#include <unordered_map>
#include <queue>
#include <string>
#include <cmath>
#include <cstring>
#include <algorithm>

#include "OgreAbiUtils.h"
#include "OgreRoot.h"
#include "OgreWindow.h"
#include "OgreWindowEventUtilities.h"
#include "OgreSceneManager.h"
#include "OgreRenderQueue.h"
#include "OgreCamera.h"
#include "OgreManualObject2.h"
#include "Compositor/OgreCompositorManager2.h"
#include "Compositor/OgreCompositorWorkspace.h"
#include "OgreHlmsManager.h"
#include "OgreHlmsPbs.h"
#include "OgreHlmsPbsDatablock.h"
#include "OgreHlmsUnlit.h"
#include "OgreHlmsUnlitDatablock.h"
#include "OgreHlmsSamplerblock.h"
#include "OgreTextureGpuManager.h"
#include "OgreStagingTexture.h"
#include "OgreTextureBox.h"
#include "OgreArchiveManager.h"

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------
struct TexInfo {
    Ogre::TextureGpu* gpu = nullptr;
    bool has_cutout_alpha = false;
    bool linear_filter = true;
};

struct CachedMesh {
    std::vector<BridgeVertex3D> cpu_verts;
    std::vector<uint16_t> cpu_indices;
    uint32_t tex_id = 1;
    float avg_spec = 0.35f;
    float base_a = 1.0f;
    bool is_glass = false;
    bool is_water = false;
    std::vector<BridgeVertex3D> animated_verts;

    std::vector<Ogre::ManualObject*> pool_objs;
    std::vector<Ogre::SceneNode*> pool_nodes;
    std::vector<std::string> pool_db_names;
    size_t used_this_frame = 0;
};

struct UiCmd {
    uint32_t tex_id = 1;
    std::vector<BridgeVertex2D> verts;
    std::vector<uint16_t> indices;
};

struct OgreContext {
    Ogre::Root* root = nullptr;
    Ogre::Window* window = nullptr;
    HWND hwnd = nullptr;
    WNDPROC orig_wndproc = nullptr;

    Ogre::SceneManager* sceneMgr = nullptr;
    Ogre::Camera* camera = nullptr;
    Ogre::SceneNode* camNode = nullptr;
    Ogre::Light* sunLight = nullptr;
    Ogre::Light* skyRimLight = nullptr;
    Ogre::ManualObject* skyManual = nullptr;
    Ogre::SceneNode* skyNode = nullptr;
    Ogre::Vector3 skySunDir = Ogre::Vector3(0.35f, 0.15f, 0.4f);
    float skySunElevation = 0.15f;

    Ogre::ManualObject* uiManual = nullptr;
    Ogre::SceneNode* uiNode = nullptr;
    const Ogre::HlmsSamplerblock* anisoSampler = nullptr;
    const Ogre::HlmsSamplerblock* pointSampler = nullptr;
    Ogre::HlmsPbsDatablock* waterDatablock = nullptr;
    uint32_t waterNormalIds[3] = { 0, 0, 0 };
    bool waterRefractionEnabled = false;

    uint32_t next_tex_id = 1;
    std::unordered_map<uint32_t, TexInfo> textures;
    std::unordered_map<uint64_t, std::string> pbs_cache;
    std::unordered_map<uint32_t, Ogre::HlmsDatablock*> ui_datablocks;

    uint32_t next_mesh_id = 1;
    std::unordered_map<uint32_t, CachedMesh> meshes;

    std::vector<UiCmd> ui_cmds;
    float wheel_delta = 0.0f;
    std::queue<uint32_t> char_queue;
};

static OgreContext* g_active_ctx = nullptr;

// OGRE-Next 3.x uses numeric queue IDs.
// Queue 200 is reserved for the native HLMS refractive-object compositor pass.
static constexpr uint8_t kSkyRenderQueueGroup = 5;
static constexpr uint8_t kWorldRenderQueueGroup = 50;
static constexpr uint8_t kTransparentRenderQueueGroup = 95;
static constexpr uint8_t kUiRenderQueueGroup = 99;
static constexpr uint8_t kWaterRenderQueueGroup = 200;

// ---------------------------------------------------------------------------
// Forward declarations (sky / water helpers defined later in the file)
// ---------------------------------------------------------------------------
static float skyClamp01(float x);
static float skySmoothstep(float a, float b, float x);
static Ogre::Vector3 skyMix(const Ogre::Vector3& a, const Ogre::Vector3& b, float t);
static Ogre::Vector3 waterReflectionColour(const Ogre::Vector3& rayWorld,
                                           const Ogre::Vector3& sunDirWorld,
                                           float sunElevation,
                                           const Ogre::Vector3& observerWorld,
                                           float timeSeconds);

// ---------------------------------------------------------------------------
// WndProc hook
// ---------------------------------------------------------------------------
static LRESULT CALLBACK BridgeWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    if (g_active_ctx && hwnd == g_active_ctx->hwnd) {
        if (msg == WM_MOUSEWHEEL) {
            g_active_ctx->wheel_delta +=
                (float)GET_WHEEL_DELTA_WPARAM(wParam) / (float)WHEEL_DELTA;
        } else if (msg == WM_CHAR) {
            if (wParam >= 32) g_active_ctx->char_queue.push((uint32_t)wParam);
        }
        if (g_active_ctx->orig_wndproc) {
            return CallWindowProcW(g_active_ctx->orig_wndproc, hwnd, msg, wParam, lParam);
        }
    }
    return DefWindowProcW(hwnd, msg, wParam, lParam);
}

// ---------------------------------------------------------------------------
// HLMS registration
// ---------------------------------------------------------------------------
static std::string getBridgeProjectRoot() {
    HMODULE module = nullptr;
    GetModuleHandleExA(
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCSTR>(&getBridgeProjectRoot), &module);
    char modulePath[MAX_PATH] = {};
    const DWORD length = module ? GetModuleFileNameA(module, modulePath, MAX_PATH) : 0;
    if (length == 0 || length >= MAX_PATH) return ".";
    std::string path(modulePath, length);
    size_t slash = path.find_last_of("\\/");
    if (slash == std::string::npos) return ".";
    path.resize(slash);
    for (int i = 0; i < 2; ++i) {
        slash = path.find_last_of("\\/");
        if (slash == std::string::npos) return ".";
        path.resize(slash);
    }
    return path;
}

static void registerHlms(Ogre::Root* root) {
    Ogre::String mediaRoot =
        "C:/biqfr/build_ogre_scripts-v3-0/Ogre/ogre-next/Samples/Media/";

    auto setupHlms = [&](bool isPbs) {
        Ogre::String mainFolderPath;
        Ogre::StringVector libraryFoldersPaths;
        if (isPbs) Ogre::HlmsPbs::getDefaultPaths(mainFolderPath, libraryFoldersPaths);
        else       Ogre::HlmsUnlit::getDefaultPaths(mainFolderPath, libraryFoldersPaths);

        Ogre::Archive* archiveLibrary = Ogre::ArchiveManager::getSingletonPtr()->load(
            mediaRoot + mainFolderPath, "FileSystem", true);

        Ogre::ArchiveVec archiveLibraryFolders;
        for (const auto& lib : libraryFoldersPaths) {
            archiveLibraryFolders.push_back(Ogre::ArchiveManager::getSingletonPtr()->load(
                mediaRoot + lib, "FileSystem", true));
        }

        if (isPbs) {
            Ogre::HlmsPbs* hlmsPbs = OGRE_NEW Ogre::HlmsPbs(archiveLibrary, &archiveLibraryFolders);
            root->getHlmsManager()->registerHlms(hlmsPbs);
        } else {
            Ogre::HlmsUnlit* hlmsUnlit = OGRE_NEW Ogre::HlmsUnlit(archiveLibrary, &archiveLibraryFolders);
            root->getHlmsManager()->registerHlms(hlmsUnlit);
        }
    };

    setupHlms(false);
    setupHlms(true);

    auto& rgm = Ogre::ResourceGroupManager::getSingleton();
    rgm.addResourceLocation(mediaRoot + "2.0/scripts/materials/Common", "FileSystem", "General");
    rgm.addResourceLocation(mediaRoot + "2.0/scripts/materials/Common/HLSL", "FileSystem", "General");
    rgm.addResourceLocation(mediaRoot + "2.0/scripts/materials/ScreenSpaceReflections", "FileSystem", "General");
    rgm.addResourceLocation(mediaRoot + "2.0/scripts/materials/ScreenSpaceReflections/HLSL", "FileSystem", "General");
    const Ogre::String waterMediaRoot = getBridgeProjectRoot() + "/water_media";
    rgm.addResourceLocation(waterMediaRoot, "FileSystem", "General");
    rgm.initialiseAllResourceGroups(true);
}

// ---------------------------------------------------------------------------
// Textures  (قبل ogre_init لتفادي خطأ "identifier not found")
// ---------------------------------------------------------------------------
BRIDGE_API uint32_t ogre_create_texture(void* raw_ctx, uint32_t width, uint32_t height,
                                        const uint8_t* rgba, bool linear_filter) {
    if (!raw_ctx || width == 0 || height == 0 || width > 8192 || height > 8192) return 1;
    OgreContext* ctx = (OgreContext*)raw_ctx;
    uint32_t id = ctx->next_tex_id++;
    std::string name = "ApexTex_" + std::to_string(id);

    // فحص هل النسيج فيه أجزاء شفافة مقصوصة (أوراق الأشجار والعشب والأسوار)
    bool cutout = false;
    if (rgba && width > 4 && height > 4) {
        size_t total = (size_t)width * height;
        size_t step = std::max<size_t>(1, total / 512);
        size_t zero_alpha = 0;
        for (size_t i = 0; i < total; i += step) {
            uint8_t a = rgba[i * 4 + 3];
            if (a < 30) zero_alpha++;
        }
        if (zero_alpha > 3) cutout = true;
    }

    Ogre::TextureGpuManager* texMgr = ctx->root->getRenderSystem()->getTextureGpuManager();
    Ogre::TextureGpu* tex = texMgr->createTexture(
        name, Ogre::GpuPageOutStrategy::Discard,
        Ogre::TextureFlags::ManualTexture, Ogre::TextureTypes::Type2D);

    tex->setResolution(width, height);
    tex->setPixelFormat(Ogre::PFG_RGBA8_UNORM_SRGB);
    tex->scheduleTransitionTo(Ogre::GpuResidency::Resident);

    if (rgba) {
        Ogre::StagingTexture* staging =
            texMgr->getStagingTexture(width, height, 1, 1, Ogre::PFG_RGBA8_UNORM_SRGB);
        staging->startMapRegion();
        Ogre::TextureBox box = staging->mapRegion(width, height, 1, 1, Ogre::PFG_RGBA8_UNORM_SRGB);
        for (uint32_t y = 0; y < height; ++y) {
            memcpy((uint8_t*)box.at(0, y, 0),
                   rgba + (size_t)y * width * 4,
                   (size_t)width * 4);
        }
        staging->stopMapRegion();
        staging->upload(box, tex, 0, nullptr, nullptr, false);
        texMgr->removeStagingTexture(staging);
    }

    ctx->textures[id] = { tex, cutout, linear_filter };
    return id;
}

BRIDGE_API void ogre_update_texture(void* raw_ctx, uint32_t tex_id,
                                    uint32_t width, uint32_t height,
                                    const uint8_t* rgba) {
    if (!raw_ctx || !rgba || width == 0 || height == 0) return;
    OgreContext* ctx = (OgreContext*)raw_ctx;
    auto it = ctx->textures.find(tex_id);
    if (it == ctx->textures.end()) return;

    Ogre::TextureGpu* tex = it->second.gpu;
    Ogre::TextureGpuManager* texMgr = ctx->root->getRenderSystem()->getTextureGpuManager();

    if (tex->getWidth() != width || tex->getHeight() != height) {
        const uint32_t old_width = tex->getWidth();
        const uint32_t old_height = tex->getHeight();

        // A font atlas may grow in the middle of a frame. Earlier text batches
        // already queued for this texture still contain UVs normalized to the
        // old dimensions. Rescale those queued UVs before changing the texture,
        // otherwise the earlier labels can sample the wrong glyph for one frame.
        const float u_scale = static_cast<float>(old_width) / static_cast<float>(width);
        const float v_scale = static_cast<float>(old_height) / static_cast<float>(height);
        for (UiCmd& ui_cmd : ctx->ui_cmds) {
            if (ui_cmd.tex_id != tex_id) continue;
            for (BridgeVertex2D& vertex : ui_cmd.verts) {
                vertex.u *= u_scale;
                vertex.v *= v_scale;
            }
        }

        tex->scheduleTransitionTo(Ogre::GpuResidency::OnStorage);
        tex->setResolution(width, height);
        tex->scheduleTransitionTo(Ogre::GpuResidency::Resident);
    }

    Ogre::StagingTexture* staging =
        texMgr->getStagingTexture(width, height, 1, 1, Ogre::PFG_RGBA8_UNORM_SRGB);
    staging->startMapRegion();
    Ogre::TextureBox box = staging->mapRegion(width, height, 1, 1, Ogre::PFG_RGBA8_UNORM_SRGB);
    for (uint32_t y = 0; y < height; ++y) {
        memcpy((uint8_t*)box.at(0, y, 0),
               rgba + (size_t)y * width * 4,
               (size_t)width * 4);
    }
    staging->stopMapRegion();
    staging->upload(box, tex, 0, nullptr, nullptr, false);
    texMgr->removeStagingTexture(staging);
}

BRIDGE_API void ogre_set_texture_filter(void* raw_ctx, uint32_t tex_id, bool linear_filter) {
    if (!raw_ctx) return;
    OgreContext* ctx = (OgreContext*)raw_ctx;
    auto it = ctx->textures.find(tex_id);
    if (it == ctx->textures.end()) return;
    it->second.linear_filter = linear_filter;
}

static uint32_t createProceduralWaterNormal(OgreContext* ctx, const char* textureName, int variant) {
    if (!ctx || !ctx->root || !textureName) return 0;
    constexpr uint32_t width = 512, height = 512;
    constexpr float tau = 6.2831853071795864769f;
    struct NormalWave { float kx, ky, amplitude, phase; };
    static const NormalWave waveSets[3][8] = {
        {
            {2,3,0.070f,0.10f},{5,-3,0.045f,1.70f},{7,6,0.028f,2.30f},{9,-7,0.018f,0.80f},
            {4,11,0.020f,2.80f},{13,5,0.012f,1.20f},{15,-9,0.008f,0.45f},{8,17,0.006f,2.10f}
        },
        {
            {5,8,0.050f,0.30f},{11,-7,0.030f,1.30f},{17,9,0.020f,2.20f},{21,-13,0.014f,0.75f},
            {9,23,0.013f,2.75f},{27,5,0.010f,1.80f},{31,-17,0.007f,0.10f},{15,33,0.006f,2.55f}
        },
        {
            {13,17,0.028f,0.50f},{23,-19,0.020f,1.10f},{31,11,0.014f,2.00f},{39,-27,0.010f,0.35f},
            {17,43,0.009f,2.65f},{47,21,0.007f,1.50f},{53,-31,0.005f,0.95f},{29,57,0.004f,2.35f}
        }
    };
    const NormalWave* waves = waveSets[std::max(0, std::min(variant, 2))];
    const float strength = variant == 0 ? 0.085f : (variant == 1 ? 0.040f : 0.028f);
    std::vector<uint8_t> pixels(static_cast<size_t>(width) * height * 4u);
    for (uint32_t y = 0; y < height; ++y) {
        const float v = static_cast<float>(y) / static_cast<float>(height);
        for (uint32_t x = 0; x < width; ++x) {
            const float u = static_cast<float>(x) / static_cast<float>(width);
            float dhdu = 0.0f, dhdv = 0.0f;
            for (int i = 0; i < 8; ++i) {
                const NormalWave& w = waves[i];
                const float phase = tau * (w.kx * u + w.ky * v) + w.phase;
                const float slope = w.amplitude * std::cos(phase) * tau;
                dhdu += slope * w.kx;
                dhdv += slope * w.ky;
            }
            Ogre::Vector3 n(-dhdu * strength, -dhdv * strength, 1.0f);
            n.normalise();
            const size_t at = (static_cast<size_t>(y) * width + x) * 4u;
            pixels[at+0] = static_cast<uint8_t>((n.x * 0.5f + 0.5f) * 255.0f);
            pixels[at+1] = static_cast<uint8_t>((n.y * 0.5f + 0.5f) * 255.0f);
            pixels[at+2] = static_cast<uint8_t>((n.z * 0.5f + 0.5f) * 255.0f);
            pixels[at+3] = 255;
        }
    }
    try {
        Ogre::TextureGpuManager* texMgr = ctx->root->getRenderSystem()->getTextureGpuManager();
        Ogre::TextureGpu* tex = texMgr->createTexture(
            textureName, Ogre::GpuPageOutStrategy::Discard,
            Ogre::TextureFlags::ManualTexture, Ogre::TextureTypes::Type2D);
        tex->setResolution(width, height);
        tex->setPixelFormat(Ogre::PFG_RGBA8_UNORM); // linear normal data
        tex->scheduleTransitionTo(Ogre::GpuResidency::Resident);
        Ogre::StagingTexture* staging =
            texMgr->getStagingTexture(width, height, 1, 1, Ogre::PFG_RGBA8_UNORM);
        staging->startMapRegion();
        Ogre::TextureBox box =
            staging->mapRegion(width, height, 1, 1, Ogre::PFG_RGBA8_UNORM);
        for (uint32_t y = 0; y < height; ++y)
            memcpy(static_cast<uint8_t*>(box.at(0,y,0)),
                   pixels.data() + static_cast<size_t>(y) * width * 4u,
                   static_cast<size_t>(width) * 4u);
        staging->stopMapRegion();
        staging->upload(box, tex, 0, nullptr, nullptr, false);
        texMgr->removeStagingTexture(staging);
        const uint32_t id = ctx->next_tex_id++;
        ctx->textures[id] = { tex, false, true };
        return id;
    } catch (...) { return 0; }
}

static void createWaterPbsMaterial(OgreContext* ctx) {
    if (!ctx || !ctx->root) return;
    Ogre::HlmsPbs* pbs = static_cast<Ogre::HlmsPbs*>(
        ctx->root->getHlmsManager()->getHlms(Ogre::HLMS_PBS));
    if (!pbs) return;
    ctx->waterNormalIds[0] = createProceduralWaterNormal(ctx, "ApexWaterNormal_Macro", 0);
    ctx->waterNormalIds[1] = createProceduralWaterNormal(ctx, "ApexWaterNormal_Detail", 1);
    ctx->waterNormalIds[2] = createProceduralWaterNormal(ctx, "ApexWaterNormal_Micro", 2);

    Ogre::HlmsMacroblock macro;
    macro.mDepthCheck = true;
    macro.mDepthWrite = false;
    macro.mCullMode = Ogre::CULL_NONE;
    Ogre::HlmsBlendblock blend;
    ctx->waterDatablock = static_cast<Ogre::HlmsPbsDatablock*>(
        pbs->createDatablock("ApexWaterPbr", "ApexWaterPbr", macro, blend, Ogre::HlmsParamVec()));
    if (!ctx->waterDatablock) return;

    Ogre::HlmsPbsDatablock* db = ctx->waterDatablock;
    db->setWorkflow(Ogre::HlmsPbsDatablock::SpecularAsFresnelWorkflow);
    db->setDiffuse(Ogre::Vector3(0.075f, 0.30f, 0.37f));
    db->setSpecular(Ogre::Vector3(1.0f, 1.0f, 1.0f));
    db->setIndexOfRefraction(Ogre::Vector3(1.333f, 1.333f, 1.333f), false);
    db->setRoughness(0.055f);
    db->setNormalMapWeight(1.0f);
    db->setTwoSidedLighting(true, false);
    db->setTransparency(
        0.52f,
        ctx->waterRefractionEnabled ? Ogre::HlmsPbsDatablock::Refractive
                                   : Ogre::HlmsPbsDatablock::Transparent,
        false, true);
    db->setRefractionStrength(0.105f);

    auto getTexture = [&](uint32_t id) -> Ogre::TextureGpu* {
        const auto it = ctx->textures.find(id);
        return it == ctx->textures.end() ? nullptr : it->second.gpu;
    };
    Ogre::TextureGpu* macroNormal = getTexture(ctx->waterNormalIds[0]);
    Ogre::TextureGpu* detailNormal = getTexture(ctx->waterNormalIds[1]);
    Ogre::TextureGpu* microNormal = getTexture(ctx->waterNormalIds[2]);
    if (macroNormal) db->setTexture(Ogre::PBSM_NORMAL, macroNormal, ctx->anisoSampler);
    if (detailNormal) {
        db->setTexture(Ogre::PBSM_DETAIL0_NM, detailNormal, ctx->anisoSampler);
        db->setDetailMapWeight(0, 1.0f);
        db->setDetailNormalWeight(0, 0.72f);
        db->setDetailMapOffsetScale(0, Ogre::Vector4(0.0f,0.0f,72.0f,72.0f));
    }
    if (microNormal) {
        db->setTexture(Ogre::PBSM_DETAIL1_NM, microNormal, ctx->anisoSampler);
        db->setDetailMapWeight(1, 1.0f);
        db->setDetailNormalWeight(1, 0.32f);
        db->setDetailMapOffsetScale(1, Ogre::Vector4(0.0f,0.0f,210.0f,210.0f));
    }
}

static void updateWaterPbsAnimation(OgreContext* ctx) {
    if (!ctx || !ctx->waterDatablock) return;
    const float t = static_cast<float>(GetTickCount64()) * 0.001f;
    ctx->waterDatablock->setDetailMapOffsetScale(
        0, Ogre::Vector4(std::fmod(t*0.012f,1.0f),std::fmod(t*0.007f,1.0f),72.0f,72.0f));
    ctx->waterDatablock->setDetailMapOffsetScale(
        1, Ogre::Vector4(std::fmod(-t*0.008f+1.0f,1.0f),std::fmod(t*0.010f,1.0f),210.0f,210.0f));
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------
BRIDGE_API void* ogre_init(const char* title, int width, int height, int msaa) {
    OgreContext* ctx = new OgreContext();
    g_active_ctx = ctx;

    const Ogre::AbiCookie abiCookie = Ogre::generateAbiCookie();
    ctx->root = new Ogre::Root(&abiCookie, "plugins.cfg", "ogre.cfg", "Ogre.log", title);

    Ogre::RenderSystem* rs = ctx->root->getRenderSystemByName("Direct3D11 Rendering Subsystem");
    if (!rs) {
        const Ogre::RenderSystemList& list = ctx->root->getAvailableRenderers();
        if (!list.empty()) rs = list.front();
    }
    ctx->root->setRenderSystem(rs);
    ctx->root->initialise(false);

    Ogre::NameValuePairList params;
    params["vsync"] = "true";
    params["vsync_interval"] = "1";
    params["gamma"] = "true";
    params["FSAA"]  = msaa > 1 ? std::to_string(msaa) : "0";
    ctx->window = ctx->root->createRenderWindow(title, width, height, false, &params);

    ctx->window->getCustomAttribute("WINDOW", &ctx->hwnd);
    if (ctx->hwnd) {
        ctx->orig_wndproc =
            (WNDPROC)SetWindowLongPtrW(ctx->hwnd, GWLP_WNDPROC, (LONG_PTR)BridgeWndProc);
    }
    registerHlms(ctx->root);

    // --- Anisotropic sampler للطرق والأسفلت ---
    Ogre::HlmsSamplerblock sb;
    sb.mMinFilter = Ogre::FO_ANISOTROPIC;
    sb.mMagFilter = Ogre::FO_ANISOTROPIC;
    sb.mMipFilter = Ogre::FO_LINEAR;
    sb.mMaxAnisotropy = 8.0f;
    sb.setAddressingMode(Ogre::TAM_WRAP);
    ctx->anisoSampler = ctx->root->getHlmsManager()->getSamplerblock(sb);
    Ogre::HlmsSamplerblock pointSb;
    pointSb.mMinFilter = Ogre::FO_POINT;
    pointSb.mMagFilter = Ogre::FO_POINT;
    pointSb.mMipFilter = Ogre::FO_POINT;
    pointSb.setAddressingMode(Ogre::TAM_WRAP);
    ctx->pointSampler = ctx->root->getHlmsManager()->getSamplerblock(pointSb);

    // --- المشهد والكاميرا (الكاميرا مربوطة بعقدة camNode) ---
    ctx->sceneMgr = ctx->root->createSceneManager(Ogre::ST_GENERIC, 2, "ApexMainScene");
    ctx->camera = ctx->sceneMgr->createCamera("MainCam");
    ctx->camera->detachFromParent();
    ctx->camNode = ctx->sceneMgr->getRootSceneNode(Ogre::SCENE_DYNAMIC)
        ->createChildSceneNode(Ogre::SCENE_DYNAMIC);
    ctx->camNode->attachObject(ctx->camera);
    ctx->camera->setNearClipDistance(0.12f);
    ctx->camera->setFarClipDistance(2200.0f);
    ctx->camera->setAutoAspectRatio(true);

    // --- الإضاءة ---
    ctx->sceneMgr->setAmbientLight(
        Ogre::ColourValue(0.45f, 0.58f, 0.78f),
        Ogre::ColourValue(0.28f, 0.25f, 0.22f),
        Ogre::Vector3::UNIT_Y);

    // الشمس الرئيسية (دافئة)
    ctx->sunLight = ctx->sceneMgr->createLight();
    Ogre::SceneNode* sunNode = ctx->sceneMgr->getRootSceneNode()->createChildSceneNode();
    sunNode->attachObject(ctx->sunLight);
    ctx->sunLight->setType(Ogre::Light::LT_DIRECTIONAL);
    ctx->sunLight->setDiffuseColour(1.0f, 0.94f, 0.84f);
    ctx->sunLight->setSpecularColour(1.0f, 0.96f, 0.90f);
    ctx->sunLight->setDirection(Ogre::Vector3(-0.5f, -0.8f, -0.3f).normalisedCopy());
    ctx->sunLight->setPowerScale(Ogre::Math::PI * 1.65f);

    // إضاءة الحواف (Rim / Sky Fill)
    ctx->skyRimLight = ctx->sceneMgr->createLight();
    Ogre::SceneNode* rimNode = ctx->sceneMgr->getRootSceneNode()->createChildSceneNode();
    rimNode->attachObject(ctx->skyRimLight);
    ctx->skyRimLight->setType(Ogre::Light::LT_DIRECTIONAL);
    ctx->skyRimLight->setDiffuseColour(0.55f, 0.72f, 1.0f);
    ctx->skyRimLight->setSpecularColour(0.65f, 0.80f, 1.0f);
    ctx->skyRimLight->setDirection(Ogre::Vector3(0.5f, -0.35f, 0.6f).normalisedCopy());
    ctx->skyRimLight->setPowerScale(Ogre::Math::PI * 0.45f);

    // --- طبقة الواجهة: ابن مباشر لعقدة الكاميرا ---
    ctx->uiManual = ctx->sceneMgr->createManualObject(Ogre::SCENE_DYNAMIC);
    ctx->uiManual->setRenderQueueGroup(kUiRenderQueueGroup);
    // Overlay queue: keep alpha-blended UI after the 3D scene and preserve submission order.
    ctx->sceneMgr->getRenderQueue()->setSortRenderQueue(kUiRenderQueueGroup, Ogre::RenderQueue::DisableSort);
    ctx->uiManual->setCastShadows(false);
    ctx->uiManual->setLocalAabb(Ogre::Aabb::BOX_INFINITE);
    ctx->uiNode = ctx->camNode->createChildSceneNode(Ogre::SCENE_DYNAMIC);
    ctx->uiNode->attachObject(ctx->uiManual);

    // Procedural camera-following sky gradient. The Macroquad sky shader is not
    // executed by the Ogre bridge, so this supplies the missing real sky pass.
    Ogre::HlmsUnlit* skyUnlit = (Ogre::HlmsUnlit*)ctx->root->getHlmsManager()->getHlms(Ogre::HLMS_UNLIT);
    Ogre::HlmsMacroblock skyMacro;
    skyMacro.mDepthCheck = false;
    skyMacro.mDepthWrite = false;
    skyMacro.mCullMode = Ogre::CULL_NONE;
    Ogre::HlmsBlendblock skyBlend;
    skyUnlit->createDatablock("ApexSkyGradient", "ApexSkyGradient", skyMacro, skyBlend, Ogre::HlmsParamVec());
    ctx->skyManual = ctx->sceneMgr->createManualObject(Ogre::SCENE_DYNAMIC);
    ctx->skyManual->setRenderQueueGroup(kSkyRenderQueueGroup);

    // Dedicated opaque water surface. Vertex colours carry Fresnel/sky reflection
    // and sun glints; no generic PBS transparency can bleach the terrain below.
    Ogre::HlmsMacroblock waterMacro;
    waterMacro.mDepthCheck = true;
    waterMacro.mDepthWrite = true;
    waterMacro.mCullMode = Ogre::CULL_NONE;
    Ogre::HlmsBlendblock waterBlend;
    skyUnlit->createDatablock(
        "ApexWaterSurface", "ApexWaterSurface", waterMacro, waterBlend, Ogre::HlmsParamVec());
    ctx->skyManual->setCastShadows(false);
    ctx->skyManual->setLocalAabb(Ogre::Aabb::BOX_INFINITE);
    ctx->skyNode = ctx->camNode->createChildSceneNode(Ogre::SCENE_DYNAMIC);
    ctx->skyNode->attachObject(ctx->skyManual);
    ctx->sceneMgr->getRenderQueue()->setSortRenderQueue(kSkyRenderQueueGroup, Ogre::RenderQueue::DisableSort);

    // --- Official OGRE screen-space refraction workspace ---
    Ogre::CompositorManager2* compMgr = ctx->root->getCompositorManager2();
    const Ogre::String highQualityWaterWs = msaa > 1
        ? "ApexWaterSSRWorkspaceMsaa" : "ApexWaterSSRWorkspace";
    const Ogre::String refractionFallbackWs = msaa > 1
        ? "RefractionsWorkspaceMsaa" : "RefractionsWorkspace";
    if (compMgr->hasWorkspaceDefinition(highQualityWaterWs)) {
        compMgr->addWorkspace(ctx->sceneMgr, ctx->window->getTexture(), ctx->camera, highQualityWaterWs, true);
        ctx->waterRefractionEnabled = true;
    } else if (compMgr->hasWorkspaceDefinition(refractionFallbackWs)) {
        compMgr->addWorkspace(ctx->sceneMgr, ctx->window->getTexture(), ctx->camera, refractionFallbackWs, true);
        ctx->waterRefractionEnabled = true;
    } else {
        const Ogre::String fallbackWs = "ApexBasicWS";
        if (!compMgr->hasWorkspaceDefinition(fallbackWs))
            compMgr->createBasicWorkspaceDef(fallbackWs, Ogre::ColourValue(0.006f,0.012f,0.028f,1.0f), Ogre::IdString());
        compMgr->addWorkspace(ctx->sceneMgr, ctx->window->getTexture(), ctx->camera, fallbackWs, true);
        ctx->waterRefractionEnabled = false;
    }
    uint32_t white_pixel = 0xFFFFFFFF;
    ogre_create_texture(ctx, 1, 1, (const uint8_t*)&white_pixel, false);
    createWaterPbsMaterial(ctx);
    return ctx;
}

// ---------------------------------------------------------------------------
// Meshes (ManualObject V2)
// ---------------------------------------------------------------------------
static void populateManualObject(
    Ogre::ManualObject* mo,
    const std::string& dbName,
    const std::vector<BridgeVertex3D>& verts,
    const std::vector<uint16_t>& indices,
    bool addWaterTangents)
{
    mo->clear();
    if (verts.empty() || indices.size() < 3) return;
    mo->begin(dbName, Ogre::OT_TRIANGLE_LIST);
    Ogre::Vector3 bmin(1e9f, 1e9f, 1e9f);
    Ogre::Vector3 bmax(-1e9f, -1e9f, -1e9f);
    for (size_t i = 0; i < verts.size(); ++i) {
        const BridgeVertex3D& v = verts[i];
        mo->position(v.px, v.py, v.pz);
        Ogre::Vector3 normal(v.nx, v.ny, v.nz);
        if (normal.squaredLength() < 1.0e-8f) normal = Ogre::Vector3::UNIT_Y;
        else normal.normalise();
        mo->normal(normal);
        if (addWaterTangents) {
            Ogre::Vector3 tangent = Ogre::Vector3::UNIT_X - normal * normal.dotProduct(Ogre::Vector3::UNIT_X);
            if (tangent.squaredLength() < 1.0e-6f)
                tangent = Ogre::Vector3::UNIT_Z - normal * normal.dotProduct(Ogre::Vector3::UNIT_Z);
            tangent.normalise();
            mo->tangent(tangent);
        }
        mo->textureCoord(v.u, v.v);
        mo->colour(v.r / 255.0f, v.g / 255.0f, v.b / 255.0f, v.a / 255.0f);
        bmin.makeFloor(Ogre::Vector3(v.px, v.py, v.pz));
        bmax.makeCeil(Ogre::Vector3(v.px, v.py, v.pz));
    }
    for (size_t i = 0; i + 2 < indices.size(); i += 3) {
        if (indices[i] >= verts.size() || indices[i + 1] >= verts.size() || indices[i + 2] >= verts.size()) continue;
        mo->triangle(indices[i], indices[i + 1], indices[i + 2]);
    }
    mo->end();
    Ogre::Aabb aabb = Ogre::Aabb::newFromExtents(bmin - Ogre::Vector3(0.25f),
                                                 bmax + Ogre::Vector3(0.25f));
    mo->setLocalAabb(aabb);
}

BRIDGE_API uint32_t ogre_upload_mesh(
    void* raw_ctx,
    const BridgeVertex3D* verts, uint32_t vcount,
    const uint16_t* indices, uint32_t icount,
    uint32_t tex_id, bool is_glass, bool is_water)
{
    OgreContext* ctx = (OgreContext*)raw_ctx;
    if (!ctx || !verts || !indices || vcount == 0 || icount < 3) return 0;
    uint32_t id = ctx->next_mesh_id++;
    CachedMesh cm;
    cm.cpu_verts.assign(verts, verts + vcount);
    cm.cpu_indices.assign(indices, indices + icount);
    cm.tex_id   = (tex_id == 0) ? 1 : tex_id;
    cm.is_glass = is_glass;
    cm.is_water = is_water;
    if (is_water) cm.animated_verts = cm.cpu_verts;
    cm.avg_spec = verts->spec;
    cm.base_a   = verts->a / 255.0f;
    ctx->meshes[id] = std::move(cm);
    return id;
}

BRIDGE_API void ogre_update_mesh_vertices(
    void* raw_ctx, uint32_t mesh_id,
    const BridgeVertex3D* verts, uint32_t vcount,
    const uint16_t* indices, uint32_t icount)
{
    if (!raw_ctx || !verts || !indices || vcount == 0 || icount < 3) return;
    OgreContext* ctx = (OgreContext*)raw_ctx;
    auto it = ctx->meshes.find(mesh_id);
    if (it == ctx->meshes.end()) return;
    CachedMesh& cm = it->second;
    cm.cpu_verts.assign(verts, verts + vcount);
    cm.cpu_indices.assign(indices, indices + icount);
    if (cm.is_water) cm.animated_verts = cm.cpu_verts;
    cm.avg_spec = verts->spec;
    for (size_t i = 0; i < cm.pool_objs.size(); ++i) {
        populateManualObject(cm.pool_objs[i], cm.pool_db_names[i], cm.cpu_verts, cm.cpu_indices, cm.is_water);
    }
}

BRIDGE_API void ogre_set_mesh_material(void* raw_ctx, uint32_t mesh_id, uint32_t tex_id, bool is_glass, bool is_water) {
    if (!raw_ctx) return;
    OgreContext* ctx = (OgreContext*)raw_ctx;
    auto it = ctx->meshes.find(mesh_id);
    if (it == ctx->meshes.end()) return;
    CachedMesh& cm = it->second;
    cm.tex_id = tex_id == 0 ? 1 : tex_id;
    cm.is_glass = is_glass;
    cm.is_water = is_water;
}

// ---------------------------------------------------------------------------
// PBS datablocks
// ---------------------------------------------------------------------------
static std::string getOrCreatePbsDatablockName(
    OgreContext* ctx, uint32_t tex_id,
    float tr, float tg, float tb,
    float spec, bool glass, bool water, float alpha)
{
    uint32_t qr = (uint32_t)(tr * 63.0f) & 63;
    uint32_t qg = (uint32_t)(tg * 63.0f) & 63;
    uint32_t qb = (uint32_t)(tb * 63.0f) & 63;
    uint32_t qs = (uint32_t)((spec + 2.0f) * 20.0f) & 255;
    uint64_t key = ((uint64_t)tex_id << 32) |
        ((uint64_t)qr << 24) | ((uint64_t)qg << 18) |
        ((uint64_t)qb << 12) | ((uint64_t)qs << 2)  |
        (glass ? 1ULL : 0ULL) |
        (water ? 2ULL : 0ULL);
    auto it = ctx->pbs_cache.find(key);
    if (it != ctx->pbs_cache.end()) return it->second;

    auto texIt = ctx->textures.find(tex_id == 0 ? 1 : tex_id);
    bool is_cutout = (texIt != ctx->textures.end()) ? texIt->second.has_cutout_alpha : false;
    Ogre::HlmsPbs* hlmsPbs =
        (Ogre::HlmsPbs*)ctx->root->getHlmsManager()->getHlms(Ogre::HLMS_PBS);
    Ogre::HlmsMacroblock macro;
    Ogre::HlmsBlendblock blend;
    macro.mCullMode = Ogre::CULL_NONE;
    if ((glass && !is_cutout) || (water && !is_cutout)) {
        macro.mDepthWrite = false;
        blend.setBlendType(Ogre::SBT_TRANSPARENT_ALPHA);
    }
    std::string dbName = "ApexPBS_" + std::to_string(key);
    Ogre::HlmsPbsDatablock* db = (Ogre::HlmsPbsDatablock*)hlmsPbs->createDatablock(
        dbName, dbName, macro, blend, Ogre::HlmsParamVec());

    if (water && !is_cutout) {
        // Water has a dark teal body, dielectric Fresnel and a sharp highlight.
        db->setWorkflow(Ogre::HlmsPbsDatablock::SpecularAsFresnelWorkflow);
        db->setDiffuse(Ogre::Vector3(0.22f, 0.43f, 0.56f));
        db->setSpecular(Ogre::Vector3(0.96f, 0.985f, 1.0f));
        db->setFresnel(Ogre::Vector3(0.035f, 0.035f, 0.035f), false);
        db->setRoughness(0.065f);
        db->setTransparency(0.96f, Ogre::HlmsPbsDatablock::Transparent, false, false);
        db->setTwoSidedLighting(true, false);
    } else if (spec > 2.0f && spec < 2.5f) {
        // Automotive metallic base coat with Ogre PBS clear-coat layer.
        db->setWorkflow(Ogre::HlmsPbsDatablock::MetallicWorkflow);
        db->setDiffuse(Ogre::Vector3(tr * 0.92f, tg * 0.92f, tb * 0.92f));
        db->setMetalness(0.80f);
        db->setRoughness(0.22f);
        db->setClearCoat(1.0f);
        db->setClearCoatRoughness(0.06f);
    } else if (glass && !is_cutout) {
        // زجاج السيارات
        db->setWorkflow(Ogre::HlmsPbsDatablock::SpecularAsFresnelWorkflow);
        db->setDiffuse(Ogre::Vector3(0.06f, 0.09f, 0.13f));
        db->setSpecular(Ogre::Vector3(0.95f, 0.98f, 1.0f));
        db->setRoughness(0.03f);
        db->setFresnel(Ogre::Vector3(0.35f, 0.35f, 0.35f), false);
        db->setTransparency(0.42f, Ogre::HlmsPbsDatablock::Transparent);
    } else if (spec < 0.0f) {
        // مطاط الإطارات
        db->setWorkflow(Ogre::HlmsPbsDatablock::MetallicWorkflow);
        db->setDiffuse(Ogre::Vector3(tr * 0.85f, tg * 0.85f, tb * 0.85f));
        db->setMetalness(0.04f);
        db->setRoughness(0.68f);
    } else if (spec > 0.85f && spec <= 0.95f) {
        // جنوط كروم وألمنيوم ومصابيح
        db->setWorkflow(Ogre::HlmsPbsDatablock::MetallicWorkflow);
        db->setDiffuse(Ogre::Vector3(tr, tg, tb));
        db->setMetalness(0.92f);
        db->setRoughness(0.10f);
    } else {
        // أسفلت ومبانٍ وأعشاب وأشجار
        db->setWorkflow(Ogre::HlmsPbsDatablock::MetallicWorkflow);
        db->setDiffuse(Ogre::Vector3(tr, tg, tb));
        db->setMetalness(0.04f);
        db->setRoughness(is_cutout ? 0.72f : 0.55f);
    }

    // Alpha Test للعشب وأوراق الشجر
    if (is_cutout) {
        db->setAlphaTest(Ogre::CMPF_GREATER_EQUAL);
        db->setAlphaTestThreshold(0.35f);
    }
    if (texIt != ctx->textures.end() && tex_id > 1) {
        db->setTexture(Ogre::PBSM_DIFFUSE, texIt->second.gpu,
                       texIt->second.linear_filter ? ctx->anisoSampler : ctx->pointSampler);
    }
    ctx->pbs_cache[key] = dbName;
    return dbName;
}

// ---------------------------------------------------------------------------
// Drawing 3D (TRS: موضع + دوران كواتيرنيون + مقياس)
// ---------------------------------------------------------------------------
BRIDGE_API void ogre_draw_mesh_trs(
    void* raw_ctx, uint32_t mesh_id,
    float px, float py, float pz,
    float qx, float qy, float qz, float qw,
    float sx, float sy, float sz,
    float tint_r, float tint_g, float tint_b,
    bool is_glass)
{
    if (!raw_ctx) return;
    OgreContext* ctx = (OgreContext*)raw_ctx;
    auto it = ctx->meshes.find(mesh_id);
    if (it == ctx->meshes.end() || it->second.cpu_indices.size() < 3) return;
    if (!std::isfinite(px) || !std::isfinite(py) || !std::isfinite(pz) ||
        !std::isfinite(qx) || !std::isfinite(qy) || !std::isfinite(qz) || !std::isfinite(qw) ||
        !std::isfinite(sx) || !std::isfinite(sy) || !std::isfinite(sz) ||
        std::fabs(sx) < 1.0e-6f || std::fabs(sy) < 1.0e-6f || std::fabs(sz) < 1.0e-6f) return;
    CachedMesh& cm = it->second;

    // Water has its own reflective vertex-colour pass; it must not use the generic
    // PBS alpha material that was washing out the ground under incorrect water cells.
    std::string dbName = cm.is_water
        ? std::string("ApexWaterPbr")
        : getOrCreatePbsDatablockName(
            ctx, cm.tex_id, tint_r, tint_g, tint_b,
            cm.avg_spec, is_glass || cm.is_glass, false, cm.base_a);

    size_t idx = cm.used_this_frame++;
    if (idx >= cm.pool_objs.size()) {
        Ogre::ManualObject* newObj = ctx->sceneMgr->createManualObject(Ogre::SCENE_DYNAMIC);
        newObj->setRenderQueueGroup(kWorldRenderQueueGroup);
        populateManualObject(newObj, dbName, cm.cpu_verts, cm.cpu_indices, cm.is_water);
        Ogre::SceneNode* newNode = ctx->sceneMgr->getRootSceneNode(Ogre::SCENE_DYNAMIC)
            ->createChildSceneNode(Ogre::SCENE_DYNAMIC);
        newNode->attachObject(newObj);
        cm.pool_objs.push_back(newObj);
        cm.pool_nodes.push_back(newNode);
        cm.pool_db_names.push_back(dbName);
    } else if (cm.pool_db_names[idx] != dbName) {
        cm.pool_objs[idx]->setDatablock(0, dbName);
        cm.pool_db_names[idx] = dbName;
    }
    Ogre::ManualObject* mo   = cm.pool_objs[idx];
    Ogre::SceneNode*    node = cm.pool_nodes[idx];

    // Transparent meshes must be rendered after opaque world geometry but before UI.
    const bool is_transparent = cm.is_water || is_glass || cm.is_glass;
    const uint8_t queueGroup = cm.is_water && ctx->waterRefractionEnabled
        ? kWaterRenderQueueGroup
        : (is_transparent ? kTransparentRenderQueueGroup : kWorldRenderQueueGroup);
    mo->setRenderQueueGroup(queueGroup);
    mo->setCastShadows(!is_transparent);
    mo->setVisible(true);

    if (cm.is_water) {
        const float waveTime = static_cast<float>(GetTickCount64()) * 0.001f;
        const Ogre::Quaternion waterOrientation(qw, qx, qy, qz);
        const Ogre::Vector3 waterPosition(px, py, pz);
        const Ogre::Vector3 cameraPosition = ctx->camNode->getPosition();
        const Ogre::Vector3 sunDirection = ctx->skySunDir.normalisedCopy();
        auto& waveVerts = cm.animated_verts;
        if (waveVerts.size() != cm.cpu_verts.size()) waveVerts.resize(cm.cpu_verts.size());

        auto addGerstner = [&](float x, float z, float dx, float dz, float wavelength,
                               float amplitude, float speed, float steepness,
                               Ogre::Vector3& displacement, Ogre::Vector3& normal) {
            const float k = 6.28318530718f / wavelength;
            const float phase = k * (dx * x + dz * z) + waveTime * speed;
            const float sn = std::sin(phase), cs = std::cos(phase);
            displacement.x += steepness * amplitude * dx * cs;
            displacement.y += amplitude * sn;
            displacement.z += steepness * amplitude * dz * cs;
            normal.x -= dx * k * amplitude * cs;
            normal.y -= steepness * k * amplitude * sn;
            normal.z -= dz * k * amplitude * cs;
        };

        for (size_t vi = 0; vi < cm.cpu_verts.size(); ++vi) {
            const BridgeVertex3D& src = cm.cpu_verts[vi];
            BridgeVertex3D& dst = waveVerts[vi];
            dst = src;
            const float x = src.px, z = src.pz;
            Ogre::Vector3 displacement = Ogre::Vector3::ZERO;
            Ogre::Vector3 localNormal = Ogre::Vector3::UNIT_Y;
            addGerstner(x,z, 0.88f, 0.47f,26.0f,0.070f,1.00f,0.42f,displacement,localNormal);
            addGerstner(x,z,-0.36f, 0.93f,15.0f,0.040f,1.27f,0.32f,displacement,localNormal);
            addGerstner(x,z, 0.14f,-0.99f, 8.5f,0.018f,1.72f,0.20f,displacement,localNormal);
            localNormal.normalise();
            dst.px = src.px + displacement.x;
            dst.py = src.py + displacement.y;
            dst.pz = src.pz + displacement.z;
            dst.nx = localNormal.x; dst.ny = localNormal.y; dst.nz = localNormal.z;

            const Ogre::Vector3 localPosition(dst.px*sx, dst.py*sy, dst.pz*sz);
            const Ogre::Vector3 worldPosition = waterPosition + waterOrientation * localPosition;
            Ogre::Vector3 worldNormal(localNormal.x/sx, localNormal.y/sy, localNormal.z/sz);
            worldNormal = (waterOrientation * worldNormal).normalisedCopy();
            Ogre::Vector3 viewDirection = cameraPosition - worldPosition;
            if (viewDirection.squaredLength() < 1.0e-6f) viewDirection = Ogre::Vector3::UNIT_Y;
            else viewDirection.normalise();

            const float ndv = skyClamp01(worldNormal.dotProduct(viewDirection));
            Ogre::Vector3 reflected = -viewDirection + worldNormal * (2.0f * ndv);
            if (reflected.y < 0.0f) reflected.y = -reflected.y;
            if (reflected.squaredLength() < 1.0e-6f) reflected = Ogre::Vector3::UNIT_Y;
            else reflected.normalise();
            const Ogre::Vector3 reflectedSky = waterReflectionColour(
                reflected, sunDirection, ctx->skySunElevation, worldPosition, waveTime);

            const Ogre::Vector3 srcColour(src.r/255.0f,src.g/255.0f,src.b/255.0f);
            const Ogre::Vector3 deepWater(0.008f+srcColour.x*0.050f,
                                          0.045f+srcColour.y*0.105f,
                                          0.065f+srcColour.z*0.125f);
            const float fresnel = 0.02f + 0.98f * std::pow(1.0f-ndv,5.0f);
            const float reflectionWeight = 0.10f + 0.78f*fresnel;
            Ogre::Vector3 waterColour = deepWater*(1.0f-reflectionWeight) +
                                        reflectedSky*(reflectionWeight*0.75f);
            Ogre::Vector3 halfVector = viewDirection + sunDirection;
            if (halfVector.squaredLength() < 1.0e-6f) halfVector = Ogre::Vector3::UNIT_Y;
            else halfVector.normalise();
            const float sunGlint = std::pow(std::fmax(worldNormal.dotProduct(halfVector),0.0f),150.0f) *
                                   skySmoothstep(-0.10f,0.22f,ctx->skySunElevation);
            waterColour += Ogre::Vector3(1.0f,0.82f,0.63f) * (sunGlint*0.65f);

            const float shoreSignal = skyClamp01((255.0f-static_cast<float>(src.a))/45.0f);
            const float foamA = 0.5f+0.5f*std::sin(x*0.31f+z*0.19f+waveTime*0.82f);
            const float foamB = 0.5f+0.5f*std::sin(x*0.17f-z*0.37f-waveTime*0.54f);
            const float foamNoise = 0.55f*foamA+0.45f*foamB;
            const float shoreFoam = skySmoothstep(0.02f,0.92f,shoreSignal)*(0.68f+0.32f*foamNoise);
            waterColour = skyMix(waterColour,Ogre::Vector3(0.78f,0.90f,0.95f),shoreFoam*0.72f);

            dst.r = static_cast<uint8_t>(skyClamp01(waterColour.x)*255.0f);
            dst.g = static_cast<uint8_t>(skyClamp01(waterColour.y)*255.0f);
            dst.b = static_cast<uint8_t>(skyClamp01(waterColour.z)*255.0f);
            dst.a = 255;
        }
        populateManualObject(mo, dbName, waveVerts, cm.cpu_indices, true);
    }
    node->setPosition(Ogre::Vector3(px, py, pz));
    node->setOrientation(Ogre::Quaternion(qw, qx, qy, qz));  // ترتيب Ogre: w,x,y,z
    node->setScale(Ogre::Vector3(sx, sy, sz));
}

// ---------------------------------------------------------------------------
// Camera & environment
// ---------------------------------------------------------------------------
BRIDGE_API void ogre_set_camera_pose(
    void* raw_ctx,
    float px, float py, float pz,
    float qx, float qy, float qz, float qw,
    float fov_rad, float znear, float zfar)
{
    OgreContext* ctx = (OgreContext*)raw_ctx;
    ctx->camera->setFOVy(Ogre::Radian(fov_rad));
    ctx->camera->setNearClipDistance(znear > 0.02f ? znear : 0.12f);
    ctx->camera->setFarClipDistance(zfar > 10.0f ? zfar : 2200.0f);
    ctx->camNode->setPosition(Ogre::Vector3(px, py, pz));
    ctx->camNode->setOrientation(Ogre::Quaternion(qw, qx, qy, qz));
}

BRIDGE_API void ogre_set_env(void* raw_ctx,
    float sun_x, float sun_y, float sun_z,
    float fog_r, float fog_g, float fog_b,
    float sun_elev) {
    if (!raw_ctx) return;
    OgreContext* ctx = (OgreContext*)raw_ctx;
    ctx->skySunElevation = sun_elev;
    ctx->skySunDir = Ogre::Vector3(sun_x, sun_elev, sun_z);
    if (ctx->skySunDir.squaredLength() < 1.0e-4f) ctx->skySunDir = Ogre::Vector3(0.4f, 0.2f, 0.3f);
    ctx->skySunDir.normalise();
    Ogre::Vector3 dir(-sun_x, -sun_y, -sun_z);
    if (dir.squaredLength() < 1e-4f) dir = Ogre::Vector3(-0.4f, -0.8f, -0.3f);
    dir.normalise();
    ctx->sunLight->setDirection(dir);
    ctx->skyRimLight->setDirection(Ogre::Vector3(-dir.x, -0.35f, -dir.z).normalisedCopy());
    float day = std::fmin(1.0f, std::fmax(0.0f, (sun_elev + 0.12f) / 0.42f));
    float twilight = std::fmax(0.0f, 1.0f - std::fabs(sun_elev) / 0.22f);
    ctx->sunLight->setPowerScale(Ogre::Math::PI * (0.035f + 1.35f * day + 0.50f * twilight));
    ctx->skyRimLight->setPowerScale(Ogre::Math::PI * (0.10f + 0.24f * day + 0.30f * twilight));
    const Ogre::ColourValue ambientTop(0.018f + 0.30f * day + 0.20f * twilight, 0.026f + 0.35f * day + 0.11f * twilight, 0.055f + 0.48f * day + 0.18f * twilight);
    const Ogre::ColourValue ambientBottom(0.012f + 0.10f * day + 0.055f * twilight, 0.015f + 0.11f * day + 0.035f * twilight, 0.025f + 0.14f * day + 0.040f * twilight);
    ctx->sceneMgr->setAmbientLight(ambientTop, ambientBottom, Ogre::Vector3::UNIT_Y);
}

// ---------------------------------------------------------------------------
// UI (2D)
// ---------------------------------------------------------------------------
static std::string getOrCreateUiDatablockName(OgreContext* ctx, uint32_t tex_id) {
    uint32_t tid = (tex_id == 0) ? 1 : tex_id;
    std::string name = "ApexUI_" + std::to_string(tid);
    if (ctx->ui_datablocks.find(tid) != ctx->ui_datablocks.end()) return name;
    Ogre::HlmsUnlit* unlit =
        (Ogre::HlmsUnlit*)ctx->root->getHlmsManager()->getHlms(Ogre::HLMS_UNLIT);
    Ogre::HlmsMacroblock macro;
    macro.mDepthCheck = false;
    macro.mDepthWrite = false;
    macro.mCullMode   = Ogre::CULL_NONE;
    Ogre::HlmsBlendblock blend;
    blend.setBlendType(Ogre::SBT_TRANSPARENT_ALPHA);
    Ogre::HlmsUnlitDatablock* db = (Ogre::HlmsUnlitDatablock*)unlit->createDatablock(
        name, name, macro, blend, Ogre::HlmsParamVec());
    auto texIt = ctx->textures.find(tid);
    if (texIt != ctx->textures.end()) {
        db->setTexture(0, texIt->second.gpu,
                       texIt->second.linear_filter ? ctx->anisoSampler : ctx->pointSampler);
    }
    ctx->ui_datablocks[tid] = db;
    return name;
}

BRIDGE_API void ogre_draw_2d_batch(
    void* raw_ctx,
    const BridgeVertex2D* verts, uint32_t vcount,
    const uint16_t* indices, uint32_t icount,
    uint32_t tex_id)
{
    // Every 2D batch is a triangle list with 16-bit local indices. Reject malformed
    // input before it can wrap an index while batches are merged.
    if (!raw_ctx || !verts || !indices || vcount < 3 || icount < 3 ||
        vcount >= 60000 || icount > 180000 || (icount % 3) != 0) return;
    for (uint32_t i = 0; i < icount; ++i) {
        if (indices[i] >= vcount) return;
    }

    OgreContext* ctx = (OgreContext*)raw_ctx;
    uint32_t tid = (tex_id == 0) ? 1 : tex_id;
    if (!ctx->ui_cmds.empty() &&
        ctx->ui_cmds.back().tex_id == tid &&
        ctx->ui_cmds.back().verts.size() + static_cast<size_t>(vcount) < 60000) {
        UiCmd& cmd = ctx->ui_cmds.back();
        const size_t base = cmd.verts.size();
        cmd.verts.insert(cmd.verts.end(), verts, verts + vcount);
        for (uint32_t i = 0; i < icount; ++i) {
            cmd.indices.push_back(static_cast<uint16_t>(base + indices[i]));
        }
    } else {
        UiCmd cmd;
        cmd.tex_id = tid;
        cmd.verts.assign(verts, verts + vcount);
        cmd.indices.assign(indices, indices + icount);
        ctx->ui_cmds.push_back(std::move(cmd));
    }
}

// ---------------------------------------------------------------------------
// Procedural atmospheric sky pass (camera-following, rendered before world)
// ---------------------------------------------------------------------------
static float skyClamp01(float x) {
    return std::fmax(0.0f, std::fmin(1.0f, x));
}
static float skySmoothstep(float a, float b, float x) {
    const float t = skyClamp01((x - a) / (b - a));
    return t * t * (3.0f - 2.0f * t);
}
static Ogre::Vector3 skyMix(const Ogre::Vector3& a, const Ogre::Vector3& b, float t) {
    return a * (1.0f - t) + b * t;
}
static float skyHash2(float x, float y) {
    // Integer hash avoids a costly sin() for every lattice corner while keeping
    // smooth value-noise interpolation in skyNoise2.
    const uint32_t ix = static_cast<uint32_t>(static_cast<int32_t>(std::floor(x)));
    const uint32_t iy = static_cast<uint32_t>(static_cast<int32_t>(std::floor(y)));
    uint32_t h = ix * 0x8da6b343u ^ iy * 0xd8163841u;
    h ^= h >> 16;
    h *= 0x7feb352du;
    h ^= h >> 15;
    h *= 0x846ca68bu;
    h ^= h >> 16;
    return static_cast<float>(h & 0x00ffffffu) * (1.0f / 16777215.0f);
}
static float skyNoise2(float x, float y) {
    const float ix = std::floor(x), iy = std::floor(y);
    const float fx0 = x - ix, fy0 = y - iy;
    const float fx = fx0 * fx0 * (3.0f - 2.0f * fx0);
    const float fy = fy0 * fy0 * (3.0f - 2.0f * fy0);
    const float a = skyHash2(ix, iy);
    const float b = skyHash2(ix + 1.0f, iy);
    const float c = skyHash2(ix, iy + 1.0f);
    const float d = skyHash2(ix + 1.0f, iy + 1.0f);
    return (a * (1.0f - fx) + b * fx) * (1.0f - fy) +
           (c * (1.0f - fx) + d * fx) * fy;
}
static float skyFbm2(float x, float y) {
    float amplitude = 0.52f;
    float sum = 0.0f;
    float norm = 0.0f;
    for (int octave = 0; octave < 3; ++octave) {
        sum += skyNoise2(x, y) * amplitude;
        norm += amplitude;
        x = x * 2.03f + 17.13f;
        y = y * 2.03f + 9.17f;
        amplitude *= 0.5f;
    }
    return sum / std::fmax(norm, 1.0e-4f);
}

// Direction is world-space so both the camera sky and water can sample the
// same animated cloud layers, sun halo and atmospheric scattering.
static Ogre::Vector3 skyColourForRay(const Ogre::Vector3& rayWorld,
                                     const Ogre::Vector3& sunDirWorld,
                                     float sunElevation,
                                     const Ogre::Vector3& observerWorld,
                                     float timeSeconds) {
    const Ogre::Vector3 ray = rayWorld.normalisedCopy();
    const Ogre::Vector3 sun = sunDirWorld.normalisedCopy();
    const float day = skySmoothstep(-0.10f, 0.25f, sunElevation);
    const float twilight = skySmoothstep(-0.24f, 0.015f, sunElevation) *
                           (1.0f - skySmoothstep(0.06f, 0.34f, sunElevation));
    const float night = 1.0f - skySmoothstep(-0.16f, 0.035f, sunElevation);

    const Ogre::Vector3 zenNight(0.004f, 0.008f, 0.024f);
    const Ogre::Vector3 zenDay(0.018f, 0.105f, 0.34f);
    const Ogre::Vector3 zenTwilight(0.020f, 0.032f, 0.115f);
    const Ogre::Vector3 horNight(0.010f, 0.018f, 0.044f);
    const Ogre::Vector3 horDay(0.50f, 0.69f, 0.88f);
    const Ogre::Vector3 horSun(1.15f, 0.34f, 0.105f);
    const Ogre::Vector3 horAnti(0.24f, 0.13f, 0.30f);

    Ogre::Vector3 sunFlat(sun.x, 0.0f, sun.z);
    if (sunFlat.squaredLength() < 1.0e-4f) sunFlat = Ogre::Vector3::UNIT_Z;
    sunFlat.normalise();
    Ogre::Vector3 rayFlat(ray.x, 0.0f, ray.z);
    if (rayFlat.squaredLength() < 1.0e-4f) rayFlat = Ogre::Vector3::UNIT_Z;
    rayFlat.normalise();
    const float azimuth = skyClamp01(sunFlat.dotProduct(rayFlat) * 0.5f + 0.5f);

    Ogre::Vector3 zen = skyMix(zenNight, zenDay, day);
    zen = skyMix(zen, zenTwilight, twilight * 0.92f);
    Ogre::Vector3 horizon = skyMix(horNight, horDay, day);
    horizon = skyMix(horizon,
        skyMix(horAnti, horSun, std::pow(azimuth, 1.42f)), twilight);

    const float y = skyClamp01(ray.y);
    const float vertical = std::pow(y, 0.50f);
    Ogre::Vector3 colour = skyMix(horizon, zen, vertical);
    const float haze = std::exp(-y * 10.0f) * std::pow(azimuth, 2.0f) * twilight;
    colour += Ogre::Vector3(0.22f, 0.075f, 0.020f) * haze;

    const float sunDot = std::fmax(ray.dotProduct(sun), 0.0f);
    const float sunVisible = skySmoothstep(-0.10f, 0.035f, sunElevation);
    const Ogre::Vector3 sunTint = skyMix(
        Ogre::Vector3(1.75f, 0.43f, 0.10f),
        Ogre::Vector3(1.15f, 0.93f, 0.73f),
        skySmoothstep(0.0f, 0.32f, sunElevation));
    const float disc = std::pow(sunDot, 420.0f) * sunVisible;
    const float halo = std::pow(sunDot, 18.0f) * (0.12f + 0.34f * twilight) * sunVisible;
    const float broadScatter = std::pow(sunDot, 3.0f) * (0.018f + 0.055f * twilight) * sunVisible;
    colour += sunTint * (disc * 2.0f + halo + broadScatter);

    // Stable star field on the celestial sphere, naturally hidden by clouds.
    if (night > 0.02f && ray.y > 0.025f) {
        const float starU = std::atan2(ray.z, ray.x) * 205.0f;
        const float starV = std::asin(skyClamp01(ray.y)) * 165.0f;
        const float starHash = skyHash2(std::floor(starU), std::floor(starV));
        const float star = skySmoothstep(0.9935f, 0.9997f, starHash) *
                           (0.65f + 0.35f * std::sin(timeSeconds * 1.7f + starHash * 91.0f));
        colour += Ogre::Vector3(0.60f, 0.72f, 1.0f) * star * night * 0.9f;
    }

    float cumulusDensity = 0.0f;
    float cirrusDensity = 0.0f;
    float cumulusShade = 0.50f;
    if (ray.y > -0.005f) {
        const float projectedY = std::fmax(ray.y, 0.035f);
        const float lowDistance = std::fmax((1050.0f - observerWorld.y) / projectedY, 0.0f);
        const float cloudX = observerWorld.x + ray.x * lowDistance + timeSeconds * 7.5f;
        const float cloudZ = observerWorld.z + ray.z * lowDistance - timeSeconds * 3.2f;
        // Smaller, warped cloud volumes with a soft multi-scale edge. Domain
        // warping breaks the old repeated square/strip silhouettes into rounded
        // cumulus lobes without changing the day/sunrise/sunset colour curves.
        const float px = cloudX * 0.00162f;
        const float pz = cloudZ * 0.00162f;
        const float warpX = (skyNoise2(px * 0.42f + 13.1f, pz * 0.42f - 4.7f) - 0.5f) * 0.52f;
        const float warpZ = (skyNoise2(px * 0.42f - 7.3f, pz * 0.42f + 17.9f) - 0.5f) * 0.52f;
        const float wx = px + warpX;
        const float wz = pz + warpZ;
        const float broad = skyFbm2(wx * 0.96f, wz * 0.96f);
        const float medium = skyFbm2(wx * 2.25f + 11.2f, wz * 2.25f - 6.4f);
        const float erosion = skyNoise2(wx * 5.4f - 3.7f, wz * 5.4f + 8.1f);
        const float billows = broad * 0.63f + medium * 0.29f + erosion * 0.08f;
        const float shaped = billows + (erosion - 0.5f) * 0.035f;
        cumulusDensity = skySmoothstep(0.515f, 0.675f, shaped);
        cumulusShade = skyClamp01(0.30f + medium * 0.46f + erosion * 0.14f + sunVisible * 0.10f);

        // A separate stretched high layer creates wispy cirrus streaks.
        const float highDistance = std::fmax((2650.0f - observerWorld.y) / projectedY, 0.0f);
        const float highX = (observerWorld.x + ray.x * highDistance) * 0.00042f + timeSeconds * 0.0018f;
        const float highZ = (observerWorld.z + ray.z * highDistance) * 0.00165f - timeSeconds * 0.0042f;
        const float cirrusNoise = skyFbm2(highX, highZ);
        cirrusDensity = skySmoothstep(0.605f, 0.755f, cirrusNoise) *
                        (0.18f + 0.20f * day + 0.10f * twilight + 0.08f * night);
    }

    if (cumulusDensity > 0.002f || cirrusDensity > 0.002f) {
        const Ogre::Vector3 topDay(1.18f, 1.20f, 1.18f);
        const Ogre::Vector3 bottomDay(0.40f, 0.49f, 0.62f);
        const Ogre::Vector3 topTwilight(1.48f, 0.66f, 0.31f);
        const Ogre::Vector3 bottomTwilight(0.17f, 0.105f, 0.20f);
        const Ogre::Vector3 topNight(0.075f, 0.095f, 0.15f);
        const Ogre::Vector3 bottomNight(0.012f, 0.018f, 0.038f);

        Ogre::Vector3 cloudTop = skyMix(topNight, topDay, day);
        cloudTop = skyMix(cloudTop, topTwilight, twilight);
        Ogre::Vector3 cloudBottom = skyMix(bottomNight, bottomDay, day);
        cloudBottom = skyMix(cloudBottom, bottomTwilight, twilight);
        const float cloudLight = skyClamp01(cumulusShade + sunVisible * 0.10f);
        Ogre::Vector3 cloudColour = skyMix(cloudBottom, cloudTop, cloudLight);
        const float silverLining = std::pow(sunDot, 5.0f) *
                                   (0.22f + 0.75f * twilight) * sunVisible;
        cloudColour += sunTint * silverLining * 0.42f;

        const float horizonFade = 0.78f + 0.22f * skySmoothstep(-0.02f, 0.16f, ray.y);
        const float totalCloud = 1.0f - (1.0f - cumulusDensity * 0.96f) *
                                       (1.0f - cirrusDensity);
        colour = skyMix(colour, cloudColour,
                        skyClamp01(totalCloud * horizonFade));
    }

    if (night > 0.02f) {
        Ogre::Vector3 moonDir(-sun.x, std::fmax(-sun.y, 0.16f), -sun.z);
        if (moonDir.squaredLength() > 1.0e-4f) moonDir.normalise();
        const float moonDot = std::fmax(ray.dotProduct(moonDir), 0.0f);
        colour += Ogre::Vector3(0.12f, 0.18f, 0.32f) *
                  std::pow(moonDot, 22.0f) * night * 0.55f;
    }

    if (ray.y < 0.0f) {
        const Ogre::Vector3 groundNight(0.008f, 0.012f, 0.024f);
        const Ogre::Vector3 groundDay(0.20f, 0.24f, 0.20f);
        colour = skyMix(horizon, skyMix(groundNight, groundDay, day),
                        skyClamp01(-ray.y * 6.0f));
    }
    return Ogre::Vector3(skyClamp01(colour.x), skyClamp01(colour.y), skyClamp01(colour.z));
}

static Ogre::Vector3 waterReflectionColour(const Ogre::Vector3& rayWorld,
                                           const Ogre::Vector3& sunDirWorld,
                                           float sunElevation,
                                           const Ogre::Vector3& observerWorld,
                                           float timeSeconds) {
    const Ogre::Vector3 ray = rayWorld.normalisedCopy();
    const Ogre::Vector3 sun = sunDirWorld.normalisedCopy();
    const float day = skySmoothstep(-0.10f, 0.25f, sunElevation);
    const float twilight = skySmoothstep(-0.24f, 0.015f, sunElevation) *
                           (1.0f - skySmoothstep(0.06f, 0.34f, sunElevation));
    const float night = 1.0f - skySmoothstep(-0.16f, 0.035f, sunElevation);

    const Ogre::Vector3 zenNight(0.004f, 0.008f, 0.024f);
    const Ogre::Vector3 zenDay(0.018f, 0.105f, 0.34f);
    const Ogre::Vector3 zenTwilight(0.020f, 0.032f, 0.115f);
    const Ogre::Vector3 horNight(0.010f, 0.018f, 0.044f);
    const Ogre::Vector3 horDay(0.50f, 0.69f, 0.88f);
    const Ogre::Vector3 horSun(1.15f, 0.34f, 0.105f);
    const Ogre::Vector3 horAnti(0.24f, 0.13f, 0.30f);

    Ogre::Vector3 sunFlat(sun.x, 0.0f, sun.z);
    if (sunFlat.squaredLength() < 1.0e-4f) sunFlat = Ogre::Vector3::UNIT_Z;
    sunFlat.normalise();
    Ogre::Vector3 rayFlat(ray.x, 0.0f, ray.z);
    if (rayFlat.squaredLength() < 1.0e-4f) rayFlat = Ogre::Vector3::UNIT_Z;
    rayFlat.normalise();
    const float azimuth = skyClamp01(sunFlat.dotProduct(rayFlat) * 0.5f + 0.5f);

    Ogre::Vector3 zen = skyMix(zenNight, zenDay, day);
    zen = skyMix(zen, zenTwilight, twilight);
    Ogre::Vector3 horizon = skyMix(horNight, horDay, day);
    horizon = skyMix(horizon,
        skyMix(horAnti, horSun, std::pow(azimuth, 1.42f)), twilight);
    Ogre::Vector3 result = skyMix(horizon, zen, std::pow(skyClamp01(ray.y), 0.50f));

    const float sunDot = std::fmax(ray.dotProduct(sun), 0.0f);
    const float sunVisible = skySmoothstep(-0.10f, 0.035f, sunElevation);
    const Ogre::Vector3 sunTint = skyMix(
        Ogre::Vector3(1.75f, 0.43f, 0.10f),
        Ogre::Vector3(1.15f, 0.93f, 0.73f),
        skySmoothstep(0.0f, 0.32f, sunElevation));
    result += sunTint * (
        std::pow(sunDot, 280.0f) * 1.55f +
        std::pow(sunDot, 14.0f) * (0.10f + 0.30f * twilight) * sunVisible);

    // One compact cloud sample per water vertex; it tracks the same wind and
    // large-scale cumulus field as the detailed camera sky at a lower CPU cost.
    if (ray.y > -0.005f) {
        const float projY = std::fmax(ray.y, 0.035f);
        const float distance = std::fmax((1050.0f - observerWorld.y) / projY, 0.0f);
        const float cloudX = observerWorld.x + ray.x * distance + timeSeconds * 7.5f;
        const float cloudZ = observerWorld.z + ray.z * distance - timeSeconds * 3.2f;
        const float px = cloudX * 0.00162f;
        const float pz = cloudZ * 0.00162f;
        const float broad = skyNoise2(px, pz);
        const float detail = skyNoise2(px * 2.65f + 11.2f, pz * 2.65f - 6.4f);
        // Wider transition keeps reflected clouds smooth on the water mesh's
        // per-vertex colour path instead of producing hard pixel-like patches.
        const float density = skySmoothstep(0.505f, 0.715f, broad * 0.72f + detail * 0.28f);
        const Ogre::Vector3 topDay(1.18f, 1.20f, 1.18f);
        const Ogre::Vector3 bottomDay(0.40f, 0.49f, 0.62f);
        const Ogre::Vector3 topTwilight(1.48f, 0.66f, 0.31f);
        const Ogre::Vector3 bottomTwilight(0.17f, 0.105f, 0.20f);
        const Ogre::Vector3 topNight(0.075f, 0.095f, 0.15f);
        const Ogre::Vector3 bottomNight(0.012f, 0.018f, 0.038f);
        Ogre::Vector3 top = skyMix(topNight, topDay, day);
        top = skyMix(top, topTwilight, twilight);
        Ogre::Vector3 bottom = skyMix(bottomNight, bottomDay, day);
        bottom = skyMix(bottom, bottomTwilight, twilight);
        const float cloudLight = skyClamp01(0.35f + detail * 0.48f + sunVisible * 0.12f);
        Ogre::Vector3 cloud = skyMix(bottom, top, cloudLight);
        cloud += sunTint * (std::pow(sunDot, 5.0f) * (0.25f + 0.55f * twilight) * sunVisible);
        result = skyMix(result, cloud, density * 0.94f);
    }

    if (night > 0.01f) {
        Ogre::Vector3 moon(-sun.x, std::fmax(-sun.y, 0.16f), -sun.z);
        if (moon.squaredLength() > 1.0e-4f) moon.normalise();
        result += Ogre::Vector3(0.12f, 0.18f, 0.32f) *
                  std::pow(std::fmax(ray.dotProduct(moon), 0.0f), 22.0f) * night * 0.55f;
    }
    return Ogre::Vector3(skyClamp01(result.x), skyClamp01(result.y), skyClamp01(result.z));
}

static void updateSkyGradient(OgreContext* ctx, float sw, float sh) {
    if (!ctx || !ctx->skyManual || !ctx->camera || !ctx->camNode) return;
    const float aspect = sw / std::fmax(sh, 1.0f);
    const float d = 1.0f;
    const float tanHalf = std::tan(ctx->camera->getFOVy().valueRadians() * 0.5f);
    const float halfH = d * tanHalf * 1.08f;
    const float halfW = halfH * aspect;
    const Ogre::Vector3 sunWorld = ctx->skySunDir.normalisedCopy();
    const Ogre::Vector3 cameraWorld = ctx->camNode->getPosition();
    const Ogre::Quaternion cameraOrientation = ctx->camNode->getOrientation();
    const float timeSeconds = static_cast<float>(GetTickCount64()) * 0.001f;

    constexpr int cols = 256;
    constexpr int rows = 144;
    ctx->skyManual->clear();
    ctx->skyManual->setRenderQueueGroup(kSkyRenderQueueGroup);
    ctx->skyManual->setLocalAabb(Ogre::Aabb::BOX_INFINITE);
    ctx->skyManual->begin("ApexSkyGradient", Ogre::OT_TRIANGLE_LIST);
    for (int y = 0; y <= rows; ++y) {
        const float ny = 1.08f - 2.16f * static_cast<float>(y) / static_cast<float>(rows);
        for (int x = 0; x <= cols; ++x) {
            const float nx = -1.08f + 2.16f * static_cast<float>(x) / static_cast<float>(cols);
            Ogre::Vector3 rayLocal(nx * aspect * tanHalf, ny * tanHalf, -1.0f);
            rayLocal.normalise();
            const Ogre::Vector3 rayWorld = (cameraOrientation * rayLocal).normalisedCopy();
            const Ogre::Vector3 colour = skyColourForRay(
                rayWorld, sunWorld, ctx->skySunElevation, cameraWorld, timeSeconds);
            ctx->skyManual->position(nx * halfW, ny * halfH, -d);
            ctx->skyManual->colour(colour.x, colour.y, colour.z, 1.0f);
        }
    }
    for (int y = 0; y < rows; ++y) {
        for (int x = 0; x < cols; ++x) {
            const uint32_t a = static_cast<uint32_t>(y * (cols + 1) + x);
            const uint32_t b = a + 1;
            const uint32_t c0 = a + static_cast<uint32_t>(cols + 1);
            const uint32_t d0 = c0 + 1;
            ctx->skyManual->triangle(a, c0, b);
            ctx->skyManual->triangle(b, c0, d0);
        }
    }
    ctx->skyManual->end();
    ctx->sceneMgr->getRenderQueue()->setSortRenderQueue(kSkyRenderQueueGroup, Ogre::RenderQueue::DisableSort);
}

// ---------------------------------------------------------------------------
// Frame
// ---------------------------------------------------------------------------
BRIDGE_API bool ogre_begin_frame(void* raw_ctx, int* out_w, int* out_h) {
    if (!raw_ctx) return false;
    OgreContext* ctx = (OgreContext*)raw_ctx;
    Ogre::WindowEventUtilities::messagePump();
    if (ctx->window->isClosed()) return false;
    *out_w = (int)ctx->window->getWidth();
    *out_h = (int)ctx->window->getHeight();
    for (auto& kv : ctx->meshes) {
        kv.second.used_this_frame = 0;
    }
    ctx->ui_cmds.clear();
    return true;
}

BRIDGE_API bool ogre_end_frame(void* raw_ctx) {
    if (!raw_ctx) return false;
    OgreContext* ctx = (OgreContext*)raw_ctx;

    // إخفاء الكائنات غير المستخدمة هذا الإطار
    for (auto& kv : ctx->meshes) {
        CachedMesh& cm = kv.second;
        for (size_t i = cm.used_this_frame; i < cm.pool_objs.size(); ++i) {
            cm.pool_objs[i]->setVisible(false);
        }
    }

    // بناء الواجهة (uiNode ابن لـ camNode فتتبع الكاميرا مباشرة)
    ctx->uiManual->clear();
    ctx->uiManual->setRenderQueueGroup(kUiRenderQueueGroup);
    ctx->uiManual->setLocalAabb(Ogre::Aabb::BOX_INFINITE);
    ctx->sceneMgr->getRenderQueue()->setSortRenderQueue(kUiRenderQueueGroup, Ogre::RenderQueue::DisableSort);
    float sw = (float)(ctx->window->getWidth()  > 0 ? ctx->window->getWidth()  : 1);
    float sh = (float)(ctx->window->getHeight() > 0 ? ctx->window->getHeight() : 1);
    float aspect = sw / sh;
    updateSkyGradient(ctx, sw, sh);
    updateWaterPbsAnimation(ctx);
    const float base_d = 1.5f;
    float tan_half_fov = std::tan(ctx->camera->getFOVy().valueRadians() * 0.5f);
    for (const UiCmd& cmd : ctx->ui_cmds) {
        if (cmd.verts.size() < 3 || cmd.indices.size() < 3) continue;
        // All UI batches share one camera-space plane; painter order is the batch order.
        const float d = base_d;
        float half_h = d * tan_half_fov;
        float half_w = half_h * aspect;
        std::string dbName = getOrCreateUiDatablockName(ctx, cmd.tex_id);
        ctx->uiManual->begin(dbName, Ogre::OT_TRIANGLE_LIST);
        for (const auto& v : cmd.verts) {
            float ndc_x = (v.x / sw) * 2.0f - 1.0f;
            float ndc_y = 1.0f - (v.y / sh) * 2.0f;
            ctx->uiManual->position(ndc_x * half_w, ndc_y * half_h, -d);
            ctx->uiManual->textureCoord(v.u, v.v);
            ctx->uiManual->colour(v.r / 255.0f, v.g / 255.0f, v.b / 255.0f, v.a / 255.0f);
        }
        for (size_t i = 0; i + 2 < cmd.indices.size(); i += 3) {
            ctx->uiManual->triangle(cmd.indices[i], cmd.indices[i + 1], cmd.indices[i + 2]);
        }
        ctx->uiManual->end();
    }
    return ctx->root->renderOneFrame();
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------
BRIDGE_API bool ogre_key_down(int vk_code) {
    if (!g_active_ctx || GetForegroundWindow() != g_active_ctx->hwnd) return false;
    return (GetAsyncKeyState(vk_code) & 0x8000) != 0;
}

BRIDGE_API void ogre_mouse_state(void* raw_ctx,
    float* mx, float* my, float* wheel_y,
    bool* left_down, bool* right_down, bool* mid_down) {
    OgreContext* ctx = (OgreContext*)raw_ctx;
    POINT pt = { 0, 0 };
    GetCursorPos(&pt);
    if (ctx->hwnd) ScreenToClient(ctx->hwnd, &pt);
    *mx = (float)pt.x;
    *my = (float)pt.y;
    *wheel_y = ctx->wheel_delta;
    ctx->wheel_delta = 0.0f;
    bool focused = (GetForegroundWindow() == ctx->hwnd);
    *left_down  = focused && ((GetAsyncKeyState(VK_LBUTTON) & 0x8000) != 0);
    *right_down = focused && ((GetAsyncKeyState(VK_RBUTTON) & 0x8000) != 0);
    *mid_down   = focused && ((GetAsyncKeyState(VK_MBUTTON) & 0x8000) != 0);
}

BRIDGE_API uint32_t ogre_pop_char(void* raw_ctx) {
    OgreContext* ctx = (OgreContext*)raw_ctx;
    if (ctx->char_queue.empty()) return 0;
    uint32_t c = ctx->char_queue.front();
    ctx->char_queue.pop();
    return c;
}

// ---------------------------------------------------------------------------
// Shutdown
// ---------------------------------------------------------------------------
BRIDGE_API void ogre_shutdown(void* raw_ctx) {
    if (!raw_ctx) return;
    OgreContext* ctx = (OgreContext*)raw_ctx;
    if (ctx->hwnd && ctx->orig_wndproc) {
        SetWindowLongPtrW(ctx->hwnd, GWLP_WNDPROC, (LONG_PTR)ctx->orig_wndproc);
    }
    g_active_ctx = nullptr;
    delete ctx->root;
    delete ctx;
}