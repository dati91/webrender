//#line 1

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifndef WR_DX11
void main(void) {
#else
void main(in v2p IN, out p2f OUT) {
    int vGradientAddress = IN.vGradientAddress;
    float vGradientRepeat = IN.vGradientRepeat;

    vec2 vScaledDir = IN.vScaledDir;
    vec2 vStartPoint = IN.vStartPoint;

    vec2 vTileSize = IN.vTileSize;
    vec2 vTileRepeat = IN.vTileRepeat;

    vec2 vPos = IN.vPos;

    vec4 gl_FragCoord = IN.Position;
#endif //WR_DX11
    vec2 pos = mod(vPos, vTileRepeat);

    if (pos.x >= vTileSize.x ||
        pos.y >= vTileSize.y) {
        discard;
    }

    float offset = dot(pos - vStartPoint, vScaledDir);

    SHADER_OUT(Target0, sample_gradient(vGradientAddress, offset, vGradientRepeat, gl_FragCoord));
}
