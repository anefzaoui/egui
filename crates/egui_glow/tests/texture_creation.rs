//! A texture OpenGL can't create (`glGenTextures` gives 0, as with no current context) neither
//! panics nor is lost: the painter keeps it and creates it on a later frame.
//!
//! The OpenGL functions are fakes, so this runs without a GPU or a window.

#![expect(unsafe_code)]
#![expect(clippy::fn_to_numeric_cast_any)] // A loader gives OpenGL's functions as pointers.

use core::ffi::{CStr, c_char, c_void};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use egui::epaint::ImageDelta;

/// Whether the fake `glGenTextures` creates a texture (else it gives 0).
static CAN_CREATE: AtomicBool = AtomicBool::new(true);
static NEXT_NAME: AtomicU32 = AtomicU32::new(1);
static TEX_IMAGES: AtomicU32 = AtomicU32::new(0);
static TEX_SUB_IMAGES: AtomicU32 = AtomicU32::new(0);

extern "system" fn get_string(name: u32) -> *const u8 {
    let text: &'static CStr = match name {
        glow::VERSION => c"2.1 fake",
        glow::SHADING_LANGUAGE_VERSION => c"1.20",
        _ => c"",
    };
    text.as_ptr().cast()
}
extern "system" fn get_error() -> u32 {
    glow::NO_ERROR
}
extern "system" fn get_integer(pname: u32, data: *mut i32) {
    let value = if pname == glow::MAX_TEXTURE_SIZE {
        4096
    } else {
        0
    };
    // SAFETY: the painter asks for one value at a time, into an `i32`.
    unsafe { *data = value };
}
extern "system" fn gen_names(n: i32, names: *mut u32) {
    for i in 0..n as usize {
        // SAFETY: OpenGL's contract: `names` has room for `n` names.
        unsafe { *names.add(i) = NEXT_NAME.fetch_add(1, Ordering::SeqCst) };
    }
}
extern "system" fn gen_textures(n: i32, names: *mut u32) {
    if CAN_CREATE.load(Ordering::SeqCst) {
        gen_names(n, names);
    } else {
        for i in 0..n as usize {
            // SAFETY: OpenGL's contract: `names` has room for `n` names.
            unsafe { *names.add(i) = 0 };
        }
    }
}
extern "system" fn create_name() -> u32 {
    NEXT_NAME.fetch_add(1, Ordering::SeqCst)
}
extern "system" fn create_shader(_: u32) -> u32 {
    create_name()
}
extern "system" fn get_object_iv(_: u32, pname: u32, params: *mut i32) {
    // Compiled and linked; info logs are empty.
    let value = i32::from(pname != glow::INFO_LOG_LENGTH);
    // SAFETY: the painter asks for one value at a time, into an `i32`.
    unsafe { *params = value };
}
extern "system" fn get_location(_: u32, _: *const c_char) -> i32 {
    0
}
extern "system" fn shader_source(_: u32, _: i32, _: *const *const c_char, _: *const i32) {}
extern "system" fn one_name(_: u32) {}
extern "system" fn two_names(_: u32, _: u32) {}
extern "system" fn two_names_one_int(_: u32, _: u32, _: i32) {}
extern "system" fn one_name_one_int(_: u32, _: i32) {}
extern "system" fn blend_func_separate(_: u32, _: u32, _: u32, _: u32) {}
extern "system" fn color_mask(_: u8, _: u8, _: u8, _: u8) {}
extern "system" fn viewport(_: i32, _: i32, _: i32, _: i32) {}
extern "system" fn uniform_2f(_: i32, _: f32, _: f32) {}
extern "system" fn uniform_1i(_: i32, _: i32) {}
extern "system" fn buffer_data(_: u32, _: isize, _: *const c_void, _: u32) {}
extern "system" fn vertex_attrib_pointer(_: u32, _: i32, _: u32, _: u8, _: i32, _: *const c_void) {}
extern "system" fn draw_elements(_: u32, _: i32, _: u32, _: *const c_void) {}
extern "system" fn delete_names(_: i32, _: *const u32) {}
extern "system" fn tex_image_2d(
    _: u32,
    _: i32,
    _: i32,
    _: i32,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
    _: *const c_void,
) {
    TEX_IMAGES.fetch_add(1, Ordering::SeqCst);
}
extern "system" fn tex_sub_image_2d(
    _: u32,
    _: i32,
    _: i32,
    _: i32,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
    _: *const c_void,
) {
    TEX_SUB_IMAGES.fetch_add(1, Ordering::SeqCst);
}

