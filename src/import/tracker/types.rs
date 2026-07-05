#[derive(Debug, Clone)]
struct TrackerSample {
    _name: String,
    length: usize,
    is_16_bit: bool,
    volume: f32,
    finetune_cents: f32,
    loop_start: usize,
    loop_len: usize,
    ping_pong_loop: bool,
    data: Vec<f32>,
    relative_note: i8,
    pan: f32,
    envelope: Option<SampleEnvelope>,
}

#[derive(Debug, Clone)]
struct XmInstrument {
    keymap: [usize; 96],
    samples: Vec<TrackerSample>,
}

#[derive(Debug, Clone, Copy)]
struct TrackerCell {
    note: Option<u8>,
    period: Option<u16>,
    instrument: usize,
    volume: Option<f32>,
    pan: Option<f32>,
    effect: u8,
    effect_param: u8,
    key_off: bool,
}

#[derive(Debug, Clone, Copy)]
struct ActiveTrackerNote {
    start_index: u64,
    instrument: usize,
    sample_slot: usize,
    sample_offset: usize,
    period: Option<f32>,
    playback_rate_correction: f32,
    note: u8,
    velocity: f32,
}

#[derive(Debug, Clone, Copy)]
struct TrackerTiming {
    speed: u32,
    bpm: u32,
}

impl TrackerTiming {
    fn rows_per_minute(self) -> f64 {
        f64::from(self.bpm.max(1)) * 24.0 / f64::from(self.speed.max(1))
    }
}

