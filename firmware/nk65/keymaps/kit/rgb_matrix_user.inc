// SPDX-License-Identifier: GPL-2.0-or-later (built into QMK)
// kit's lighting effects, run by the keyboard itself (QMK RGB matrix).
// They come after QMK's own effects, so the stock effect numbers stay the same.
//
// All of them use the color (hue, saturation), brightness and speed set in kit
// or VIA. Math is in float: the NK65's STM32F303 has an FPU, and the frame
// budget is tiny next to the 67 LEDs.

RGB_MATRIX_EFFECT(KIT_RIPPLE)
RGB_MATRIX_EFFECT(KIT_PRISM)
RGB_MATRIX_EFFECT(KIT_INK)
RGB_MATRIX_EFFECT(KIT_AFTERGLOW)
RGB_MATRIX_EFFECT(KIT_AURORA)
RGB_MATRIX_EFFECT(KIT_LAVA)
RGB_MATRIX_EFFECT(KIT_TIDE)
RGB_MATRIX_EFFECT(KIT_FIREFLIES)

#ifdef RGB_MATRIX_CUSTOM_EFFECT_IMPLS

#    include <math.h>

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

// The set color, with its hue moved by `hue` (0-255 = one turn), saturation
// scaled by `sat` and value `v` (0-1) of the set brightness.
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

// Rings that spread out from each key you press. Ripple: on a dim base, each
// ring tinted by where on the board it started. Prism: thin rings on black
// that run through the rainbow as they travel.
static bool kit_ripple(effect_params_t *params, bool prism) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    float speed = 70.0f + 110.0f * kit_speed(); // board units per second (the board is 224 wide)
    float width = prism ? 11.0f : 16.0f;
    float reach = prism ? 200.0f : 160.0f;
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float best = 0.0f, hue = 0.0f;
        for (uint8_t j = 0; j < g_last_hit_tracker.count; j++) {
            float r    = g_last_hit_tracker.tick[j] / 1000.0f * speed;
            float fade = 1.0f - r / reach;
            if (fade <= 0.0f) continue;
            float d   = kit_dist(i, g_last_hit_tracker.x[j], g_last_hit_tracker.y[j]);
            float off = fabsf(d - r);
            if (off >= width) continue;
            float a = kit_smooth(1.0f - off / width) * fade * fade;
            if (a > best) {
                best = a;
                hue  = prism ? d * 1.2f : g_last_hit_tracker.x[j] * 0.3f;
            }
        }
        float base = prism ? 0.0f : 0.12f;
        kit_set(i, hue, 1.0f, base + (1.0f - base) * best);
    }
    return rgb_matrix_check_finished_leds(led_max);
}

bool KIT_RIPPLE(effect_params_t *params) {
    return kit_ripple(params, false);
}

bool KIT_PRISM(effect_params_t *params) {
    return kit_ripple(params, true);
}

// Each press drops a blot of color that spreads softly and fades; blots that
// overlap mix their colors. Every key has its own shade near the set color.
bool KIT_INK(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    float sp = kit_speed();
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float r = 0.0f, g = 0.0f, b = 0.0f;
        for (uint8_t j = 0; j < g_last_hit_tracker.count; j++) {
            float age = g_last_hit_tracker.tick[j] / 1000.0f;
            float amp = expf(-age * sp / 1.4f);
            if (amp < 0.01f) continue;
            float radius = 8.0f + 34.0f * (1.0f - expf(-age * sp * 2.5f));
            float d      = kit_dist(i, g_last_hit_tracker.x[j], g_last_hit_tracker.y[j]);
            float a      = kit_smooth((radius - d) / 14.0f + 0.5f) * amp;
            if (a <= 0.0f) continue;
            hsv_t hsv = rgb_matrix_config.hsv;
            hsv.h += (uint8_t)((g_last_hit_tracker.index[j] * 53u) % 96u) - 48;
            hsv.v     = 255;
            rgb_t c   = hsv_to_rgb_nocie(hsv);
            r += c.r * a;
            g += c.g * a;
            b += c.b * a;
        }
        // A faint base so the board isn't dark between presses.
        hsv_t base = rgb_matrix_config.hsv;
        base.v     = 255;
        rgb_t c    = hsv_to_rgb_nocie(base);
        float v    = rgb_matrix_config.hsv.v / 255.0f;
        r          = (r + c.r * 0.05f) * v;
        g          = (g + c.g * 0.05f) * v;
        b          = (b + c.b * 0.05f) * v;
        rgb_matrix_set_color(i, r > 255.0f ? 255 : (uint8_t)r, g > 255.0f ? 255 : (uint8_t)g, b > 255.0f ? 255 : (uint8_t)b);
    }
    return rgb_matrix_check_finished_leds(led_max);
}

