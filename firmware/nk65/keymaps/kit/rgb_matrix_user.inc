// SPDX-License-Identifier: GPL-2.0-or-later (built into QMK)
// kit's lighting effects, run by the keyboard itself (QMK RGB matrix).
// They come after QMK's own effects, so the stock effect numbers stay the same.
//
// From version 4 they use two colors: QMK's own (hue, saturation) and a second
// one kit sets on its custom VIA channel, each with a role per effect (a ring
// and what it fades to, water and crests), plus a background level, 0 = off
// between presses. Brightness and speed are QMK's. Math is in float: the
// NK65's STM32F303 has an FPU, and the frame budget is tiny next to the 67 LEDs.

RGB_MATRIX_EFFECT(KIT_RIPPLE)
RGB_MATRIX_EFFECT(KIT_PRISM)
RGB_MATRIX_EFFECT(KIT_INK)
RGB_MATRIX_EFFECT(KIT_AFTERGLOW)
RGB_MATRIX_EFFECT(KIT_AURORA)
RGB_MATRIX_EFFECT(KIT_LAVA)
RGB_MATRIX_EFFECT(KIT_TIDE)
RGB_MATRIX_EFFECT(KIT_FIREFLIES)
RGB_MATRIX_EFFECT(KIT_GRADIENT)

#ifdef RGB_MATRIX_CUSTOM_EFFECT_IMPLS

#    include <math.h>

// keymap.c: the second color, the background level and the lightness curve.
extern uint8_t kit_color2[2];
extern uint8_t kit_bg;
extern uint8_t kit_curve[256];

// 0.25x (speed 0) to 2x (speed 255).
static float kit_speed(void) {
    return 0.25f + 1.75f * rgb_matrix_config.speed / 255.0f;
}

// Seconds, scaled by speed. Wraps every ~18 hours of uptime (a small jump).
static float kit_time(void) {
    return (float)(g_rgb_timer % 65536000u) / 1000.0f * kit_speed();
}

static float kit_clamp01(float x) {
    return x < 0.0f ? 0.0f : x > 1.0f ? 1.0f : x;
}

static float kit_smooth(float x) {
    x = kit_clamp01(x);
    return x * x * (3.0f - 2.0f * x);
}

// The two colors at full value, worked out once per frame.
static rgb_t kit_c1, kit_c2;
static void kit_colors(void) {
    hsv_t a = rgb_matrix_config.hsv;
    a.v     = 255;
    kit_c1  = hsv_to_rgb_nocie(a);
    hsv_t b = {kit_color2[0], kit_color2[1], 255};
    kit_c2  = hsv_to_rgb_nocie(b);
}

// LED `i` at `t` of the way from color 1 to color 2, at `v` (0-1) of the set
// brightness. The brightness setting goes through the lightness curve; `v`
// scales the light itself, so the effects' shapes fade out to fully off. The
// blend keeps full value, so halfway isn't darker.
static void kit_px(uint8_t i, float t, float v) {
    t       = kit_clamp01(t);
    float r = kit_c1.r + (kit_c2.r - kit_c1.r) * t;
    float g = kit_c1.g + (kit_c2.g - kit_c1.g) * t;
    float b = kit_c1.b + (kit_c2.b - kit_c1.b) * t;
    float m = r > g ? (r > b ? r : b) : (g > b ? g : b);
    float k = m > 0.0f ? kit_curve[rgb_matrix_config.hsv.v] * kit_clamp01(v) / m : 0.0f;
    rgb_matrix_set_color(i, (uint8_t)(r * k + 0.5f), (uint8_t)(g * k + 0.5f), (uint8_t)(b * k + 0.5f));
}

// From the background level (0) to full (1).
static float kit_lift(float a) {
    float bg = kit_bg / 255.0f;
    return bg + (1.0f - bg) * a;
}

// Version 1-3 helper, still used by Prism ripple: the set color with its hue
// moved by `hue` (0-255 = one turn), saturation scaled by `sat`, value `v`.
static void kit_set(uint8_t i, float hue, float sat, float v) {
    hsv_t hsv = rgb_matrix_config.hsv;
    hsv.h += (uint8_t)(int16_t)hue;
    hsv.s = (uint8_t)(hsv.s * kit_clamp01(sat));
    hsv.v = (uint8_t)(rgb_matrix_config.hsv.v * kit_clamp01(v));
    rgb_t rgb = rgb_matrix_hsv_to_rgb(hsv);
    rgb_matrix_set_color(i, rgb.r, rgb.g, rgb.b);
}

