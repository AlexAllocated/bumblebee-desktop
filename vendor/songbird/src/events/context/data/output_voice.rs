/// The final mixed PCM sent by the driver during one 20ms voice tick.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct OutputVoiceTick {
    /// Interleaved floating-point PCM samples after track volume and soft clipping.
    pub samples: Vec<f32>,
    /// PCM sample rate in Hz.
    pub sample_rate: u32,
    /// Number of interleaved PCM channels.
    pub channels: u16,
}
