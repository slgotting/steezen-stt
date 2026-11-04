use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::sync::{Arc, Mutex};

pub use steezen_core::wav::{WHISPER_RATE, read_wav, resample, wav_bytes};

pub struct Recording {
    _stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    rate: u32,
    started: std::time::Instant,
}

impl Recording {
    pub fn finish(self) -> (Vec<f32>, std::time::Duration) {
        let elapsed = self.started.elapsed();
        drop(self._stream);
        let samples = std::mem::take(&mut *self.samples.lock().unwrap());
        (resample(&samples, self.rate, WHISPER_RATE), elapsed)
    }
}

fn device_name(d: &cpal::Device) -> String {
    d.description().map(|d| d.name().to_string()).unwrap_or_else(|_| d.to_string())
}

pub fn input_devices() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|it| it.map(|d| device_name(&d)).collect())
        .unwrap_or_default()
}

pub fn start(device: &str) -> Result<Recording, String> {
    let host = cpal::default_host();
    let dev = host
        .input_devices()
        .ok()
        .and_then(|mut it| it.find(|d| !device.is_empty() && device_name(d) == device))
        .or_else(|| host.default_input_device())
        .ok_or("No microphone found")?;
    let config = dev.default_input_config().map_err(|e| e.to_string())?;
    let samples = Arc::new(Mutex::new(Vec::with_capacity(config.sample_rate() as usize * 30)));
    let channels = config.channels() as usize;
    let rate = config.sample_rate();
    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(&dev, config.into(), channels, samples.clone()),
        SampleFormat::I16 => build::<i16>(&dev, config.into(), channels, samples.clone()),
        SampleFormat::I32 => build::<i32>(&dev, config.into(), channels, samples.clone()),
        SampleFormat::U16 => build::<u16>(&dev, config.into(), channels, samples.clone()),
        SampleFormat::I8 => build::<i8>(&dev, config.into(), channels, samples.clone()),
        f => return Err(format!("Unsupported sample format {f}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Recording { _stream: stream, samples, rate, started: std::time::Instant::now() })
}

fn build<T>(dev: &cpal::Device, config: cpal::StreamConfig, channels: usize, out: Arc<Mutex<Vec<f32>>>) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    dev.build_input_stream(
        config,
        move |data: &[T], _: &_| {
            let mut out = out.lock().unwrap();
            out.extend(data.chunks(channels).map(|frame| frame.iter().map(|&s| f32::from_sample(s)).sum::<f32>() / channels as f32));
        },
        |e| eprintln!("audio stream error: {e}"),
        None,
    )
    .map_err(|e| e.to_string())
}

#[derive(Clone, Copy)]
pub enum Sound {
    Start,
    Stop,
    Cancel,
    Complete,
    Warning,
}

impl Sound {
    fn notes(self) -> &'static [f32] {
        match self {
            Sound::Start => &[660.0, 880.0],
            Sound::Stop => &[880.0, 660.0],
            Sound::Cancel => &[440.0, 330.0],
            Sound::Complete => &[660.0, 880.0, 1100.0],
            Sound::Warning => &[523.0, 392.0],
        }
    }
}

pub fn play(sound: Sound) {
    std::thread::spawn(move || {
        let Some(dev) = cpal::default_host().default_output_device() else { return };
        let Ok(config) = dev.default_output_config() else { return };
        if config.sample_format() != SampleFormat::F32 {
            return;
        }
        let rate = config.sample_rate() as f32;
        let channels = config.channels() as usize;
        let note_len = (rate * 0.07) as usize;
        let tone: Vec<f32> = sound
            .notes()
            .iter()
            .flat_map(|&f| (0..note_len).map(move |i| {
                let env = (i.min(note_len - i) as f32 / (rate * 0.01)).min(1.0);
                (i as f32 * f * std::f32::consts::TAU / rate).sin() * 0.15 * env
            }))
            .collect();
        let total = tone.len();
        let mut pos = 0;
        let Ok(stream) = dev.build_output_stream(
            config.into(),
            move |out: &mut [f32], _: &_| {
                for frame in out.chunks_mut(channels) {
                    let v = tone.get(pos).copied().unwrap_or(0.0);
                    frame.fill(v);
                    pos += 1;
                }
            },
            |_| {},
            None,
        ) else { return };
        if stream.play().is_ok() {
            std::thread::sleep(std::time::Duration::from_secs_f32(total as f32 / rate + 0.1));
        }
    });
}
