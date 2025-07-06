use std::env;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    cbindgen::Builder::new()
        .with_language(cbindgen::Language::C)
        .with_include_guard("UPNG_H")
        .with_header(HEADER)
        .with_crate(crate_dir)
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file("upng.h");
}

const HEADER: &str = "/*
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
*/";
