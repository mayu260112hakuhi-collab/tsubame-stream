use rodio::{source::UniformSourceIterator, Decoder, OutputStream, Sink, Source};
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    time::Duration,
};
use stream_audio::ExternalPcmSender;

#[derive(Debug, Clone)]
pub struct BgmLayerSource {
    pub enabled: bool,
    pub name: String,
    pub path: PathBuf,
    pub volume_percent: f32,
    pub muted: bool,
    pub loop_enabled: bool,
}

impl BgmLayerSource {
    pub fn from_path(path: &Path) -> Self {
        Self {
            enabled: true,
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("BGM")
                .to_owned(),
            path: path.to_path_buf(),
            volume_percent: 70.0,
            muted: false,
            loop_enabled: true,
        }
    }

    pub fn volume_linear(&self) -> f32 {
        (self.volume_percent / 100.0).clamp(0.0, 1.0)
    }

    pub fn effective_volume(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            self.volume_linear()
        }
    }
}

struct PcmTapSource<I>
where
    I: Source<Item = f32>,
{
    input: I,
    sender: ExternalPcmSender,
    chunk: Vec<f32>,
    drained: bool,
}

impl<I> PcmTapSource<I>
where
    I: Source<Item = f32>,
{
    fn new(input: I, sender: ExternalPcmSender) -> Self {
        Self {
            input,
            sender,
            chunk: Vec::with_capacity(1_920), // 20 ms @ 48 kHz stereo
            drained: false,
        }
    }

    fn flush_chunk(&mut self, pad_to_20ms: bool) {
        if self.chunk.is_empty() {
            return;
        }
        if pad_to_20ms {
            self.chunk.resize(1_920, 0.0);
        }
        self.sender.push_f32_stereo_48k(&self.chunk);
        self.chunk.clear();
    }
}

impl<I> Iterator for PcmTapSource<I>
where
    I: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        match self.input.next() {
            Some(sample) => {
                self.chunk.push(sample);
                if self.chunk.len() >= 1_920 {
                    self.flush_chunk(false);
                }
                Some(sample)
            }
            None => {
                if !self.drained {
                    self.flush_chunk(true);
                    self.sender.clear_level();
                    self.drained = true;
                }
                None
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.input.size_hint()
    }
}

impl<I> Source for PcmTapSource<I>
where
    I: Source<Item = f32>,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.input.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.input.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
}

pub struct BgmPlayer {
    // OutputStream must stay alive while Sink is playing.
    _stream: OutputStream,
    sink: Sink,
    pcm_sender: Option<ExternalPcmSender>,
}

impl BgmPlayer {
    pub fn play_file(
        path: &Path,
        loop_enabled: bool,
        volume: f32,
        pcm_sender: Option<ExternalPcmSender>,
    ) -> Result<Self, String> {
        let (stream, stream_handle) = OutputStream::try_default()
            .map_err(|err| format!("BGM出力デバイスを開けませんでした: {err}"))?;
        let sink = Sink::try_new(&stream_handle)
            .map_err(|err| format!("BGM再生を開始できませんでした: {err}"))?;

        let file = File::open(path)
            .map_err(|err| format!("BGMファイルを開けませんでした: {err}"))?;
        let decoder = Decoder::new(BufReader::new(file))
            .map_err(|err| format!("BGMをデコードできませんでした: {err}"))?;

        if loop_enabled {
            let source = UniformSourceIterator::<_, f32>::new(
                decoder.repeat_infinite().convert_samples::<f32>(),
                2,
                48_000,
            );
            if let Some(sender) = pcm_sender.clone() {
                sink.append(PcmTapSource::new(source, sender));
            } else {
                sink.append(source);
            }
        } else {
            let source = UniformSourceIterator::<_, f32>::new(decoder.convert_samples::<f32>(), 2, 48_000);
            if let Some(sender) = pcm_sender.clone() {
                sink.append(PcmTapSource::new(source, sender));
            } else {
                sink.append(source);
            }
        }
        sink.set_volume(volume.clamp(0.0, 1.0));
        sink.play();

        Ok(Self {
            _stream: stream,
            sink,
            pcm_sender,
        })
    }

    pub fn pause(&self) {
        self.sink.pause();
        if let Some(sender) = &self.pcm_sender {
            sender.clear_level();
        }
    }

    pub fn resume(&self) {
        self.sink.play();
    }

    pub fn stop(&self) {
        self.sink.stop();
        if let Some(sender) = &self.pcm_sender {
            sender.clear_level();
        }
    }

    pub fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume.clamp(0.0, 1.0));
    }

    pub fn is_paused(&self) -> bool {
        self.sink.is_paused()
    }

    pub fn is_finished(&self) -> bool {
        self.sink.empty()
    }
}
