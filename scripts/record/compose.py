#!/usr/bin/env python3
"""Compose the demo video: phone recording (left) + simulator frames (right).

Inputs (from record.sh):
  <out>/video_start_ms.txt   wall-clock ms when the browser video started
  <out>/*.webm               Playwright recording of play.html (480x1040)
  <frames>/<unix-ms>.png     simulator frames saved while runs/rec/ON existed

The simulator frames are timestamped, so they are laid on the browser video's
timeline with the ffmpeg concat demuxer (variable frame durations): both halves
stay in sync without any manual offset.

  python3 compose.py <out_dir> <frames_dir> <output.mp4> [--crf 28]
"""
import glob
import os
import subprocess
import sys

out_dir, frames_dir, output = sys.argv[1:4]
crf = sys.argv[sys.argv.index("--crf") + 1] if "--crf" in sys.argv else "28"

start = int(open(os.path.join(out_dir, "video_start_ms.txt")).read())
webm = glob.glob(os.path.join(out_dir, "*.webm"))[0]
frames = sorted(glob.glob(os.path.join(frames_dir, "*.png")), key=lambda f: int(os.path.basename(f)[:-4]))
if not frames:
    sys.exit("no simulator frames in " + frames_dir)
ts = [int(os.path.basename(f)[:-4]) for f in frames]

listing = os.path.join(out_dir, "frames.txt")
with open(listing, "w") as fh:
    # Hold the first frame from the browser video's t=0 until frames begin.
    fh.write(f"file '{os.path.abspath(frames[0])}'\nduration {max(ts[0] - start, 0) / 1000:.3f}\n")
    for i, f in enumerate(frames):
        d = (ts[i + 1] - ts[i]) / 1000 if i + 1 < len(frames) else 5.0
        fh.write(f"file '{os.path.abspath(f)}'\nduration {d:.3f}\n")
    fh.write(f"file '{os.path.abspath(frames[-1])}'\n")

duration = float(subprocess.check_output(
    ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", webm]).decode())

# 1920x1080: phone at native 480x1040 on the left, simulator 1380x776 on the right.
graph = (
    "color=c=0x141416:s=1920x1080:r=25[bg];"
    "[1:v]fps=25,setpts=PTS-STARTPTS[phone];"
    "[0:v]fps=25,scale=1380:776:flags=lanczos,setpts=PTS-STARTPTS[sim];"
    "[bg][phone]overlay=x=20:y=20:shortest=1[a];"
    "[a][sim]overlay=x=520:y=152:eof_action=repeat[v]"
)
subprocess.run([
    "ffmpeg", "-y", "-v", "error", "-f", "concat", "-safe", "0", "-i", listing, "-i", webm,
    "-filter_complex", graph, "-map", "[v]", "-c:v", "libx264", "-preset", "slow", "-crf", crf,
    "-pix_fmt", "yuv420p", "-movflags", "+faststart", "-t", f"{duration - 1:.2f}", output,
], check=True)
print(f"wrote {output}: {len(frames)} simulator frames, {duration:.0f} s")
