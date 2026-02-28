# Research: Terminal Tools for Media Metadata, Playback, and Preview

Research date: 2026-02-28. This document covers the state of the art for terminal/TUI media
handling across five areas: metadata extraction, audio playback, video preview, file format
support, and real-world CLI workflows.

---

## 1. Media Metadata Extraction

### 1.1 ffprobe (part of FFmpeg)

The gold standard for video/audio metadata extraction. Outputs JSON for scripting.

**Canonical invocation:**
```bash
ffprobe -v quiet -show_format -show_streams -print_format json input.mp4
```

**Key fields available:**

General (format section):
- `filename`, `format_name`, `format_long_name`
- `duration` (seconds as float string), `size` (bytes), `bit_rate`
- `nb_streams`, `nb_programs`
- `tags` → embedded title, artist, album, date, comment, encoder

Per-stream (streams array):
- `codec_type` → "video" | "audio" | "subtitle" | "data"
- `codec_name`, `codec_long_name` (e.g. "h264", "aac")
- `codec_tag_string` (four-char-code)
- `width`, `height`, `coded_width`, `coded_height` (video)
- `sample_aspect_ratio`, `display_aspect_ratio` (video)
- `pix_fmt` (e.g. "yuv420p"), `color_space`, `color_range`, `color_primaries`
- `r_frame_rate`, `avg_frame_rate` (e.g. "30/1")
- `bit_rate` (per stream), `nb_frames`
- `sample_rate`, `channels`, `channel_layout` (audio)
- `sample_fmt` (e.g. "fltp"), `bits_per_raw_sample`
- `profile` (e.g. "High", "AAC LC")
- `level` (H.264 level)
- `field_order` ("progressive" / "interlaced")
- `tags` → title, language, handler_name

**Selective extraction (faster):**
```bash
# Duration only
ffprobe -v quiet -show_entries format=duration -of csv=p=0 input.mp4

# Resolution only
ffprobe -v quiet -show_entries stream=width,height -of csv=p=0 input.mp4

# Video codec
ffprobe -v quiet -show_entries stream=codec_name -select_streams v:0 -of csv=p=0 input.mp4
```

**Seeking for fast thumbnail extraction:**
- Put `-ss` *before* `-i` for fast I-frame seeking (container-level seek, much faster than decoding)
- Put `-ss` *after* `-i` for accurate seek (decodes to exact frame, slower)

```bash
# Fast single-frame thumbnail at 10s mark
ffmpeg -ss 00:00:10 -i video.mp4 -frames:v 1 -vf "thumbnail,setsar=1" -y cover.jpg
```

**Performance notes:**
- Probing large files (especially remote or HDD) is slow if reading full streams
- `-v quiet` suppresses info spam
- Use `-probesize` and `-analyzeduration` flags to limit initial probe cost
- For thumbnails: seeking-based extraction is 3.8x faster than fps-filter-based extraction

---

### 1.2 mediainfo

More human-readable output, excellent container analysis, particularly strong for MKV/MP4.

**JSON output:**
```bash
mediainfo --Output=JSON input.mkv
```

**Key field groups:**

General track:
- `FileSize`, `Duration` (ms), `OverallBitRate`
- `Format`, `Format_Version`, `Format_Profile`, `Format_Commercial_IfAny`
- `Encoded_Date`, `Tagged_Date`, `Encoded_Application`, `Encoded_Library`
- `CompleteName`, `UniqueID`
- Title/album/track tags (mapped from Matroska, ID3, Vorbis, APEv2, etc.)

Video track:
- `Format` (codec), `Format_Profile`, `Format_Level`, `Format_Tier`
- `CodecID`, `Width`, `Height`, `Sampled_Width/Height`
- `DisplayAspectRatio`, `PixelAspectRatio`
- `FrameRate`, `FrameRate_Mode` (CFR vs VFR)
- `BitDepth`, `ColorSpace`, `ChromaSubsampling`
- `colour_primaries`, `transfer_characteristics`, `matrix_coefficients`
- `HDR_Format`, `HDR_Format_Compatibility` (HDR10, Dolby Vision, HLG)
- `ScanType` (Progressive/Interlaced)
- `Delay` (stream start delay)

Audio track:
- `Format` (codec), `Format_Profile`, `Format_Commercial_IfAny`
- `Channels`, `ChannelPositions`, `ChannelLayout`
- `SamplingRate`, `BitDepth`
- `BitRate`, `BitRate_Mode` (CBR/VBR)
- `Compression_Ratio`, `Language`
- `Delay` (relative to container start)

