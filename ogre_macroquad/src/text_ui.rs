use crate::ffi::{ogre_create_texture, ogre_update_texture, BridgeVertex2D};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::rc::Rc;

struct GlyphEntry {
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    w: f32,
    h: f32,
    xmin: f32,
    ymin: f32,
    advance: f32,
}

pub struct FontInner {
    font: fontdue::Font,
    atlas_w: u32,
    atlas_h: u32,
    atlas_rgba: Vec<u8>,
    tex_id: u32,
    dirty: bool,
    cur_x: u32,
    cur_y: u32,
    row_h: u32,
    glyphs: HashMap<(char, u16), GlyphEntry>,
}

#[derive(Clone)]
pub struct Font {
    pub(crate) inner: Option<Rc<RefCell<FontInner>>>,
}

#[derive(Clone, Copy, Default)]
pub struct TextDimensions {
    pub width: f32,
    pub height: f32,
    pub offset_y: f32,
}

impl Font {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| format!("{:?}", e))?;
        let atlas_w = 1024u32;
        let atlas_h = 1024u32;
        Ok(Self {
            inner: Some(Rc::new(RefCell::new(FontInner {
                font,
                atlas_w,
                atlas_h,
                atlas_rgba: vec![0u8; (atlas_w * atlas_h * 4) as usize],
                tex_id: 0,
                dirty: false,
                cur_x: 2,
                cur_y: 2,
                row_h: 0,
                glyphs: HashMap::new(),
            }))),
        })
    }
}

impl FontInner {
    fn reserve_glyph_rect(&mut self, gw: u32, gh: u32) -> bool {
        if gw + 4 >= self.atlas_w || gh + 4 >= 4096 { return false; }
        if self.cur_x + gw + 2 >= self.atlas_w {
            self.cur_x = 2;
            self.cur_y += self.row_h + 2;
            self.row_h = 0;
        }
        while self.cur_y + gh + 2 >= self.atlas_h {
            if self.atlas_h >= 4096 { return false; }
            let old_h = self.atlas_h;
            let new_h = (self.atlas_h * 2).min(4096);
            let mut grown = vec![0u8; (self.atlas_w * new_h * 4) as usize];
            grown[..self.atlas_rgba.len()].copy_from_slice(&self.atlas_rgba);
            self.atlas_rgba = grown;
            self.atlas_h = new_h;
            for glyph in self.glyphs.values_mut() {
                let py0 = (glyph.v0 * old_h as f32).round();
                let py1 = (glyph.v1 * old_h as f32).round();
                glyph.v0 = py0 / new_h as f32;
                glyph.v1 = py1 / new_h as f32;
            }
            // The GPU texture is resized by ogre_update_texture on the next sync.
            self.dirty = true;
        }
        true
    }

    fn get_or_rasterize(&mut self, ch: char, px_size: u16) -> GlyphEntry {
        let key = (ch, px_size);
        if let Some(g) = self.glyphs.get(&key) {
            return GlyphEntry { ..*g };
        }

        let (metrics, bitmap) = self.font.rasterize(ch, px_size as f32);
        let gw = metrics.width as u32;
        let gh = metrics.height as u32;

        let mut packed = false;
        let (u0, v0, u1, v1) = if gw > 0 && gh > 0 {
            if self.reserve_glyph_rect(gw, gh) {
                for row in 0..gh {
                    for col in 0..gw {
                        let a = bitmap[(row * gw + col) as usize];
                        let dst = (((self.cur_y + row) * self.atlas_w + (self.cur_x + col)) * 4) as usize;
                        self.atlas_rgba[dst] = 255;
                        self.atlas_rgba[dst + 1] = 255;
                        self.atlas_rgba[dst + 2] = 255;
                        self.atlas_rgba[dst + 3] = a;
                    }
                }
                let u0 = self.cur_x as f32 / self.atlas_w as f32;
                let v0 = self.cur_y as f32 / self.atlas_h as f32;
                let u1 = (self.cur_x + gw) as f32 / self.atlas_w as f32;
                let v1 = (self.cur_y + gh) as f32 / self.atlas_h as f32;
                self.cur_x += gw + 2;
                self.row_h = self.row_h.max(gh);
                self.dirty = true;
                packed = true;
                (u0, v0, u1, v1)
            } else {
                (0.0, 0.0, 0.0, 0.0)
            }
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };

        let entry = GlyphEntry {
            u0,
            v0,
            u1,
            v1,
            // Failed-to-pack glyphs keep their advance but are omitted from quad generation.
            w: if packed { gw as f32 } else { 0.0 },
            h: if packed { gh as f32 } else { 0.0 },
            xmin: metrics.xmin as f32,
            ymin: metrics.ymin as f32,
            advance: metrics.advance_width,
        };
        self.glyphs.insert(key, GlyphEntry { ..entry });
        entry
    }

