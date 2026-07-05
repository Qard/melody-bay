#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    sample_rate: u32,
    channels: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioBufferOptions {
    pub number_of_channels: usize,
    pub length: usize,
    pub sample_rate: u32,
}

impl AudioBuffer {
    pub fn try_from_mono(
        sample_rate: u32,
        length: usize,
        samples: impl IntoIterator<Item = f32>,
    ) -> Result<Self, GraphError> {
        Self::try_from_channels(sample_rate, length, [samples])
    }

    pub fn try_from_stereo(
        sample_rate: u32,
        length: usize,
        left: impl IntoIterator<Item = f32>,
        right: impl IntoIterator<Item = f32>,
    ) -> Result<Self, GraphError> {
        if length == 0 || !(3_000..=384_000).contains(&sample_rate) {
            return Err(GraphError::InvalidAudioBuffer);
        }
        let mut left = left.into_iter().take(length).collect::<Vec<_>>();
        left.resize(length, 0.0);
        let mut right = right.into_iter().take(length).collect::<Vec<_>>();
        right.resize(length, 0.0);
        Ok(Self {
            sample_rate,
            channels: vec![left, right],
        })
    }

    #[must_use]
    pub(crate) fn from_stereo(
        sample_rate: u32,
        length: usize,
        left: impl IntoIterator<Item = f32>,
        right: impl IntoIterator<Item = f32>,
    ) -> Self {
        let mut left = left.into_iter().take(length).collect::<Vec<_>>();
        left.resize(length, 0.0);
        let mut right = right.into_iter().take(length).collect::<Vec<_>>();
        right.resize(length, 0.0);
        Self {
            sample_rate: sample_rate.max(1),
            channels: vec![left, right],
        }
    }

    pub fn try_from_channels<I>(
        sample_rate: u32,
        length: usize,
        channels: impl IntoIterator<Item = I>,
    ) -> Result<Self, GraphError>
    where
        I: IntoIterator<Item = f32>,
    {
        if length == 0 || !(3_000..=384_000).contains(&sample_rate) {
            return Err(GraphError::InvalidAudioBuffer);
        }
        let channels = channels
            .into_iter()
            .map(|samples| {
                let mut channel = samples.into_iter().take(length).collect::<Vec<_>>();
                channel.resize(length, 0.0);
                channel
            })
            .collect::<Vec<_>>();
        if channels.is_empty() || channels.len() > 32 {
            return Err(GraphError::InvalidAudioBuffer);
        }
        Ok(Self {
            sample_rate,
            channels,
        })
    }

    #[must_use]
    pub(crate) fn from_channels<I>(
        sample_rate: u32,
        length: usize,
        channels: impl IntoIterator<Item = I>,
    ) -> Self
    where
        I: IntoIterator<Item = f32>,
    {
        let channels = channels
            .into_iter()
            .map(|samples| {
                let mut channel = samples.into_iter().take(length).collect::<Vec<_>>();
                channel.resize(length, 0.0);
                channel
            })
            .collect::<Vec<_>>();
        Self {
            sample_rate: sample_rate.max(1),
            channels,
        }
    }

    pub fn try_from_frames(sample_rate: u32, frames: &[Frame]) -> Result<Self, GraphError> {
        let mut left = Vec::with_capacity(frames.len());
        let mut right = Vec::with_capacity(frames.len());
        for frame in frames {
            left.push(frame.left);
            right.push(frame.right);
        }
        Self::try_from_stereo(sample_rate, frames.len(), left, right)
    }

