/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifdef WR_DX11
struct v2p {
    vec4 gl_Position : SV_Position;
    vec4 vColor : vColor;
};
#else
varying vec4 vColor;
#endif //WR_DX11

#ifndef WR_DX11
void main(void) {
#else
void main(in v2p IN, out p2f OUT) {
    vec4 vColor = IN.vColor;
#endif //WR_DX11
    SHADER_OUT(Target0, vColor);
}