    pub fn sync_gpu(&mut self, ctx: *mut c_void) -> u32 {
        if self.tex_id == 0 && !ctx.is_null() {
            self.tex_id = unsafe {
                ogre_create_texture(ctx, self.atlas_w, self.atlas_h, self.atlas_rgba.as_ptr(), true)
            };
            self.dirty = false;
        } else if self.dirty && self.tex_id != 0 && !ctx.is_null() {
            unsafe {
                ogre_update_texture(ctx, self.tex_id, self.atlas_w, self.atlas_h, self.atlas_rgba.as_ptr());
            }
            self.dirty = false;
        }
        self.tex_id
    }
}

pub fn measure_font_text(text: &str, font: Option<&Font>, font_size: u16, font_scale: f32) -> TextDimensions {
    let px = ((font_size as f32) * font_scale).round().max(8.0) as u16;
    if let Some(f) = font.and_then(|f| f.inner.as_ref()) {
        let mut inner = f.borrow_mut();
        let mut w = 0.0f32;
        for ch in text.chars() {
            let g = inner.get_or_rasterize(ch, px);
            w += g.advance;
        }
        return TextDimensions {
            width: w,
            height: px as f32,
            offset_y: px as f32 * 0.8,
        };
    }
    TextDimensions {
        width: text.chars().count() as f32 * (px as f32 * 0.56),
        height: px as f32,
        offset_y: px as f32 * 0.8,
    }
}

pub fn build_text_quads(
    ctx: *mut c_void,
    text: &str,
    x: f32,
    y: f32,
    font: Option<&Font>,
    font_size: u16,
    font_scale: f32,
    rgba: [u8; 4],
) -> Option<(u32, Vec<BridgeVertex2D>, Vec<u16>)> {
    let f_rc = font.and_then(|f| f.inner.as_ref())?;
    let mut inner = f_rc.borrow_mut();
    let px = ((font_size as f32) * font_scale).round().max(8.0) as u16;

    let mut verts = Vec::new();
    let mut indices = Vec::new();
    let mut pen_x = x;

    for ch in text.chars() {
        let g = inner.get_or_rasterize(ch, px);
        if g.w > 0.0 && g.h > 0.0 {
            let gx0 = pen_x + g.xmin;
            let gy0 = y - g.h - g.ymin;
            let gx1 = gx0 + g.w;
            let gy1 = gy0 + g.h;

            let base = verts.len() as u16;
            verts.push(BridgeVertex2D { x: gx0, y: gy0, u: g.u0, v: g.v0, r: rgba[0], g: rgba[1], b: rgba[2], a: rgba[3] });
            verts.push(BridgeVertex2D { x: gx1, y: gy0, u: g.u1, v: g.v0, r: rgba[0], g: rgba[1], b: rgba[2], a: rgba[3] });
            verts.push(BridgeVertex2D { x: gx1, y: gy1, u: g.u1, v: g.v1, r: rgba[0], g: rgba[1], b: rgba[2], a: rgba[3] });
            verts.push(BridgeVertex2D { x: gx0, y: gy1, u: g.u0, v: g.v1, r: rgba[0], g: rgba[1], b: rgba[2], a: rgba[3] });
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        pen_x += g.advance;
    }

    let tex_id = inner.sync_gpu(ctx);
    if verts.is_empty() || tex_id == 0 {
        None
    } else {
        Some((tex_id, verts, indices))
    }
}