    #[must_use]
    fn from_frames(sample_rate: u32, frames: &[Frame]) -> Self {
        let mut left = Vec::with_capacity(frames.len());
        let mut right = Vec::with_capacity(frames.len());
        for frame in frames {
            left.push(frame.left);
            right.push(frame.right);
        }
        Self::from_stereo(sample_rate, frames.len(), left, right)
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub fn length(&self) -> usize {
        self.len()
    }

    #[must_use]
    pub fn duration(&self) -> f32 {
        self.len() as f32 / self.sample_rate as f32
    }

    #[must_use]
    pub fn number_of_channels(&self) -> usize {
        self.channels.len()
    }

    #[must_use]
    pub fn channel_data(&self, channel: usize) -> Option<&[f32]> {
        self.channels.get(channel).map(Vec::as_slice)
    }

    #[must_use]
    pub fn channel_data_mut(&mut self, channel: usize) -> Option<&mut [f32]> {
        self.channels.get_mut(channel).map(Vec::as_mut_slice)
    }

    pub fn copy_from_channel(
        &self,
        destination: &mut [f32],
        channel: usize,
        start_in_channel: usize,
    ) -> Result<(), GraphError> {
        let Some(channel) = self.channels.get(channel) else {
            return Err(GraphError::InvalidChannel);
        };
        if start_in_channel >= channel.len() {
            return Ok(());
        }
        let source = &channel[start_in_channel..];
        let length = destination.len().min(source.len());
        destination[..length].copy_from_slice(&source[..length]);
        Ok(())
    }

    pub fn copy_to_channel(
        &mut self,
        source: impl AsRef<[f32]>,
        channel: usize,
        start_in_channel: usize,
    ) -> Result<(), GraphError> {
        let Some(destination) = self.channels.get_mut(channel) else {
            return Err(GraphError::InvalidChannel);
        };
        if start_in_channel >= destination.len() {
            return Ok(());
        }
        let destination = &mut destination[start_in_channel..];
        let source = source.as_ref();
        let length = destination.len().min(source.len());
        destination[..length].copy_from_slice(&source[..length]);
        Ok(())
    }

    fn bus_at(&self, time: f64) -> AudioBus {
        if !time.is_finite() || time <= 0.0 {
            return self.bus_at_index(0);
        }
        let position = time * self.sample_rate as f64;
        let index = position.floor() as usize;
        let fraction = (position - index as f64) as f32;
        let first = self.bus_at_index(index);
        if fraction <= f32::EPSILON {
            return first;
        }
        let second = self.bus_at_index(index.saturating_add(1));
        interpolate_bus(&first, &second, fraction)
    }

    fn bus_at_looping(&self, time: f64, loop_start: f64, loop_end: f64) -> AudioBus {
        if !time.is_finite() || time <= 0.0 {
            return self.bus_at_index(0);
        }
        let position = time * self.sample_rate as f64;
        let index = position.floor().max(0.0) as usize;
        let fraction = (position - index as f64) as f32;
        let first = self.bus_at_index(index);
        if fraction <= f32::EPSILON {
            return first;
        }
        let loop_start_frame = (loop_start * self.sample_rate as f64).round().max(0.0) as usize;
        let loop_end_frame = (loop_end * self.sample_rate as f64)
            .round()
            .max(loop_start_frame as f64 + 1.0) as usize;
        let next = index.saturating_add(1);
        let next = if next >= loop_end_frame {
            loop_start_frame
        } else {
            next
        };
        let second = self.bus_at_index(next);
        interpolate_bus(&first, &second, fraction)
    }

    fn bus_between(
        &self,
        start_time: f64,
        end_time: f64,
        loop_range: Option<(f64, f64)>,
    ) -> AudioBus {
        if !start_time.is_finite() || !end_time.is_finite() || end_time <= start_time {
            return if let Some((loop_start, loop_end)) = loop_range {
                self.bus_at_looping(start_time, loop_start, loop_end)
            } else {
                self.bus_at(start_time)
            };
        }
        let source_frames = (end_time - start_time).abs() * self.sample_rate as f64;
        if source_frames <= 1.0 {
            return if let Some((loop_start, loop_end)) = loop_range {
                self.bus_at_looping(start_time, loop_start, loop_end)
            } else {
                self.bus_at(start_time)
            };
        }
        let samples = (source_frames.ceil() as usize).clamp(2, 32);
        let mut sum = AudioBus::silent(self.number_of_channels());
        for sample_index in 0..samples {
            let amount = (sample_index as f64 + 0.5) / samples as f64;
            let time = start_time + (end_time - start_time) * amount;
            let bus = if let Some((loop_start, loop_end)) = loop_range {
                let time = wrap_loop_source_time(time, loop_start, loop_end, 1.0);
                self.bus_at_looping(time, loop_start, loop_end)
            } else {
                self.bus_at(time)
            };
            for (channel, value) in bus.channels.iter().copied().enumerate() {
                if let Some(sum) = sum.channels.get_mut(channel) {
                    *sum += value;
                }
            }
        }
        sum.scaled(1.0 / samples as f32)
    }

    fn bus_at_index(&self, index: usize) -> AudioBus {
        if self.channels.is_empty() {
            return AudioBus::silent(1);
        }
        AudioBus::from_channels(
            self.channels
                .iter()
                .map(|channel| channel.get(index).copied().unwrap_or(0.0))
                .collect(),
        )
    }

    fn len(&self) -> usize {
        self.channels
            .iter()
            .map(std::vec::Vec::len)
            .max()
            .unwrap_or(0)
    }

    fn convolver_normalization_scale(&self) -> f32 {
        const GAIN_CALIBRATION: f32 = 0.00125;
        const GAIN_CALIBRATION_SAMPLE_RATE: f32 = 44_100.0;
        const MIN_POWER: f32 = 0.000_125;

        let sample_count = self.number_of_channels().saturating_mul(self.len());
        if sample_count == 0 {
            return 1.0;
        }
        let power = self
            .channels
            .iter()
            .flatten()
            .copied()
            .map(|sample| sample * sample)
            .sum::<f32>()
            / sample_count as f32;
        if !power.is_finite() {
            return 1.0;
        }
        let mut scale = power.max(MIN_POWER).sqrt().recip() * GAIN_CALIBRATION;
        scale *= GAIN_CALIBRATION_SAMPLE_RATE / self.sample_rate as f32;
        if self.number_of_channels() == 4 {
            scale *= 0.5;
        }
        scale
    }
}

#[cfg(test)]
mod audio_buffer_tests {
    use super::*;

