
@group(0) @binding(0)
var ytexture: texture_2d<f32>;
@group(0) @binding(1)
var uvtexture: texture_2d<f32>;
@group(0) @binding(2)
var uvsamp: sampler;
@group(0) @binding(3) 
var rgbstorage : texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8,8,1)
fn main(@builtin(workgroup_id) WorkGroupID : vec3<u32>,
  @builtin(global_invocation_id) global_id: vec3<u32>,
  @builtin(local_invocation_id) LocalInvocationID : vec3<u32>) {
    let ydims = vec2<i32>(textureDimensions(ytexture, 0));
    let uvdims = vec2<i32>(textureDimensions(uvtexture, 0));
    let baseIndex : vec2<i32> = vec2<i32>(global_id.xy);
      
    let y:f32 = textureLoad(
      ytexture,
      baseIndex,
      0
    ).r - 0.0625;
    
    let v:f32 = textureSampleLevel(
      uvtexture,
      uvsamp,
      vec2<f32>(baseIndex) / vec2<f32>(ydims), 
      0.0 
    ).r - 0.5;

    let u:f32 = textureSampleLevel(
      uvtexture,
      uvsamp,
      vec2<f32>(baseIndex) / vec2<f32>(ydims), 
      0.0
    ).g - 0.5;

    var r = 1.164 * (y) + 1.596 * (v);
    var g = 1.164 * (y) - 0.813 * (v) - 0.392 * (u);
    var b = 1.164 * (y) + 2.017 * (u);
    var rgb : vec3<f32> = vec3<f32>(r,g,b);
    // rgb = pow(rgb,vec3<f32>(2.2));

    textureStore(rgbstorage, baseIndex, vec4<f32>(rgb, 1.0));
}