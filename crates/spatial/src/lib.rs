//! Virtual surround for headphones.
//!
//! A multichannel stream (usually 7.1 from a game) is played through virtual
//! speakers around the listener: each channel is convolved with a Neumann
//! KU100 dummy head's response from that speaker's direction, plus a measured
//! listening room whose level is adjustable. The result is stereo for the
//! headset. Everything here is plain Rust with no Windows dependencies.

pub mod conv;
pub mod filters;
pub mod layout;
pub mod limiter;
pub mod pacer;
pub mod render;

pub use layout::{speakers, Speaker, MASK_5_1, MASK_7_1, MASK_STEREO};
pub use pacer::Pacer;
pub use render::{room_level, Renderer, SpeakerTest};
