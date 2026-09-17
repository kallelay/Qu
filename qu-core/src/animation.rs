//! Exact-clock keyframe sampling shared by plots, scenes, and physics playback.

use crate::geometry::Vec3;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform3 {
    pub translation: Vec3,
    /// Euler angles in radians for the reference representation.
    pub rotation: Vec3,
    pub scale: Vec3,
}

impl Transform3 {
    pub const IDENTITY: Self = Self {
        translation: Vec3::new(0.0, 0.0, 0.0),
        rotation: Vec3::new(0.0, 0.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
    };

    pub fn lerp(self, other: Self, amount: f64) -> Self {
        Self {
            translation: self.translation.lerp(other.translation, amount),
            rotation: self.rotation.lerp(other.rotation, amount),
            scale: self.scale.lerp(other.scale, amount),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Easing {
    Linear,
    In,
    Out,
    InOut,
}

impl Easing {
    fn apply(self, amount: f64) -> f64 {
        match self {
            Self::Linear => amount,
            Self::In => amount * amount,
            Self::Out => 1.0 - (1.0 - amount) * (1.0 - amount),
            Self::InOut if amount < 0.5 => 2.0 * amount * amount,
            Self::InOut => 1.0 - (-2.0 * amount + 2.0).powi(2) / 2.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    Clamp,
    Repeat,
    PingPong,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keyframe3 {
    pub time: f64,
    pub transform: Transform3,
    /// Easing used from this keyframe to the next.
    pub ease: Easing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationClip3 {
    frames: Vec<Keyframe3>,
    duration: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AnimationError {
    Empty,
    InvalidTime { frame: usize },
    TimeNotIncreasing { frame: usize },
}

impl Display for AnimationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "animation requires at least one keyframe"),
            Self::InvalidTime { frame } => {
                write!(formatter, "keyframe {frame} has an invalid time")
            }
            Self::TimeNotIncreasing { frame } => {
                write!(
                    formatter,
                    "keyframe {frame} time must be greater than the previous frame"
                )
            }
        }
    }
}

impl Error for AnimationError {}

impl AnimationClip3 {
    pub fn new(frames: Vec<Keyframe3>) -> Result<Self, AnimationError> {
        if frames.is_empty() {
            return Err(AnimationError::Empty);
        }
        for (index, frame) in frames.iter().enumerate() {
            if !frame.time.is_finite() || frame.time < 0.0 {
                return Err(AnimationError::InvalidTime { frame: index });
            }
            if index > 0 && frame.time <= frames[index - 1].time {
                return Err(AnimationError::TimeNotIncreasing { frame: index });
            }
        }
        let duration = frames.last().expect("nonempty animation").time;
        Ok(Self { frames, duration })
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn frames(&self) -> &[Keyframe3] {
        &self.frames
    }

    pub fn sample(&self, time: f64, loop_mode: LoopMode) -> Transform3 {
        if self.frames.len() == 1 || self.duration == 0.0 {
            return self.frames[0].transform;
        }
        let local_time = normalize_time(time, self.duration, loop_mode);
        if local_time <= self.frames[0].time {
            return self.frames[0].transform;
        }
        let next_index = self
            .frames
            .partition_point(|frame| frame.time <= local_time);
        if next_index >= self.frames.len() {
            return self.frames[self.frames.len() - 1].transform;
        }
        let previous = self.frames[next_index - 1];
        let next = self.frames[next_index];
        let amount = (local_time - previous.time) / (next.time - previous.time);
        previous
            .transform
            .lerp(next.transform, previous.ease.apply(amount))
    }
}

fn normalize_time(time: f64, duration: f64, loop_mode: LoopMode) -> f64 {
    let safe_time = if time.is_finite() { time.max(0.0) } else { 0.0 };
    match loop_mode {
        LoopMode::Clamp => safe_time.min(duration),
        LoopMode::Repeat => safe_time.rem_euclid(duration),
        LoopMode::PingPong => {
            let phase = safe_time.rem_euclid(duration * 2.0);
            if phase <= duration {
                phase
            } else {
                duration * 2.0 - phase
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(time: f64, x: f64, ease: Easing) -> Keyframe3 {
        Keyframe3 {
            time,
            transform: Transform3 {
                translation: Vec3::new(x, 0.0, 0.0),
                ..Transform3::IDENTITY
            },
            ease,
        }
    }

    #[test]
    fn linear_timeline_samples_exactly_between_frames() {
        let clip = AnimationClip3::new(vec![
            frame(0.0, 0.0, Easing::Linear),
            frame(2.0, 10.0, Easing::Linear),
        ])
        .unwrap();
        assert_eq!(clip.sample(1.0, LoopMode::Clamp).translation.x, 5.0);
    }

    #[test]
    fn repeated_and_ping_pong_clocks_are_deterministic() {
        let clip = AnimationClip3::new(vec![
            frame(0.0, 0.0, Easing::Linear),
            frame(2.0, 10.0, Easing::Linear),
        ])
        .unwrap();
        assert_eq!(clip.sample(2.5, LoopMode::Repeat).translation.x, 2.5);
        assert_eq!(clip.sample(2.5, LoopMode::PingPong).translation.x, 7.5);
    }

    #[test]
    fn frame_times_must_be_strictly_increasing() {
        let error = AnimationClip3::new(vec![
            frame(1.0, 0.0, Easing::Linear),
            frame(1.0, 1.0, Easing::Linear),
        ])
        .unwrap_err();
        assert_eq!(error, AnimationError::TimeNotIncreasing { frame: 1 });
    }
}
