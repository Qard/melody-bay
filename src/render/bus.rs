#[derive(Debug, Clone, PartialEq)]
struct AudioBus {
    channels: AudioBusChannels,
}

#[derive(Debug, Clone, PartialEq)]
enum AudioBusChannels {
    Mono(f32),
    Stereo(Frame),
    Multi(Vec<f32>),
}

impl Default for AudioBus {
    fn default() -> Self {
        Self::mono(0.0)
    }
}

impl AudioBus {
    fn silent(channels: usize) -> Self {
        Self {
            channels: AudioBusChannels::silent(channels),
        }
    }

    fn mono(sample: f32) -> Self {
        Self {
            channels: AudioBusChannels::Mono(sample),
        }
    }

    fn from_frame(frame: Frame) -> Self {
        Self {
            channels: AudioBusChannels::Stereo(frame),
        }
    }

    fn from_channels(channels: Vec<f32>) -> Self {
        Self {
            channels: AudioBusChannels::from_vec(channels),
        }
    }

    fn channels_len(&self) -> usize {
        self.channels.len()
    }

    fn iter_channels(&self) -> impl Iterator<Item = f32> + '_ {
        (0..self.channels_len()).map(|channel| self.channel(channel))
    }

    fn resize(&mut self, channels: usize, value: f32) {
        self.channels.resize(channels, value);
    }

    fn set_channel(&mut self, index: usize, value: f32) {
        if index >= self.channels_len() {
            return;
        }
        if let Some(sample) = self.channels.get_mut(index) {
            *sample = value;
        }
    }

    fn add_to_channel(&mut self, index: usize, value: f32) {
        if index >= self.channels_len() {
            return;
        }
        let next = self.channel(index) + value;
        self.set_channel(index, next);
    }

    #[cfg(test)]
    fn storage_kind_for_tests(&self) -> &'static str {
        self.channels.storage_kind_for_tests()
    }

    fn to_frame(&self) -> Frame {
        Frame::new(self.channel(0), self.channel(1))
    }

    fn channel(&self, index: usize) -> f32 {
        self.channels
            .get(index)
            .copied()
            .unwrap_or_else(|| self.channels.first().copied().unwrap_or(0.0))
    }

    fn add_assign(&mut self, other: &Self) {
        if self.channels_len() < other.channels_len() {
            self.resize(other.channels_len(), 0.0);
        }
        for (index, sample) in other.iter_channels().enumerate() {
            self.add_to_channel(index, sample);
        }
    }

    fn scaled(&self, gain: f32) -> Self {
        match self {
            Self { channels } => Self {
                channels: channels.scaled(gain),
            },
        }
    }
}

impl AudioBusChannels {
    fn silent(channels: usize) -> Self {
        match channels.max(1) {
            1 => Self::Mono(0.0),
            2 => Self::Stereo(Frame::ZERO),
            channels => Self::Multi(vec![0.0; channels]),
        }
    }

    fn from_vec(channels: Vec<f32>) -> Self {
        match channels.len() {
            0 => Self::Mono(0.0),
            1 => Self::Mono(channels[0]),
            2 => Self::Stereo(Frame::new(channels[0], channels[1])),
            _ => Self::Multi(channels),
        }
    }

    #[cfg(test)]
    fn storage_kind_for_tests(&self) -> &'static str {
        match self {
            Self::Mono(_) => "mono",
            Self::Stereo(_) => "stereo",
            Self::Multi(_) => "multi",
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Mono(_) => 1,
            Self::Stereo(_) => 2,
            Self::Multi(channels) => channels.len(),
        }
    }

    fn get(&self, index: usize) -> Option<&f32> {
        match self {
            Self::Mono(sample) => (index == 0).then_some(sample),
            Self::Stereo(frame) => match index {
                0 => Some(&frame.left),
                1 => Some(&frame.right),
                _ => None,
            },
            Self::Multi(channels) => channels.get(index),
        }
    }

    fn first(&self) -> Option<&f32> {
        self.get(0)
    }

    fn get_mut(&mut self, index: usize) -> Option<&mut f32> {
        match self {
            Self::Mono(sample) => (index == 0).then_some(sample),
            Self::Stereo(frame) => match index {
                0 => Some(&mut frame.left),
                1 => Some(&mut frame.right),
                _ => None,
            },
            Self::Multi(channels) => channels.get_mut(index),
        }
    }

    fn iter(&self) -> impl Iterator<Item = &f32> {
        AudioBusChannelsIter::new(self)
    }

    fn resize(&mut self, channels: usize, value: f32) {
        let mut values = self.iter().copied().collect::<Vec<_>>();
        values.resize(channels.max(1), value);
        *self = Self::from_vec(values);
    }

    fn fill(&mut self, value: f32) {
        match self {
            Self::Mono(sample) => *sample = value,
            Self::Stereo(frame) => *frame = Frame::new(value, value),
            Self::Multi(channels) => channels.fill(value),
        }
    }

    fn clear(&mut self) {
        *self = Self::Multi(Vec::new());
    }

    fn push(&mut self, value: f32) {
        let mut values = self.iter().copied().collect::<Vec<_>>();
        values.push(value);
        *self = Self::from_vec(values);
    }

    fn scaled(&self, gain: f32) -> Self {
        match self {
            Self::Mono(sample) => Self::Mono(sample * gain),
            Self::Stereo(frame) => Self::Stereo(*frame * gain),
            Self::Multi(channels) => {
                Self::Multi(channels.iter().map(|sample| sample * gain).collect())
            }
        }
    }
}

enum AudioBusChannelsIter<'a> {
    Mono(std::option::IntoIter<&'a f32>),
    Stereo(std::array::IntoIter<&'a f32, 2>),
    Multi(std::slice::Iter<'a, f32>),
}

impl<'a> AudioBusChannelsIter<'a> {
    fn new(channels: &'a AudioBusChannels) -> Self {
        match channels {
            AudioBusChannels::Mono(sample) => Self::Mono(Some(sample).into_iter()),
            AudioBusChannels::Stereo(frame) => {
                Self::Stereo([&frame.left, &frame.right].into_iter())
            }
            AudioBusChannels::Multi(channels) => Self::Multi(channels.iter()),
        }
    }
}

impl<'a> Iterator for AudioBusChannelsIter<'a> {
    type Item = &'a f32;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Mono(iter) => iter.next(),
            Self::Stereo(iter) => iter.next(),
            Self::Multi(iter) => iter.next(),
        }
    }
}

impl std::ops::Index<usize> for AudioBusChannels {
    type Output = f32;

    fn index(&self, index: usize) -> &Self::Output {
        match self {
            Self::Mono(sample) if index == 0 => sample,
            Self::Stereo(frame) if index == 0 => &frame.left,
            Self::Stereo(frame) if index == 1 => &frame.right,
            Self::Multi(channels) => &channels[index],
            _ => panic!("audio bus channel index out of bounds"),
        }
    }
}

impl std::ops::IndexMut<usize> for AudioBusChannels {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match self {
            Self::Mono(sample) if index == 0 => sample,
            Self::Stereo(frame) => match index {
                0 => &mut frame.left,
                1 => &mut frame.right,
                _ => panic!("audio bus channel index out of bounds"),
            },
            Self::Multi(channels) => &mut channels[index],
            _ => panic!("audio bus channel index out of bounds"),
        }
    }
}
