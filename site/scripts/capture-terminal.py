# /// script
# requires-python = ">=3.11"
# dependencies = ["pyte==0.8.2", "pillow>=11,<13"]
# ///
"""Capture the real mls TUI with generated media, using a virtual terminal.

Run `uv run scripts/capture-terminal.py` on macOS with mls and ffmpeg installed.
Only the temporary sample directory is scanned. The PNG preserves the emitted
terminal cells and ANSI colors; no application UI is recreated by this script.
"""
import json
import fcntl
import os
from pathlib import Path
import pty
import select
import shutil
import struct
import subprocess
import tempfile
import termios
import time

import pyte
from PIL import Image, ImageDraw, ImageFont

for dependency in ("mls", "ffmpeg"):
    if not shutil.which(dependency):
        raise SystemExit(f"Missing {dependency}; install it before capturing the TUI")

OUTPUT = Path(__file__).resolve().parents[1] / "public/screenshots"
OUTPUT.mkdir(parents=True, exist_ok=True)
COLS, ROWS = 120, 32
FONT = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 20)
CELL_W, CELL_H = 12, 25
PALETTE = dict(default="#d9dce3", black="#101318", red="#e06c75", green="#98c379",
               brown="#e5c07b", blue="#61afef", magenta="#c678dd", cyan="#56b6c2", white="#edf0f5",
               brightblack="#727986", brightred="#ff8b94", brightgreen="#b3df96", brightbrown="#f6dc99",
               brightblue="#8bc4ff", brightmagenta="#db9cf3", brightcyan="#81d6df", brightwhite="#ffffff")


def color(value, background=False):
    if value == "default" and background:
        return "#101318"
    # Match a readable terminal palette for ANSI blue; pyte stores index 4 as RGB.
    if value == "0000ff":
        return PALETTE["blue"]
    return PALETTE.get(value, f"#{value}")


def capture(screen, name):
    image = Image.new("RGB", (COLS * CELL_W + 40, ROWS * CELL_H + 40), "#101318")
    draw = ImageDraw.Draw(image)
    for y in range(ROWS):
        for x in range(COLS):
            cell = screen.buffer[y][x]
            fg, bg = color(cell.fg), color(cell.bg, True)
            if cell.reverse:
                fg, bg = bg, fg
            px, py = 20 + x * CELL_W, 20 + y * CELL_H
            draw.rectangle((px, py, px + CELL_W - 1, py + CELL_H - 1), fill=bg)
            draw.text((px, py), cell.data, font=FONT, fill=fg, stroke_width=int(cell.bold), stroke_fill=fg)
    image.save(OUTPUT / f"{name}.png", optimize=True)
    (OUTPUT / f"{name}.txt").write_text("\n".join(line.rstrip() for line in screen.display) + "\n")


with tempfile.TemporaryDirectory(prefix="mls-capture-") as temp:
    library = Path(temp) / "Media"
    movies = library / "Movies"
    for folder in (movies, library / "Music", library / "Photos"):
        folder.mkdir(parents=True)
    for title, slug in (("Charge", "charge"), ("Spring", "spring"), ("Sprite Fright", "sprite-fright")):
        artwork = OUTPUT.parent / "artwork" / f"{slug}.jpg"
        subprocess.run(["ffmpeg", "-v", "error", "-loop", "1", "-i", str(artwork),
                        "-vf", "scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2",
                        "-t", "12", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-metadata", f"title={title}",
                        str(movies / f"{title}.mp4")], check=True)
        shutil.copyfile(artwork, movies / f"Poster - {title}.jpg")
    (movies / "Film notes.md").write_text("# Open films\n\nSample files using CC BY 4.0 artwork from Blender Studio.\n")
    # Synthetic tones keep the audio examples reproducible and free of recordings.
    for title, extension, rate, channels, codec in (
        ("Studio check", "wav", 48000, 1, "pcm_s16le"),
        ("Stereo reference", "flac", 44100, 2, "flac"),
    ):
        subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i",
                        f"sine=frequency=440:sample_rate={rate}:duration=12",
                        "-ac", str(channels), "-c:a", codec,
                        "-metadata", f"title={title}", "-metadata", "artist=Sample library",
                        str(library / "Music" / f"{title}.{extension}")], check=True)
    output = subprocess.run(["mls", "--json", "--sort", "name", str(movies)], capture_output=True, text=True, check=True)
    entries = json.loads(output.stdout)["entries"]
    audio_output = subprocess.run(["mls", "--json", "--sort", "name", str(library / "Music")], capture_output=True, text=True, check=True)
    entries.extend(json.loads(audio_output.stdout)["entries"])
    entries.sort(key=lambda entry: entry["file_name"])
    # Excerpts omit machine paths, timestamps, and the enclosing list schema.
    samples = []
    for entry in entries:
        media = entry["media"]
        excerpt = {"file_name": entry["file_name"], "media": {"kind": media["kind"]}}
        for key in ("duration_ms", "video", "audio", "tags", "doc"):
            if media.get(key) is not None:
                excerpt["media"][key] = media[key]
        samples.append(excerpt)
    (OUTPUT.parent / "samples.json").write_text(json.dumps(samples, indent=2) + "\n")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, ROWS * CELL_H, COLS * CELL_W))
    # The agent shell disables color globally; this child is a color terminal.
    capture_env = {k: v for k, v in os.environ.items() if k != "NO_COLOR"}
    capture_env.update(TERM="xterm-256color", COLORTERM="truecolor")
    process = subprocess.Popen(["mls", "--tui", "--sort", "name", str(movies)], stdin=slave, stdout=slave,
                               stderr=slave, cwd=movies, env=capture_env)
    os.close(slave)
    screen = pyte.Screen(COLS, ROWS)
    stream = pyte.ByteStream(screen)

    def drain(seconds):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if select.select([master], [], [], 0.1)[0]:
                chunk = os.read(master, 65536)
                stream.feed(chunk)
                # Reply to the terminal cursor query used during capability detection.
                if b"\x1b[6n" in chunk:
                    os.write(master, b"\x1b[1;1R")
                if b"\x1b[c" in chunk or b"\x1b[0c" in chunk:
                    os.write(master, b"\x1b[?1;2c")

    try:
        drain(4)
        if process.poll() is not None:
            raise RuntimeError("mls exited before capture: " + "\n".join(screen.display))
        if any("No preview" in line or "Loading preview" in line for line in screen.display):
            raise RuntimeError("The sample video thumbnail was not ready for capture")
        capture(screen, "browse")
    finally:
        process.terminate()
        os.close(master)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            # A TUI may retain a blocking terminal reader during shutdown. This
            # subprocess belongs only to the capture; never find/kill other MLS instances.
            process.kill()
            process.wait(timeout=5)
            print("Stopped the capture subprocess after its shutdown timed out")
print(f"Captured the installed mls TUI in {OUTPUT}")