**ffprobe vs. mediainfo comparison:**
- ffprobe: better for stream-level analysis, automation, HDR metadata, frame-level probing
- mediainfo: better for container structure analysis, more readable output, broader codec
  identification strings, stronger handling of broadcast/archival formats

**Custom templates** (mediainfo unique feature):
```bash
# Template for just resolution + duration
mediainfo --Inform="Video;%Width%x%Height%\nGeneral;%Duration/String%\n" input.mp4
```

---

### 1.3 exiftool

The definitive tool for EXIF, XMP, IPTC, and manufacturer-specific metadata. Handles 100+
file formats including all major RAW photo formats.

**JSON output:**
```bash
exiftool -json -g input.jpg   # -g groups by metadata type (EXIF, XMP, etc.)
exiftool -json input.cr2       # RAW file
```

**Key metadata categories for photos:**

EXIF Camera Info:
- `Make`, `Model`, `LensModel`, `LensInfo`
- `Software`, `FirmwareVersion`
- `SerialNumber`, `LensSerialNumber`

Exposure:
- `ExposureTime`, `FNumber`, `ISO`
- `ExposureProgram`, `MeteringMode`, `ExposureCompensation`
- `Flash`, `FlashMode`, `FlashCompensation`
- `ShutterSpeedValue`, `ApertureValue`

Focus/Depth:
- `FocusMode`, `FocusDistance`, `SubjectDistance`
- `DepthOfField`, `HyperfocalDistance`

Image Data:
- `ImageWidth`, `ImageHeight`, `ExifImageWidth/Height` (may differ for RAW)
- `BitsPerSample`, `ColorSpace`, `Compression`
- `Orientation`, `XResolution`, `YResolution`, `ResolutionUnit`

GPS:
- `GPSLatitude`, `GPSLongitude`, `GPSAltitude`, `GPSAltitudeRef`
- `GPSLatitudeRef` (N/S), `GPSLongitudeRef` (E/W)
- `GPSDateStamp`, `GPSTimeStamp`, `GPSSpeed`, `GPSTrack`
- `GPSImgDirection`, `GPSMapDatum` (usually "WGS-84")

Timestamps:
- `DateTimeOriginal` (when shutter clicked)
- `CreateDate` (when written to file)
- `ModifyDate`

XMP (extended/creative):
- `Rating`, `Label`, `Keywords`, `Subject`
- `Creator`, `Copyright`, `Description`
- `Lens`, `CreatorTool`

RAW-specific (via MakerNotes):
- `WhiteBalance`, `WhiteBalanceTemperature`
- `DriveMode`, `BurstMode`
- `HighlightTonePriority`, `ShadowTonePriority`
- `DigitalZoom`, `OpticalZoom`
- Canon: `CameraTemperature`, `AFMicroAdjustment`
- Nikon: `ActiveDLighting`, `DistortionControl`, `VRMode`
- Sony: `SonyModelID`, `CreativeStyle`

**Practical field selection for a TUI display:**
```bash
# What matters most at-a-glance for a photo
exiftool -s -Make -Model -LensModel -ExposureTime -FNumber -ISO \
         -FocalLength -DateTimeOriginal -ImageWidth -ImageHeight \
         -GPSLatitude -GPSLongitude -GPSAltitude \
         input.jpg
```

**exiftool for video files:**
- Reads QuickTime/MP4 tags: `Duration`, `ImageWidth`, `ImageHeight`, `AvgBitrate`
- Reads XMP in video containers
- Does NOT decode video stream details (use ffprobe for that)

---

### 1.4 Rust Libraries for Metadata

**lofty** (audio metadata, pure Rust):
- Crate: `lofty` v0.22.x (MIT/Apache)
- Reads/writes: ID3v1, ID3v2, APE, Vorbis Comments, iTunes ilst
- Formats: MP3, FLAC, AAC, AIFF, WAV, Ogg Vorbis, Opus, Speex, MP4/M4A, WavPack, MPC
- API: `read_from_path()` → `TaggedFile` → `.primary_tag()` → tag fields
- Does NOT decode audio, only reads tag metadata

```rust
use lofty::{read_from_path, file::TaggedFileExt, tag::Accessor};
let tagged = read_from_path("track.flac")?;
let tag = tagged.primary_tag().unwrap();
println!("{:?}", tag.title());    // Option<&str>
println!("{:?}", tag.artist());
println!("{:?}", tag.album());
println!("{:?}", tag.year());
```

