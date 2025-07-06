#![no_std]
#![no_main]

use core::{ffi::c_void, mem, ptr};

use glu_sys::{
    GL_BLEND, GL_COLOR_BUFFER_BIT, GL_CULL_FACE, GL_DEPTH_TEST, GL_LINEAR, GL_LUMINANCE,
    GL_LUMINANCE_ALPHA, GL_MODELVIEW, GL_ONE_MINUS_SRC_ALPHA, GL_PROJECTION, GL_QUADS, GL_RGB,
    GL_RGBA, GL_SRC_ALPHA, GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_TEXTURE_MIN_FILTER,
    GL_UNSIGNED_BYTE, GLint, GLsizei, GLuint, GLvoid, glBegin, glBindTexture, glBlendFunc, glClear,
    glClearColor, glDeleteTextures, glDisable, glEnable, glEnd, glGenTextures, glLoadIdentity,
    glMatrixMode, glOrtho, glTexCoord2f, glTexImage2D, glTexParameteri, glVertex2f,
};
use libc::{c_char, c_int, c_uchar, calloc, free, printf};

use sdl::{gl::ll::SDL_GL_SwapBuffers, video::ll::SDL_SetVideoMode};
use sdl2_sys::{SDL_Event, SDL_EventType, SDL_INIT_VIDEO, SDL_Init, SDL_Quit, SDL_WaitEvent};
use upng::ffi::{
    upng_decode, upng_error::UPNG_EOK, upng_free, upng_get_buffer, upng_get_components,
    upng_get_error, upng_get_error_line, upng_get_height, upng_get_width, upng_new_from_file,
    upng_t,
};

pub const SDL_OPENGL: u32 = 2;
pub const SDL_DOUBLEBUF: u32 = 1_073_741_824;

fn checkboard(w: GLuint, h: GLuint) -> GLuint {
    let mut xc = 0;
    let mut dark = 0;
    let mut texture = 0;

    let buffer: *mut c_uchar = unsafe { calloc((w * h) as usize, 3).cast::<c_uchar>() };

    unsafe { printf(c"%i %i\n".as_ptr(), w, h) };
    for y in 0..=h {
        for x in 0..=w {
            xc += 1;

            if (xc % (w >> 3)) == 0 {
                dark = 1 - dark;
            }

            if dark != 0 {
                unsafe {
                    buffer.add((y * w * 3 + x * 3) as usize).write(0x6F);
                    buffer.add((y * w * 3 + x * 3 + 1) as usize).write(0x6F);
                    buffer.add((y * w * 3 + x * 3 + 2) as usize).write(0x6F);
                }
            } else {
                unsafe {
                    buffer.add((y * w * 3 + x * 3) as usize).write(0xAF);
                    buffer.add((y * w * 3 + x * 3 + 1) as usize).write(0xAF);
                    buffer.add((y * w * 3 + x * 3 + 2) as usize).write(0xAF);
                }
            }
        }

        if (y % (h >> 3)) == 0 {
            dark = 1 - dark;
        }
    }

    unsafe {
        glEnable(GL_TEXTURE_2D);
        glGenTextures(1, &raw mut texture);
        glBindTexture(GL_TEXTURE_2D, texture);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR as i32);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR as i32);
        glTexImage2D(
            GL_TEXTURE_2D,
            0,
            3,
            w as GLsizei,
            h as GLsizei,
            0,
            GL_RGB,
            GL_UNSIGNED_BYTE,
            buffer as *const GLvoid,
        );

        free(buffer.cast::<c_void>());

        texture
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn main(argc: c_int, argv: *const *const c_char) -> c_int {
    if argc <= 1 {
        return 0;
    }

    let upng = unsafe { load_image(argv.add(1).read()) };
    if upng.is_null() {
        return 0;
    }

    unsafe { setup_sdl(upng) };

    let texture = unsafe { create_texture(upng) };
    if texture == 0 {
        return 1;
    }

    let cb = checkboard(unsafe { upng_get_width(upng) }, unsafe {
        upng_get_height(upng)
    });

    let mut event: SDL_Event = unsafe { mem::zeroed() };
    unsafe { run_event_loop(&mut event, texture, cb) };

    unsafe { glDeleteTextures(1, &texture) };
    unsafe { glDeleteTextures(1, &cb) };
    unsafe { SDL_Quit() };

    0
}

