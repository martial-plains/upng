use std::{
    ffi::c_uchar,
    mem::{self, zeroed},
    ptr,
};

use libc::{
    INT_MAX, SEEK_END, c_char, c_int, c_long, c_uint, c_ulong, c_void, fclose, fopen, fread, free,
    fseek, ftell, malloc, memcpy, memset, rewind, size_t,
};

macro_rules! MAKE_BYTE {
    ($b:expr) => {
        $b & 0xFF
    };
}

macro_rules! MAKE_DWORD {
    ($a:expr, $b:expr, $c:expr, $d:expr) => {
        (MAKE_BYTE!($a) as u32) << 24
            | (MAKE_BYTE!($b) as u32) << 16
            | (MAKE_BYTE!($c) as u32) << 8
            | MAKE_BYTE!($d) as u32
    };
}

macro_rules! MAKE_DWORD_PTR {
    ($p:expr) => {
        (MAKE_DWORD!(*$p.offset(0), *$p.offset(1), *$p.offset(2), *$p.offset(3)))
    };
}

const CHUNK_IHDR: u32 = MAKE_DWORD!(b'I', b'H', b'D', b'R');
const CHUNK_IDAT: u32 = MAKE_DWORD!(b'I', b'D', b'A', b'T');
const CHUNK_IEND: u32 = MAKE_DWORD!(b'I', b'E', b'N', b'D');

const FIRST_LENGTH_CODE_INDEX: usize = 257;
const LAST_LENGTH_CODE_INDEX: usize = 285;

/// 256 literals, the end code, some length codes, and 2 unused codes
const NUM_DEFLATE_CODE_SYMBOLS: usize = 288;

/// The distance codes have their own symbols, 30 used, 2 unused
const NUM_DISTANCE_SYMBOLS: usize = 32;

/// The code length codes. 0-15: code lengths, 16: copy previous 3-6 times, 17: 3-10 zeros, 18: 11-138 zeros
const NUM_CODE_LENGTH_CODES: usize = 19;

/// Largest number of symbols used by any tree type
const MAX_SYMBOLS: usize = 288;

const DEFLATE_CODE_BITLEN: usize = 15;
const DISTANCE_BITLEN: usize = 15;
const CODE_LENGTH_BITLEN: usize = 7;
/// Largest bitlen used by any tree type
const MAX_BIT_LENGTH: usize = 15;

const DEFLATE_CODE_BUFFER_SIZE: usize = NUM_DEFLATE_CODE_SYMBOLS * 2;
const DISTANCE_BUFFER_SIZE: usize = NUM_DISTANCE_SYMBOLS * 2;
const CODE_LENGTH_BUFFER_SIZE: usize = NUM_CODE_LENGTH_CODES * 2;

const SET_ERROR: fn(*mut upng_t, upng_error) = |upng: *mut upng_t, code: upng_error| unsafe {
    (*upng).error = code;
    (*upng).error_line = line!();
};

macro_rules! upng_chunk_length {
    ($chunk:expr) => {
        MAKE_DWORD_PTR!($chunk)
    };
}

macro_rules! upng_chunk_type {
    ($chunk:expr) => {
        MAKE_DWORD_PTR!($chunk.add(4))
    };
}