**symphonia** (audio decoding + container demux + metadata, pure Rust):
- Crate: `symphonia` v0.5.4 (MPL-2.0), ~141k downloads/month
- Formats: AIFF, CAF, ISO/MP4, MKV/WebM, OGG, WAV (feature-gated)
- Codecs: AAC-LC, ADPCM, ALAC, FLAC, MP1/2/3, PCM, Vorbis
- Reads metadata: Vorbis Comments, ID3v1/v2
- Used by: termusic (rusty backend), many other audio players
- Does full audio decoding (the library rodio delegates to symphonia by default)

```rust
// Getting audio file properties + metadata
let file = Box::new(File::open(path)?);
let mut hint = Hint::new();
let mut format = symphonia::default::get_probe()
    .format(&hint, MediaSourceStream::new(file, Default::default()),
            &FormatOptions::default(), &MetadataOptions::default())?
    .format;

// Duration from track
let track = format.tracks().first().unwrap();
let time_base = track.codec_params.time_base.unwrap();
let n_frames = track.codec_params.n_frames.unwrap_or(0);
```

**For RAW photos:**
- `rawkit` v0.1.0: Sony ARW only currently (from Graphite project), limited use
- `quickraw` v0.2.x: Pure Rust, LGPL-2.1, limited camera support
- `raw_preview_rs` v0.1.2: Wraps libraw (C), supports 27+ RAW formats (CR2, NEF, ARW, RAF etc.)
  - Provides EXIF extraction + quick JPEG preview generation
  - Requires native build tools (C/C++ linkage)
- `rsraw` v0.1.0: Another libraw wrapper
- **Recommendation**: For comprehensive RAW support, calling `exiftool` as subprocess or
  linking libraw via FFI is the practical path. Pure-Rust RAW support is immature.

**For calling ffprobe/exiftool as subprocess:**
```rust
use std::process::Command;
let output = Command::new("ffprobe")
    .args(["-v", "quiet", "-show_format", "-show_streams",
           "-print_format", "json", &path])
    .output()?;
let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
```

---

## 2. Audio Playback in Terminal

### 2.1 rodio (pure Rust audio playback)

- Crate: `rodio` v0.21.1 (MIT/Apache)
- Downloads: 5.3M total, 844k recent - widely used
- Built on `cpal` for cross-platform audio I/O
- Feature flags enable format support:
  - Default: WAV, Vorbis, MP3 (via minimp3), FLAC (via claxon)
  - Optional: symphonia backend (preferred for broader format support)

```rust
use rodio::{Decoder, OutputStream, Sink};
use std::fs::File;
use std::io::BufReader;

// Setup output device
let (_stream, stream_handle) = OutputStream::try_default()?;
let sink = Sink::try_new(&stream_handle)?;

// Load and play
let file = BufReader::new(File::open("audio.mp3")?);
let source = Decoder::new(file)?;
sink.append(source);
sink.sleep_until_end();
```

**Key rodio features:**
- `Sink` for playback control (pause/resume/stop/volume/speed)
- `sink.set_volume(0.5)` — range 0.0..1.0
- `sink.set_speed(1.5)` — playback speed
- `sink.skip_one()` — skip current track
- Multiple sources can be appended to a `Sink` (queue)
- `PeriodicAccess` for getting real-time position
- Supports mixing multiple sources simultaneously

**Integration with ratatui for a TUI music player:**
- Run audio playback on a background thread via `std::thread::spawn` or `tokio::task::spawn_blocking`
- Use channels (`std::sync::mpsc`) to send commands (play/pause/seek/volume)
- Use shared state (`Arc<Mutex<PlayerState>>`) or `Arc<AtomicU64>` for position

**Getting playback position in rodio:**
- rodio v0.20+ has `sink.get_pos()` → `Duration`
- Before v0.20: required wrapping source in custom `Source` that tracked samples

**Ratatui + rodio example (from DEV.to article):**
```rust
// In ratatui event loop thread:
// sender sends Command::Play, Command::Pause, Command::Stop etc.
// Separate audio thread receives and acts on commands
```

---

### 2.2 mpv as audio backend

Many terminal music players delegate to mpv rather than implementing their own audio stack:

- **termusic**: supports mpv backend, GStreamer backend, and native symphonia ("rusty") backend
- **mpvc**: POSIX shell music player using mpv + fzf
- **mpv-ipc**: Control running mpv via Unix socket with JSON IPC

**mpv IPC protocol:**
```bash
# Start mpv with IPC socket
mpv --input-ipc-server=/tmp/mpvsocket audio.flac

# Send commands via socket
echo '{ "command": ["get_property", "time-pos"] }' | socat - /tmp/mpvsocket
echo '{ "command": ["set_property", "pause", true] }' | socat - /tmp/mpvsocket
echo '{ "command": ["playlist-next"] }' | socat - /tmp/mpvsocket
```

