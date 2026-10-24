"""Packs video clips for the Marco park's big screen (crates/skate-game/src/xr/marcovr/video/).

Each clip is cut (optionally), fitted to 16:9 (a vertical clip sits on a
blurred copy of itself), and the clips play one after another as a single
loop: small JPEG frames packed into one file, and mono 16-bit audio.

    python tools/make_tv_video.py clip1.mp4 [clip2.mp4@1:20-1:35 ...]
    python tools/make_tv_video.py --empty      # no clip: the screen shows "em breve"
    python tools/make_tv_video.py --mute clip.mp4               # the big screen, silent
    python tools/make_tv_video.py --reels --mute reels.mp4      # the vertical screen (9:16), always silent

The big screen's pack is frames.bin + audio.pcm; the vertical screen's is reels_frames.bin (no sound).

frames.bin layout (little endian): u32 fps, u32 frame count, then for each
frame u32 offset and u32 length into the data that follows the table.
audio.pcm is raw mono s16le at 22050 Hz, as long as the frames.
"""
import argparse
import struct
import subprocess
import tempfile
from pathlib import Path

import imageio_ffmpeg

OUT = Path(__file__).resolve().parent.parent / "crates/skate-game/src/xr/marcovr/video"
W, H, FPS, RATE = 640, 360, 15, 22050


def seconds(t):
    parts = [float(p) for p in t.split(":")]
    return sum(p * 60 ** i for i, p in enumerate(reversed(parts)))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("clips", nargs="*", help="video file, optionally @start-end (m:ss)")
    ap.add_argument("--empty", action="store_true", help="pack no clip at all")
    ap.add_argument("--max-seconds", type=float, default=60.0, help="cap on the whole loop")
    ap.add_argument("--mute", action="store_true", help="no sound (an empty audio.pcm)")
    ap.add_argument("--reels", action="store_true", help="the vertical 9:16 screen's pack instead")
    args = ap.parse_args()
    w, h = (H, W) if args.reels else (W, H)
    ffmpeg = imageio_ffmpeg.get_ffmpeg_exe()
    OUT.mkdir(parents=True, exist_ok=True)
    frames, audio = [], bytearray()
    budget = args.max_seconds
    if args.empty:
        args.clips = []
    with tempfile.TemporaryDirectory() as tmp:
        for n, spec in enumerate(args.clips):
            path, _, cut = spec.partition("@")
            trim = []
            if cut:
                a, b = cut.split("-")
                trim = ["-ss", str(seconds(a)), "-to", str(seconds(b))]
            limit = ["-t", str(budget)]
            # 16:9 (or 9:16) frame: the clip fitted in the middle over a blurred, filled copy.
            graph = (
                f"[0:v]split[bg][fg];"
                f"[bg]scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},boxblur=12:2,eq=brightness=-0.15[b];"
                f"[fg]scale={w}:{h}:force_original_aspect_ratio=decrease[f];"
                f"[b][f]overlay=(W-w)/2:(H-h)/2,fps={FPS}"
            )
            pattern = str(Path(tmp) / f"c{n}_%05d.jpg")
            subprocess.run([ffmpeg, "-v", "error", *trim, "-i", path, *limit, "-filter_complex", graph,
                            "-q:v", "6", pattern], check=True)
            clip_frames = sorted(Path(tmp).glob(f"c{n}_*.jpg"))
            pcm = subprocess.run([ffmpeg, "-v", "error", *trim, "-i", path, *limit, "-vn", "-ac", "1",
                                  "-ar", str(RATE), "-f", "s16le", "-"], check=True, capture_output=True).stdout
            # Keep sound and pictures the same length (silence if a clip has none).
            want = len(clip_frames) * RATE // FPS * 2
            pcm = (pcm + bytes(want))[:want]
            frames += [f.read_bytes() for f in clip_frames]
            audio += pcm
            budget -= len(clip_frames) / FPS
            print(f"{path}: {len(clip_frames)} frames ({len(clip_frames) / FPS:.1f} s)")
            if budget <= 0:
                break
    table = bytearray(struct.pack("<II", FPS, len(frames)))
    offset = 0
    for f in frames:
        table += struct.pack("<II", offset, len(f))
        offset += len(f)
    if args.mute or args.reels:
        audio = bytearray()
    (OUT / ("reels_frames.bin" if args.reels else "frames.bin")).write_bytes(bytes(table) + b"".join(frames))
    if not args.reels:
        (OUT / "audio.pcm").write_bytes(bytes(audio))
    print(f"wrote {len(frames)} frames ({offset / 1e6:.1f} MB) and {len(audio) / 1e6:.1f} MB of audio")


if __name__ == "__main__":
    main()
