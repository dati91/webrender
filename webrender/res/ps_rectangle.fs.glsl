/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifndef WR_DX11
void main(void) {
#else
void main(in v2p IN, out p2f OUT) {
    vec4 vColor = IN.vColor;
#endif
    float alpha = 1.0;
#ifdef WR_FEATURE_TRANSFORM
    alpha = 0.0;
#ifdef WR_DX11
    vec3 vLocalPos = IN.vLocalPos;
#endif
    init_transform_fs(vLocalPos, alpha);
#endif

#ifdef WR_FEATURE_CLIP
	//TODO: Replace the two zero vec4
    alpha = min(alpha, do_clip(vec4(0.0, 0.0, 0.0, 0.0), vec4(0.0, 0.0, 0.0, 0.0)));
#endif
    SHADER_OUT(Target0, vColor * vec4(1.0, 1.0, 1.0, alpha));
}
