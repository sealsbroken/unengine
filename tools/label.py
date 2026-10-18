#!/usr/bin/env python3
"""Labels photos with an open-vocabulary detector (OWLv2, Apache-2.0) and writes a COCO dataset
that tools/train_detector.py trains on. No Ultralytics anywhere: the labels and the model trained
on them stay under permissive licences.

    uv run --with torch,transformers,pillow python tools/label.py \\
        --images datasets/raw/openimages --images datasets/raw/table:8 --out datasets/toys

Each --images is a directory of .jpg/.png, optionally with ":N" to repeat it N times in the
training split (a few photos of your own table count as much as many photos from elsewhere).
Every --class is ``label=prompt[,prompt...]``; a label starting with ``_`` is a decoy the engine
drops (a hand, a finger), learned so that it is never taken for a toy. The defaults label every
toy as ``toy`` and hands and fingers as ``_hand``.

The output is ``<out>/{train,valid}/_annotations.coco.json`` plus the images, and
``<out>/labels.json`` with the class names in order. ``--preview N`` also writes N images with
the boxes drawn into ``<out>/preview`` for checking the labels by eye.
"""

from __future__ import annotations

import argparse
import json
import random
import shutil
import sys
from pathlib import Path

DEFAULT_CLASSES = [
    "toy=toy,toy block,lego brick,toy car,toy truck,toy train,toy ball,teddy bear,plush toy,doll,"
    "toy animal,animal figurine,toy dinosaur,small toy",
    "_hand=hand,finger",
]
TEACHER = "google/owlv2-base-patch16-ensemble"


def parse_classes(specs: list[str]) -> list[tuple[str, list[str]]]:
    out = []
    for spec in specs:
        label, _, prompts = spec.partition("=")
        out.append((label.strip(), [p.strip() for p in prompts.split(",") if p.strip()]))
    return out


def iou(a, b) -> float:
    x1, y1, x2, y2 = max(a[0], b[0]), max(a[1], b[1]), min(a[2], b[2]), min(a[3], b[3])
    inter = max(0.0, x2 - x1) * max(0.0, y2 - y1)
    union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter
    return inter / union if union > 0 else 0.0


def nms(boxes: list[tuple[list[float], float, int]], threshold: float) -> list[tuple[list[float], float, int]]:
    """Class-agnostic: the engine keeps one box per object, whichever class scores best."""
    kept: list[tuple[list[float], float, int]] = []
    for box in sorted(boxes, key=lambda b: -b[1]):
        if all(iou(box[0], k[0]) < threshold for k in kept):
            kept.append(box)
    return kept


def inside(inner: list[float], outer: list[float], share: float = 0.75) -> bool:
    """Most of `inner` lies in `outer`, which is clearly larger."""
    inter = max(0.0, min(inner[2], outer[2]) - max(inner[0], outer[0])) * max(0.0, min(inner[3], outer[3]) - max(inner[1], outer[1]))
    area = (inner[2] - inner[0]) * (inner[3] - inner[1])
    return area > 0 and inter / area >= share and area < 0.7 * (outer[2] - outer[0]) * (outer[3] - outer[1])


