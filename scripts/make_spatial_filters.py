"""Build crates/spatial/data/ku100.bin, the virtual speaker filters for kit's surround.

Source: the SADIE II database (University of York, Apache License 2.0), subject D1, a Neumann KU100
dummy head, both at 48 kHz:
  D1_HRIR_SOFA/D1_48K_24bit_256tap_FIR_SOFA.sofa   free-field HRIRs, 8802 directions
  D1_BRIR_SOFA/D1_48K_24bit_0.3s_FIR_SOFA.sofa     room responses (treated listening room), 50 directions
Download: https://zenodo.org/records/12092466 (D1_HRIR_SOFA.zip, D1_BRIR_SOFA.zip).

Each virtual speaker gets three responses per ear, all on one timeline starting at sample 0:
  direct  the HRIR at the speaker's exact angle
  early   the room's first 35 ms from the nearest measured horizontal direction, without its direct sound
  tail    35-250 ms of room, shared by every speaker (the late reverb is diffuse anyway, and sharing it
          makes the renderer about three times cheaper)
The renderer plays direct + room * (early + tail), so the Room slider changes only the room level.
Room is calibrated so room = 1 is the measured room at its natural level relative to the direct sound.

Usage: python -I scripts/make_spatial_filters.py HRIR.sofa BRIR.sofa crates/spatial/data/ku100.bin
Needs h5py, numpy.
"""

import struct
import sys

import h5py
import numpy as np

FS = 48000
MS = FS // 1000
# Azimuths kit uses: positive is to the left, 0 straight ahead.
AZIMUTHS = [0, 15, -15, 30, -30, 100, -100, 110, -110, 145, -145, 180]
# The 7.1 layout whose rooms are averaged into the shared tail.
TAIL_SPEAKERS = [30, -30, 0, 100, -100, 145, -145]
DIRECT_END = 3 * MS  # the room starts 3 ms after the direct sound (first reflection is at ~4.9 ms)
FADE_IN = 1 * MS
XFADE = (30 * MS, 35 * MS)  # early fades out and tail fades in here
TAIL_END = (200 * MS, 250 * MS)  # tail fades out here


def unit(az, el):
    a, e = np.radians(az), np.radians(el)
    return np.stack([np.cos(e) * np.cos(a), np.cos(e) * np.sin(a), np.sin(e)], -1)


def nearest(pos, az, el=0.0, horizontal_only=False):
    cand = np.arange(len(pos))
    if horizontal_only:
        cand = cand[np.abs(pos[:, 1]) < 1]
    i = cand[np.argmax(unit(pos[cand, 0], pos[cand, 1]) @ unit(az, el))]
    return int(i)


def band_energy(x, lo=500, hi=4000):
    spec = np.abs(np.fft.rfft(x, 8192)) ** 2
    f = np.fft.rfftfreq(8192, 1 / FS)
    return spec[(f >= lo) & (f < hi)].mean()


def ramp(n, rising):
    t = (np.arange(n) + 0.5) / n
    return np.sin(t * np.pi / 2) if rising else np.cos(t * np.pi / 2)


def main(hrir_path, brir_path, out_path):
    h, b = h5py.File(hrir_path, "r"), h5py.File(brir_path, "r")
    assert float(np.array(h["Data.SamplingRate"])[0]) == FS and float(np.array(b["Data.SamplingRate"])[0]) == FS
    hpos, hir = np.array(h["SourcePosition"]), np.array(h["Data.IR"])
    bpos, bir = np.array(b["SourcePosition"]), np.array(b["Data.IR"])

    directs = {az: hir[nearest(hpos, az)] for az in AZIMUTHS}
    for az in AZIMUTHS:
        print(f"az {az:4}: HRIR at {hpos[nearest(hpos, az)][:2]}, room from {bpos[nearest(bpos, az, horizontal_only=True)][:2]}")

    # Drop the silence every HRIR starts with, keeping their relative timing (the ITD).
    onset = min(int(np.argmax(np.abs(d).max(0) > 1e-3 * np.abs(d).max())) for d in directs.values())
    trim = max(onset - 8, 0)
    directs = {az: d[:, trim:] for az, d in directs.items()}

    # HRIRs and BRIRs come from different setups; match the BRIR's direct sound to the HRIR's level so
    # the room keeps its measured level relative to the direct sound.
    ratios = []
    for az in TAIL_SPEAKERS:
        y = bir[nearest(bpos, az, horizontal_only=True)]
        pk = int(np.argmax((y**2).sum(0)))
        ratios.append(sum(band_energy(directs[az][e]) for e in range(2)) / sum(band_energy(y[e, : pk + DIRECT_END]) for e in range(2)))
    room_scale = np.sqrt(np.mean(ratios))

    # Overall level: a sound in the front left speaker keeps its loudness (both ears together).
    gain = 1 / np.sqrt(sum(band_energy(directs[30][e]) for e in range(2)))

    length = TAIL_END[1]
    rooms = {}
    for az in AZIMUTHS:
        y = bir[nearest(bpos, az, horizontal_only=True)]
        pk_b = int(np.argmax((y**2).sum(0)))
        pk_h = int(np.argmax((directs[az] ** 2).sum(0)))
        room = np.zeros((2, length))
        # room[n] lines the BRIR's direct sound up with the HRIR's.
        src = np.arange(length) + pk_b - pk_h
        ok = (src >= 0) & (src < y.shape[1])
        room[:, ok] = y[:, src[ok]]
        w = np.zeros(length)
        start = pk_h + DIRECT_END
        w[start : start + FADE_IN] = ramp(FADE_IN, True)
        w[start + FADE_IN :] = 1
        rooms[az] = room * w * room_scale

    early_w = np.zeros(length)
    early_w[: XFADE[0]] = 1
    early_w[XFADE[0] : XFADE[1]] = ramp(XFADE[1] - XFADE[0], False)
    tail_w = np.zeros(length)
    tail_w[XFADE[0] : XFADE[1]] = ramp(XFADE[1] - XFADE[0], True)
    tail_w[XFADE[1] : TAIL_END[0]] = 1
    tail_w[TAIL_END[0] : TAIL_END[1]] = ramp(TAIL_END[1] - TAIL_END[0], False)

    tail = sum(rooms[az] for az in TAIL_SPEAKERS) / np.sqrt(len(TAIL_SPEAKERS)) * tail_w
    early_len = XFADE[1]

    with open(out_path, "wb") as f:
        f.write(b"KITSPAT1")
        f.write(struct.pack("<6I", FS, len(AZIMUTHS), directs[0].shape[1], early_len, length, onset - trim))
        for az in AZIMUTHS:
            f.write(struct.pack("<i", az))
            for ear in range(2):
                f.write((directs[az][ear] * gain).astype("<f4").tobytes())
            for ear in range(2):
                f.write((rooms[az][ear, :early_len] * early_w[:early_len] * gain).astype("<f4").tobytes())
        for ear in range(2):
            f.write((tail[ear] * gain).astype("<f4").tobytes())

    d = sum(band_energy(directs[0][e]) for e in range(2))
    r = sum(band_energy(rooms[0][e]) for e in range(2))
    print(f"trim {trim}, onset after trim {onset - trim}, room scale {room_scale:.3f}, gain {gain:.3f}")
    print(f"front direct-to-room ratio at room = 1: {10 * np.log10(d / r):.1f} dB")


if __name__ == "__main__":
    main(*sys.argv[1:4])
