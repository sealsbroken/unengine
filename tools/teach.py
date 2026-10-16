#!/usr/bin/env python3
"""Fine-tune a detector on frames recorded with `un collect`, then export it for the app.

    un collect --camera 0 --model toys --out datasets/toys --seconds 90
    uv run --with ultralytics,onnx,onnxslim python3 tools/teach.py --dataset datasets/toys --name my-toys

``un collect`` labels each saved frame with a teacher (a zero-shot detector exported with
tools/export_detector.py) and writes YOLO txt labels next to the images. Move the objects
between grabs, add and remove some, cover a few with a hand, so the student sees variety.

This script splits the frames 85/15, fine-tunes YOLO26n on them and writes
``models/<name>.onnx`` + ``.json``. The student only knows the labels it saw while
collecting, but it is about twice as fast as the zero-shot teacher and far more confident on
exactly those objects, from exactly this camera angle. Licence note: Ultralytics training code
and weights are AGPL-3.0; keep that in mind for a closed product.
"""

from __future__ import annotations

import argparse
import json
import random
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUNS = ROOT / "runs"
WEIGHTS = ROOT / "weights"
IMAGE_SUFFIXES = {".jpg", ".jpeg", ".png"}

# Identical small objects on a table seen from a fixed-ish angle. Flips and mosaic multiply
# instances per batch; colour jitter covers daylight vs lamp light; geometry stays mild so
# boxes stay tight.
AUGMENTATION: dict[str, float] = {
    "hsv_h": 0.015, "hsv_s": 0.5, "hsv_v": 0.4,
    "degrees": 5.0, "translate": 0.1, "scale": 0.4, "shear": 0.0, "perspective": 0.0,
    "flipud": 0.3, "fliplr": 0.5, "mosaic": 1.0, "mixup": 0.0, "copy_paste": 0.0, "close_mosaic": 10,
}


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--dataset", type=Path, default=ROOT / "datasets" / "toys", help="Dataset from `un collect`.")
    parser.add_argument("--name", default="my-toys", help="Run name and models/<name>.onnx (default my-toys).")
    parser.add_argument("--model", default="yolo26n.pt", help="Pretrained weights to start from (default yolo26n.pt).")
    parser.add_argument("--epochs", type=int, default=30, help="Epochs (default 30).")
    parser.add_argument("--batch", type=int, default=16, help="Batch size (default 16).")
    parser.add_argument("--imgsz", type=int, default=640, help="Training image size (default 640).")
    parser.add_argument("--device", help="mps | cpu | 0 (GPU index). Default: cuda or mps when available, else cpu.")
    parser.add_argument("--workers", type=int, default=4, help="Dataloader workers (default 4).")
    parser.add_argument("--val", type=float, default=0.15, help="Validation fraction (default 0.15).")
    parser.add_argument("--seed", type=int, default=0, help="Split and training seed (default 0).")
    parser.add_argument("--no-export", action="store_true", help="Train only; skip the ONNX export.")
    return parser.parse_args(argv)


def pick_device(wanted: str | None) -> str:
    if wanted:
        return wanted
    import torch

    if torch.cuda.is_available():
        return "0"
    if torch.backends.mps.is_available():
        return "mps"
    return "cpu"


def write_split(dataset: Path, val: float, seed: int) -> tuple[Path, dict[str, int]]:
    """Shuffle the frames, write train.txt / val.txt and data.yaml; return the yaml path and class counts."""
    images = sorted(p for p in (dataset / "images").iterdir() if p.suffix.lower() in IMAGE_SUFFIXES)
    classes = json.loads((dataset / "classes.json").read_text())
    if len(images) < 4:
        raise SystemExit(f"{dataset} has {len(images)} frames; collect more first")
    counts = dict.fromkeys(classes, 0)
    for image in images:
        label = dataset / "labels" / (image.stem + ".txt")
        for line in label.read_text().splitlines() if label.exists() else []:
            counts[classes[int(line.split()[0])]] += 1
    random.Random(seed).shuffle(images)
    n_val = max(1, round(len(images) * val))
    (dataset / "val.txt").write_text("".join(f"{p.resolve()}\n" for p in images[:n_val]))
    (dataset / "train.txt").write_text("".join(f"{p.resolve()}\n" for p in images[n_val:]))
    yaml = dataset / "data.yaml"
    names = "".join(f"  {i}: {name}\n" for i, name in enumerate(classes))
    yaml.write_text(f"path: {dataset.resolve()}\ntrain: train.txt\nval: val.txt\nnames:\n{names}")
    print(f"{len(images)} frames -> {len(images) - n_val} train / {n_val} val; boxes per class: {counts}", file=sys.stderr)
    return yaml, counts


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if not (args.dataset / "classes.json").exists():
        print(f"error: {args.dataset} has no classes.json; run `un collect` first", file=sys.stderr)
        return 1
    yaml, counts = write_split(args.dataset, args.val, args.seed)
    if not any(counts.values()):
        print("error: no boxes at all in the dataset; the teacher saw nothing", file=sys.stderr)
        return 1
    device = pick_device(args.device)
    print(f"training {args.model} on {yaml} | imgsz={args.imgsz} epochs={args.epochs} batch={args.batch} device={device}",
          file=sys.stderr)

    from ultralytics import YOLO  # late import: keeps --help fast

    WEIGHTS.mkdir(exist_ok=True)
    model_path = Path(args.model)
    if not model_path.exists() and (WEIGHTS / model_path.name).exists():
        model_path = WEIGHTS / model_path.name
    model = YOLO(str(model_path))
    model.train(
        data=str(yaml), imgsz=args.imgsz, epochs=args.epochs, batch=args.batch, device=device,
        workers=args.workers, project=str(RUNS), name=args.name, exist_ok=True, seed=args.seed,
        patience=max(10, args.epochs // 3), plots=False, verbose=False, **AUGMENTATION,
    )
    best = RUNS / args.name / "weights" / "best.pt"
    if not best.exists():
        print(f"error: training produced no {best}", file=sys.stderr)
        return 1
    metrics = model.val(data=str(yaml), imgsz=args.imgsz, device=device, plots=False, verbose=False)
    print(f"val mAP50 {metrics.box.map50:.3f}  mAP50-95 {metrics.box.map:.3f}  -> {best}", file=sys.stderr)
    if args.no_export:
        return 0
    return subprocess.call([sys.executable, str(ROOT / "tools" / "export_detector.py"), "--model", str(best), "--name", args.name])


if __name__ == "__main__":
    raise SystemExit(main())