unsafe fn load_image(file: *const c_char) -> *mut upng_t {
    let upng = unsafe { upng_new_from_file(file) };
    unsafe { upng_decode(upng) };

    if unsafe { upng_get_error(upng) } != UPNG_EOK {
        unsafe {
            printf(
                c"error: %u %u\n".as_ptr(),
                upng_get_error(upng),
                upng_get_error_line(upng),
            )
        };
        unsafe { upng_free(upng) };
        return ptr::null_mut();
    }

    upng
}

unsafe fn create_texture(upng: *const upng_t) -> GLuint {
    let mut texture: GLuint = 0;

    unsafe { glEnable(GL_TEXTURE_2D) };
    unsafe { glGenTextures(1, &raw mut texture) };

    unsafe { glBindTexture(GL_TEXTURE_2D, texture) };

    unsafe { glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR as GLint) };
    unsafe { glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR as GLint) };

    let width = unsafe { upng_get_width(upng) } as GLsizei;
    let height = unsafe { upng_get_height(upng) } as GLsizei;
    let buffer = unsafe { upng_get_buffer(upng).cast::<c_void>() };

    match unsafe { upng_get_components(upng) } {
        1 => unsafe {
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_LUMINANCE as GLint,
                width,
                height,
                0,
                GL_LUMINANCE,
                GL_UNSIGNED_BYTE,
                buffer,
            )
        },
        2 => unsafe {
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_LUMINANCE_ALPHA as GLint,
                width,
                height,
                0,
                GL_LUMINANCE_ALPHA,
                GL_UNSIGNED_BYTE,
                buffer,
            )
        },
        3 => unsafe {
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_RGB as GLint,
                width,
                height,
                0,
                GL_RGB,
                GL_UNSIGNED_BYTE,
                buffer,
            )
        },
        4 => unsafe {
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_RGBA as GLint,
                width,
                height,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                buffer,
            )
        },
        _ => return 0,
    };

    texture
}

unsafe fn setup_sdl(upng: *const upng_t) {
    unsafe { SDL_Init(SDL_INIT_VIDEO) };
    unsafe {
        SDL_SetVideoMode(
            upng_get_width(upng) as c_int,
            upng_get_height(upng) as c_int,
            0,
            SDL_OPENGL | SDL_DOUBLEBUF,
        )
    };

    unsafe { glDisable(GL_DEPTH_TEST) };
    unsafe { glDisable(GL_CULL_FACE) };
    unsafe { glEnable(GL_BLEND) };
    unsafe { glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA) };
    unsafe { glClearColor(0.0, 0.0, 0.0, 0.0) };

    unsafe { glMatrixMode(GL_PROJECTION) };
    unsafe { glLoadIdentity() };
    unsafe { glOrtho(0.0, 1.0, 0.0, 1.0, 0.0, 1.0) };

    unsafe { glMatrixMode(GL_MODELVIEW) };
    unsafe { glLoadIdentity() };
}

unsafe fn run_event_loop(event: &mut SDL_Event, texture: GLuint, cb: GLuint) {
    while unsafe { SDL_WaitEvent(event) } != 0 {
        if unsafe { event.type_ } == SDL_EventType::SDL_QUIT as u32 {
            break;
        }

        unsafe { render_frame(texture, cb) };
    }
}

unsafe fn render_frame(texture: GLuint, cb: GLuint) {
    unsafe { glClear(GL_COLOR_BUFFER_BIT) };

    unsafe { draw_texture(cb) };
    unsafe { draw_texture(texture) };

    unsafe { SDL_GL_SwapBuffers() };
}

unsafe fn draw_texture(texture: GLuint) {
    unsafe { glBindTexture(GL_TEXTURE_2D, texture) };
    unsafe { glBegin(GL_QUADS) };
    unsafe { glTexCoord2f(0.0, 1.0) };
    unsafe { glVertex2f(0.0, 0.0) };
    unsafe { glTexCoord2f(0.0, 0.0) };
    unsafe { glVertex2f(0.0, 1.0) };
    unsafe { glTexCoord2f(1.0, 0.0) };
    unsafe { glVertex2f(1.0, 1.0) };
    unsafe { glTexCoord2f(1.0, 1.0) };
    unsafe { glVertex2f(1.0, 0.0) };
    unsafe { glEnd() };
}