**mpv useful properties for TUI integration:**
- `time-pos` — current playback position (seconds)
- `duration` — total duration
- `percent-pos` — 0-100
- `pause` — boolean
- `volume` — 0-100
- `playlist-pos` — current index
- `playlist-count` — total items
- `metadata` — dict with title/artist/album/etc.
- `media-title` — display title (from metadata or filename)
- `audio-bitrate`, `video-bitrate`

**mpv advantages for a TUI:**
- Handles every format ffmpeg handles (vast compatibility)
- Hardware decoding (videotoolbox, vaapi, etc.)
- Built-in IPC makes it controllable without reimplementing playback
- Can play network streams (HTTP, HLS, RTMP, YouTube via yt-dlp)

**mpv disadvantages:**
- External dependency (not embeddable as Rust library)
- Extra process overhead
- IPC socket handling requires async or polling

---

### 2.3 Audio Visualization in Terminal

**Waveform / Spectrum options:**

1. **tui-equalizer** (Rust/Ratatui):
   - Crate: `tui-equalizer` v0.1.2 (MIT/Apache)
   - A vertical bar chart equalizer widget for ratatui
   - Each band takes a 0.0..1.0 value → renders colored bars
   - Requires FFT computation separately

2. **audio-visualizer** (Rust):
   - Crate: `audio-visualizer` v0.5.0
   - Waveform + spectrum visualization library
   - "for developers to visually check audio samples"
   - Not a full TUI but provides the computation layer

3. **Custom FFT approach** (referenced Rust project: 500 LOC, Ratatui):
   - Uses Fast Fourier Transform on audio samples
   - 60 FPS target rendering
   - Terminal-width based responsive design
   - True RGB color gradients in terminal
   - Multi-threaded: audio decode thread + FFT thread + render thread

**Practical FFT pipeline for spectrum analyzer:**
```
Audio samples → FFT (rustfft crate) → frequency bins → normalize →
map to terminal columns → ratatui BarChart or custom canvas
```

**Key crates:**
- `rustfft` — fast FFT implementation
- `cpal` — low-level audio input (for capture) / output
- `dasp` — digital audio signal processing primitives
- `rubato` — sample rate conversion

**Waveform rendering (static, not real-time):**
```
samples → downsample to terminal width → map amplitude to row →
render with half-block characters (▄▀█) in ratatui
```

---

### 2.4 Terminal Music Player Reference Implementations

**termusic** (Rust, active, MIT):
- Multi-backend: Symphonia (native Rust), mpv, GStreamer
- Supported formats vary by backend:
  - Symphonia: FLAC, MP3, WAV, OGG Vorbis, M4A, WebM
  - mpv: essentially everything
  - GStreamer: everything GStreamer supports
- Has TUI with tag editor, playlist, library browser
- MPRIS support via D-Bus (Linux)
- Uses ratatui (or tui-rs predecessor)

**rmpc** (Rust, MPD client):
- MPD (Music Player Daemon) client
- Album art via terminal image protocols (Kitty, iTerm2, Sixel, Überzug++)
- Highly configurable

**fum** (Rust, MPRIS client):
- Minimal MPRIS music status display
- Customizable with configuration

**cmus** (C):
- Uses its own plugin system for audio output (ALSA, PulseAudio, OSS, libao, wavpack)
- Plugin for input codecs: libmad (MP3), libvorbis, libFLAC, libopus, ffmpeg (via plugin)
- Socket control: `cmus-remote` command for IPC
- The architecture is: file → input plugin (decode) → output plugin (playback)

**ncmpcpp** (C++):
- Client for MPD (Music Player Daemon)
- MPD handles all audio; ncmpcpp is pure UI
- Built-in spectrum visualizer (via `/tmp/mpd.fifo` audio output)
- Visualizer types: wave, wave_filled, frequency_spectrum, ellipse

---

## 3. Video Preview and Playback

### 3.1 Terminal Image/Video Protocols

**Available protocols (ranked by quality and adoption):**

| Protocol | Support | Quality | Video? |
|----------|---------|---------|--------|
| Kitty Graphics Protocol | Kitty, Ghostty, WezTerm, Konsole | Pixel-perfect | Animated |
| iTerm2 Inline Images | iTerm2, WezTerm, Tabby, VSCode | Pixel-perfect | Via frame loop |
| Sixel | WezTerm, Windows Terminal, foot, Black Box | Good | Via frame loop |
| Überzug++ | Any X11/Wayland via overlay | Excellent | Via frame loop |
| Unicode half-blocks (▄▀) | Every terminal | Low-medium | Via frame loop |
| Chafa (ANSI + Unicode) | Every terminal | Medium-high | Via frame loop |

