use rodio::{Decoder, OutputStream, Sink, Source};
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

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

pub struct BgmPlayer {
    // OutputStream must stay alive while Sink is playing.
    _stream: OutputStream,
    sink: Sink,
}

impl BgmPlayer {
    pub fn play_file(path: &Path, loop_enabled: bool, volume: f32) -> Result<Self, String> {
        let (stream, stream_handle) = OutputStream::try_default()
            .map_err(|err| format!("BGM出力デバイスを開けませんでした: {err}"))?;
        let sink = Sink::try_new(&stream_handle)
            .map_err(|err| format!("BGM再生を開始できませんでした: {err}"))?;

        let file = File::open(path)
            .map_err(|err| format!("BGMファイルを開けませんでした: {err}"))?;
        let decoder = Decoder::new(BufReader::new(file))
            .map_err(|err| format!("BGMをデコードできませんでした: {err}"))?;

        if loop_enabled {
            sink.append(decoder.repeat_infinite());
        } else {
            sink.append(decoder);
        }
        sink.set_volume(volume.clamp(0.0, 1.0));
        sink.play();

        Ok(Self {
            _stream: stream,
            sink,
        })
    }

    pub fn pause(&self) {
        self.sink.pause();
    }

    pub fn resume(&self) {
        self.sink.play();
    }

    pub fn stop(&self) {
        self.sink.stop();
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