fn fake_gl(name: &CStr) -> *const c_void {
    let f: *const c_void = match name.to_str().expect("OpenGL function names are ASCII") {
        "glGetString" => get_string as *const c_void,
        "glGetError" => get_error as *const c_void,
        "glGetIntegerv" => get_integer as *const c_void,
        "glGenBuffers" => gen_names as *const c_void,
        "glGenTextures" => gen_textures as *const c_void,
        "glCreateShader" => create_shader as *const c_void,
        "glCreateProgram" => create_name as *const c_void,
        "glShaderSource" => shader_source as *const c_void,
        "glGetShaderiv" | "glGetProgramiv" => get_object_iv as *const c_void,
        "glGetUniformLocation" | "glGetAttribLocation" => get_location as *const c_void,
        "glCompileShader"
        | "glLinkProgram"
        | "glDeleteShader"
        | "glUseProgram"
        | "glDeleteProgram"
        | "glEnable"
        | "glDisable"
        | "glActiveTexture"
        | "glGenerateMipmap"
        | "glEnableVertexAttribArray"
        | "glDisableVertexAttribArray" => one_name as *const c_void,
        "glAttachShader"
        | "glDetachShader"
        | "glBindBuffer"
        | "glBindTexture"
        | "glBlendEquationSeparate" => two_names as *const c_void,
        "glTexParameteri" => two_names_one_int as *const c_void,
        "glPixelStorei" => one_name_one_int as *const c_void,
        "glBlendFuncSeparate" => blend_func_separate as *const c_void,
        "glColorMask" => color_mask as *const c_void,
        "glViewport" | "glScissor" => viewport as *const c_void,
        "glUniform2f" => uniform_2f as *const c_void,
        "glUniform1i" => uniform_1i as *const c_void,
        "glBufferData" => buffer_data as *const c_void,
        "glVertexAttribPointer" => vertex_attrib_pointer as *const c_void,
        "glDrawElements" => draw_elements as *const c_void,
        "glDeleteTextures" | "glDeleteBuffers" => delete_names as *const c_void,
        "glTexImage2D" => tex_image_2d as *const c_void,
        "glTexSubImage2D" => tex_sub_image_2d as *const c_void,
        _ => core::ptr::null(),
    };
    f
}

fn whole(size: usize) -> ImageDelta {
    ImageDelta::full(
        egui::ColorImage::filled([size, size], egui::Color32::GREEN),
        egui::TextureOptions::NEAREST,
    )
}

fn part(size: usize) -> ImageDelta {
    ImageDelta::partial(
        [0, 0],
        egui::ColorImage::filled([size, size], egui::Color32::RED),
        egui::TextureOptions::NEAREST,
    )
}

#[test]
fn a_texture_that_cant_be_created_is_kept_for_a_later_frame() {
    // SAFETY: every function the painter calls is one of the fakes above, with its signature.
    let gl = unsafe { glow::Context::from_loader_function_cstr(fake_gl) };
    let mut painter =
        egui_glow::Painter::new(Arc::new(gl), "", None, false).expect("a painter on the fakes");

    let new = egui::TextureId::Managed(7);
    let freed = egui::TextureId::Managed(8);
    let mut delta = egui::TexturesDelta::default();
    delta.push(new, whole(4));
    delta.push(new, part(2));
    delta.push(freed, whole(4));
    delta.free(freed);

    // No context: nothing can be created, and nothing panics.
    CAN_CREATE.store(false, Ordering::SeqCst);
    painter.paint_and_update_textures([64, 64], 1.0, &[], &mut delta);
    assert_eq!(painter.texture(new), None);
    assert_eq!(TEX_IMAGES.load(Ordering::SeqCst), 0);
    // Kept, in order; the one freed in the same frame is dropped with its free.
    assert_eq!(delta.set.len(), 1);
    assert_eq!(delta.set[&new].len(), 2);
    assert!(delta.free.is_empty());

    // The context is back: the next frame creates it, then applies the change after it.
    CAN_CREATE.store(true, Ordering::SeqCst);
    painter.paint_and_update_textures([64, 64], 1.0, &[], &mut delta);
    assert!(painter.texture(new).is_some());
    assert_eq!(painter.texture(freed), None);
    assert!(delta.is_empty());
    assert_eq!(TEX_IMAGES.load(Ordering::SeqCst), 1);
    assert_eq!(TEX_SUB_IMAGES.load(Ordering::SeqCst), 1);

    painter.destroy();
}