// Pressed keys light up hot and cool off slowly, glowing a little onto their
// neighbors; the color drifts back to the set color as they cool.
bool KIT_AFTERGLOW(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    float tau = 2.0f / kit_speed();
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float best = 0.0f;
        for (uint8_t j = 0; j < g_last_hit_tracker.count; j++) {
            float amp = expf(-g_last_hit_tracker.tick[j] / 1000.0f / tau);
            if (amp < 0.01f) continue;
            float d    = kit_dist(i, g_last_hit_tracker.x[j], g_last_hit_tracker.y[j]);
            float near = kit_clamp01(1.0f - (d - 4.0f) / 18.0f);
            float a    = amp * near * near;
            if (a > best) best = a;
        }
        kit_set(i, 40.0f * best, 1.0f - 0.35f * best * best, 0.07f + 0.93f * best);
    }
    return rgb_matrix_check_finished_leds(led_max);
}

#    endif // RGB_MATRIX_KEYREACTIVE_ENABLED

// Slow curtains of light that sway across the board, shifting a little
// either side of the set color.
bool KIT_AURORA(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    float t = kit_time();
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float x       = g_led_config.point[i].x, y = g_led_config.point[i].y;
        float curtain = 0.5f + 0.5f * sinf(x * 0.035f + t * 0.6f + 1.6f * sinf(y * 0.05f + t * 0.3f));
        float swell   = 0.5f + 0.5f * sinf(x * 0.018f - t * 0.45f);
        float v       = curtain * curtain * (0.35f + 0.65f * swell);
        kit_set(i, 28.0f * sinf(x * 0.012f + t * 0.2f), 1.0f, 0.06f + 0.94f * v);
    }
    return rgb_matrix_check_finished_leds(led_max);
}

// Blobs that drift, merge and split, like a lava lamp seen from above.
bool KIT_LAVA(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    float t  = kit_time() * 0.6f;
    float cx = 112.0f + 70.0f * sinf(t * 0.37f), cy = 32.0f + 22.0f * sinf(t * 0.53f);
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float x = g_led_config.point[i].x, y = g_led_config.point[i].y;
        float p = sinf(x * 0.045f + t) + sinf(y * 0.09f - t * 0.7f) + sinf((x + y) * 0.03f + t * 0.5f) + sinf(sqrtf((x - cx) * (x - cx) + (y - cy) * (y - cy)) * 0.06f - t);
        float n = 0.5f + p / 8.0f; // 0-1
        kit_set(i, 36.0f * (n - 0.5f), 1.0f, 0.25f + 0.75f * kit_smooth(n * 1.3f - 0.15f));
    }
    return rgb_matrix_check_finished_leds(led_max);
}

// Waves rolling across the board; crests turn lighter and paler, like foam.
bool KIT_TIDE(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    float t = kit_time() * 1.4f;
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float x     = g_led_config.point[i].x, y = g_led_config.point[i].y;
        float wave  = 0.5f + 0.5f * sinf(x * 0.04f - t + 0.8f * sinf(y * 0.08f + t * 0.5f));
        float crest = wave * wave * wave;
        kit_set(i, 18.0f * crest, 1.0f - 0.6f * crest * crest, 0.18f + 0.82f * crest);
    }
    return rgb_matrix_check_finished_leds(led_max);
}

// Keys glow up and fade on their own, each at its own pace, mostly dark.
bool KIT_FIREFLIES(effect_params_t *params) {
    RGB_MATRIX_USE_LIMITS(led_min, led_max);
    float t = kit_time();
    for (uint8_t i = led_min; i < led_max; i++) {
        RGB_MATRIX_TEST_LED_FLAGS();
        float rate  = 0.35f + 0.6f * kit_hash(i, 0);
        float phase = 6.2832f * kit_hash(i, 1);
        float e     = sinf(t * rate + phase);
        float glow  = e > 0.55f ? kit_smooth((e - 0.55f) / 0.45f) : 0.0f;
        kit_set(i, 32.0f * (kit_hash(i, 2) - 0.5f), 1.0f, 0.03f + 0.97f * glow);
    }
    return rgb_matrix_check_finished_leds(led_max);
}

#endif // RGB_MATRIX_CUSTOM_EFFECT_IMPLS
