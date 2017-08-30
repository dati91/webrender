/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifdef WR_DX11
struct a2vDebug {
    vec4 color : aColor;
    vec4 color_text_coord : aColorTexCoord;
    vec3 pos : aPosition;
};

struct v2p {
    vec4 gl_Position : SV_Position;
    vec4 vColor : vColor;
    vec2 vColorTexCoord : vColorTexCoord;
};
#else
in vec4 aColor;
in vec4 aColorTexCoord;

varying vec2 vColorTexCoord;
varying vec4 vColor;
#endif //WR_DX11

#ifndef WR_DX11
void main(void) {
#else
void main(in a2vDebug IN, out v2p OUT) {
    vec4 aColor = IN.color;
    vec4 aColorTexCoord = IN.color_text_coord;
    vec3 aPosition = IN.pos;
#endif //WR_DX11
    SHADER_OUT(vColor, aColor);
    SHADER_OUT(vColorTexCoord, vec2(aColorTexCoord.xy));
    vec4 pos = vec4(aPosition, 1.0);
    pos.xy = floor(pos.xy * uDevicePixelRatio + 0.5) / uDevicePixelRatio;
    SHADER_OUT(gl_Position, mul(pos, uTransform));
}