macro_rules! upng_chunk_critical {
    ($chunk:expr) => {
        (($chunk.add(4).read() & 32) == 0)
    };
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum upng_error {
    UPNG_EOK = 0,
    UPNG_ENOMEM = 1,
    UPNG_ENOTFOUND = 2,
    UPNG_ENOTPNG = 3,
    UPNG_EMALFORMED = 4,
    UPNG_EUNSUPPORTED = 5,
    UPNG_EUNINTERLACED = 6,
    UPNG_EUNFORMAT = 7,
    UPNG_EPARAM = 8,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum upng_format {
    UPNG_BADFORMAT,
    UPNG_RGB8,
    UPNG_RGB16,
    UPNG_RGBA8,
    UPNG_RGBA16,
    UPNG_LUMINANCE1,
    UPNG_LUMINANCE2,
    UPNG_LUMINANCE4,
    UPNG_LUMINANCE8,
    UPNG_LUMINANCE_ALPHA1,
    UPNG_LUMINANCE_ALPHA2,
    UPNG_LUMINANCE_ALPHA4,
    UPNG_LUMINANCE_ALPHA8,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum upng_state {
    UPNG_ERROR = -1,
    UPNG_DECODED = 0,
    UPNG_HEADER = 1,
    UPNG_NEW = 2,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum upng_color {
    UPNG_LUM = 0,
    UPNG_RGB = 2,
    UPNG_LUMA = 4,
    UPNG_RGBA = 6,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct upng_source {
    pub buffer: *const c_uchar,
    pub size: c_ulong,
    pub owning: c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct upng_t {
    pub width: c_uint,
    pub height: c_uint,

    pub color_type: upng_color,
    pub color_depth: c_uint,
    pub format: upng_format,

    pub buffer: *mut c_uchar,
    pub size: c_ulong,

    pub error: upng_error,
    pub error_line: c_uint,

    pub state: upng_state,
    pub source: upng_source,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct huffman_tree {
    pub tree2d: *mut c_uint,
    /// Maximum number of bits a single code can get
    pub maxbitlen: c_uint,
    /// Number of symbols in the alphabet = number of codes
    pub numcodes: c_uint,
}

/// The base lengths represented by codes 257-285
const LENGTH_BASE: [c_uint; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];

/// The extra bits used by codes 257-285 (added to base length
const LENGTH_EXTRA: [c_uint; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

/// the base backwards distances (the bits of distance codes appear after length codes and use their own huffman tree
const DISTANCE_BASE: [c_uint; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];

/// The extra bits of backwards distances (added to base)
const DISTANCE_EXTRA: [c_uint; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

const CLCL: [c_uint; NUM_CODE_LENGTH_CODES] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

static mut FIXED_DEFLATE_CODE_TREE: [c_uint; NUM_DEFLATE_CODE_SYMBOLS * 2] = [
    289, 370, 290, 307, 546, 291, 561, 292, 293, 300, 294, 297, 295, 296, 0, 1, 2, 3, 298, 299, 4,
    5, 6, 7, 301, 304, 302, 303, 8, 9, 10, 11, 305, 306, 12, 13, 14, 15, 308, 339, 309, 324, 310,
    317, 311, 314, 312, 313, 16, 17, 18, 19, 315, 316, 20, 21, 22, 23, 318, 321, 319, 320, 24, 25,
    26, 27, 322, 323, 28, 29, 30, 31, 325, 332, 326, 329, 327, 328, 32, 33, 34, 35, 330, 331, 36,
    37, 38, 39, 333, 336, 334, 335, 40, 41, 42, 43, 337, 338, 44, 45, 46, 47, 340, 355, 341, 348,
    342, 345, 343, 344, 48, 49, 50, 51, 346, 347, 52, 53, 54, 55, 349, 352, 350, 351, 56, 57, 58,
    59, 353, 354, 60, 61, 62, 63, 356, 363, 357, 360, 358, 359, 64, 65, 66, 67, 361, 362, 68, 69,
    70, 71, 364, 367, 365, 366, 72, 73, 74, 75, 368, 369, 76, 77, 78, 79, 371, 434, 372, 403, 373,
    388, 374, 381, 375, 378, 376, 377, 80, 81, 82, 83, 379, 380, 84, 85, 86, 87, 382, 385, 383,
    384, 88, 89, 90, 91, 386, 387, 92, 93, 94, 95, 389, 396, 390, 393, 391, 392, 96, 97, 98, 99,
    394, 395, 100, 101, 102, 103, 397, 400, 398, 399, 104, 105, 106, 107, 401, 402, 108, 109, 110,
    111, 404, 419, 405, 412, 406, 409, 407, 408, 112, 113, 114, 115, 410, 411, 116, 117, 118, 119,
    413, 416, 414, 415, 120, 121, 122, 123, 417, 418, 124, 125, 126, 127, 420, 427, 421, 424, 422,
    423, 128, 129, 130, 131, 425, 426, 132, 133, 134, 135, 428, 431, 429, 430, 136, 137, 138, 139,
    432, 433, 140, 141, 142, 143, 435, 483, 436, 452, 568, 437, 438, 445, 439, 442, 440, 441, 144,
    145, 146, 147, 443, 444, 148, 149, 150, 151, 446, 449, 447, 448, 152, 153, 154, 155, 450, 451,
    156, 157, 158, 159, 453, 468, 454, 461, 455, 458, 456, 457, 160, 161, 162, 163, 459, 460, 164,
    165, 166, 167, 462, 465, 463, 464, 168, 169, 170, 171, 466, 467, 172, 173, 174, 175, 469, 476,
    470, 473, 471, 472, 176, 177, 178, 179, 474, 475, 180, 181, 182, 183, 477, 480, 478, 479, 184,
    185, 186, 187, 481, 482, 188, 189, 190, 191, 484, 515, 485, 500, 486, 493, 487, 490, 488, 489,
    192, 193, 194, 195, 491, 492, 196, 197, 198, 199, 494, 497, 495, 496, 200, 201, 202, 203, 498,
    499, 204, 205, 206, 207, 501, 508, 502, 505, 503, 504, 208, 209, 210, 211, 506, 507, 212, 213,
    214, 215, 509, 512, 510, 511, 216, 217, 218, 219, 513, 514, 220, 221, 222, 223, 516, 531, 517,
    524, 518, 521, 519, 520, 224, 225, 226, 227, 522, 523, 228, 229, 230, 231, 525, 528, 526, 527,
    232, 233, 234, 235, 529, 530, 236, 237, 238, 239, 532, 539, 533, 536, 534, 535, 240, 241, 242,
    243, 537, 538, 244, 245, 246, 247, 540, 543, 541, 542, 248, 249, 250, 251, 544, 545, 252, 253,
    254, 255, 547, 554, 548, 551, 549, 550, 256, 257, 258, 259, 552, 553, 260, 261, 262, 263, 555,
    558, 556, 557, 264, 265, 266, 267, 559, 560, 268, 269, 270, 271, 562, 565, 563, 564, 272, 273,
    274, 275, 566, 567, 276, 277, 278, 279, 569, 572, 570, 571, 280, 281, 282, 283, 573, 574, 284,
    285, 286, 287, 0, 0,
];

static mut FIXED_DISTANCE_TREE: [c_uint; NUM_DISTANCE_SYMBOLS * 2] = [
    33, 48, 34, 41, 35, 38, 36, 37, 0, 1, 2, 3, 39, 40, 4, 5, 6, 7, 42, 45, 43, 44, 8, 9, 10, 11,
    46, 47, 12, 13, 14, 15, 49, 56, 50, 53, 51, 52, 16, 17, 18, 19, 54, 55, 20, 21, 22, 23, 57, 60,
    58, 59, 24, 25, 26, 27, 61, 62, 28, 29, 30, 31, 0, 0,
];

fn read_bit(bitpointer: *mut c_ulong, bitstream: *const c_uchar) -> c_uchar {
    let result =
        unsafe { bitstream.add(((*bitpointer) >> 3) as usize).read() >> ((*bitpointer) & 0x7) & 1 };

    unsafe { (*bitpointer) += 1 }
    result
}

fn read_bits(bitpointer: *mut c_ulong, bitstream: *const c_uchar, nbits: c_ulong) -> c_uint {
    let mut result = 0;

    for (i, _) in (0..nbits).enumerate() {
        result |= (read_bit(bitpointer, bitstream) as c_uint) << i;
    }

    result
}

/// The buffer must be numcodes*2 in size!
fn huffman_tree_init(
    tree: *mut huffman_tree,
    buffer: *mut c_uint,
    numcodes: c_uint,
    maxbitlen: c_uint,
) {
    unsafe {
        (*tree).tree2d = buffer;
        (*tree).numcodes = numcodes;
        (*tree).maxbitlen = maxbitlen;
    }
}

/// Given the code lengths (as stored in the PNG file), generate the tree as
/// defined by Deflate. maxbitlen is the maximum bits that a code in the tree can
/// have. return value is error.
fn huffman_tree_create_lengths(upng: *mut upng_t, tree: *mut huffman_tree, bitlen: *const c_uint) {
    let mut tree1d = [0; MAX_SYMBOLS];
    let mut blcount = [0; MAX_BIT_LENGTH];
    let mut nextcode = [0; MAX_BIT_LENGTH + 1];
    let mut nodefilled = 0; // up to which node it is filled 
    let mut treepos = 0; // position in the tree (1 of the numcodes columns)

    // initialize local vectors
    unsafe { memset(blcount.as_mut_ptr().cast(), 0, size_of_val(&blcount)) };

    // step 1: count number of instances of each code length
    for bits in 0..(unsafe { *tree }).numcodes as usize {
        blcount[unsafe { bitlen.add(bits) } as usize] += 1;
    }

    // step 2: generate the nextcode values
    for bits in 1..=(unsafe { *tree }).maxbitlen as usize {
        nextcode[bits] = (nextcode[bits - 1] + blcount[bits - 1]) << 1;
    }

    // step 3: generate all the codes
    unsafe {
        for n in 0..(*tree).numcodes as usize {
            if bitlen.add(n).read() != 0 {
                nextcode[bitlen.add(n).read() as usize] += 1;
                tree1d[n] = nextcode[bitlen.add(n).read() as usize]
            }
        }
    }

    // convert tree1d[] to tree2d[][]. In the 2D array, a value of 32767 means
    // uninited, a value >= numcodes is an address to another bit, a value <
    // numcodes is a code. The 2 rows are the 2 possible bit values (0 or 1),
    // there are as many columns as codes - 1 a good huffmann tree has N * 2 - 1
    // nodes, of which N - 1 are internal nodes. Here, the internal nodes are
    // stored (what their 0 and 1 option point to). There is only memory for such
    // good tree currently, if there are more nodes (due to too long length
    // codes), error 55 will happen
    unsafe {
        for n in 0..(*tree).numcodes as usize * 2 {
            *(*tree).tree2d.add(n) = 32767; // 32767 here means the tree2d isn't filled there yet
        }
    }

    unsafe {
        for n in 0..(*tree).numcodes as usize {
            // the codes
            for i in 0..*bitlen.add(n) {
                // the bits for this code
                let bit = (tree1d[n] >> (*bitlen.add(n) - i - 1)) & 1;
                // check if oversubscribed
                if treepos > (*tree).numcodes - 2 {
                    SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
                    return;
                }

                if *(*tree).tree2d.add((2 * treepos + bit) as usize) == 32767 {
                    // not yet filled in
                    if i + 1 == *bitlen.add(n) {
                        // last bit
                        *(*tree).tree2d.add((2 * treepos + bit) as usize) = n as u32; // put the current code in it
                        treepos = 0;
                    } else {
                        // put address of the next step in here, first that address has
                        // to be found of course (it's just nodefilled + 1)...
                        nodefilled += 1;
                        *(*tree).tree2d.add((2 * treepos + bit) as usize) =
                            nodefilled + (*tree).numcodes; // addresses encoded with numcodes added to it
                    }
                } else {
                    treepos = *(*tree).tree2d.add((2 * treepos + bit) as usize)
                }
            }
        }

        for n in 0..(*tree).numcodes as usize * 2 {
            if *(*tree).tree2d.add(n) == 32767 {
                *(*tree).tree2d.add(n) = 0 // remove possible remaining 32767's
            }
        }
    }
}

fn huffman_decode_symbol(
    upng: *mut upng_t,
    r#in: *const c_uchar,
    bp: *mut c_ulong,
    codetree: *const huffman_tree,
    inlength: c_ulong,
) -> c_uint {
    let mut treepos: c_uint = 0;
    let mut ct: c_uint;
    let mut bit: c_uchar;

    loop {
        // error: end of input memory reached without endcode
        if unsafe { ((*bp) & 0x07) == 0 && ((*bp) >> 3) > inlength } {
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
            return 0;
        }

        bit = read_bit(bp, r#in);

        ct = unsafe {
            *(*codetree)
                .tree2d
                .add(((treepos << 1) | bit as c_uint) as usize)
        };
        if ct < (unsafe { *codetree }).numcodes {
            return ct;
        }

        treepos = ct - (unsafe { *codetree }).numcodes;
        if treepos >= (unsafe { *codetree }).numcodes {
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
            return 0;
        }
    }
}

/// Get the tree of a deflated block with dynamic tree, the tree itself is also
/// Huffman compressed with a known tree
fn get_tree_inflate_dynamic(
    upng: *mut upng_t,
    codetree: *mut huffman_tree,
    codetreeD: *mut huffman_tree,
    codelengthcodetree: *mut huffman_tree,
    r#in: *const c_uchar,
    bp: *mut c_ulong,
    inlength: c_ulong,
) {
    let mut codelengthcode = [0; NUM_CODE_LENGTH_CODES];
    let mut bitlen = [0; NUM_DEFLATE_CODE_SYMBOLS];
    let mut bitlenD = [0; NUM_DISTANCE_SYMBOLS];

    // make sure that length values that aren't filled in will be 0, or a wrong
    // tree will be generated
    if (unsafe { *bp }) >> 3 >= inlength - 2 {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
    }

    // clear bitlen arrays
    unsafe {
        memset(bitlen.as_mut_ptr().cast(), 0, size_of_val(&bitlen));
        memset(bitlenD.as_mut_ptr().cast(), 0, size_of_val(&bitlenD));
    }

    // the bit pointer is or will go past the memory
    let hlit = read_bits(bp, r#in, 5) + 257; /*number of literal/length codes + 257. Unlike the spec, the value
    257 is added to it here already */
    let hdist = read_bits(bp, r#in, 5) + 1; /*number of distance codes. Unlike the spec, the
    value 1 is added to it here already */
    let hclen = read_bits(bp, r#in, 4) + 4; /*number of code length codes. Unlike the spec,
    the value 4 is added to it here already */

    for i in 0..NUM_CODE_LENGTH_CODES {
        if i < hclen as usize {
            codelengthcode[CLCL[i] as usize] = read_bits(bp, r#in, 3);
        } else {
            codelengthcode[CLCL[i] as usize] = 0; // if not, it must stay 0
        }
    }

    huffman_tree_create_lengths(upng, codelengthcodetree, codelengthcode.as_ptr());

    // bail now if we encountered an error earlier
    if unsafe { !matches!((*upng).error, upng_error::UPNG_EOK) } {
        return;
    }

    /*now we can use this tree to read the lengths for the tree that this function
     * will return */
    let mut i = 0;
    while i < (hlit + hdist) as usize {
        /*i is the current symbol we're reading in the part
        that contains the code lengths of lit/len codes
        and dist codes */
        let code = huffman_decode_symbol(upng, r#in, bp, codelengthcodetree, inlength);
        if unsafe { !matches!((*upng).error, upng_error::UPNG_EOK) } {
            break;
        }

        if code <= 15 {
            // a length code
            if i < hlit as usize {
                bitlen[i] = code;
            } else {
                bitlenD[i - hlit as usize] = code;
            }
            i += 1;
        } else if code == 16 {
            // repeat previous
            let mut replength = 3; // read in the 2 bits that indicate repeat length (3-6)

            if (unsafe { *bp }) >> 3 >= inlength {
                SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
                break;
            }
            // error, bit pointer jumps past memory
            replength += read_bits(bp, r#in, 2);

            // set value to the previous code
            let value: c_uint = if (i - 1) < hlit as usize {
                bitlen[i - 1]
            } else {
                bitlenD[i - hlit as usize - 1]
            };

            // repeat this value in the next lengths
            for _ in 0..replength {
                // i is larger than the amount of codes
                if i >= (hlit + hdist) as usize {
                    SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
                    break;
                }

                if i < hlit as usize {
                    bitlen[i] = value;
                } else {
                    bitlenD[i - hlit as usize] = value;
                }
                i += 1;
            }
        } else if code == 17 {
            // repeat "0" 3-10 times
            let replength = 3;
            if (unsafe { *bp }) >> 3 >= inlength {
                SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
                break;
            }

            // error, bit pointer jumps past memory
            for _ in 0..replength {
                // error: i is larger than the amount of codes
                if i >= (hlit + hdist) as usize {
                    SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
                    break;
                }

                if i < hlit as usize {
                    bitlen[i] = 0;
                } else {
                    bitlenD[i - hlit as usize] = 0;
                }

                i += 1;
            }
        } else if code == 18 {
            // repeat "0" 11-138 times
            let mut replength = 11; // read in the bits that indicate repeat length
            // error, bit pointer jumps past memory
            if (unsafe { *bp }) >> 3 >= inlength {
                SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
                break;
            }

            replength += read_bits(bp, r#in, 7);

            // repeat this value in the next lengths
            for _ in 0..replength {
                // i is larger than the amount the codes
                if i >= (hlit + hdist) as usize {
                    SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
                    break;
                }

                if i < hlit as usize {
                    bitlen[i] = 0;
                } else {
                    bitlenD[i - hlit as usize] = 0;
                }
                i += 1;
            }
        } else {
            // somehow an unexisting code appeared. This can never happen.
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
            break;
        }
    }

    if matches!((unsafe { *upng }).error, upng_error::UPNG_EOK) && bitlen[256] == 0 {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
    }

    /*the length of the end code 256 must be larger than 0 */
    /*now we've finally got hlit and hdist, so generate the code trees, and the
     * function is done */

    if matches!((unsafe { *upng }).error, upng_error::UPNG_EOK) {
        huffman_tree_create_lengths(upng, codetree, bitlen.as_ptr());
    }

    if matches!((unsafe { *upng }).error, upng_error::UPNG_EOK) {
        huffman_tree_create_lengths(upng, codetreeD, bitlenD.as_ptr());
    }
}

#[allow(static_mut_refs)]
#[allow(clippy::too_many_arguments)]
fn inflate_huffman(
    upng: *mut upng_t,
    out: *mut c_uchar,
    outsize: c_ulong,
    r#in: *const c_uchar,
    bp: *mut c_ulong,
    pos: *mut c_ulong,
    inlength: c_ulong,
    btype: c_uint,
) {
    let mut codetree_buffer = [0; DEFLATE_CODE_BUFFER_SIZE];
    let mut codetreeD_buffer = [0; DISTANCE_BUFFER_SIZE];
    let mut done = 0;

    let mut codetree: huffman_tree = unsafe { zeroed() };
    let mut codetreeD: huffman_tree = unsafe { zeroed() };

    if btype == 1 {
        // fixed trees
        huffman_tree_init(
            &raw mut codetree,
            unsafe { FIXED_DEFLATE_CODE_TREE.as_mut_ptr() },
            NUM_DEFLATE_CODE_SYMBOLS as c_uint,
            DEFLATE_CODE_BITLEN as c_uint,
        );

        huffman_tree_init(
            &raw mut codetreeD,
            unsafe { FIXED_DISTANCE_TREE.as_mut_ptr() },
            NUM_DISTANCE_SYMBOLS as c_uint,
            DISTANCE_BITLEN as c_uint,
        );
    } else if btype == 2 {
        // dynamic trees
        let mut codelengthcodetree_buffer = [0; CODE_LENGTH_BUFFER_SIZE];
        let mut codelengthcodetree: huffman_tree = unsafe { zeroed() };

        huffman_tree_init(
            &raw mut codetree,
            codetree_buffer.as_mut_ptr(),
            NUM_DEFLATE_CODE_SYMBOLS as c_uint,
            DEFLATE_CODE_BITLEN as c_uint,
        );
        huffman_tree_init(
            &raw mut codetreeD,
            codetreeD_buffer.as_mut_ptr(),
            NUM_DISTANCE_SYMBOLS as c_uint,
            DISTANCE_BITLEN as c_uint,
        );
        huffman_tree_init(
            &raw mut codelengthcodetree,
            codelengthcodetree_buffer.as_mut_ptr(),
            NUM_CODE_LENGTH_CODES as c_uint,
            CODE_LENGTH_BITLEN as c_uint,
        );

        get_tree_inflate_dynamic(
            upng,
            &raw mut codetree,
            &raw mut codetreeD,
            &raw mut codelengthcodetree,
            r#in,
            bp,
            inlength,
        );
    }

    while done == 0 {
        let code = huffman_decode_symbol(upng, r#in, bp, &raw const codetree, inlength);
        if !matches!((unsafe { *upng }).error, upng_error::UPNG_EOK) {
            return;
        }

        if code == 256 {
            // end code
            done = 1;
        } else if code <= 255 {
            // literal symbol
            if (unsafe { *pos }) >= outsize {
                SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
                return;
            }

            // store output
            unsafe {
                (*pos) += 1;

                *out.add(*pos as usize) = code as c_uchar;
            };
        } else if code as usize >= FIRST_LENGTH_CODE_INDEX
            && code as usize <= LAST_LENGTH_CODE_INDEX
        {
            // length code
            // part 1: get legth base
            let mut length = LENGTH_BASE[code as usize - FIRST_LENGTH_CODE_INDEX];

            // part 2: get extra bits and add the value of that to length
            let numextrabits = LENGTH_EXTRA[code as usize - FIRST_LENGTH_CODE_INDEX];

            // error, bit pointer will jump past memory
            if (unsafe { *bp } >> 3) >= inlength {
                SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
                return;
            }
            length += read_bits(bp, r#in, numextrabits as c_ulong);

            // part 3: get distance code
            let codeD = huffman_decode_symbol(upng, r#in, bp, &raw const codetreeD, inlength);
            if !matches!((unsafe { *upng }).error, upng_error::UPNG_EOK) {
                return;
            }

            // invalid distance (30-31 are never used)
            if codeD > 29 {
                SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
                return;
            }

            let mut distance = DISTANCE_BASE[codeD as usize];

            // part 4: get extra bits from distance
            let numextrabitsD = DISTANCE_EXTRA[codeD as usize];

            // error, bit pointer will jump past memory
            if (unsafe { *bp } >> 3) >= inlength {
                SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
                return;
            }

            distance += read_bits(bp, r#in, numextrabitsD as c_ulong);

            // part 5: fill in all the out[n] values based on the length and dist
            let start = unsafe { *pos };
            let mut backward = start - distance as c_ulong;

            if (unsafe { *pos } + length as c_ulong >= outsize) {
                SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
                return;
            }

            for _ in 0..length {
                unsafe {
                    *pos += 1;
                    *out.add(pos as usize) = *out.add(backward as usize);
                    backward += 1;
                }

                if backward >= start {
                    backward = start - distance as c_ulong;
                }
            }
        }
    }
}

fn inflate_uncompressed(
    upng: *mut upng_t,
    out: *mut c_uchar,
    outsize: c_ulong,
    r#in: *const c_uchar,
    bp: *mut c_ulong,
    pos: *mut c_ulong,
    inlength: c_ulong,
) {
    // go to first boundary of byte
    while (unsafe { *bp } & 0x7) != 0 {
        unsafe {
            *bp += 1;
        }
    }

    let mut p = unsafe { *bp } / 8; // byte position

    // read (2 bytes) and nlen (2 bytes)
    if p >= inlength - 4 {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
        return;
    }

    let len = unsafe { *r#in.add(p as usize) as c_int + 256 * *r#in.add(p as usize + 1) as c_int };
    p += 2;
    let nlen = unsafe { r#in.add(p as usize) as c_int + 256 * r#in.add(p as usize + 1) as c_int };
    p += 2;

    // check if 16-bit nlen is really the one's complement of len
    if len + nlen != 65535 {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
        return;
    }

    if unsafe { *pos } + len as c_ulong >= outsize {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
        return;
    }

    // read the literal data: len bytes are now stored in the out buffer
    if p + len as c_ulong > inlength {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
        return;
    }

    for _ in 0..len as usize {
        unsafe {
            *pos += 1;
        }
        p += 1;

        unsafe { *out.add((*pos) as usize) = *r#in.add(p as usize) };
    }

    unsafe {
        *bp = p * 8;
    }
}

fn uz_inflate_data(
    upng: *mut upng_t,
    out: *mut c_uchar,
    outsize: c_ulong,
    r#in: *const c_uchar,
    insize: c_ulong,
    inpos: c_ulong,
) -> upng_error {
    let mut bp = 0; /*bit pointer in the "in" data, current byte is bp >> 3, current bit is
    bp & 0x7 (from lsb to msb of the byte) */
    let mut pos = 0; // byte position in the out buffer

    let mut done = 0;

    while done == 0 {
        // ensure next bit doesn't point past the end of the buffer
        if (bp >> 3) >= insize {
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
            return (unsafe { *upng }).error;
        }

        // read block control bits
        done = unsafe { read_bit(&raw mut bp, r#in.add(inpos as usize)) };
        let btype = unsafe {
            read_bit(&raw mut bp, r#in.add(inpos as usize))
                | (read_bit(&raw mut bp, r#in.add(inpos as usize)) << 1)
        };

        // process control type appropriately
        if btype == 3 {
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
            return (unsafe { *upng }).error;
        } else if btype == 0 {
            inflate_uncompressed(
                upng,
                out,
                outsize,
                unsafe { r#in.add(inpos as usize) },
                &raw mut bp,
                &raw mut pos,
                insize,
            ); // no compression
        } else {
            inflate_huffman(
                upng,
                out,
                outsize,
                unsafe { r#in.add(inpos as usize) },
                &raw mut bp,
                &raw mut pos,
                insize,
                btype as c_uint,
            );
        }

        // stop if an error has occured
        if matches!((unsafe { *upng }).error, upng_error::UPNG_EOK) {
            return (unsafe { *upng }).error;
        }
    }

    (unsafe { *upng }).error
}

fn uz_inflate(
    upng: *mut upng_t,
    out: *mut c_uchar,
    outsize: c_ulong,
    r#in: *const c_uchar,
    insize: c_ulong,
) -> upng_error {
    // we require two bytes for the zlib data header
    if insize < 2 {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
        return (unsafe { *upng }).error;
    }

    // 256 * in[0] + in[1] must be a multiple of 31, the FCHECK value is supposed
    // to be made that way
    if unsafe { *r#in as c_int * 256 + *r#in.add(1) as c_int % 31 != 0 } {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
        return (unsafe { *upng }).error;
    }

    // error: only compression method 8: inflate with sliding window of 32k is
    // supported by the PNG spec
    if unsafe { (*r#in & 15) != 8 || ((*r#in >> 4) & 15) > 7 } {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
        return (unsafe { *upng }).error;
    }

    // create output buffer
    uz_inflate_data(upng, out, outsize, r#in, insize, 2);

    (unsafe { *upng }).error
}

/// Paeth predicter, used by PNG filter type 4
fn paeth_predictor(a: c_int, b: c_int, c: c_int) -> c_int {
    let p = a + b - c;
    let pa = if p > a { p - a } else { a - p };
    let pb = if p > b { p - b } else { b - p };
    let pc = if p > c { p - c } else { c - p };

    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn unfilter_scanline(
    upng: *mut upng_t,
    recon: *mut c_uchar,
    scanline: *const c_uchar,
    precon: *const c_uchar,
    bytewidth: c_ulong,
    filterType: c_uchar,
    length: c_ulong,
) {
    /*
      For PNG filter method 0
      unfilter a PNG image scanline by scanline. when the pixels are smaller than
      1 byte, the filter works byte per byte (bytewidth = 1) precon is the
      previous unfiltered scanline, recon the result, scanline the current one
      the incoming scanlines do NOT include the filtertype byte, that one is
      given in the parameter filterType instead recon and scanline MAY be the
      same memory address! precon must be disjoint.
    */

    match filterType {
        0 => {
            for i in 0..length as usize {
                unsafe {
                    *recon.add(i) = *scanline.add(i);
                }
            }
        }

        1 => {
            for i in 0..bytewidth as usize {
                unsafe {
                    *recon.add(i) = *scanline.add(i);
                }
            }

            for i in bytewidth..length {
                let i = i as usize;
                unsafe {
                    *recon.add(i) = *scanline.add(i) + *recon.add(i - bytewidth as usize);
                }
            }
        }

        2 => {
            if !precon.is_null() {
                for i in 0..length as usize {
                    unsafe {
                        *recon.add(i) = *scanline.add(i) + *precon.add(i);
                    }
                }
            } else {
                for i in 0..length as usize {
                    unsafe {
                        *recon.add(i) = *scanline.add(i);
                    }
                }
            }
        }

        3 => {
            if !precon.is_null() {
                for i in 0..bytewidth as usize {
                    unsafe {
                        *recon.add(i) = *scanline.add(i) + *precon.add(i) / 2;
                    }
                }

                for i in bytewidth..length {
                    let i = i as usize;
                    unsafe {
                        *recon.add(i) = *scanline.add(i)
                            + ((*recon.add(i - bytewidth as usize) + *precon.add(i)) / 2);
                    }
                }
            } else {
                for i in 0..bytewidth as usize {
                    unsafe { *recon.add(i) = *scanline.add(i) }
                }

                for i in bytewidth..length {
                    let i = i as usize;
                    unsafe {
                        *recon.add(i) = *scanline.add(i) + *recon.add(i - bytewidth as usize) / 2;
                    }
                }
            }
        }

        4 => {
            if !precon.is_null() {
                for i in 0..bytewidth as usize {
                    unsafe {
                        *recon.add(i) = (*scanline.add(i) as c_int
                            + paeth_predictor(0, *precon.add(i) as c_int, 0))
                            as c_uchar;
                    }
                }

                for i in bytewidth..length {
                    let i = i as usize;
                    unsafe {
                        *recon.add(i) = (*scanline.add(i) as c_int
                            + paeth_predictor(
                                *recon.add(i - bytewidth as usize) as c_int,
                                *precon.add(i) as c_int,
                                *precon.add(i - bytewidth as usize) as c_int,
                            )) as c_uchar;
                    }
                }
            } else {
                for i in 0..bytewidth as usize {
                    unsafe { *recon.add(i) = (*scanline.add(i) as c_int) as c_uchar }
                }

                for i in bytewidth..length {
                    let i = i as usize;
                    unsafe {
                        *recon.add(i) = (*scanline.add(i) as c_int
                            + paeth_predictor(*recon.add(i - bytewidth as usize) as c_int, 0, 0))
                            as c_uchar;
                    }
                }
            }
        }

        _ => {
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
        }
    }
}

fn unfilter(
    upng: *mut upng_t,
    out: *mut c_uchar,
    r#in: *const c_uchar,
    w: c_uint,
    h: c_uint,
    bpp: c_uint,
) {
    /*
      For PNG filter method 0
      this function unfilters a single image (e.g. without interlacing this is
      called once, with Adam7 it's called 7 times) out must have enough bytes
      allocated already, in must have the scanlines + 1 filtertype byte per
      scanline w and h are image dimensions or dimensions of reduced image, bpp
      is bpp per pixel in and out are allowed to be the same memory address!
    */

    let mut prevline: *mut c_uchar = ptr::null_mut();
    let bytewidth = bpp.div_ceil(8); /*bytewidth is used for filtering, is 1 when bpp < 8,
    number of bytes per pixel otherwise */
    let linebytes = (w * bpp).div_ceil(8);

    for y in 0..h {
        let outindex = linebytes * y;
        let inindex = (1 + linebytes) * y;
        let filterType = unsafe { *r#in.add(inindex as usize) };

        unfilter_scanline(
            upng,
            unsafe { out.add(outindex as usize) },
            unsafe { r#in.add(inindex as usize + 1) },
            prevline,
            bytewidth as c_ulong,
            filterType,
            linebytes as c_ulong,
        );

        if !matches!((unsafe { *upng }).error, upng_error::UPNG_EOK) {
            return;
        }

        prevline = unsafe { out.add(outindex as usize) };
    }
}

fn remove_padding_bits(
    out: *mut c_uchar,
    r#in: *const c_uchar,
    olinebits: c_ulong,
    ilinebits: c_ulong,
    h: c_uint,
) {
    /*
      After filtering there are still padding bpp if scanlines have non multiple
      of 8 bit amounts. They need to be removed (except at last scanline of
      (Adam7-reduced) image) before working with pure image buffers for the Adam7
      code, the color convert code and the output to the user. in and out are
      allowed to be the same buffer, in may also be higher but still overlapping;
      in must have >= ilinebits*h bpp, out must have >= olinebits*h bpp,
      olinebits must be <= ilinebits also used to move bpp after earlier such
      operations happened, e.g. in a sequence of reduced images from Adam7 only
      useful if (ilinebits - olinebits) is a value in the range 1..7
    */

    let diff = ilinebits - olinebits;
    let (mut obp, mut ibp) = (0, 0); // bit pointers

    for _ in 0..h {
        for _ in 0..olinebits {
            let bit = ((unsafe { r#in.add((ibp) >> 3) } as usize) >> (7 - (ibp & 0x7))) as c_uchar;

            ibp += 1;

            if bit == 0 {
                unsafe { *out.add((obp >> 3) as usize) &= !(1 << (7 - (obp & 0x7))) };
            } else {
                unsafe { *out.add((obp >> 3) as usize) |= 1 << (7 - (obp & 0x7)) };
            }
            obp += 1;
        }
        ibp += diff as usize;
    }
}

fn post_process_scanlines(
    upng: *mut upng_t,
    out: *mut c_uchar,
    r#in: *const c_uchar,
    info_png: *const upng_t,
) {
    let bpp = unsafe { upng_get_bpp(info_png) };
    let w = unsafe { (*info_png).width };
    let h = unsafe { (*info_png).height };

    if bpp == 0 {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
        return;
    }

    if bpp < 8 && w * bpp != (w * bpp).div_ceil(8) * 8 {
        unfilter(upng, r#in.cast_mut(), r#in, w, h, bpp);

        if unsafe { !matches!((*upng).error, upng_error::UPNG_EOK) } {
            return;
        }
        remove_padding_bits(
            out,
            r#in,
            (w * bpp) as c_ulong,
            (w * bpp).div_ceil(8) as c_ulong,
            h,
        );
    } else {
        unfilter(upng, out, r#in, w, h, bpp); /*we can immediatly filter into the out
        buffer, no other steps needed */
    }
}

fn determine_format(upng: *mut upng_t) -> upng_format {
    match unsafe { (*upng).color_type } {
        upng_color::UPNG_LUM => match unsafe { (*upng).color_depth } {
            1 => upng_format::UPNG_LUMINANCE1,

            2 => upng_format::UPNG_LUMINANCE2,

            4 => upng_format::UPNG_LUMINANCE4,

            8 => upng_format::UPNG_LUMINANCE8,

            _ => upng_format::UPNG_BADFORMAT,
        },
        upng_color::UPNG_RGB => match unsafe { (*upng).color_depth } {
            8 => upng_format::UPNG_RGB8,

            16 => upng_format::UPNG_RGB16,

            _ => upng_format::UPNG_BADFORMAT,
        },
        upng_color::UPNG_LUMA => match unsafe { (*upng).color_depth } {
            1 => upng_format::UPNG_LUMINANCE_ALPHA1,
            2 => upng_format::UPNG_LUMINANCE_ALPHA2,
            4 => upng_format::UPNG_LUMINANCE_ALPHA4,
            8 => upng_format::UPNG_LUMINANCE_ALPHA8,
            _ => upng_format::UPNG_BADFORMAT,
        },
        upng_color::UPNG_RGBA => match unsafe { (*upng).color_depth } {
            8 => upng_format::UPNG_RGBA8,

            16 => upng_format::UPNG_RGBA16,

            _ => upng_format::UPNG_BADFORMAT,
        },
    }
}

fn upng_free_source(upng: *mut upng_t) {
    unsafe {
        if (*upng).source.owning != 0 {
            free((*upng).source.buffer.cast_mut() as *mut c_void);
        }

        (*upng).source.buffer = ptr::null_mut();
        (*upng).source.size = 0;
        (*upng).source.owning = 0;
    }
}

fn upng_new() -> *mut upng_t {
    let upng = unsafe { malloc(size_of::<upng_t>()) as *mut upng_t };
    if upng.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*upng).buffer = ptr::null_mut();
        (*upng).size = 0;

        (*upng).height = 0;
        (*upng).width = 0;

        (*upng).color_type = upng_color::UPNG_RGBA;
        (*upng).color_depth = 0;
        (*upng).format = upng_format::UPNG_RGB8;

        (*upng).state = upng_state::UPNG_NEW;

        (*upng).error = upng_error::UPNG_EOK;
        (*upng).error_line = 0;

        (*upng).source.buffer = ptr::null_mut();
        (*upng).source.size = 0;
        (*upng).source.owning = 0
    }

    upng
}

#[unsafe(no_mangle)]
pub extern "C" fn upng_new_from_bytes(buffer: *const c_uchar, size: c_ulong) -> *mut upng_t {
    let upng = upng_new();
    if upng.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*upng).source.buffer = buffer;
        (*upng).source.size = size;
        (*upng).source.owning = 0;
    }

    upng
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_new_from_file(filename: *const c_char) -> *mut upng_t {
    let upng = upng_new();
    if upng.is_null() {
        return ptr::null_mut();
    }

    let file = unsafe { fopen(filename, c"rb".as_ptr()) };
    if file.is_null() {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
        return upng;
    }

    // get filesize
    unsafe { fseek(file, 0, SEEK_END) };
    let size: c_long = unsafe { ftell(file) };
    unsafe { rewind(file) };

    // read contents of the file into the vector
    let buffer: *mut c_uchar = unsafe { malloc(size as usize) as *mut c_uchar };

    if buffer.is_null() {
        unsafe { fclose(file) };
        SET_ERROR(upng, upng_error::UPNG_ENOMEM);
        return upng;
    }

    unsafe { fread(buffer as *mut c_void, 1, size as size_t, file) };
    unsafe { fclose(file) };

    unsafe {
        (*upng).source.buffer = buffer;
        (*upng).source.size = size as c_ulong;
        (*upng).source.owning = 1;
    }

    upng
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_free(upng: *mut upng_t) {
    // deallocate image buffer
    unsafe {
        if !(*upng).buffer.is_null() {
            free((*upng).buffer.cast());
        }
    }

    // deallocate source buffer, if necessary
    upng_free_source(upng);

    // deallocate struct itself
    unsafe { free(upng as *mut c_void) };
}

/*read the information from the header and store it in the upng_Info. return
* value is error*/
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_header(upng: *mut upng_t) -> upng_error {
    // if we have an error state, bail now
    if unsafe { matches!((*upng).error, upng_error::UPNG_EOK) } {
        return unsafe { (*upng).error };
    }

    /* if the state is not NEW (meaning we are ready to parse the header), stop
     * now */
    unsafe {
        if matches!((*upng).state, upng_state::UPNG_NEW) {
            return (*upng).error;
        }
    }

    /* minimum length of a valid PNG file is 29 bytes
     * FIXME: verify this against the specification, or
     * better against the actual code below */
    if unsafe { (*upng).size < 29 } {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
        return unsafe { (*upng).error };
    }

    // check that PNG header matches expected value
    if unsafe {
        *(*upng).buffer.add(0) != 137
            || *(*upng).buffer.add(1) != 80
            || *(*upng).buffer.add(2) != 78
            || *(*upng).buffer.add(3) != 71
            || *(*upng).buffer.add(4) != 13
            || *(*upng).buffer.add(5) != 10
            || *(*upng).buffer.add(6) != 26
            || *(*upng).buffer.add(7) != 10
    } {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
        return unsafe { (*upng).error };
    }
    // check that the first chunk is the IHDR chunk
    if unsafe { MAKE_DWORD_PTR!((*upng).source.buffer.add(12)) != CHUNK_IHDR } {
        SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
        return unsafe { (*upng).error };
    }

    unsafe {
        // read the values given in the header
        (*upng).width = MAKE_DWORD_PTR!((*upng).buffer.add(16));
        (*upng).height = MAKE_DWORD_PTR!((*upng).buffer.add(20));
        (*upng).color_depth = *(*upng).source.buffer.add(24) as c_uint;
        (*upng).color_type = mem::transmute::<c_uchar, upng_color>(*(*upng).source.buffer.add(25));

        // determine the color format
        (*upng).format = determine_format(upng);
        if matches!((*upng).format, upng_format::UPNG_BADFORMAT) {
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
            return (*upng).error;
        }

        /* check that the compression method (byte 27) is 0 (only allowed value in
         * spec) */
        if *(*upng).source.buffer.add(26) != 0 {
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
            return (*upng).error;
        }

        /* check that the compression method (byte 27) is 0 (only allowed value in
         * spec) */
        if *(*upng).source.buffer.add(27) != 0 {
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
            return (*upng).error;
        }

        /* check that the compression method (byte 27) is 0 (spec allows 1, but uPNG
         * does not support it) */
        if *(*upng).source.buffer.add(28) != 0 {
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
            return (*upng).error;
        }

        (*upng).state = upng_state::UPNG_HEADER;
        (*upng).error
    }
}

/*read a PNG, the result will be in the same color type as the PNG (hence
* "generic")*/
#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_decode(upng: *mut upng_t) -> upng_error {
    let mut chunk: *const c_uchar;

    let mut compressed_size: c_ulong = 0;
    let mut compressed_index: c_ulong = 0;

    // if we have an error state, bail now
    unsafe {
        if !matches!((*upng).error, upng_error::UPNG_EOK) {
            return (*upng).error;
        }
    }

    // parse the main header, if necessary
    unsafe { upng_header(upng) };
    unsafe {
        if !matches!((*upng).error, upng_error::UPNG_EOK) {
            return (*upng).error;
        }
    }

    /* if the state is not HEADER (meaning we are ready to decode the image), stop
     * now */
    unsafe {
        if !matches!((*upng).state, upng_state::UPNG_HEADER) {
            return (*upng).error;
        }
    }

    /* release old result, if any */
    unsafe {
        if !(*upng).buffer.is_null() {
            free((*upng).buffer as *mut c_void);
            (*upng).buffer = ptr::null_mut();
            (*upng).size = 0;
        }
    }

    // first byte of the first chunk after the header
    chunk = unsafe { (*upng).source.buffer.add(33) };

    /* scan through the chunks, finding the size of all IDAT chunks, and also
     * verify general well-formed-ness */
    while chunk < unsafe { (*upng).source.buffer.add((*upng).source.size as usize) } {
        // make sure chunk header is not larger than the total compressed
        if unsafe {
            (chunk.sub(*(*upng).source.buffer.add(12) as usize) as usize)
                >= (*upng).source.size as usize
        } {
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
            return unsafe { (*upng).error };
        }

        // get length; sanity check it
        let length = unsafe { upng_chunk_length!(chunk) };
        if length > INT_MAX as c_uint {
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
            return unsafe { (*upng).error };
        }

        // make sure chunk header+paylaod is not larger than the total compressed
        if unsafe {
            *(chunk.sub((*upng).source.buffer.add(length as usize + 12) as usize)) as usize
                > (*upng).source.size as usize
        } {
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
            return unsafe { (*upng).error };
        }

        // get pointer to payload
        let data: *mut c_uchar = unsafe { chunk.add(8).cast_mut() }; // the data in the chunk

        // parse chunks
        if unsafe { upng_chunk_type!(chunk) == CHUNK_IDAT } {
            compressed_size += length as c_ulong;
        } else if unsafe { upng_chunk_type!(chunk) == CHUNK_IEND } {
            break;
        } else if unsafe { upng_chunk_critical!(chunk) } {
            SET_ERROR(upng, upng_error::UPNG_EMALFORMED);
            return unsafe { (*upng).error };
        }

        unsafe {
            chunk = chunk.add((*chunk as u32 + upng_chunk_length!(chunk) + 12) as usize);
        }
    }

    // allocate enough space for the (compressed and filtered) image data

    let compressed: *const c_uchar = unsafe { malloc(compressed_size as usize) as *mut c_uchar };
    if compressed.is_null() {
        SET_ERROR(upng, upng_error::UPNG_ENOMEM);
        return unsafe { (*upng).error };
    }

    /* scan through the chunks again, this time copying the values into
     * our compressed buffer.  there's no reason to validate anything a second time. */
    chunk = unsafe { (*upng).source.buffer.add(33) };
    while chunk < unsafe { (*upng).source.buffer.add((*upng).source.size as usize) } {
        let length = 0;
        let data: *const c_uchar = unsafe { zeroed() };
        // parse chunks
        if unsafe { upng_chunk_type!(chunk) } == CHUNK_IDAT {
            unsafe {
                memcpy(
                    compressed.add(compressed_index as usize) as *mut c_void,
                    data as *const c_void,
                    length as usize,
                )
            };
            compressed_index += length as c_ulong;
        } else if unsafe { upng_chunk_type!(chunk) == CHUNK_IEND } {
            break;
        }

        unsafe {
            chunk = chunk.add((*chunk as u32 + upng_chunk_length!(chunk) + 12) as usize);
        }
    }

    // allocate space to store inflated (but still filtered) data
    let inflated_size: c_ulong = unsafe {
        ((((*upng).width * ((*upng).height * upng_get_bpp(upng) + 7)) / 8) + (*upng).height)
            as c_ulong
    };
    let inflated: *mut c_uchar = unsafe { malloc(inflated_size as usize) as *mut c_uchar };
    if inflated.is_null() {
        unsafe { free(compressed as *mut c_void) };
        SET_ERROR(upng, upng_error::UPNG_ENOMEM);
        return unsafe { (*upng).error };
    }

    // decompress image data
    let error: upng_error = uz_inflate(upng, inflated, inflated_size, compressed, compressed_index);
    if !matches!(error, upng_error::UPNG_EOK) {
        unsafe {
            free(compressed as *mut c_void);
            free(inflated as *mut c_void);
            return (*upng).error;
        }
    }

    // free the compressed compressed data
    unsafe { free(compressed as *mut c_void) };

    // allocate final image buffer
    unsafe {
        (*upng).size = ((*upng).height * (*upng).width * upng_get_bpp(upng)).div_ceil(8) as c_ulong;
        (*upng).buffer = malloc((*upng).size as usize) as *mut c_uchar;
        if (*upng).buffer.is_null() {
            free(inflated as *mut c_void);
            (*upng).size = 0;
            SET_ERROR(upng, upng_error::UPNG_ENOMEM);
            return (*upng).error;
        }
    }

    // unfilter scanlines
    post_process_scanlines(upng, unsafe { (*upng).buffer }, inflated, upng);
    unsafe { free(inflated as *mut c_void) };

    if unsafe { !matches!((*upng).error, upng_error::UPNG_EOK) } {
        unsafe {
            free((*upng).buffer as *mut c_void);
            (*upng).buffer = ptr::null_mut();
            (*upng).size = 0;
        }
    } else {
        unsafe {
            (*upng).state = upng_state::UPNG_DECODED;
        }
    }

    // we are done with our input buffer; free it if we own it
    upng_free_source(upng);

    unsafe { (*upng).error }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_get_error(upng: *const upng_t) -> upng_error {
    (unsafe { *upng }).error
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_get_error_line(upng: *const upng_t) -> c_uint {
    (unsafe { *upng }).error_line
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_get_width(upng: *const upng_t) -> c_uint {
    (unsafe { *upng }).width
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_get_height(upng: *const upng_t) -> c_uint {
    (unsafe { *upng }).height
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_get_bpp(upng: *const upng_t) -> c_uint {
    unsafe { upng_get_bitdepth(upng) * upng_get_components(upng) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_get_bitdepth(upng: *const upng_t) -> c_uint {
    (unsafe { *upng }).color_depth
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_get_components(upng: *const upng_t) -> c_uint {
    match unsafe { (*upng).color_type } {
        upng_color::UPNG_LUM => 1,
        upng_color::UPNG_RGB => 3,
        upng_color::UPNG_LUMA => 2,
        upng_color::UPNG_RGBA => 4,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_get_pixelsize(upng: *const upng_t) -> c_uint {
    let mut bits = unsafe { upng_get_bitdepth(upng) * upng_get_components(upng) };
    bits += bits % 8;
    bits
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_get_format(upng: *const upng_t) -> upng_format {
    unsafe { (*upng).format }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_get_buffer(upng: *const upng_t) -> *const c_uchar {
    (unsafe { *upng }).buffer
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn upng_get_size(upng: *const upng_t) -> c_uint {
    (unsafe { *upng }).size as c_uint
}
