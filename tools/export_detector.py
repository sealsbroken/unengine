#!/usr/bin/env python3
"""Export an object detector to models/<name>.onnx with the .json sidecar Un's detector reads.

Needs a Python environment with ultralytics (uv: `uv run --with ultralytics,onnx,onnxslim python tools/export_detector.py`).

Three kinds of detector:

* Zero-shot YOLOE (default): object types are text prompts baked into the model. Each --class is
  ``label=prompt[,prompt...]``; labels starting with ``_`` are decoys (things the camera sees that
  must not be counted, e.g. the laptop keyboard) and are dropped by the engine after NMS. The
  label ``finger`` is special too: the engine shows those boxes but never counts them, so the
  child can point at an object without the finger becoming one.
* Any Ultralytics detector with no --class: your own trained weights (``--model best.pt``, see
  tools/teach.py). RT-DETR weights (``--model rtdetr-l.pt``) export with
  the ``rtdetr`` head.
* Anything else: write the sidecar by hand: {"labels": [...], "imgsz": [H, W], "head": "yolo|e2e|rtdetr"}.

Licences: Ultralytics code and weights are AGPL-3.0. For a closed product prefer Apache-2.0
detectors (RT-DETR, D-FINE, YOLOX) or your own training pipeline.

    python3 tools/export_detector.py                      # models/toys.onnx: every toy kind below
    python3 tools/export_detector.py --name cars --class "car=toy car,toy truck" \
        --class "finger=finger,index finger,fingertip,pointing finger" --class "_hand=hand"
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MODELS = ROOT / "models"
WEIGHTS = ROOT / "weights"

DEFAULT_CLASSES = [
    "cube=toy block,wooden toy block,toy cube,lego brick,building block",
    "car=toy car,toy truck,toy bus,toy train",
    "ball=toy ball,small ball,rubber ball",
    "bear=teddy bear,plush bear",
    "doll=doll,toy doll",
    "animal=toy animal,plastic animal,toy dinosaur,animal figurine,stuffed animal,plush toy",
    # Catch-all: a toy none of the kinds above names.
    "toy=toy,small toy,plastic toy",
    # Shown over the picture, never counted: the child points with it.
    "finger=finger,index finger,fingertip,pointing finger",
    # Decoys: soak up boxes on things that are always in view.
    "_keyboard=laptop keyboard,computer keyboard,trackpad",
    "_hand=hand",
]


def parse_classes(specs: list[str]) -> tuple[list[str], list[str]]:
    prompts: list[str] = []
    labels: list[str] = []
    for spec in specs:
        label, sep, rest = spec.partition("=")
        items = [p.strip() for p in rest.split(",") if p.strip()]
        if not sep or not label.strip() or not items:
            raise SystemExit(f"--class must be LABEL=PROMPT[,PROMPT...], got {spec!r}")
        prompts += items
        labels += [label.strip()] * len(items)
    return prompts, labels


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--class", dest="classes", action="append", metavar="LABEL=PROMPTS",
                        help="Object type and its text prompts (YOLOE only); repeat per type.")
    parser.add_argument("--model", default="yoloe-11s-seg.pt", help="Weights (downloaded into weights/).")
    # Square, and large: phones and tablets send portrait frames, desktop cameras landscape, and
    # small toys seen from above score far higher at 960 than at 640 (0.8 against 0.3 for a
    # figurine filling 1/10 of a portrait frame).
    parser.add_argument("--imgsz", type=int, nargs=2, default=[960, 960], metavar=("H", "W"),
                        help="Model input, height width (default 960 960: any orientation).")
    parser.add_argument("--name", default="toys", help="Output name in models/ (default toys).")
    args = parser.parse_args()

    WEIGHTS.mkdir(exist_ok=True)
    MODELS.mkdir(exist_ok=True)
    os.chdir(WEIGHTS)  # Ultralytics downloads weights (and YOLOE's text encoder) into the working directory.

    name = Path(args.model).name.lower()
    head = "rtdetr" if "rtdetr" in name else "auto"
    if "yoloe" in name:
        from ultralytics import YOLOE

        prompts, labels = parse_classes(args.classes or DEFAULT_CLASSES)
        model = YOLOE(args.model)
        model.set_classes(prompts, model.get_text_pe(prompts))
    else:
        if args.classes:
            raise SystemExit("--class needs a YOLOE model")
        if head == "rtdetr":
            from ultralytics import RTDETR as Model
        else:
            from ultralytics import YOLO as Model
        model = Model(args.model)
        names = model.names
        prompts, labels = [], [names[i] for i in range(len(names))]

    exported = Path(model.export(format="onnx", imgsz=args.imgsz, simplify=True, dynamic=False))
    destination = MODELS / f"{args.name}.onnx"
    shutil.move(str(exported), destination)
    meta = {"labels": labels, "prompts": prompts, "imgsz": args.imgsz, "head": head, "source": args.model}
    destination.with_suffix(".json").write_text(json.dumps(meta, indent=2) + "\n")
    print(f"wrote {destination} ({destination.stat().st_size / 1e6:.1f} MB) with labels {sorted(set(labels))}")


if __name__ == "__main__":
    main()
