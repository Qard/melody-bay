#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportedFormat {
    Midi,
    Mod,
    Xm,
}

#[derive(Debug, Clone)]
pub struct ImportedSequence {
    pub sequence: IndexedSequence,
    pub source_format: ImportedFormat,
    pub metadata: SequenceMetadata,
    pub warnings: Vec<ImportWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Default)]
pub struct ImportWarning {
    pub message: String,
    pub kind: ImportWarningKind,
}

impl ImportWarning {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: ImportWarningKind::InvalidMetadata,
        }
    }

    #[allow(dead_code)]
    fn unsupported_event(format: ImportedFormat, event: impl Into<String>) -> Self {
        let event = event.into();
        Self {
            message: format!("unsupported {format:?} event {event}"),
            kind: ImportWarningKind::UnsupportedEvent { format, event },
        }
    }

    fn unsupported_effect(format: ImportedFormat, effect: impl Into<String>) -> Self {
        let effect = effect.into();
        Self {
            message: format!("unsupported {format:?} effect {effect}"),
            kind: ImportWarningKind::UnsupportedEffect { format, effect },
        }
    }

    fn approximated_timing(format: ImportedFormat, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            message: detail.clone(),
            kind: ImportWarningKind::ApproximatedTiming { format, detail },
        }
    }

    fn dropped_controller(format: ImportedFormat, controller: impl Into<String>) -> Self {
        let controller = controller.into();
        Self {
            message: format!("dropped {format:?} controller or automation {controller}"),
            kind: ImportWarningKind::DroppedControllerOrAutomation { format, controller },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Default)]
pub enum ImportWarningKind {
    UnsupportedEvent {
        format: ImportedFormat,
        event: String,
    },
    UnsupportedEffect {
        format: ImportedFormat,
        effect: String,
    },
    ApproximatedTiming {
        format: ImportedFormat,
        detail: String,
    },
    UnsupportedSampleEncoding {
        format: ImportedFormat,
        encoding: String,
    },
    #[default]
    InvalidMetadata,
    DroppedControllerOrAutomation {
        format: ImportedFormat,
        controller: String,
    },
}



