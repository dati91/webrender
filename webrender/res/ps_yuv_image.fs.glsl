/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#if !defined(WR_FEATURE_YUV_REC601) && !defined(WR_FEATURE_YUV_REC709)
#define WR_FEATURE_YUV_REC601
#endif

// The constants added to the Y, U and V components are applied in the fragment shader.
#if defined(WR_FEATURE_YUV_REC601)
// From Rec601:
// [R]   [1.1643835616438356,  0.0,                 1.5960267857142858   ]   [Y -  16]
// [G] = [1.1643835616438358, -0.3917622900949137, -0.8129676472377708   ] x [U - 128]
// [B]   [1.1643835616438356,  2.017232142857143,   8.862867620416422e-17]   [V - 128]
//
// For the range [0,1] instead of [0,255].
//
// The matrix is stored in column-major.
static const mat3 YuvColorMatrix = mat3(
    1.16438,  1.16438, 1.16438,
    0.0,     -0.39176, 2.01723,
    1.59603, -0.81297, 0.0
);
#elif defined(WR_FEATURE_YUV_REC709)
// From Rec709:
// [R]   [1.1643835616438356,  4.2781193979771426e-17, 1.7927410714285714]   [Y -  16]
// [G] = [1.1643835616438358, -0.21324861427372963,   -0.532909328559444 ] x [U - 128]
// [B]   [1.1643835616438356,  2.1124017857142854,     0.0               ]   [V - 128]
//
// For the range [0,1] instead of [0,255]:
//
// The matrix is stored in column-major.
static const mat3 YuvColorMatrix = mat3(
    1.16438,  1.16438,  1.16438,
    0.0    , -0.21325,  2.11240,
    1.79274, -0.53291,  0.0
);
#endif

#ifndef WR_DX11
void main(void) {
#else
void main(in v2p IN, out p2f OUT) {
    vec4 vClipMaskUvBounds = IN.vClipMaskUvBounds;
    vec3 vClipMaskUv = IN.vClipMaskUv;
    vec2 vTextureOffsetY = IN.vTextureOffsetY;
    vec2 vTextureOffsetU = IN.vTextureOffsetU;
    vec2 vTextureOffsetV = IN.vTextureOffsetV;
    vec2 vTextureSizeY = IN.vTextureSizeY;
    vec2 vTextureSizeUv = IN.vTextureSizeUv;
    vec2 vStretchSize = IN.vStretchSize;
    vec2 vHalfTexelY = IN.vHalfTexelY;
    vec2 vHalfTexelUv = IN.vHalfTexelUv;
    vec3 vLayers = IN.vLayers;
#ifdef WR_FEATURE_TRANSFORM
    vec3 vLocalPos = IN.vLocalPos;
    vec4 vLocalBounds = IN.vLocalBounds;
#else
    vec2 vLocalPos = IN.vLocalPos;
#endif //WR_FEATURE_TRANSFORM
#endif //WR_DX11
#ifdef WR_FEATURE_TRANSFORM
    float alpha = 0.0;
    vec2 pos = init_transform_fs(vLocalPos, vLocalBounds, alpha);

    // We clamp the texture coordinate calculation here to the local rectangle boundaries,
    // which makes the edge of the texture stretch instead of repeat.
    vec2 relative_pos_in_rect = clamp(pos, vLocalBounds.xy, vLocalBounds.zw) - vLocalBounds.xy;
#else
    float alpha = 1.0;
    vec2 relative_pos_in_rect = vLocalPos;
#endif

    alpha = min(alpha, do_clip(vClipMaskUvBounds, vClipMaskUv));

    // We clamp the texture coordinates to the half-pixel offset from the borders
    // in order to avoid sampling outside of the texture area.
    vec2 st_y = vTextureOffsetY + clamp(
        relative_pos_in_rect / vStretchSize * vTextureSizeY,
        vHalfTexelY, vTextureSizeY - vHalfTexelY);
#ifndef WR_FEATURE_INTERLEAVED_Y_CB_CR
    vec2 uv_offset = clamp(
        relative_pos_in_rect / vStretchSize * vTextureSizeUv,
        vHalfTexelUv, vTextureSizeUv - vHalfTexelUv);
    // NV12 only uses 2 textures. The sColor0 is for y and sColor1 is for uv.
    // The texture coordinates of u and v are the same. So, we could skip the
    // st_v if the format is NV12.
    vec2 st_u = vTextureOffsetU + uv_offset;
#endif

    vec3 yuv_value;
#ifdef WR_FEATURE_INTERLEAVED_Y_CB_CR
    // "The Y, Cb and Cr color channels within the 422 data are mapped into
    // the existing green, blue and red color channels."
    // https://www.khronos.org/registry/OpenGL/extensions/APPLE/APPLE_rgb_422.txt
    yuv_value = TEX_SAMPLE(sColor0, vec3(st_y, vLayers.x)).gbr;
#elif defined(WR_FEATURE_NV12)
    yuv_value.x = TEX_SAMPLE(sColor0, vec3(st_y, vLayers.x)).r;
    yuv_value.yz = TEX_SAMPLE(sColor1, vec3(st_u, vLayers.y)).rg;
#else
    // The yuv_planar format should have this third texture coordinate.
    vec2 st_v = vTextureOffsetV + uv_offset;

    yuv_value.x = TEX_SAMPLE(sColor0, vec3(st_y, vLayers.x)).r;
    yuv_value.y = TEX_SAMPLE(sColor1, vec3(st_u, vLayers.y)).r;
    yuv_value.z = TEX_SAMPLE(sColor2, vec3(st_v, vLayers.z)).r;
#endif

    // See the YuvColorMatrix definition for an explanation of where the constants come from.
    vec3 yuv_val = yuv_value - vec3(0.06275, 0.50196, 0.50196);
    vec3 rgb = mul(yuv_val, YuvColorMatrix);
    SHADER_OUT(Target0, vec4(rgb, alpha));
}
