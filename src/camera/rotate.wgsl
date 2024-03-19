struct RotationConfig {
    degree : i32,
}

@group(0) @binding(0) var input_texture : texture_2d<f32>;
@group(0) @binding(1) var output_texture : texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<storage, read> config : RotationConfig;

@compute @workgroup_size(16,16)
fn main(@builtin(global_invocation_id) global_id : vec3u) {
    let dimensions = textureDimensions(input_texture);
    let coords = vec2<i32>(global_id.xy);

    if(coords.x >= i32(dimensions.x) || coords.y >= i32(dimensions.y)) {
        return;
    }

    let pixel = textureLoad(input_texture, coords.xy, 0);

    let output_dim = textureDimensions(output_texture);

    if config.degree == 90{
        // 旋转90度
        // 原图 60x30 旋转90度后= 30x60
        // (0, 0) => (29, 0)
        // (1, 0) => (29, 1)
        // (2, 0) => (29, 2)
        // 即 (x, y) => (output_dim.x-y, x)
        textureStore(output_texture, vec2<i32>(i32(output_dim.x) - coords.y, coords.x), pixel);
    }else if config.degree == 180{
        // 旋转180度
        // 原图 60x30 旋转180度后 = 仍然是 60x30
        // (0, 0) => (29, 59)
        // (1, 0) => (28, 59)
        // (2, 0) => (27, 59)
        // (2, 1) => (27, 58)
        // 即 (x, y) => (output_dim.x-x, output_dim.y-y)
        textureStore(output_texture, vec2<i32>(i32(output_dim.x) - coords.x, i32(output_dim.y) - coords.y), pixel);
    }else if config.degree == 270{
        // 旋转270度
        // 原图 60x30 旋转180度后 = 30x60
        // (0, 0) => (0, 59)
        // (1, 0) => (0, 58)
        // (2, 0) => (0, 57)
        // (2, 1) => (1, 58)
        // 即 (x, y) => (coords.y, output_dim.y-x)
        textureStore(output_texture, vec2<i32>(coords.y, i32(output_dim.y) - coords.x), pixel);
    }
}