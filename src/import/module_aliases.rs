mod importers {
    use super::*;

    pub(crate) mod builtins {
        use super::*;

        pub(crate) fn gm_instrument(program: u8) -> Instrument {
            super::super::gm_instrument_impl(program)
        }

        pub(crate) fn gm_drum(note: u8) -> Instrument {
            super::super::gm_drum_impl(note)
        }
    }

    pub(crate) mod midi {
        use super::*;

        pub(crate) fn import(bytes: &[u8]) -> Result<ImportedSequence, ImportError> {
            super::super::import_midi_impl(bytes)
        }
    }

    pub(crate) mod mod_file {
        use super::*;

        pub(crate) fn import(bytes: &[u8]) -> Result<ImportedSequence, ImportError> {
            super::super::import_mod_impl(bytes)
        }
    }

    pub(crate) mod xm {
        use super::*;

        pub(crate) fn import(bytes: &[u8]) -> Result<ImportedSequence, ImportError> {
            super::super::import_xm_impl(bytes)
        }
    }
}