**Terminal support matrix (2024/2025):**
- Kitty graphics: Kitty, Ghostty, WezTerm, Konsole (not Alacritty, iTerm2, Windows Terminal)
- iTerm2 protocol: iTerm2, WezTerm, Tabby, Hyper, VSCode, Bobcat (not Kitty, Ghostty)
- Sixel: WezTerm, Windows Terminal, foot, Black Box (not Kitty, iTerm2, Alacritty, Ghostty)

**ratatui-image** (the Rust/Ratatui solution):
- Crate: `ratatui-image` v10.x (MIT), 264k downloads, actively maintained
- Auto-detects available protocol via env vars + terminal query sequences
- Falls back gracefully: Kitty → iTerm2 → Sixel → Unicode halfblocks
- Handles font-size querying (needed to map pixels to character cells)
- API: `StatefulImage` widget + `protocol::StatefulProtocol` state

```rust
// ratatui-image usage sketch
use ratatui_image::picker::Picker;
use ratatui_image::StatefulImage;

let picker = Picker::from_query_stdio()?;  // auto-detect protocol
let image = picker.new_resize_protocol(dyn_image);  // DynamicImage from `image` crate
// In render():
frame.render_stateful_widget(StatefulImage::new(), area, &mut image_state);
```

---

### 3.2 Tools for Terminal Image/Video Display

**chafa** (C library + CLI):
- Converts raster images → ANSI/Unicode art
- Output modes: Sixels, Kitty, iTerm2, Unicode mosaics (auto-detected)
- Symbol sets: block, border, edge, dot, quad, half, ASCII, braille, etc.
- v1.14 (Jan 2024): pixel-perfect output via padding
- Achieves 20-30 FPS for video playback (with tuning)
- Used by: Yazi fallback, many fzf preview scripts

```bash
# Image preview
chafa --size 80x24 image.jpg

# Video preview (pipe frames from ffmpeg)
ffmpeg -i video.mp4 -vf fps=10 -f rawvideo -pix_fmt rgba - | \
  chafa --size 120x40 --format sixels -

# Animated GIF
chafa animation.gif
```

**timg** (C++):
- Terminal image and video viewer, 2.2k GitHub stars
- Supports: images (JPG, PNG, GIF, WebP, TIFF, BMP), video (via ffmpeg), PDF
- Protocols: Kitty, iTerm2, Sixel, Unicode halfblocks (auto-detected)
- Grid mode: `timg --grid=4 *.jpg` (4-column grid)
- Inline with terminal scrollback (output stays in terminal history)
- Can scroll static images
- Invoked: `timg image.jpg`, `timg video.mp4`

**viu** (Rust):
- Image viewer using Kitty or halfblock fallback
- Simple, no video support