static float kit_dist(uint8_t i, uint8_t x, uint8_t y) {
    float dx = (float)g_led_config.point[i].x - x;
    float dy = (float)g_led_config.point[i].y - y;
    return sqrtf(dx * dx + dy * dy);
}

// A fixed pseudo-random number per LED, 0-1.
static float kit_hash(uint8_t i, uint8_t salt) {
    uint32_t h = (i + 1u) * 2654435761u ^ (salt + 1u) * 40503u;
    h ^= h >> 15;
    h *= 2246822519u;
    h ^= h >> 13;
    return (h & 0xFFFF) / 65535.0f;
}

#    ifdef RGB_MATRIX_KEYREACTIVE_ENABLED

// Rings that spread out from each key you press, in color 1, turning to
// color 2 as they travel and fade. The background shows color 2.
bool KIT_RIPPLE(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    kit_colors();
    float speed = 70.0f + 110.0f * kit_speed(); // board units per second (the board is 224 wide)
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float best = 0.0f, travel = 1.0f;
        for (uint8_t j = 0; j < g_last_hit_tracker.count; j++) {
            float r    = g_last_hit_tracker.tick[j] / 1000.0f * speed;
            float fade = 1.0f - r / 160.0f;
            if (fade <= 0.0f) continue;
            float off = fabsf(kit_dist(i, g_last_hit_tracker.x[j], g_last_hit_tracker.y[j]) - r);
            if (off >= 20.0f) continue;
            float a = kit_smooth(1.0f - off / 20.0f) * fade * fade;
            if (a > best) {
                best   = a;
                travel = r / 160.0f;
            }
        }
        kit_px(i, 1.0f - best * (1.0f - travel), kit_lift(best));
    }
    return rgb_matrix_check_finished_leds(led_max);
}

// Thin rings on black that run through the rainbow as they travel (versions
// 1-3; kit no longer offers it).
bool KIT_PRISM(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    float speed = 70.0f + 110.0f * kit_speed();
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float best = 0.0f, hue = 0.0f;
        for (uint8_t j = 0; j < g_last_hit_tracker.count; j++) {
            float r    = g_last_hit_tracker.tick[j] / 1000.0f * speed;
            float fade = 1.0f - r / 200.0f;
            if (fade <= 0.0f) continue;
            float d   = kit_dist(i, g_last_hit_tracker.x[j], g_last_hit_tracker.y[j]);
            float off = fabsf(d - r);
            if (off >= 11.0f) continue;
            float a = kit_smooth(1.0f - off / 11.0f) * fade * fade;
            if (a > best) {
                best = a;
                hue  = d * 1.2f;
            }
        }
        kit_set(i, hue, 1.0f, best);
    }
    return rgb_matrix_check_finished_leds(led_max);
}

// Each press drops a blot that spreads softly and fades; blots that overlap
// mix. Every key's blot has its own shade between color 1 and color 2. The
// background shows color 1.
bool KIT_INK(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    kit_colors();
    float sp = kit_speed();
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float sum = 0.0f, tsum = 0.0f;
        for (uint8_t j = 0; j < g_last_hit_tracker.count; j++) {
            float age = g_last_hit_tracker.tick[j] / 1000.0f;
            float amp = expf(-age * sp / 1.4f);
            if (amp < 0.01f) continue;
            float radius = 8.0f + 34.0f * (1.0f - expf(-age * sp * 2.5f));
            float d      = kit_dist(i, g_last_hit_tracker.x[j], g_last_hit_tracker.y[j]);
            float a      = kit_smooth((radius - d) / 14.0f + 0.5f) * amp;
            if (a <= 0.0f) continue;
            sum += a;
            tsum += a * kit_hash(g_last_hit_tracker.index[j], 3);
        }
        kit_px(i, sum > 0.0f ? tsum / sum : 0.0f, kit_lift(kit_clamp01(sum)));
    }
    return rgb_matrix_check_finished_leds(led_max);
}

