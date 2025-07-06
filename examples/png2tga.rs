#![no_std]
#![no_main]

use libc::{FILE, c_char, c_int, fclose, fopen, fprintf, fputc, printf};
use upng::ffi::{
    upng_error::UPNG_EOK, upng_format::UPNG_RGB8, upng_free, upng_get_bitdepth, upng_get_bpp,
    upng_get_buffer, upng_get_error, upng_get_error_line, upng_get_format, upng_get_height,
    upng_get_size, upng_get_width, upng_new_from_file, upng_t,
};

const HI: fn(c_int) -> c_int = |w| (((w) >> 8) & 0xFF);
const LO: fn(c_int) -> c_int = |w| ((w) & 0xFF);

#[unsafe(no_mangle)]
unsafe extern "C" fn main(argc: c_int, argv: *const *const c_char) -> isize {
    let file: *mut FILE;

    let (width, height, depth);

    if argc <= 2 {
        return 0;
    }

    let upng: *mut upng_t = unsafe { upng_new_from_file(argv.add(1).read()) };

    if unsafe { upng_get_error(upng) } == UPNG_EOK {
        unsafe {
            printf(
                c"error &u %u\n".as_ptr(),
                upng_get_error(upng),
                upng_get_error_line(upng),
            )
        };
        return 0;
    }

    width = unsafe { upng_get_width(upng) };
    height = unsafe { upng_get_height(upng) };
    depth = unsafe { upng_get_bpp(upng) };

    unsafe {
        printf(
            c"size:	%ux%ux%u (%u)\n".as_ptr(),
            width,
            height,
            upng_get_bpp(upng),
            upng_get_size(upng),
        )
    };

    unsafe { printf(c"format:	%u\n".as_ptr(), upng_get_format(upng)) };

    if unsafe { upng_get_format(upng) } == UPNG_RGB8
        || unsafe { upng_get_format(upng) } == UPNG_RGB8
    {
        file = unsafe { fopen(argv.add(2).read(), c"wb".as_ptr()) };
        unsafe { fprintf(file, c"%c%c%c".as_ptr(), 0, 0, 2) };
        unsafe { fprintf(file, c"%c%c%c%c%c".as_ptr(), 0, 0, 0, 0, 0) };
        unsafe {
            fprintf(
                file,
                c"%c%c%c%c%c%c%c%c%c%c".as_ptr(),
                0,
                0,
                0,
                0,
                LO(width as c_int),
                HI(width as c_int),
                LO(height as c_int),
                HI(height as c_int),
                upng_get_bpp(upng),
                upng_get_bitdepth(upng),
            )
        };

        for y in 0..height {
            for x in 0..width {
                for d in 0..depth {
                    unsafe {
                        fputc(
                            upng_get_buffer(upng).wrapping_add(
                                ((height - y - 1) * width * depth + x * depth + (depth - d - 1))
                                    as usize,
                            ) as c_int,
                            file,
                        )
                    };
                }
            }
        }

        unsafe { fclose(file) };
    }

    unsafe { upng_free(upng) };
    0
}
