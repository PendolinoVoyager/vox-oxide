#[derive(Debug, Clone, PartialEq)]
pub enum AudioFilterResult {
    NoWorkDone,
    FilterError,
    Success,
}
pub trait AudioFilter {
    fn transform(&mut self, data: &mut [f32]) -> AudioFilterResult;
    fn transform_sample(&mut self, sample: &mut f32) -> AudioFilterResult;
}

pub struct DefaultAudioFilter {
    /// Silence threshold in percent, where 1.0 is total silence, 0.0 is no noise suppression at all.
    noise_threshold: f32,
}
impl DefaultAudioFilter {
    pub fn new(noise_threshold: f32) -> Self {
        Self {
            noise_threshold: noise_threshold.clamp(0.0, 1.0),
        }
    }
}
impl AudioFilter for DefaultAudioFilter {
    fn transform(&mut self, data: &mut [f32]) -> AudioFilterResult {
        for sample in data {
            if sample.abs() < self.noise_threshold {
                *sample = 0.0;
            }
        }
        return AudioFilterResult::Success;
    }
    fn transform_sample(&mut self, sample: &mut f32) -> AudioFilterResult {
        if *sample >= self.noise_threshold {
            *sample = 0.0;
            return AudioFilterResult::NoWorkDone;
        }
        AudioFilterResult::Success
    }
}
