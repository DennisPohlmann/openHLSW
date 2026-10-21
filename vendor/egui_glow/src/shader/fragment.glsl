#ifdef GL_ES
    // To avoid weird distortion issues when rendering text etc, we want highp if possible.
    // But apparently some devices don't support it, so we have to check first.
    #if defined(GL_FRAGMENT_PRECISION_HIGH) && GL_FRAGMENT_PRECISION_HIGH == 1
        precision highp float;
    #else
        precision mediump float;
    #endif
#endif

uniform sampler2D u_sampler;

#if NEW_SHADER_INTERFACE
    in vec4 v_rgba_in_gamma;
    in vec2 v_tc;
    #if DUAL_SOURCE
        // openHLSW: second output = the blend factor of each color channel (sub-pixel text).
        layout(location = 0, index = 0) out vec4 f_color;
        layout(location = 0, index = 1) out vec4 f_blend;
    #else
        out vec4 f_color;
    #endif
    // a dirty hack applied to support webGL2
    #define gl_FragColor f_color
    #define texture2D texture
#else
    varying vec4 v_rgba_in_gamma;
    varying vec2 v_tc;
#endif

// -----------------------------------------------
// Adapted from
// https://www.shadertoy.com/view/llVGzG
// Originally presented in:
// Jimenez 2014, "Next Generation Post-Processing in Call of Duty"
//
// A good overview can be found in
// https://blog.demofox.org/2022/01/01/interleaved-gradient-noise-a-different-kind-of-low-discrepancy-sequence/
// via https://github.com/rerun-io/rerun/
float interleaved_gradient_noise(vec2 n) {
    float f = 0.06711056 * n.x + 0.00583715 * n.y;
    return fract(52.9829189 * fract(f));
}

vec3 dither_interleaved(vec3 rgb, float levels) {
    float noise = interleaved_gradient_noise(gl_FragCoord.xy);
    // scale down the noise slightly to ensure flat colors aren't getting dithered
    noise = (noise - 0.5) * 0.95;
    return rgb + noise / (levels - 1.0);
}

void main() {
    vec4 texture_in_gamma = texture2D(u_sampler, v_tc);

    // openHLSW: sub-pixel (LCD) text texels store the coverage of each color stripe in RGB and
    // the smallest as alpha, so their color is above their alpha. Everything else egui draws is
    // premultiplied (color <= alpha), where `coverage` is just the alpha.
    vec3 coverage = max(texture_in_gamma.rgb, vec3(texture_in_gamma.a));
#if DUAL_SOURCE
    // Each channel of the background is covered by its own amount.
    f_blend = vec4(v_rgba_in_gamma.a * coverage, v_rgba_in_gamma.a * texture_in_gamma.a);
#else
    // No per-channel blending: draw sub-pixel texels as gray coverage.
    if (any(greaterThan(texture_in_gamma.rgb, vec3(texture_in_gamma.a)))) {
        texture_in_gamma = vec4((coverage.r + coverage.g + coverage.b) / 3.0);
    }
#endif

    // We multiply the colors in gamma space, because that's the only way to get text to look right.
    vec4 frag_color_gamma = v_rgba_in_gamma * texture_in_gamma;

    // Dither the float color down to eight bits to reduce banding.
    // This step is optional for egui backends.
#if DITHERING
    frag_color_gamma.rgb = dither_interleaved(frag_color_gamma.rgb, 256.);
#endif
    gl_FragColor = frag_color_gamma;
}
