#pragma once
#include <stdint.h>

#ifdef _WIN32
  #define BRIDGE_API __declspec(dllexport)
#else
  #define BRIDGE_API
#endif

struct BridgeVertex3D {
    float px, py, pz;
    float nx, ny, nz;
    float u, v;
    uint8_t r, g, b, a;
    float spec;
};

struct BridgeVertex2D {
    float x, y;
    float u, v;
    uint8_t r, g, b, a;
};

extern "C" {
    BRIDGE_API void* ogre_init(const char* title, int width, int height, int msaa);
    BRIDGE_API bool  ogre_begin_frame(void* ctx, int* out_w, int* out_h);
    BRIDGE_API bool  ogre_end_frame(void* ctx);
    BRIDGE_API void  ogre_shutdown(void* ctx);

    BRIDGE_API void ogre_set_camera_pose(
        void* ctx, float px, float py, float pz,
        float qx, float qy, float qz, float qw,
        float fov_rad, float znear, float zfar);
    BRIDGE_API void ogre_set_env(void* ctx,
        float sun_x, float sun_y, float sun_z,
        float fog_r, float fog_g, float fog_b, float sun_elev);

    BRIDGE_API uint32_t ogre_create_texture(void* ctx, uint32_t width, uint32_t height,
        const uint8_t* rgba, bool linear_filter);
    BRIDGE_API void ogre_update_texture(void* ctx, uint32_t tex_id, uint32_t width,
        uint32_t height, const uint8_t* rgba);
    BRIDGE_API void ogre_set_texture_filter(void* ctx, uint32_t tex_id, bool linear_filter);

    BRIDGE_API uint32_t ogre_upload_mesh(void* ctx,
        const BridgeVertex3D* verts, uint32_t vcount,
        const uint16_t* indices, uint32_t icount,
        uint32_t tex_id, bool is_glass, bool is_water);
    BRIDGE_API void ogre_update_mesh_vertices(void* ctx, uint32_t mesh_id,
        const BridgeVertex3D* verts, uint32_t vcount,
        const uint16_t* indices, uint32_t icount);
    BRIDGE_API void ogre_set_mesh_material(void* ctx, uint32_t mesh_id,
        uint32_t tex_id, bool is_glass, bool is_water);
    BRIDGE_API void ogre_draw_mesh_trs(void* ctx, uint32_t mesh_id,
        float px, float py, float pz,
        float qx, float qy, float qz, float qw,
        float sx, float sy, float sz,
        float tint_r, float tint_g, float tint_b, bool is_glass);

    BRIDGE_API void ogre_draw_2d_batch(void* ctx,
        const BridgeVertex2D* verts, uint32_t vcount,
        const uint16_t* indices, uint32_t icount, uint32_t tex_id);

    BRIDGE_API bool ogre_key_down(int vk_code);
    BRIDGE_API void ogre_mouse_state(void* ctx, float* mx, float* my, float* wheel_y,
        bool* left_down, bool* right_down, bool* mid_down);
    BRIDGE_API uint32_t ogre_pop_char(void* ctx);
}