    #[test]
    fn bus_at_interpolates_fractional_sample_positions() {
        let buffer = AudioBuffer::try_from_mono(10_000, 3, [0.0, 1.0, 0.0]).unwrap();

        assert!((buffer.bus_at(0.00005).channel(0) - 0.5).abs() < 0.0001);
        assert!((buffer.bus_at(0.00015).channel(0) - 0.5).abs() < 0.0001);
    }

    #[test]
    fn audio_bus_uses_small_storage_for_mono_and_stereo() {
        assert_eq!(AudioBus::mono(0.25).storage_kind_for_tests(), "mono");
        assert_eq!(
            AudioBus::from_frame(Frame::new(0.25, -0.5)).storage_kind_for_tests(),
            "stereo"
        );
        assert_eq!(AudioBus::silent(4).storage_kind_for_tests(), "multi");
    }
}

fn validate_periodic_wave_coefficients(real: &[f32], imag: &[f32]) -> Result<(), GraphError> {
    if real.len() != imag.len()
        || real.len() < 2
        || real
            .iter()
            .chain(imag.iter())
            .any(|coefficient| !coefficient.is_finite())
    {
        return Err(GraphError::InvalidPeriodicWave);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct PeriodicWave {
    real: Vec<f32>,
    imag: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub struct PeriodicWaveOptions {
    pub disable_normalization: bool,
}


impl PeriodicWave {
    pub fn try_new(
        real: impl IntoIterator<Item = f32>,
        imag: impl IntoIterator<Item = f32>,
    ) -> Result<Self, GraphError> {
        Self::try_new_with_options(real, imag, PeriodicWaveOptions::default())
    }

    pub fn try_new_with_options(
        real: impl IntoIterator<Item = f32>,
        imag: impl IntoIterator<Item = f32>,
        options: PeriodicWaveOptions,
    ) -> Result<Self, GraphError> {
        let mut real = real.into_iter().collect::<Vec<_>>();
        let mut imag = imag.into_iter().collect::<Vec<_>>();
        validate_periodic_wave_coefficients(&real, &imag)?;
        real[0] = 0.0;
        imag[0] = 0.0;
        if !options.disable_normalization {
            normalize_periodic_wave_coefficients(&mut real, &mut imag);
        }
        Ok(Self::from_coefficients(real, imag))
    }

    fn from_coefficients(
        real: impl IntoIterator<Item = f32>,
        imag: impl IntoIterator<Item = f32>,
    ) -> Self {
        Self {
            real: real.into_iter().collect(),
            imag: imag.into_iter().collect(),
        }
    }

    fn sample_phase(&self, phase: f32) -> f32 {
        let phase = phase.rem_euclid(1.0) * TAU;
        let harmonic_count = self.real.len().max(self.imag.len());
        let mut sample = self.real.first().copied().unwrap_or(0.0);
        for harmonic in 1..harmonic_count {
            let angle = phase * harmonic as f32;
            let real = self.real.get(harmonic).copied().unwrap_or(0.0);
            let imag = self.imag.get(harmonic).copied().unwrap_or(0.0);
            sample += real * angle.cos() + imag * angle.sin();
        }
        sample
    }
}
