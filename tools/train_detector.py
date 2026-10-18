#!/usr/bin/env python3
"""Trains an RF-DETR detector (Roboflow, Apache-2.0: code and the nano/small/medium weights) on a
dataset written by tools/label.py, and exports it for the engine: ``models/<name>.onnx`` plus its
``.json`` sidecar. No Ultralytics: the model is yours under Apache-2.0 terms.

    uv run --with rfdetr,onnx,onnxruntime python tools/train_detector.py --dataset datasets/toys --name toys
    uv run --with rfdetr,onnx,onnxruntime python tools/train_detector.py --export-only runs/toys/checkpoint_best_total.pth --name toys

The exported graph takes what every engine detector takes: RGB in 0...1, letterboxed to a
square of ``--resolution`` pixels (``[1, 3, R, R]``). The ImageNet normalisation RF-DETR wants is
inside the graph, and the output is one tensor ``[1, queries, 4 + classes]``: normalised
``cx cy w h`` then a sigmoid score per class, the engine's ``rtdetr`` head.
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SIZES = {"nano": "RFDETRNano", "small": "RFDETRSmall", "medium": "RFDETRMedium"}
MEAN = [0.485, 0.456, 0.406]
STD = [0.229, 0.224, 0.225]


def model_class(size: str):
    import rfdetr

    return getattr(rfdetr, SIZES[size])


def train(args: argparse.Namespace) -> Path:
    dataset = args.dataset.resolve()
    # RF-DETR's Roboflow layout wants a test split too; the validation images serve.
    test = dataset / "test"
    if not test.exists():
        test.symlink_to(dataset / "valid", target_is_directory=True)
    out = (ROOT / "runs" / args.name).resolve()
    model = model_class(args.size)(resolution=args.resolution)
    model.train(
        dataset_dir=str(dataset),
        output_dir=str(out),
        epochs=args.epochs,
        batch_size=args.batch,
        grad_accum_steps=args.accumulate,
        lr=args.lr,
        resolution=args.resolution,
        # One fixed input size: on Apple GPUs every new shape builds new compute graphs, which made
        # multi-scale training an order of magnitude slower (and the enlarged scales overflow 16 GB).
        multi_scale="off",
        expanded_scales=False,
    )
    for name in ("checkpoint_best_total.pth", "checkpoint_best_ema.pth", "checkpoint_best_regular.pth", "checkpoint.pth"):
        if (out / name).exists():
            return out / name
    raise SystemExit(f"no checkpoint in {out}")


def export(args: argparse.Namespace, checkpoint: Path, labels: list[str]) -> None:
    import numpy as np
    import onnx
    import onnxruntime as ort
    from onnx import TensorProto, helper, numpy_helper

    work = (ROOT / "runs" / args.name / "export").resolve()
    shutil.rmtree(work, ignore_errors=True)
    model = model_class(args.size)(pretrain_weights=str(checkpoint), resolution=args.resolution)
    model.export(output_dir=str(work), shape=(args.resolution, args.resolution), verbose=False)
    exported = next(work.glob("*.onnx"))
    graph_model = onnx.load(str(exported))
    graph = graph_model.graph
    raw_input = graph.input[0].name
    outputs = {o.name: o for o in graph.output}
    boxes = next(name for name in outputs if "dets" in name)
    logits = next(name for name in outputs if "labels" in name)

    # In front: RGB 0...1 -> ImageNet-normalised, as RF-DETR was trained.
    mean = numpy_helper.from_array(np.array(MEAN, dtype=np.float32).reshape(1, 3, 1, 1), "un_mean")
    std = numpy_helper.from_array(np.array(STD, dtype=np.float32).reshape(1, 3, 1, 1), "un_std")
    graph.initializer.extend([mean, std])
    new_input = helper.make_tensor_value_info("images", TensorProto.FLOAT, [1, 3, args.resolution, args.resolution])
    for node in graph.node:
        node.input[:] = ["un_normalised" if name == raw_input else name for name in node.input]
    front = [
        helper.make_node("Sub", ["images", "un_mean"], ["un_centred"]),
        helper.make_node("Div", ["un_centred", "un_std"], ["un_normalised"]),
    ]
    # Behind: one [1, Q, 4 + C] tensor, boxes then sigmoid scores (the engine's rtdetr head).
    back = [
        helper.make_node("Sigmoid", [logits], ["un_scores"]),
        helper.make_node("Concat", [boxes, "un_scores"], ["output0"], axis=2),
    ]
    nodes = front + list(graph.node) + back
    del graph.node[:]
    graph.node.extend(nodes)
    del graph.input[:]
    graph.input.extend([new_input])
    del graph.output[:]
    graph.output.extend([helper.make_tensor_value_info("output0", TensorProto.FLOAT, None)])
    target = ROOT / "models" / f"{args.name}.onnx"
    onnx.save(graph_model, str(target))

    session = ort.InferenceSession(str(target), providers=["CPUExecutionProvider"])
    out = session.run(None, {"images": np.zeros((1, 3, args.resolution, args.resolution), np.float32)})[0]
    classes = out.shape[2] - 4
    # RF-DETR may keep one more class slot than the dataset has; name the extras so they are dropped.
    names = labels + [f"_unused{i}" for i in range(max(0, classes - len(labels)))]
    sidecar = {
        "labels": names[:classes],
        "imgsz": [args.resolution, args.resolution],
        "head": "rtdetr",
        # GridSample and the decoder's attention need CoreML's MLProgram format to stay on the GPU.
        "coreml": "mlprogram",
        "source": f"RF-DETR {args.size} (Apache-2.0), trained by tools/train_detector.py",
        "licence": "Apache-2.0",
    }
    (ROOT / "models" / f"{args.name}.json").write_text(json.dumps(sidecar, indent=2) + "\n")
    print(f"exported {target} ({target.stat().st_size / 1e6:.1f} MB): output {list(out.shape)}, labels {sidecar['labels']}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--dataset", type=Path, help="A tools/label.py output directory.")
    parser.add_argument("--name", default="toys", help="Model name: models/<name>.onnx.")
    parser.add_argument("--size", choices=SIZES, default="nano")
    parser.add_argument("--resolution", type=int, default=576, help="Square input side; a multiple of 32 (nano) or 64.")
    parser.add_argument("--epochs", type=int, default=30)
    parser.add_argument("--batch", type=int, default=2)
    parser.add_argument("--accumulate", type=int, default=8)
    parser.add_argument("--lr", type=float, default=1e-4)
    parser.add_argument("--labels", type=Path, help="labels.json (default: the dataset's).")
    parser.add_argument("--export-only", type=Path, metavar="CHECKPOINT", help="Skip training; export this checkpoint.")
    args = parser.parse_args()

    labels_file = args.labels or (args.dataset / "labels.json" if args.dataset else None)
    if labels_file is None or not labels_file.exists():
        parser.error("need --dataset (with labels.json) or --labels")
    labels = json.loads(labels_file.read_text())["labels"]
    checkpoint = args.export_only or train(args)
    export(args, checkpoint, labels)
    return 0


if __name__ == "__main__":
    sys.exit(main())