**Überzug++** (C++):
- Renders images as X11/Wayland window overlays on top of terminal
- Works in ANY terminal emulator (since it's not terminal-protocol-based)
- Used by many file managers as a fallback/compatibility layer
- Yazi supports it via their adapter system

---

### 3.3 Yazi's Approach (reference implementation)

Yazi (24.5k GitHub stars, Rust) is the most complete reference for media preview in a Rust TUI:

**Protocol priority (from Yazi docs):**
```
kitty (unicode placeholders) → iTerm2 → Sixel → Überzug++ → Chafa (ASCII fallback)
```

**How Yazi handles video preview:**
1. Spawns `ffmpeg` subprocess to extract a thumbnail frame:
   ```bash
   ffmpeg -ss 00:00:05 -i video.mp4 -frames:v 1 /tmp/preview.jpg
   ```
2. Displays the thumbnail image using the detected terminal protocol
3. For audio: spawns `ffmpeg` or reads embedded cover art from tags

**Yazi plugin system for media:**
- `mediainfo.yazi`: Runs `ffmpeg` + `mediainfo`, displays formatted metadata
- `exifaudio.yazi`: Runs `exiftool`, shows audio metadata + embedded cover art

**Key lesson from Yazi:** treat image display and metadata extraction as separate concerns:
- Extraction: subprocess calls to ffprobe/mediainfo/exiftool
- Display: ratatui-image or direct protocol output

---

### 3.4 ASCII Video Playback

For actual ASCII video playback in terminal (not just thumbnails):

**Approach:** ffmpeg → raw frame pipe → character art renderer → terminal

```bash
# Via chafa
ffmpeg -i video.mp4 -vf "fps=15,scale=160:45" -f rawvideo -pix_fmt rgba - | \
  chafa --size 160x45 --format symbols --symbols block -

# Via timg (built-in)
timg --frames=all video.mp4  # plays video in terminal
```

**Performance reality:**
- 20-30 FPS achievable with chafa + tuning on modern hardware
- Terminal rendering is the bottleneck, not frame extraction
- Sixel/Kitty protocols handle higher frame rates than Unicode art
- Audio sync is tricky: common approach is to play audio separately via mpv/sox

**Real-world terminal video editors/players using this approach:**
- A terminal video editor (wonger.dev, 2024) achieved smooth playback using chafa + ffmpeg pipes
- `mpv --vo=tct` — mpv's built-in terminal output using 24-bit color block characters
- `mpv --vo=sixel` — sixel output (quality depends on terminal)
- `mpv --vo=kitty` — Kitty graphics protocol output (best quality with Kitty terminal)

**mpv terminal video modes:**
```bash
# Best quality (needs kitty terminal)
mpv --vo=kitty video.mp4

# Unicode blocks (works anywhere with 24-bit color)
mpv --vo=tct video.mp4

# Sixel
mpv --vo=sixel video.mp4

# ASCII art (no color, classic look)
mpv --vo=caca video.mp4
```

---

## 4. File Format Support Considerations

### 4.1 Common Formats by Category

**Images (raster):**
- Standard: JPG/JPEG, PNG, GIF (animated), WebP, BMP, TIFF
- Modern: AVIF, HEIC/HEIF, JXL (JPEG XL)
- Web: SVG (requires rasterization for display)

**Images (RAW camera):**
- Canon: CR2, CR3, CRW
- Nikon: NEF, NRW
- Sony: ARW, SRF, SR2
- Fuji: RAF
- Olympus: ORF
- Panasonic: RW2
- Pentax: PEF, DNG (Adobe)
- Apple: ProRAW (DNG variant)
- Universal: DNG (Adobe Digital Negative)

**Audio:**
- Lossless: FLAC, WAV/WAVE, AIFF, ALAC (in M4A)
- Lossy: MP3, AAC (M4A/MP4), OGG Vorbis, Opus, WMA
- Specialized: FLAC in OGG container, OPUS in OGG, Speex
- High-res: DSD (DSF, DFF) — not supported by most tools

**Video:**
- Container: MP4/M4V, MKV, AVI, MOV, WebM, TS/MTS, FLV, WMV
- Modern: MP4 + H.264, MP4 + H.265/HEVC, MKV + AV1, WebM + VP9/AV1
- Older: AVI + DivX/XviD, FLV + H.263/Sorenson

### 4.2 Tool Coverage per Format

**ffprobe/ffmpeg** handles essentially all formats (via libavformat/libavcodec):
- Full coverage: MP4, MKV, AVI, MOV, WebM, MP3, AAC, FLAC, WAV, OGG
- RAW photos: reads EXIF from some, but not full RAW decode (needs libraw)
- Limited: DSD audio, some proprietary formats

**mediainfo** coverage:
- All common video/audio formats
- Strong: MKV, MP4, FLAC, DSD
- Better codec identification strings than ffprobe in some cases

**exiftool** coverage:
- All common image formats
- All major RAW formats (most comprehensive)
- Video files: QuickTime/MP4 metadata (not stream decoding)
- PDF, Office documents (metadata only)

**symphonia** (Rust, pure):
- Audio only: FLAC, WAV, OGG Vorbis, MP3 (with feature flag), AAC (with flag), ALAC (with flag)
- Container: OGG, WAV, MP4/M4A (with flag), MKV (limited)
- NOT supported natively: DSD, WMA, AIFF (limited), APE

**lofty** (Rust, metadata only):
- MP3 (ID3v1/v2/APE), FLAC (Vorbis Comments), MP4/M4A (iTunes ilst)
- WAV (ID3v2/RIFF INFO), AIFF (ID3v2/Text Chunks), OGG/Opus/Speex (Vorbis Comments)

### 4.3 Formats Needing Special Handling

**RAW photos:**
- Require libraw or dcraw for decode/preview
- EXIF extraction works via exiftool without full decode
- Fast approach: extract embedded JPEG preview (most RAWs contain one)
  ```bash
  # Extract embedded JPEG from RAW
  exiftool -PreviewImage -b input.cr2 > preview.jpg
  exiftool -JpgFromRaw -b input.nef > preview.jpg
  ```

**HEIC/HEIF:**
- Apple's format (iPhone default since 2017)
- Requires `libheif` for full decode
- ffmpeg handles with `libheif` compiled in
- exiftool reads metadata without decode

**AVIF/JXL:**
- Newer formats, support is still being added to tools
- ffmpeg handles both

**GIF (animated):**
- chafa handles natively for terminal display
- timg handles natively
- ratatui-image can display via the image crate

**DSD audio:**
- DSF and DFF formats from high-end audio players
- Limited tool support (mpv handles, symphonia does not)

---

## 5. Existing Media CLI Workflows

### 5.1 fzf + mpv Pipelines

The classic pattern: fzf for file selection, mpv for playback.

**Basic music player:**
```bash
# Play selected music file
find ~/Music -name "*.mp3" -o -name "*.flac" | fzf | mpv --no-video -

# Play with preview (audio metadata via ffprobe)
find ~/Music -name "*.flac" | fzf \
  --preview 'ffprobe -v quiet -show_format -print_format json {} | jq -r .format.tags' | \
  xargs mpv --no-video
```

**Video player with thumbnail preview:**
```bash
# Requires kitty terminal for image preview
find ~/Videos -name "*.mp4" | fzf \
  --preview 'ffmpeg -ss 00:00:05 -i {} -frames:v 1 /tmp/preview.jpg -y 2>/dev/null && \
             kitty +kitten icat /tmp/preview.jpg' | \
  xargs mpv
```

**mpv status output in fzf preview (known issue):**
- mpv's status line uses ANSI escape to update in-place
- In fzf preview window, these are printed as new lines (not overwriting)
- Solution: `mpv --term-status-msg="" --quiet` or redirect stderr

**fzmedia** (shell script, real-world example):
```
MEDIA_ROOT → fuzzy finder → VIDEO_PLAYER
```
- Supports "continue watching" position tracking
- Works with HTTP indexes and local directories
- Works with fzy, fzf, dmenu; plays with mpv or vlc

**mpvc** (POSIX shell, active):
- Full-featured music player built on mpv + shell scripts
- Interfaces: CLI, TUI, FZF, WEB, EQZ (equalizer)
- Can play YouTube via yt-dlp
- Radio streams (SomaFM, etc.)
- Last push: 2026-02-26 (very active)

---

### 5.2 fzf Preview Scripts

**fzf-preview** (niksingh710, shell):
- Detects file type and routes to appropriate previewer:
  - Images → kitty/chafa
  - Video → ffmpeg thumbnail + display
  - Audio → metadata via exiftool or ffprobe
  - Text → bat/cat
  - Archives → list contents

**Common pattern in preview scripts:**
```bash
#!/bin/bash
FILE="$1"
MIME=$(file --mime-type -b "$FILE")
case "$MIME" in
  image/*)    chafa --size "$FZF_PREVIEW_COLUMNS"x"$FZF_PREVIEW_LINES" "$FILE" ;;
  video/*)    # extract frame + display
              ffmpeg -ss 5 -i "$FILE" -frames:v 1 /tmp/fzf-preview.jpg -y 2>/dev/null
              chafa --size "$FZF_PREVIEW_COLUMNS"x"$FZF_PREVIEW_LINES" /tmp/fzf-preview.jpg ;;
  audio/*)    exiftool "$FILE" | grep -E "Title|Artist|Album|Duration" ;;
  *)          bat --color=always "$FILE" 2>/dev/null || cat "$FILE" ;;
esac
```

---

### 5.3 Thumbnail Caching Strategies

Real-world tools maintain a thumbnail cache to avoid re-extracting on every directory visit:

**Cache location conventions:**
- `~/.cache/thumbnails/` (freedesktop.org spec)
- `~/.cache/<appname>/thumbnails/`

**Thumbnail generation pattern:**
```bash
# Fast: seek before input (I-frame only, 3.8x faster than decode seek)
ffmpeg -ss 00:00:10 -i "$file" -frames:v 1 -vf "scale=300:-1" \
       "$cache_dir/$hash.jpg" -y 2>/dev/null

# Smarter: pick a "representative" frame
ffmpeg -ss 00:00:10 -i "$file" -frames:v 1 -vf "thumbnail,scale=300:-1" \
       "$cache_dir/$hash.jpg" -y 2>/dev/null
```

**Cache invalidation:**
- Hash the file path + mtime to detect changes
- Use MD5/SHA256 of canonical file path as filename (freedesktop spec)
- Check cache before running ffmpeg

---

### 5.4 What People Build (Common Patterns Summary)

Patterns that appear consistently across many projects:

1. **Metadata display**: ffprobe JSON → parse → display selected fields
2. **Cover art**: exiftool to extract embedded image → display via terminal protocol
3. **Video thumbnail**: `ffmpeg -ss 5 -i file -frames:v 1 thumb.jpg` → display
4. **Audio preview**: mpv with `--no-video` flag (most reliable across formats)
5. **Image preview**: chafa (universal) or kitty/iterm2 protocol (higher quality)
6. **File selection**: fzf with `--preview` for real-time previews
7. **Music library**: directory walker + lofty/symphonia metadata → in-memory index
8. **Position saving**: write `path:position_seconds` to a file, resume via `mpv --start=`

---

## Key Recommendations for a TUI Tool

### Metadata extraction strategy

Tier 1 (primary, subprocess calls):
- `ffprobe -v quiet -show_format -show_streams -print_format json` for video/audio
- `exiftool -json -g` for images (especially RAW)
- Fall back from exiftool to ffprobe for video metadata

Tier 2 (Rust libraries, no subprocess):
- `lofty` for audio tag reading (title, artist, album, year, cover art)
- `symphonia` for audio properties (duration, bitrate, sample rate)
- `image` crate for basic image dimensions without subprocess

### Audio playback

For a TUI tool the options are:
1. **rodio** (embedded, pure Rust): Simple, works for common formats (MP3/FLAC/WAV/OGG)
   - Add symphonia feature flags for broader codec support
   - Missing: AAC without flag, no network streams
2. **mpv via IPC** (external process): Maximum compatibility, network stream support
   - Better for a "media player" use case
   - Adds external dependency
3. **Hybrid**: rodio for common formats, fall back to mpv subprocess for exotic formats

### Terminal image display

Use `ratatui-image` (v10.x) which handles:
- Protocol auto-detection (Kitty → iTerm2 → Sixel → halfblocks)
- Font-size querying for correct pixel mapping
- Supports the `image` crate's `DynamicImage` as input

For video thumbnails: spawn ffmpeg to extract frame → pass to ratatui-image.

### Audio visualization

For a spectrum analyzer:
1. Use `cpal` to capture audio samples from output or file
2. Apply FFT via `rustfft`
3. Map frequency bins to terminal columns
4. Render with `tui-equalizer` widget or custom ratatui BarChart

For waveform (static, from file):
1. Decode with `symphonia` → get sample buffer
2. Downsample to terminal width
3. Render with ratatui Canvas or half-block chars

### Format support priority

Start with the high-value, commonly requested formats:
- Images: JPG, PNG, GIF, WebP (via `image` crate)
- RAW: via exiftool for metadata + embedded JPEG extraction for preview
- Audio: MP3, FLAC, WAV, OGG, AAC, OPUS (via lofty + symphonia)
- Video: MP4, MKV, WebM, MOV (via ffprobe + ffmpeg for thumbnails)

---

## Crate Reference Summary

| Crate | Purpose | Version | License | Notes |
|-------|---------|---------|---------|-------|
| `lofty` | Audio tag read/write | 0.22.x | MIT/Apache | Broadest tag format support |
| `symphonia` | Audio decode + demux | 0.5.4 | MPL-2.0 | Pure Rust, used by termusic |
| `rodio` | Audio playback | 0.21.1 | MIT/Apache | Built on cpal, uses symphonia |
| `cpal` | Low-level audio I/O | 0.15.x | Apache-2.0 | Used by rodio, direct access |
| `ratatui-image` | Terminal image display | 10.x | MIT | Kitty/iTerm2/Sixel/halfblocks |
| `image` | Image decode/encode | 0.25.x | MIT/Apache | JPEG, PNG, WebP, GIF, etc. |
| `rustfft` | Fast Fourier Transform | 6.x | MIT/Apache | For spectrum analyzer |
| `tui-equalizer` | Ratatui equalizer widget | 0.1.x | MIT/Apache | Bar chart equalizer |

## External Tool Reference Summary

| Tool | Purpose | Notes |
|------|---------|-------|
| `ffprobe` | Video/audio metadata | JSON output, fast, universal |
| `ffmpeg` | Thumbnail extraction + conversion | `-ss` before `-i` for fast seek |
| `mediainfo` | Comprehensive media analysis | Better container structure analysis |
| `exiftool` | Image/RAW EXIF metadata | Best RAW support, GPS, MakerNotes |
| `mpv` | Audio/video playback | IPC socket for control |
| `chafa` | Terminal image/video rendering | Universal fallback |
| `timg` | Terminal image/video viewer | Auto-detects protocol |
