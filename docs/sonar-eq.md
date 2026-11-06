# Matching Sonar's EQ

kit's headset EQ can run the way SteelSeries Sonar ran it, in software on the PC, without Sonar's virtual devices. This
page records what Sonar was measured to do, how kit reproduces it, and how to check both again.

## What Sonar does

Measured with `headset-probe capture`: a test program (log sweeps per channel, pink noise, a 0 dBFS sweep, sine steps
from -24 to 0 dBFS at 63 Hz, 1 kHz and 10 kHz, and a level jump) played into Sonar's devices while WASAPI loopback
recorded the Nova 5's own endpoint. The direct path (straight to the Nova 5) came back bit-exact, so the rig adds
nothing. The setup was Sonar's Game "Main" preset (55 Hz +4, 100 Hz +3, 600 Hz +4 Q 4, 5 kHz +2, 8 kHz +3,
12 kHz +3, Bass Boost 1.5), with spatial audio, volume boost, smart volume and the compressor off, and Chat flat.

- **Where it runs.** The Game device is 7.1 at 96 kHz with an endpoint effect (`RenderGameApo`,
  `{4B7758E3-9361-4F88-83E9-44D8F05184EF}`). The effect passes the audio to `SteelSeriesSonar.exe`, which plays it
  on the Nova 5. Sonar's settings are in SQLite at `C:\ProgramData\SteelSeries\GG\apps\sonar\db\database.db`.
- **The curve.** The six stored filters plus Bass Boost as a *peaking* filter at 80 Hz (the slider's value in dB,
  Q 0.707), all RBJ cookbook biquads designed at 96 kHz, with no preamp. That model matches the measurement to
  0.017 dB rms (0.08 dB at worst). The peak is +7.35 dB at 71 Hz, +0.61 dB at 1 kHz, and +4.0 dB broadband on pink
  noise. L and R are identical.
- **Level.** Nothing in Sonar limits. Above full scale, Windows' own engine limiter holds peaks at -0.13 dBFS
  (THD about -50 dB, fast attack, 100-250 ms release); the direct and Chat paths hit the same ceiling.
- **Chat** is flat (within 0.08 dB), 76 ms late. Game is 25-46 ms late, depending on the session.

kit's Main preset on the headset's own EQ came out 0.63 dB rms and 1.9 dB at worst from Sonar. Most of that is the
headset running its filters at 48 kHz: treble boosts there fall short near 20 kHz (-1.4 dB at 16 kHz, -1.9 dB at
20 kHz). The rest was Bass Boost modelled as a shelf (+1.3 dB at 20 Hz); Main now uses Sonar's peak.

## How kit matches it

The Sound page's **Run in kit** installs a second APO from `kit_apo.dll` (the noise suppression DLL) as the endpoint
effect on the Nova 5's output, so it runs after Windows has mixed every app: one output device, nothing to route. The
headset's own EQ is set flat while it's on. The app writes the curve to `HKLM\SOFTWARE\kit` (`HeadsetEq`, in
Equalizer APO's text format, and `HeadsetEqOn`); the APO's settings thread designs the filters and hands them to the
audio thread without blocking or allocating.

- **Filters** (`crates/eq`, `design.rs`): each one is designed at twice the rate, as Sonar's are, then matched at
  48 kHz. Its poles are squared, its numerator is a least-squares fit of the magnitude, and Gauss-Newton refines all
  five coefficients. For Main, that is within 0.1 dB of the 96 kHz filters up to 20 kHz (0.01 dB rms). The graph
  draws the 96 kHz response in this mode.
- **Processing:** 64-bit state, transposed direct form II, all channels alike.
- **Preamp:** *Auto* (the default) turns the curve down by its biggest boost, -7.4 dB for Main, so nothing is ever
  limited; turn the volume up to make up for it. *Off* is as loud as Sonar.
- **Limiter:** a stereo-linked lookahead peak limiter (1 ms lookahead, 120 ms release, ceiling -0.3 dBFS) catches
  what Off lets past full scale.

Measured offline, with the release DLL loaded and driven through its own COM interfaces and the curve read from the
registry (`headset-probe kit-dll`), it lands 0.017 dB rms (0.09 dB at worst) from the Sonar recording. It also
matches at +4.0 dB on pink noise and +0.61 dB at 1 kHz.

## Checking it again

```sh
cargo build -p kit-apo --release
cargo run -p headset-probe -- capture out game chat direct      # Sonar running, kit closed; the headset is muted meanwhile
cargo run -p headset-probe -- kit-dll target/release/kit_apo.dll out   # kit's DLL offline, compared in out/report.txt
cargo run -p headset-probe -- ab music.mp3 curve.txt            # blind ABX by ear: Sonar Game against kit in software
```

`ab` lines the two paths up first, with the headset muted. It matches their level within 0.1 dB and their timing to
the sample, then lets you switch A, B and X while the music plays, and reports a p-value. A curve file is Equalizer
APO text, for example the `kit-curve.txt` that `kit-dll` writes.

Once kit's EQ is installed and on (with Sonar quit), `capture out direct` measures it in place, since loopback
records after the endpoint effect.
