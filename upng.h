/*
uPNG -- derived from LodePNG version 20100808

Copyright (c) 2005-2010 Lode Vandevenne
Copyright (c) 2010 Sean Middleditch

This software is provided 'as-is', without any express or implied
warranty. In no event will the authors be held liable for any damages
arising from the use of this software.

Permission is granted to anyone to use this software for any purpose,
including commercial applications, and to alter it and redistribute it
freely, subject to the following restrictions:

                1. The origin of this software must not be misrepresented; you
must not claim that you wrote the original software. If you use this software in
a product, an acknowledgment in the product documentation would be appreciated
but is not required.

                2. Altered source versions must be plainly marked as such, and
must not be misrepresented as being the original software.

                3. This notice may not be removed or altered from any source
                distribution.
*/

#ifndef UPNG_H
#define UPNG_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

enum upng_color {
  UPNG_LUM = 0,
  UPNG_RGB = 2,
  UPNG_LUMA = 4,
  UPNG_RGBA = 6,
};
typedef uint8_t upng_color;

typedef enum upng_error {
  UPNG_EOK = 0,
  UPNG_ENOMEM = 1,
  UPNG_ENOTFOUND = 2,
  UPNG_ENOTPNG = 3,
  UPNG_EMALFORMED = 4,
  UPNG_EUNSUPPORTED = 5,
  UPNG_EUNINTERLACED = 6,
  UPNG_EUNFORMAT = 7,
  UPNG_EPARAM = 8,
} upng_error;

typedef enum upng_format {
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
} upng_format;

typedef enum upng_state {
  UPNG_ERROR = -1,
  UPNG_DECODED = 0,
  UPNG_HEADER = 1,
  UPNG_NEW = 2,
} upng_state;

typedef struct upng_source {
  const unsigned char *buffer;
  unsigned long size;
  char owning;
} upng_source;

typedef struct upng_t {
  unsigned int width;
  unsigned int height;
  upng_color color_type;
  unsigned int color_depth;
  enum upng_format format;
  unsigned char *buffer;
  unsigned long size;
  enum upng_error error;
  unsigned int error_line;
  enum upng_state state;
  struct upng_source source;
} upng_t;

struct upng_t *upng_new_from_bytes(const unsigned char *buffer, unsigned long size);

struct upng_t *upng_new_from_file(const char *filename);

void upng_free(struct upng_t *upng);

enum upng_error upng_header(struct upng_t *upng);

enum upng_error upng_decode(struct upng_t *upng);

enum upng_error upng_get_error(const struct upng_t *upng);

unsigned int upng_get_error_line(const struct upng_t *upng);

unsigned int upng_get_width(const struct upng_t *upng);

unsigned int upng_get_height(const struct upng_t *upng);

unsigned int upng_get_bpp(const struct upng_t *upng);

unsigned int upng_get_bitdepth(const struct upng_t *upng);

unsigned int upng_get_components(const struct upng_t *upng);

unsigned int upng_get_pixelsize(const struct upng_t *upng);

enum upng_format upng_get_format(const struct upng_t *upng);

const unsigned char *upng_get_buffer(const struct upng_t *upng);

unsigned int upng_get_size(const struct upng_t *upng);

#endif  /* UPNG_H */
