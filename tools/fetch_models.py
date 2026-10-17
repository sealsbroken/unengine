#!/usr/bin/env python3
"""Download the models Un needs into models/. Works on macOS, Windows and Linux; standard library only.

    python3 tools/fetch_models.py             # recognisers, VAD and voices for EN and RU (~400 MB)
    python3 tools/fetch_models.py --language en
    python3 tools/fetch_models.py --whisper small    # better recogniser for both languages (~250 MB)
    python3 tools/fetch_models.py --dest /path/to/models

Sources (all Apache-2.0 / MIT):
  recognisers: sherpa-onnx Zipformer transducers (k2-fsa) and Silero VAD
  voices:      Piper voices packaged for sherpa-onnx (with espeak-ng-data), from the sherpa-onnx tts-models release

The object detector is produced by tools/export_detector.py (it needs training weights).
"""

from __future__ import annotations

import argparse
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

SHERPA_ASR = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models"
SHERPA_TTS = "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models"
RECOGNIZERS = {"en": "sherpa-onnx-zipformer-small-en-2023-06-26", "ru": "sherpa-onnx-zipformer-ru-int8-2025-04-20"}
VOICES = {"en": "vits-piper-en_US-lessac-medium", "ru": "vits-piper-ru_RU-irina-medium"}
# Whisper hears short answers far better than the small Zipformers and covers both languages;
# the engine prefers it whenever one of these directories is present.
WHISPER_SIZES = ("tiny", "base", "small", "medium", "turbo")


def shown(path: Path) -> str:
    return str(path.relative_to(ROOT)) if path.is_relative_to(ROOT) else path.name


def download(url: str, target: Path, min_size: int = 1000) -> bool:
    if target.exists() and target.stat().st_size >= min_size:
        print(f"  {shown(target)} already present")
        return True
    target.parent.mkdir(parents=True, exist_ok=True)
    print(f"  downloading {url}")
    try:
        with urllib.request.urlopen(url, timeout=120) as response, open(target, "wb") as out:
            while chunk := response.read(1 << 20):
                out.write(chunk)
    except OSError as error:
        target.unlink(missing_ok=True)
        print(f"error: download failed: {error}", file=sys.stderr)
        return False
    print(f"  {shown(target)} ({target.stat().st_size / 1e6:.1f} MB)")
    return True


def fetch_archive(url: str, folder: Path) -> bool:
    if folder.is_dir() and any(folder.glob("*.onnx")):
        print(f"  {shown(folder)} already present")
        return True
    with tempfile.TemporaryDirectory() as tmp:
        archive = Path(tmp) / "model.tar.bz2"
        if not download(url, archive):
            return False
        print(f"  unpacking into {shown(folder.parent)}")
        with tarfile.open(archive, "r:bz2") as tar:
            if sys.version_info >= (3, 12):
                tar.extractall(folder.parent, filter="data")
            else:
                tar.extractall(folder.parent)  # noqa: S202  trusted release archives
    return folder.is_dir()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--speech", action="store_true", help=argparse.SUPPRESS)  # always on; kept for old scripts
    parser.add_argument("--whisper", choices=WHISPER_SIZES, metavar="SIZE",
                        help="Also fetch a multilingual Whisper recogniser (tiny, base, small, medium, turbo).")
    parser.add_argument("--language", nargs="+", choices=["en", "ru"], default=["en", "ru"],
                        help="Languages to fetch speech models for (default: both).")
    parser.add_argument("--dest", type=Path, default=ROOT / "models", help="Target directory (default: models/).")
    args = parser.parse_args()
    models: Path = args.dest
    speech = models / "speech"

    ok = True
    print("voice activity detection:")
    ok &= download(f"{SHERPA_ASR}/silero_vad.onnx", speech / "silero_vad.onnx", min_size=100_000)
    for lang in args.language:
        print(f"{lang}: recogniser")
        ok &= fetch_archive(f"{SHERPA_ASR}/{RECOGNIZERS[lang]}.tar.bz2", speech / RECOGNIZERS[lang])
        print(f"{lang}: voice")
        ok &= fetch_archive(f"{SHERPA_TTS}/{VOICES[lang]}.tar.bz2", speech / VOICES[lang])
    if args.whisper:
        name = f"sherpa-onnx-whisper-{args.whisper}"
        ok &= fetch_archive(f"{SHERPA_ASR}/{name}.tar.bz2", speech / name)
    print("done" if ok else "some downloads failed", file=sys.stderr)
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
