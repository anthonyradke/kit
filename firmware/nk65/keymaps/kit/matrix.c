// One matrix for both NK65 PCBs. v1 and v1.4 wire columns 7-9 (7 8 9, U I O,
// J K L, N M , and right Alt) to different pins: C13 C14 C15 on v1, B1 B0 A7
// on v1.4. Columns are inputs with pull-ups, so reading both sets is harmless;
// a pin that already reads low with no row selected is left out, so whatever
// it is wired to on the other PCB can't press keys.
// SPDX-License-Identifier: GPL-2.0-or-later
#include "matrix.h"
#include "gpio.h"
#include "wait.h"

static const pin_t row_pins[MATRIX_ROWS] = {B3, B4, B5, A8, A4};
static const pin_t col_pins[MATRIX_COLS] = {A13, A10, A9, A14, A15, B8, B9, NO_PIN, NO_PIN, NO_PIN, A0, A1, A2, A3, A5};
#define SHARED_FIRST 7
#define SHARED 3
static const pin_t shared_pins[2][SHARED] = {{C13, C14, C15}, {B1, B0, A7}};
static bool shared_ok[2][SHARED];
// Which pin set has seen a key press (bit 0 v1, bit 1 v1.4), for kit's Device page.
uint8_t kit_pcb_seen = 0;

static void unselect_row(uint8_t row) {
    gpio_set_pin_input_high(row_pins[row]);
}

void matrix_init_custom(void) {
    for (uint8_t r = 0; r < MATRIX_ROWS; r++) unselect_row(r);
    for (uint8_t c = 0; c < MATRIX_COLS; c++) {
        if (col_pins[c] != NO_PIN) gpio_set_pin_input_high(col_pins[c]);
    }
    for (uint8_t s = 0; s < 2; s++) {
        for (uint8_t k = 0; k < SHARED; k++) gpio_set_pin_input_high(shared_pins[s][k]);
    }
    wait_us(100);
    for (uint8_t s = 0; s < 2; s++) {
        for (uint8_t k = 0; k < SHARED; k++) shared_ok[s][k] = gpio_read_pin(shared_pins[s][k]);
    }
}

bool matrix_scan_custom(matrix_row_t current_matrix[]) {
    bool changed = false;
    for (uint8_t r = 0; r < MATRIX_ROWS; r++) {
        gpio_set_pin_output(row_pins[r]);
        gpio_write_pin_low(row_pins[r]);
        matrix_output_select_delay();
        matrix_row_t row = 0;
        for (uint8_t c = 0; c < MATRIX_COLS; c++) {
            if (col_pins[c] != NO_PIN && !gpio_read_pin(col_pins[c])) row |= (matrix_row_t)1 << c;
        }
        for (uint8_t s = 0; s < 2; s++) {
            for (uint8_t k = 0; k < SHARED; k++) {
                if (shared_ok[s][k] && !gpio_read_pin(shared_pins[s][k])) {
                    row |= (matrix_row_t)1 << (SHARED_FIRST + k);
                    kit_pcb_seen |= 1 << s;
                }
            }
        }
        unselect_row(r);
        matrix_output_unselect_delay(r, row != 0);
        if (current_matrix[r] != row) {
            current_matrix[r] = row;
            changed = true;
        }
    }
    return changed;
}
