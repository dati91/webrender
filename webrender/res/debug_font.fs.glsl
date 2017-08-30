/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifdef WR_DX11
struct v2p {
    vec4 gl_Position : SV_Position;
    vec4 vColorTexCoord : vColorTexCoord;
    vec4 vColor : vColor;
};
#else
varying vec2 vColorTexCoord;
varying vec4 vColor;
#endif //WR_DX11

#ifndef WR_DX11
void main(void) {
#else
void main(in v2p IN, out p2f OUT) {
    vec4 vColor = IN.vColor;
    vec4 vColorTexCoord = IN.vColorTexCoord;
#endif //WR_DX11

#ifdef SERVO_ES2
    float alpha = texture(sColor0, vColorTexCoord.xy).a;
#else
    float alpha = texture(sColor0, vColorTexCoord.xy).r;
#endif
    SHADER_OUT(Target0, vec4(vColor.xyz, vColor.w * alpha));
}
