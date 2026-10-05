//! Small OpenGL helpers: shader programs and texture upload.

use glow::HasContext;

pub unsafe fn program(gl: &glow::Context, vert: &str, frag: &str) -> glow::Program {
    let program = gl.create_program().expect("create program");
    let mut shaders = Vec::new();
    for (kind, src) in [(glow::VERTEX_SHADER, vert), (glow::FRAGMENT_SHADER, frag)] {
        let shader = gl.create_shader(kind).expect("create shader");
        gl.shader_source(shader, src);
        gl.compile_shader(shader);
        assert!(gl.get_shader_compile_status(shader), "shader compile error: {}", gl.get_shader_info_log(shader));
        gl.attach_shader(program, shader);
        shaders.push(shader);
    }
    gl.link_program(program);
    assert!(gl.get_program_link_status(program), "program link error: {}", gl.get_program_info_log(program));
    for shader in shaders {
        gl.detach_shader(program, shader);
        gl.delete_shader(shader);
    }
    program
}

/// Decode an 8-bit grayscale PNG and upload it as an R8 texture, halving its size (box filter)
/// while it is still at least `min_width` wide. The decoded pixels are dropped right after upload.
pub unsafe fn gray_png_texture(gl: &glow::Context, png_bytes: &[u8], mipmaps: bool, min_width: u32) -> glow::Texture {
    let decoder = png::Decoder::new(png_bytes);
    let mut reader = decoder.read_info().expect("png header");
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("png data");
    assert_eq!(info.color_type, png::ColorType::Grayscale, "expected grayscale png");
    assert_eq!(info.bit_depth, png::BitDepth::Eight, "expected 8-bit png");
    buf.truncate(info.buffer_size());
    let (mut w, mut h) = (info.width, info.height);
    while w / 2 >= min_width && w % 2 == 0 && h % 2 == 0 {
        buf = halve(&buf, w as usize, h as usize);
        w /= 2;
        h /= 2;
    }

    let tex = gl.create_texture().expect("create texture");
    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::R8 as i32,
        w as i32,
        h as i32,
        0,
        glow::RED,
        glow::UNSIGNED_BYTE,
        glow::PixelUnpackData::Slice(Some(&buf)),
    );
    let min = if mipmaps {
        gl.generate_mipmap(glow::TEXTURE_2D);
        glow::LINEAR_MIPMAP_LINEAR
    } else {
        glow::LINEAR
    };
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, min as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
    // Longitude wraps around the antimeridian, latitude does not.
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
    tex
}

/// Decode an 8-bit RGB PNG and upload it as a mipmapped RGB8 texture (longitude wraps).
pub unsafe fn rgb_png_texture(gl: &glow::Context, png_bytes: &[u8]) -> glow::Texture {
    let mut reader = png::Decoder::new(png_bytes).read_info().expect("png header");
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("png data");
    assert_eq!(info.color_type, png::ColorType::Rgb, "expected rgb png");
    assert_eq!(info.bit_depth, png::BitDepth::Eight, "expected 8-bit png");
    buf.truncate(info.buffer_size());

    let tex = gl.create_texture().expect("create texture");
    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::RGB8 as i32,
        info.width as i32,
        info.height as i32,
        0,
        glow::RGB,
        glow::UNSIGNED_BYTE,
        glow::PixelUnpackData::Slice(Some(&buf)),
    );
    gl.generate_mipmap(glow::TEXTURE_2D);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR_MIPMAP_LINEAR as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
    tex
}

/// 2x2 box downsample of an 8-bit single-channel image.
fn halve(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let (nw, nh) = (w / 2, h / 2);
    let mut out = vec![0u8; nw * nh];
    for y in 0..nh {
        for x in 0..nw {
            let i = 2 * y * w + 2 * x;
            let sum = src[i] as u32 + src[i + 1] as u32 + src[i + w] as u32 + src[i + w + 1] as u32;
            out[y * nw + x] = ((sum + 2) / 4) as u8;
        }
    }
    out
}