def objects_only(boxes: list[tuple[list[float], float, int]], decoys: set[int]) -> list[tuple[list[float], float, int]]:
    """Drops the teacher's "group" boxes (one box around two or more toys) and "part" boxes (the
    head of a toy already boxed whole), the same rule the engine applies: the detector should learn
    one box per toy."""
    toys = [b for b in boxes if b[2] not in decoys]
    whole = [b for b in toys if sum(1 for o in toys if o is not b and inside(o[0], b[0])) < 2]
    out: list[tuple[list[float], float, int]] = []
    for box in sorted(whole, key=lambda b: -(b[0][2] - b[0][0]) * (b[0][3] - b[0][1])):
        if not any(inside(box[0], o[0]) for o in out):
            out.append(box)
    return out + [b for b in boxes if b[2] in decoys]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--images", action="append", required=True, help="Directory of images, optionally DIR:REPEAT.")
    parser.add_argument("--class", dest="classes", action="append", help="label=prompt,prompt (repeatable).")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--threshold", type=float, default=0.25, help="Teacher score a box needs.")
    parser.add_argument("--valid", type=float, default=0.1, help="Share of images held out for validation.")
    parser.add_argument("--preview", type=int, default=0)
    parser.add_argument("--device", default=None, help="cpu, mps or cuda (default: the best available).")
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()

    import torch
    from PIL import Image, ImageDraw, ImageOps
    from transformers import AutoProcessor, Owlv2ForObjectDetection

    classes = parse_classes(args.classes or DEFAULT_CLASSES)
    labels = [label for label, _ in classes]
    prompts: list[str] = []
    prompt_class: list[int] = []
    for index, (_, texts) in enumerate(classes):
        prompts += texts
        prompt_class += [index] * len(texts)
    device = args.device or ("cuda" if torch.cuda.is_available() else "mps" if torch.backends.mps.is_available() else "cpu")
    processor = AutoProcessor.from_pretrained(TEACHER)
    model = Owlv2ForObjectDetection.from_pretrained(TEACHER).to(device).eval()

    sources: list[tuple[Path, int]] = []
    for spec in args.images:
        directory, _, repeat = spec.partition(":")
        files = sorted(p for p in Path(directory).iterdir() if p.suffix.lower() in (".jpg", ".jpeg", ".png"))
        sources += [(p, int(repeat or 1)) for p in files]
    random.Random(args.seed).shuffle(sources)
    n_valid = max(1, int(len(sources) * args.valid))

    if args.out.exists():
        shutil.rmtree(args.out)
    coco = {split: {"images": [], "annotations": [], "categories": [
        {"id": i, "name": name, "supercategory": "none"} for i, name in enumerate(labels)]} for split in ("train", "valid")}
    for split in coco:
        (args.out / split).mkdir(parents=True)
    if args.preview:
        (args.out / "preview").mkdir()

    annotation_id = 0
    for n, (path, repeat) in enumerate(sources):
        split = "valid" if n < n_valid else "train"
        try:
            image = ImageOps.exif_transpose(Image.open(path)).convert("RGB")
        except OSError as error:
            print(f"skipping {path}: {error}", flush=True)
            continue
        width, height = image.size
        side = max(width, height)  # OWLv2 pads to a square; boxes come back in that square's pixels
        inputs = processor(text=[prompts], images=image, return_tensors="pt").to(device)
        with torch.no_grad():
            outputs = model(**inputs)
        result = processor.post_process_grounded_object_detection(
            outputs, threshold=args.threshold, target_sizes=[(side, side)])[0]
        boxes = []
        for box, score, label in zip(result["boxes"].tolist(), result["scores"].tolist(), result["labels"].tolist()):
            x1, y1, x2, y2 = max(0.0, box[0]), max(0.0, box[1]), min(width, box[2]), min(height, box[3])
            # A box over most of the picture is the scene, not a toy.
            if x2 - x1 < 4 or y2 - y1 < 4 or (x2 - x1) * (y2 - y1) > 0.6 * width * height:
                continue
            boxes.append(([x1, y1, x2, y2], score, prompt_class[label]))
        boxes = objects_only(nms(boxes, 0.5), {i for i, label in enumerate(labels) if label.startswith("_")})
        copies = repeat if split == "train" else 1
        for copy in range(copies):
            name = f"{path.stem}-{copy}.jpg" if copies > 1 else f"{path.stem}.jpg"
            image.save(args.out / split / name, quality=92)
            image_id = len(coco[split]["images"])
            coco[split]["images"].append({"id": image_id, "file_name": name, "width": width, "height": height})
            for box, score, cls in boxes:
                x1, y1, x2, y2 = box
                coco[split]["annotations"].append({
                    "id": annotation_id, "image_id": image_id, "category_id": cls,
                    "bbox": [x1, y1, x2 - x1, y2 - y1], "area": (x2 - x1) * (y2 - y1), "iscrowd": 0,
                })
                annotation_id += 1
        if n < args.preview:
            draw = ImageDraw.Draw(image)
            for box, score, cls in boxes:
                colour = "red" if labels[cls].startswith("_") else "lime"
                draw.rectangle(box, outline=colour, width=max(2, side // 300))
                draw.text((box[0] + 4, box[1] + 4), f"{labels[cls]} {score:.2f}", fill=colour)
            image.save(args.out / "preview" / f"{path.stem}.jpg", quality=85)
        if n % 50 == 0:
            print(f"{n}/{len(sources)} {path.name}: {len(boxes)} boxes", flush=True)

    for split, data in coco.items():
        (args.out / split / "_annotations.coco.json").write_text(json.dumps(data))
    (args.out / "labels.json").write_text(json.dumps({"labels": labels, "teacher": TEACHER}, indent=2) + "\n")
    print(f"wrote {args.out}: " + ", ".join(f"{s} {len(d['images'])} images, {len(d['annotations'])} boxes" for s, d in coco.items()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