// Pressed keys light up in color 1 (hot) and cool slowly through color 2,
// glowing a little onto their neighbors.
bool KIT_AFTERGLOW(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    kit_colors();
    float tau = 2.0f / kit_speed();
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float best = 0.0f;
        for (uint8_t j = 0; j < g_last_hit_tracker.count; j++) {
            float amp = expf(-g_last_hit_tracker.tick[j] / 1000.0f / tau);
            if (amp < 0.01f) continue;
            float near = kit_clamp01(1.0f - (kit_dist(i, g_last_hit_tracker.x[j], g_last_hit_tracker.y[j]) - 4.0f) / 18.0f);
            float a    = amp * near * near;
            if (a > best) best = a;
        }
        kit_px(i, 1.0f - best, kit_lift(best));
    }
    return rgb_matrix_check_finished_leds(led_max);
}

#    endif // RGB_MATRIX_KEYREACTIVE_ENABLED

// Slow curtains of light swaying across the board, drifting between the two colors.
bool KIT_AURORA(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    kit_colors();
    float t = kit_time();
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float x       = g_led_config.point[i].x, y = g_led_config.point[i].y;
        float curtain = 0.5f + 0.5f * sinf(x * 0.035f + t * 0.6f + 1.6f * sinf(y * 0.05f + t * 0.3f));
        float swell   = 0.5f + 0.5f * sinf(x * 0.018f - t * 0.45f);
        kit_px(i, 0.5f + 0.5f * sinf(x * 0.012f + t * 0.2f), kit_lift(curtain * curtain * (0.35f + 0.65f * swell)));
    }
    return rgb_matrix_check_finished_leds(led_max);
}

// Blobs of color 1 that drift, merge and split over color 2, like a lava lamp seen from above.
bool KIT_LAVA(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    kit_colors();
    float t  = kit_time() * 0.6f;
    float cx = 112.0f + 70.0f * sinf(t * 0.37f), cy = 32.0f + 22.0f * sinf(t * 0.53f);
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float x = g_led_config.point[i].x, y = g_led_config.point[i].y;
        float p = sinf(x * 0.045f + t) + sinf(y * 0.09f - t * 0.7f) + sinf((x + y) * 0.03f + t * 0.5f) + sinf(sqrtf((x - cx) * (x - cx) + (y - cy) * (y - cy)) * 0.06f - t);
        float n = kit_smooth((0.5f + p / 8.0f) * 1.3f - 0.15f); // 0-1
        kit_px(i, 1.0f - n, kit_lift(0.3f + 0.7f * n));
    }
    return rgb_matrix_check_finished_leds(led_max);
}

// Waves of color 1 rolling across the board, with crests in color 2.
bool KIT_TIDE(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    kit_colors();
    float t = kit_time() * 1.4f;
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float x     = g_led_config.point[i].x, y = g_led_config.point[i].y;
        float wave  = 0.5f + 0.5f * sinf(x * 0.04f - t + 0.8f * sinf(y * 0.08f + t * 0.5f));
        float crest = wave * wave * wave;
        kit_px(i, crest * crest, kit_lift(0.18f + 0.82f * crest));
    }
    return rgb_matrix_check_finished_leds(led_max);
}

// Keys glow up and fade on their own, each at its own pace and in its own
// shade between the two colors; dark in between.
bool KIT_FIREFLIES(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    kit_colors();
    float t = kit_time();
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float rate  = 0.35f + 0.6f * kit_hash(i, 0);
        float phase = 6.2832f * kit_hash(i, 1);
        float e     = sinf(t * rate + phase);
        float glow  = e > 0.55f ? kit_smooth((e - 0.55f) / 0.45f) : 0.0f;
        kit_px(i, kit_hash(i, 2), kit_lift(glow));
    }
    return rgb_matrix_check_finished_leds(led_max);
}

// Color 1 on the left fading into color 2 on the right (from version 4).
bool KIT_GRADIENT(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    kit_colors();
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        kit_px(i, (g_led_config.point[i].x - 8) / 207.0f, 1.0f);
    }
    return rgb_matrix_check_finished_leds(led_max);
}

#endif // RGB_MATRIX_CUSTOM_EFFECT_IMPLS
