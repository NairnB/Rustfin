//! GL side of Poster Flow: textured quads in true perspective, drawn through an egui_glow paint callback.
//! The CPU projects every corner (clip-space x, y and w), so the GPU only interpolates; with w per vertex the
//! texture is perspective-correct, which a skewed 2D egui mesh can't do. Shaders: GLSL ES 1.00 / GLSL 1.20+.
use egui_glow::glow::{self, HasContext};
use std::sync::{Arc, Mutex};

/// One corner: clip-space position (x·w, y·w, 0, w), texture coordinate, premultiplied colour (multiplies the texel).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Vert {
    pub pos: [f32; 4],
    pub uv: [f32; 2],
    pub rgba: [f32; 4],
}

/// A textured quad, corners in fan order. `tex` is any egui texture (the font atlas' white texel for solid fills).
#[derive(Clone, Debug)]
pub struct Quad {
    pub tex: egui::TextureId,
    pub v: [Vert; 4],
}

/// A paint callback that draws `quads` in order over `rect` (painter's algorithm: far ones first).
pub fn callback(rect: egui::Rect, quads: Vec<Quad>) -> egui::PaintCallback {
    let quads = Arc::new(quads);
    egui::PaintCallback {
        rect,
        callback: Arc::new(egui_glow::CallbackFn::new(move |_info, painter| unsafe { draw(painter, &quads) })),
    }
}

struct Gl {
    program: glow::Program,
    buf: glow::Buffer,
    vao: Option<glow::VertexArray>,
    sampler: Option<glow::UniformLocation>,
}

// One GL context per app, so the program lives for the app's life (it is tiny).
static GL: Mutex<Option<Gl>> = Mutex::new(None);

const VS: &str = r#"
#if NEW_SHADER_INTERFACE
#define attribute in
#define varying out
#endif
attribute vec4 a_pos;
attribute vec2 a_uv;
attribute vec4 a_rgba;
varying vec2 v_uv;
varying vec4 v_rgba;
void main() {
    v_uv = a_uv;
    v_rgba = a_rgba;
    gl_Position = a_pos;
}
"#;

const FS: &str = r#"
#ifdef GL_ES
precision mediump float;
#endif
uniform sampler2D u_sampler;
#if NEW_SHADER_INTERFACE
in vec2 v_uv;
in vec4 v_rgba;
out vec4 f_color;
#define gl_FragColor f_color
#define texture2D texture
#else
varying vec2 v_uv;
varying vec4 v_rgba;
#endif
void main() {
    gl_FragColor = v_rgba * texture2D(u_sampler, v_uv);
}
"#;

unsafe fn init(gl: &glow::Context) -> Option<Gl> {
    let sv = egui_glow::ShaderVersion::get(gl);
    let head = format!(
        "{}\n#define NEW_SHADER_INTERFACE {}\n",
        sv.version_declaration(),
        if sv.is_new_shader_interface() { 1 } else { 0 }
    );
    let program = gl.create_program().ok()?;
    let mut shaders = vec![];
    for (kind, src) in [(glow::VERTEX_SHADER, VS), (glow::FRAGMENT_SHADER, FS)] {
        let s = gl.create_shader(kind).ok()?;
        gl.shader_source(s, &format!("{head}{src}"));
        gl.compile_shader(s);
        if !gl.get_shader_compile_status(s) {
            eprintln!("posterflow shader: {}", gl.get_shader_info_log(s));
            return None;
        }
        gl.attach_shader(program, s);
        shaders.push(s);
    }
    for (i, name) in ["a_pos", "a_uv", "a_rgba"].into_iter().enumerate() {
        gl.bind_attrib_location(program, i as u32, name);
    }
    gl.link_program(program);
    for s in shaders {
        gl.detach_shader(program, s);
        gl.delete_shader(s);
    }
    if !gl.get_program_link_status(program) {
        eprintln!("posterflow link: {}", gl.get_program_info_log(program));
        return None;
    }
    let sampler = gl.get_uniform_location(program, "u_sampler");
    let buf = gl.create_buffer().ok()?;
    // Core profiles need a bound VAO; GL 2 / GLES 2 have none (egui re-points its own attributes afterwards).
    let v = gl.version();
    let vao = if v.major >= 3 { gl.create_vertex_array().ok() } else { None };
    Some(Gl { program, buf, vao, sampler })
}

unsafe fn draw(painter: &egui_glow::Painter, quads: &[Quad]) {
    if quads.is_empty() {
        return;
    }
    let gl = painter.gl();
    let mut st = GL.lock().unwrap_or_else(|e| e.into_inner());
    if st.is_none() {
        *st = init(gl);
    }
    let Some(s) = st.as_ref() else { return };
    let mut data: Vec<f32> = Vec::with_capacity(quads.len() * 4 * 10);
    for q in quads {
        for v in &q.v {
            data.extend_from_slice(&v.pos);
            data.extend_from_slice(&v.uv);
            data.extend_from_slice(&v.rgba);
        }
    }
    let bytes = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 4);
    gl.use_program(Some(s.program));
    gl.uniform_1_i32(s.sampler.as_ref(), 0);
    if let Some(vao) = s.vao {
        gl.bind_vertex_array(Some(vao));
    }
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(s.buf));
    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STREAM_DRAW);
    let stride = 10 * 4;
    for (i, (n, off)) in [(4, 0), (2, 16), (4, 24)].into_iter().enumerate() {
        gl.enable_vertex_attrib_array(i as u32);
        gl.vertex_attrib_pointer_f32(i as u32, n, glow::FLOAT, false, stride, off);
    }
    gl.active_texture(glow::TEXTURE0);
    for (i, q) in quads.iter().enumerate() {
        let Some(tex) = painter.texture(q.tex) else { continue };
        gl.bind_texture(glow::TEXTURE_2D, Some(tex));
        gl.draw_arrays(glow::TRIANGLE_FAN, (i * 4) as i32, 4);
    }
    for i in 0..3 {
        gl.disable_vertex_attrib_array(i);
    }
    if s.vao.is_some() {
        gl.bind_vertex_array(None);
    }
    gl.bind_buffer(glow::ARRAY_BUFFER, None);
}
