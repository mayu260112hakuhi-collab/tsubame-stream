use crate::AudioError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationAudioSource {
    /// PID reported by the Windows audio session.
    pub session_process_id: u32,
    /// PID used for process-loopback capture. For multi-process apps such as
    /// Discord this is the highest parent process with the same executable name.
    pub capture_process_id: u32,
    pub process_name: String,
    pub display_name: String,
}

impl ApplicationAudioSource {
    pub fn new(
        session_process_id: u32,
        capture_process_id: u32,
        process_name: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Self {
        Self {
            session_process_id,
            capture_process_id,
            process_name: process_name.into(),
            display_name: display_name.into(),
        }
    }

    pub fn is_discord(&self) -> bool {
        self.process_name.to_ascii_lowercase().contains("discord")
            || self.display_name.to_ascii_lowercase().contains("discord")
    }
}

pub fn application_source_label(source: &ApplicationAudioSource) -> String {
    let name = if source.display_name.trim().is_empty() {
        source.process_name.trim()
    } else {
        source.display_name.trim()
    };

    if source.is_discord() {
        format!("★ Discord — {name}")
    } else {
        format!("{name} (PID {})", source.capture_process_id)
    }
}

pub fn sort_application_sources(sources: &mut [ApplicationAudioSource]) {
    sources.sort_by(|a, b| {
        b.is_discord()
            .cmp(&a.is_discord())
            .then_with(|| {
                application_source_label(a)
                    .to_ascii_lowercase()
                    .cmp(&application_source_label(b).to_ascii_lowercase())
            })
            .then_with(|| a.capture_process_id.cmp(&b.capture_process_id))
    });
}

#[cfg(windows)]
fn root_process_for_audio_session(system: &sysinfo::System, pid: u32) -> (u32, String) {
    use sysinfo::Pid;

    let Some(process) = system.process(Pid::from_u32(pid)) else {
        return (pid, format!("PID {pid}"));
    };

    let process_name = process.name().to_string_lossy().into_owned();
    let normalized = process_name.to_ascii_lowercase();
    let mut root = pid;
    let mut cursor = process;

    // Discord, Chromium and similar applications render audio from a child
    // process. Process-loopback can include a target's child tree, so walk up
    // only while the executable name remains the same. This avoids accidentally
    // capturing an unrelated launcher/parent process.
    loop {
        let Some(parent_pid) = cursor.parent() else {
            break;
        };
        let Some(parent) = system.process(parent_pid) else {
            break;
        };
        let parent_name = parent.name().to_string_lossy().to_ascii_lowercase();
        if parent_name != normalized {
            break;
        }
        root = parent_pid.as_u32();
        cursor = parent;
    }

    (root, process_name)
}

#[cfg(windows)]
fn enumerate_windows_application_audio_sources() -> Result<Vec<ApplicationAudioSource>, AudioError>
{
    use std::collections::BTreeMap;
    use wasapi::{deinitialize, initialize_mta, DeviceEnumerator, Direction};

    initialize_mta()
        .ok()
        .map_err(|e| AudioError::Backend(format!("COM初期化失敗: {e:?}")))?;

    let result = (|| {
        let enumerator = DeviceEnumerator::new().map_err(|e| AudioError::Backend(e.to_string()))?;
        let devices = enumerator
            .get_device_collection(&Direction::Render)
            .map_err(|e| AudioError::Backend(e.to_string()))?;
        let system = sysinfo::System::new_all();
        let mut by_capture_pid = BTreeMap::<u32, ApplicationAudioSource>::new();

        for device_result in &devices {
            let Ok(device) = device_result else {
                continue;
            };
            let Ok(manager) = device.get_iaudiosessionmanager() else {
                continue;
            };
            let Ok(sessions) = manager.get_audiosessionenumerator() else {
                continue;
            };
            let count = sessions.get_count().unwrap_or(0);

            for index in 0..count {
                let Ok(session) = sessions.get_session(index) else {
                    continue;
                };
                let Ok(pid) = session.get_process_id() else {
                    continue;
                };
                if pid == 0 || pid == std::process::id() {
                    continue;
                }

                let (capture_pid, process_name) = root_process_for_audio_session(&system, pid);
                let session_display = session.get_display_name().unwrap_or_default();
                let display_name = if session_display.trim().is_empty() {
                    process_name.trim_end_matches(".exe").to_owned()
                } else {
                    session_display
                };

                by_capture_pid.entry(capture_pid).or_insert_with(|| {
                    ApplicationAudioSource::new(pid, capture_pid, process_name, display_name)
                });
            }
        }

        let mut sources: Vec<_> = by_capture_pid.into_values().collect();
        sort_application_sources(&mut sources);
        Ok(sources)
    })();

    deinitialize();
    result
}

pub fn enumerate_application_audio_sources() -> Result<Vec<ApplicationAudioSource>, AudioError> {
    #[cfg(windows)]
    {
        return std::thread::Builder::new()
            .name("yaoyorozu-enumerate-application-audio".to_owned())
            .spawn(enumerate_windows_application_audio_sources)
            .map_err(|e| AudioError::Backend(e.to_string()))?
            .join()
            .map_err(|_| {
                AudioError::Backend("アプリ音声列挙スレッドがpanicしました".to_owned())
            })?;
    }

    #[cfg(not(windows))]
    {
        Err(AudioError::UnsupportedPlatform)
    }
}
