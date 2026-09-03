# Tsubame User Guide

> Target version: Tsubame v0.19.0-alpha  
> This guide is for the current development build. UI layout and option names may change in future releases.

## 1. What is Tsubame?

**Tsubame** is a Windows streaming and recording application focused on low resource usage, stability, and simple operation.

Typical uses:
- Game streaming
- Game recording
- Work streams
- Mixing desktop / microphone / application audio
- Image overlays

## 2. Before You Start

Requirements:
- Windows 11
- Tsubame
- FFmpeg
- Streaming configuration for services such as YouTube or Twitch

If FFmpeg is not bundled with your package, install it separately and make sure it is available through your system PATH.

```powershell
where.exe ffmpeg
```

or:

```powershell
Get-Command ffmpeg
```

## 3. Launching Tsubame

Launch `Tsubame.exe` or the executable included in the release package.

In some development builds, the executable may still be named `tsubame.exe` because of the historical internal package name.

A first-run message may appear when launching Tsubame for the first time.

## 4. Selecting a Video Source

Choose the screen or window you want to stream or record.

Typical sources:
- Desktop
- Individual windows
- Game windows

Before starting a stream or recording, confirm that the correct source is visible in the preview.

## 5. Preview

The preview lets you confirm what will be sent to the stream or recording.

Closing the preview can reduce CPU usage.

Recommended workflow:
1. Confirm the video before streaming or recording
2. Close the preview once everything looks correct
3. Continue streaming or recording

## 6. Audio Setup

Tsubame uses a compact vertical audio mixer.

Typical channels:
- Desktop audio
- Microphone
- Master
- Additional application audio channels

Depending on the channel, you can control:
- Gain
- Mute
- Mix
- Individual WAV recording

When the Windows default device is selected, the source label may appear as `Win規定`.

### Desktop Audio
Used for game audio, browser audio, and system sound.

### Microphone
Used for your streaming microphone.

### Application Audio
Supported applications such as Discord can be added as separate audio channels.

### Master
The final mixed audio sent to your stream or recording.

## 7. Recording

Before recording, check:
- Video source
- Resolution
- FPS
- Audio
- Output destination
- Encoder

Start recording once everything is ready.

After recording, play back the file and check video smoothness, audio/video sync, audio levels, resolution, and aspect ratio.

## 8. Streaming

### YouTube

Create a live stream in YouTube Studio and prepare the required settings.

For private testing, set the stream visibility to **Private** before starting the stream.

Configure as needed:
- Stream URL
- Stream key
- Bitrate
- FPS
- Encoder

### Twitch

Prepare your Twitch stream settings and stream key, then configure them in Tsubame.

## 9. Stream Keys

A stream key is sensitive information.

Do not:
- Upload it to GitHub
- Show it in screenshots
- Share it with other people
- Publish local configuration files containing secrets

Tsubame's settings system is designed so that stream keys are not stored as plain text.

## 10. Streaming and Recording at the Same Time

Tsubame is designed to support streaming and recording simultaneously.

Monitor:
- CPU usage
- GPU usage
- Game FPS
- Streaming stability
- Audio/video sync
- Frame drops

Closing the preview is recommended when minimizing overhead is important.

## 11. Overlays

Image overlays can be added to the output.

Example uses:
- Logos
- Stream frames
- Character images
- Decorative assets

Adjust position and size, then confirm the result in the preview.

## 12. Addons

Tsubame includes an addon management foundation.

The settings interface currently includes:
- General settings
- Official addons
- External addons

The project currently uses **Addon API v1** as its management foundation. External addon execution support will be expanded in future releases.

## 13. Settings Persistence

Tsubame saves major user settings and restores them on the next launch.

Examples:
- Streaming settings
- Bitrate
- Encoder
- Audio device selection
- Preview settings
- Some addon settings

If no settings file exists, Tsubame starts with default values.

## 14. Troubleshooting

### FFmpeg Not Found

```powershell
where.exe ffmpeg
```

If FFmpeg is not found, check its installation location and PATH configuration.

### Streaming Does Not Start
- Check the streaming platform
- Check the stream key
- Check the stream URL
- Check the network connection
- Confirm that FFmpeg is available

### No Audio
- Check the desktop audio device
- Check the microphone device
- Check Mute
- Check Mix
- Check Windows volume
- Check application audio selection

### High CPU Usage
- Close the preview
- Reduce FPS
- Reduce resolution
- Use a hardware encoder
- Close other recording or streaming software

### Capture FPS Is Below 60
The `Capture FPS` value shown in Tsubame does not necessarily represent the exact FPS of the final recorded file. Verify the final output by playing back the recording or checking the stream archive.

## 15. Recommended Test Procedure

1. Preview only
2. Short local recording
3. Private stream
4. Stream + recording
5. Test during a high-load game scene
6. Start a public stream once everything is stable

## 16. Bug Reports

If possible, include:
- Tsubame version
- Windows version
- CPU
- GPU
- Game / application used
- Recording or streaming
- Resolution / FPS
- Encoder
- Error message
- Reproduction steps
- Screenshot

GitHub Issues:  
https://github.com/mayu260112hakuhi-collab/tsubame-stream/issues

## 17. License

Tsubame itself is distributed under the **Apache License 2.0**.

If FFmpeg is bundled with a release, FFmpeg is treated as a separate external component under its own license.

See:
- `LICENSE`
- `NOTICE`
- `THIRD_PARTY_NOTICES.md`
- Bundled FFmpeg license files

## 18. Development Status

Tsubame is currently an Alpha release.

The following may change:
- UI
- Settings
- File structure
- Addon specifications
- Streaming behavior
- Error handling

Latest releases:  
https://github.com/mayu260112hakuhi-collab/tsubame-stream/releases