#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    InvalidFormat(&'static str),
    UnsupportedFormatFeature(&'static str),
    MalformedTiming(&'static str),
    MalformedSampleData(&'static str),
    ParserFailure(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat(message) => write!(formatter, "invalid format: {message}"),
            Self::UnsupportedFormatFeature(message) => {
                write!(formatter, "unsupported format feature: {message}")
            }
            Self::MalformedTiming(message) => write!(formatter, "malformed timing: {message}"),
            Self::MalformedSampleData(message) => {
                write!(formatter, "malformed sample data: {message}")
            }
            Self::ParserFailure(message) => write!(formatter, "parser failure: {message}"),
        }
    }
}

impl std::error::Error for ImportError {}

pub struct MidiImport;
pub struct ModImport;
pub struct XmImport;

impl MidiImport {
    pub fn from_bytes(bytes: &[u8]) -> Result<ImportedSequence, ImportError> {
        importers::midi::import(bytes)
    }
}

impl ModImport {
    pub fn from_bytes(bytes: &[u8]) -> Result<ImportedSequence, ImportError> {
        importers::mod_file::import(bytes)
    }
}

impl XmImport {
    pub fn from_bytes(bytes: &[u8]) -> Result<ImportedSequence, ImportError> {
        importers::xm::import(bytes)
    }
}

pub fn import_midi(bytes: &[u8]) -> Result<ImportedSequence, ImportError> {
    MidiImport::from_bytes(bytes)
}

pub fn import_mod(bytes: &[u8]) -> Result<ImportedSequence, ImportError> {
    ModImport::from_bytes(bytes)
}

pub fn import_xm(bytes: &[u8]) -> Result<ImportedSequence, ImportError> {
    XmImport::from_bytes(bytes)
}

#[must_use]
pub fn gm_instrument(program: u8) -> Instrument {
    importers::builtins::gm_instrument(program)
}

#[must_use]
pub fn gm_drum(note: u8) -> Instrument {
    importers::builtins::gm_drum(note)
}

#[must_use]
fn gm_instrument_impl(program: u8) -> Instrument {
    if matches!(program, 0..=15) {
        return gm_piano_graph_instrument(program);
    }
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(match program {
        16..=23 => Waveform::Square,
        24..=39 => Waveform::Sawtooth,
        40..=55 => Waveform::Triangle,
        80..=103 => Waveform::Square,
        _ => Waveform::Sine,
    });
    let _ = osc.frequency().set_value(440.0);
    let gain = graph.create_gain();
    let _ = graph.label_node(&gain, "output");
    let _ = gain.gain().set_value_at_time(0.0, 0.0);
    match program {
        16..=23 => {
            let _ = gain.gain().linear_ramp_to_value_at_time(0.18, 0.006);
            let _ = gain.gain().linear_ramp_to_value_at_time(0.06, 0.20);
            let _ = gain.gain().linear_ramp_to_value_at_time(0.0, 0.90);
        }
        40..=55 => {
            let _ = gain.gain().linear_ramp_to_value_at_time(0.12, 0.08);
            let _ = gain.gain().linear_ramp_to_value_at_time(0.10, 0.35);
        }
        56..=63 => {
            let _ = gain.gain().linear_ramp_to_value_at_time(0.16, 0.018);
            let _ = gain.gain().linear_ramp_to_value_at_time(0.10, 0.25);
        }
        _ => {
            let _ = gain.gain().linear_ramp_to_value_at_time(0.14, 0.02);
            let _ = gain.gain().linear_ramp_to_value_at_time(0.08, 0.25);
        }
    }
    let pan = graph.create_stereo_panner();
    let filter = graph.create_biquad_filter();
    let _ = filter.frequency().set_value(match program {
        32..=39 => 900.0,
        40..=55 => 2_400.0,
        56..=63 => 3_200.0,
        _ => 1_800.0,
    });
    let _ = graph.connect(osc, &filter);
    let _ = graph.connect(&filter, &gain);
    let _ = graph.connect(&gain, &pan);
    let _ = graph.connect(&pan, graph.destination());
    Instrument::graph(graph).base_note(Note::from_midi(69))
}

#[must_use]
fn gm_piano_graph_instrument(program: u8) -> Instrument {
    let peak = match program {
        8..=15 => 0.10,
        _ => 0.13,
    };
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(match program {
        8..=15 => Waveform::Triangle,
        _ => Waveform::Sine,
    });
    let _ = osc.frequency().set_value(440.0);
    let gain = graph.create_gain();
    let _ = graph.label_node(&gain, "output");
    let _ = gain.gain().set_value_at_time(0.0, 0.0);
    let _ = gain.gain().linear_ramp_to_value_at_time(peak, 0.006);
    let _ = gain.gain().linear_ramp_to_value_at_time(peak * 0.18, 0.04);
    let _ = gain
        .gain()
        .linear_ramp_to_value_at_time(peak * 0.025, 0.075);
    let _ = gain.gain().linear_ramp_to_value_at_time(0.0, 0.12);
    let _ = graph.connect(osc, &gain);
    let _ = graph.connect(&gain, graph.destination());
    Instrument::graph(graph).base_note(Note::from_midi(69))
}

#[must_use]
fn gm_drum_impl(note: u8) -> Instrument {
    let mut graph = AudioContext::new();
    let osc = graph.create_oscillator();
    osc.set_type(match note {
        35 | 36 | 41 | 43 | 45 => Waveform::Sine,
        38 | 40 => Waveform::Triangle,
        42 | 44 | 46 | 49 | 51 | 57 => Waveform::Square,
        _ => Waveform::Sawtooth,
    });
    let _ = osc.frequency().set_value(match note {
        35 | 36 => 80.0,
        38 | 40 => 180.0,
        42 | 44 | 46 => 7_000.0,
        _ => 440.0,
    });
    let gain = graph.create_gain();
    let _ = graph.label_node(&gain, "output");
    let _ = gain.gain().set_value_at_time(0.0, 0.0);
    let _ = gain.gain().linear_ramp_to_value_at_time(0.45, 0.003);
    let _ = gain.gain().linear_ramp_to_value_at_time(0.0, 0.16);
    let _ = graph.connect(osc, &gain);
    let _ = graph.connect(&gain, graph.destination());
    Instrument::graph(graph).base_note(Note::from_midi(note))
}

