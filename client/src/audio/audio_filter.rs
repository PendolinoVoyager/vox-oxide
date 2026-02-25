#![allow(unused)]
use std::i16;

use num_traits::AsPrimitive;

#[derive(Debug, Clone, PartialEq)]
pub enum AudioFilterResult {
    NoWorkDone,
    FilterError,
    Success,
}
pub trait AudioFilter<T>
where
    T: num_traits::Num,
{
    #[allow(unused)]
    fn transform(&mut self, data: &mut [T]) -> AudioFilterResult;
    fn transform_sample(&mut self, sample: &mut T) -> AudioFilterResult;
}

pub struct DefaultAudioFilter {
    /// Silence threshold in percent, where 1.0 is total silence, 0.0 is no noise suppression at all.
    noise_threshold: f32,
    _noise_threshold_i16: i16,
}
impl DefaultAudioFilter {
    pub fn new(noise_threshold: f32) -> Self {
        Self {
            noise_threshold: noise_threshold.clamp(0.0, 1.0),
            _noise_threshold_i16: (i16::MAX as f32 * noise_threshold.clamp(0.0, 1.0)) as i16,
        }
    }
}
impl AudioFilter<i16> for DefaultAudioFilter {
    fn transform(&mut self, data: &mut [i16]) -> AudioFilterResult {
        for sample in data {
            self.transform_sample(sample);
        }
        return AudioFilterResult::Success;
    }

    fn transform_sample(&mut self, sample: &mut i16) -> AudioFilterResult {
        if *sample <= self._noise_threshold_i16 {
            *sample = 0;
            return AudioFilterResult::Success;
        }
        return AudioFilterResult::NoWorkDone;
    }
}
impl AudioFilter<f32> for DefaultAudioFilter {
    fn transform(&mut self, data: &mut [f32]) -> AudioFilterResult {
        for sample in data {
            self.transform_sample(sample);
        }
        return AudioFilterResult::Success;
    }
    fn transform_sample(&mut self, sample: &mut f32) -> AudioFilterResult {
        if *sample <= self.noise_threshold {
            *sample = 0.0f32.as_();
            return AudioFilterResult::Success;
        }
        return AudioFilterResult::NoWorkDone;
    }
}
