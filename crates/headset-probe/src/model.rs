//! What the curves should be on paper, to set against the measurements.

use arctis::{Band, Filter as HsFilter};
use kit_eq::{Curve, Filter, Kind};

pub const RATE: f64 = 48000.0;

/// The rate Sonar runs its EQ at. Measured: its filters match RBJ biquads
/// designed at 96 kHz to 0.02 dB, and not at 48 kHz, where the bilinear
/// transform pulls treble boosts down near 20 kHz (by 1.9 dB at 20 kHz here).
/// The game path's fractional-sample delay fits 2x oversampling too.
pub const SONAR_RATE: f64 = 96000.0;

/// Sonar's Game "main preset" as Sonar stores it (`parametricEQ` of the
/// selected config in `ProgramData\SteelSeries\GG\apps\sonar\db\database.db`):
/// six peaking filters, no preamp (`generalGain` 0).
pub fn sonar_main_stored() -> Curve {
    let pk = |f, g, q| Filter::new(Kind::Peaking, f, g, q);
    Curve {
        preamp_db: 0.0,
        filters: vec![
            pk(55.0, 4.0, 0.707),
            pk(100.0, 3.0, 0.707),
            pk(600.0, 4.0, 4.0),
            pk(5000.0, 2.0, 0.707),
            pk(8000.0, 3.0, 0.707),
            pk(12000.0, 3.0, 0.707),
        ],
    }
}

/// Sonar's Bass Boost slider as measured at 1.5: a peaking filter at 80 Hz
/// with the slider's value as its gain and Q 0.707 (not a shelf).
pub fn sonar_bass_boost(value: f64) -> Filter {
    Filter::new(Kind::Peaking, 80.0, value, 0.707)
}

/// The whole Game path as identified, to evaluate at [`SONAR_RATE`].
pub fn sonar_main() -> Curve {
    let mut c = sonar_main_stored();
    c.filters.push(sonar_bass_boost(1.5));
    c
}

/// kit's Main preset (`app/src-tauri/src/headset/config.rs`, `seed_presets`).
pub fn kit_main() -> arctis::Eq {
    let pk = |freq, gain, q| Some(Band { filter: HsFilter::Peaking, freq, gain, q });
    [
        pk(55, 4.0, 0.707),
        pk(100, 3.0, 0.707),
        pk(600, 4.0, 4.0),
        pk(5000, 2.0, 0.707),
        pk(8000, 3.0, 0.707),
        pk(12000, 3.0, 0.707),
        pk(80, 1.5, 0.707),
        None,
        None,
        None,
    ]
}

/// What the headset's own EQ does with `eq`: the bytes it is sent decoded
/// back (gain in 0.5 dB steps, Q in thousandths, frequency clamped), as RBJ
/// biquads at 48 kHz, with no preamp because the headset has none. The
/// firmware's filter maths isn't documented; RBJ is the assumption kit's
/// graph and tests make too.
pub fn headset_curve(eq: &arctis::Eq) -> Curve {
    let sent = arctis::decode_eq(&arctis::encode_eq(eq));
    Curve {
        preamp_db: 0.0,
        filters: sent
            .iter()
            .flatten()
            .map(|b| {
                let kind = match b.filter {
                    HsFilter::Peaking => Kind::Peaking,
                    HsFilter::LowShelf => Kind::LowShelf,
                    HsFilter::HighShelf => Kind::HighShelf,
                    HsFilter::LowPass => Kind::LowPass,
                    HsFilter::HighPass => Kind::HighPass,
                };
                Filter::new(kind, b.freq as f64, b.gain as f64, b.q as f64)
            })
            .collect(),
    }
}

pub fn on_grid(curve: &Curve, rate: f64, grid: &[f64]) -> Vec<f64> {
    grid.iter().map(|&f| curve.response_db(f, rate)).collect()
}
