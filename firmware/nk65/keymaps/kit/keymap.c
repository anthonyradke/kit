// kit's NK65 keymap: QMK's default layout (VIA can change all of it), plus a
// small VIA channel that tells kit which effects this firmware has.
// SPDX-License-Identifier: GPL-2.0-or-later
#include QMK_KEYBOARD_H

const uint16_t PROGMEM keymaps[][MATRIX_ROWS][MATRIX_COLS] = {
[0] = LAYOUT_65_ansi(
    QK_GESC, KC_1,    KC_2,    KC_3,    KC_4,    KC_5,    KC_6,    KC_7,    KC_8,    KC_9,    KC_0,    KC_MINS, KC_EQL,  KC_BSPC, KC_HOME,
    KC_TAB,  KC_Q,    KC_W,    KC_E,    KC_R,    KC_T,    KC_Y,    KC_U,    KC_I,    KC_O,    KC_P,    KC_LBRC, KC_RBRC, KC_BSLS, KC_PGUP,
    KC_CAPS, KC_A,    KC_S,    KC_D,    KC_F,    KC_G,    KC_H,    KC_J,    KC_K,    KC_L,    KC_SCLN, KC_QUOT,          KC_ENT,  KC_PGDN,
    KC_LSFT, KC_Z,    KC_X,    KC_C,    KC_V,    KC_B,    KC_N,    KC_M,    KC_COMM, KC_DOT,  KC_SLSH, KC_RSFT,          KC_UP,   KC_END,
    KC_LCTL, KC_LGUI, KC_LALT,                   KC_SPC,                             KC_RALT, MO(1),   KC_RCTL, KC_LEFT, KC_DOWN, KC_RGHT),

[1] = LAYOUT_65_ansi(
    KC_GRV,  KC_F1,   KC_F2,   KC_F3,   KC_F4,   KC_F5,   KC_F6,   KC_F7,   KC_F8,   KC_F9,   KC_F10,  KC_F11,  KC_F12,  KC_DEL,  KC_TRNS,
    KC_TRNS, RM_TOGG, RM_NEXT, RM_PREV, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, QK_BOOT, KC_TRNS,
    KC_TRNS, RM_HUEU, RM_HUED, RM_SATU, RM_SATD, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS,          KC_TRNS, KC_TRNS,
    KC_TRNS, RM_VALU, RM_VALD, RM_SPDU, RM_SPDD, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS,          KC_TRNS, KC_TRNS,
    KC_VOLU, KC_VOLD, KC_MUTE,                   KC_TRNS,                            KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS),

[2] = LAYOUT_65_ansi(
    KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS,
    KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS,
    KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS,          KC_TRNS, KC_TRNS,
    KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS,          KC_TRNS, KC_TRNS,
    KC_TRNS, KC_TRNS, KC_TRNS,                   KC_TRNS,                            KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS),

[3] = LAYOUT_65_ansi(
    KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS,
    KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS,
    KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS,          KC_TRNS, KC_TRNS,
    KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS,          KC_TRNS, KC_TRNS,
    KC_TRNS, KC_TRNS, KC_TRNS,                   KC_TRNS,                            KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS, KC_TRNS),
};

// kit's values on VIA's custom channel:
//  get 0x6B ('k'): [firmware version, number of kit effects, first kit effect number,
//                   PCB pins that have seen a key press (bit 0 v1, bit 1 v1.4; from version 2)]
//  set 0x62 ('b'): restart into the bootloader, so kit can offer flashing without a key combo
#define KIT_FIRMWARE_VERSION 3
#define KIT_VALUE_INFO 0x6B
#define KIT_VALUE_BOOTLOADER 0x62

static bool kit_jump = false;
extern uint8_t kit_pcb_seen; // matrix.c

void via_custom_value_command_kb(uint8_t *data, uint8_t length) {
    // data = [command, channel, value id, value data...]
    if (data[1] == id_custom_channel) {
        if (data[0] == id_custom_get_value && data[2] == KIT_VALUE_INFO) {
            data[3] = KIT_FIRMWARE_VERSION;
            data[4] = RGB_MATRIX_EFFECT_MAX - RGB_MATRIX_CUSTOM_KIT_RIPPLE;
            data[5] = RGB_MATRIX_CUSTOM_KIT_RIPPLE;
            data[6] = kit_pcb_seen;
            return;
        }
        if (data[0] == id_custom_set_value && data[2] == KIT_VALUE_BOOTLOADER) {
            // Jump after the reply has gone out.
            kit_jump = true;
            return;
        }
    }
    data[0] = id_unhandled;
}

// QMK's NK65 layout flags every LED as a plain key, so Alphas / mods had
// nothing to give the second color. Mark the modifiers, Esc, Backspace, Enter,
// the arrows and the right-hand column (from version 3).
static const uint8_t kit_mods[][2] = {
    {0, 0}, {0, 13}, {0, 14}, {1, 0}, {1, 14}, {2, 0}, {2, 13}, {2, 14}, {3, 0}, {3, 12}, {3, 13}, {3, 14},
    {4, 0}, {4, 1}, {4, 2}, {4, 9}, {4, 10}, {4, 11}, {4, 12}, {4, 13}, {4, 14},
};

void keyboard_post_init_user(void) {
    for (uint8_t i = 0; i < sizeof(kit_mods) / sizeof(kit_mods[0]); i++) {
        uint8_t led = g_led_config.matrix_co[kit_mods[i][0]][kit_mods[i][1]];
        if (led != NO_LED) g_led_config.flags[led] |= LED_FLAG_MODIFIER;
    }
}

void housekeeping_task_user(void) {
    static uint32_t asked = 0;
    if (kit_jump) {
        if (!asked) asked = timer_read32() | 1;
        if (timer_elapsed32(asked) > 50) reset_keyboard();
    }
}
