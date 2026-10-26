// SPDX-License-Identifier: GPL-2.0-or-later (built into QMK)
// kit's NK65 firmware: stock QMK + VIA, plus kit's own lighting effects.
#pragma once

// More simultaneous ripples (QMK's default is 8).
#define LED_HITS_TO_REMEMBER 16
// After flashing (EEPROM is reset) the board starts on kit's Ripple.
#define RGB_MATRIX_DEFAULT_MODE RGB_MATRIX_CUSTOM_KIT_RIPPLE
#define RGB_MATRIX_DEFAULT_HUE 200
#define RGB_MATRIX_DEFAULT_SAT 200
#define RGB_MATRIX_DEFAULT_SPD 127
