#!/usr/bin/env python3
"""Export a YOLOv8 .pt model to ONNX format for lnp2 vision pipeline.

Usage:
    python scripts/export_model.py --weights path/to/best.pt --output models/custom.onnx

Requirements:
    pip install ultralytics
"""
import argparse
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description="Export YOLOv8 model to ONNX")
    parser.add_argument("--weights", required=True, help="Path to .pt weights file")
    parser.add_argument("--output", default="models/model.onnx", help="Output .onnx path")
    parser.add_argument("--imgsz", type=int, default=640, help="Input image size (default: 640)")
    parser.add_argument("--opset", type=int, default=17, help="ONNX opset version (default: 17)")
    args = parser.parse_args()

    try:
        from ultralytics import YOLO
    except ImportError:
        print("Error: ultralytics not installed. Run: pip install ultralytics")
        return 1

    model = YOLO(args.weights)
    model.export(
        format="onnx",
        imgsz=args.imgsz,
        opset=args.opset,
        simplify=True,
        dynamic=False,
    )

    # ultralytics exports next to the .pt file, move to desired output
    exported = Path(args.weights).with_suffix(".onnx")
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)

    if exported.exists() and str(exported) != str(output):
        exported.rename(output)
        print(f"Model exported to {output}")
    elif exported.exists():
        print(f"Model exported to {exported}")
    else:
        print("Warning: export completed but .onnx file not found at expected location")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
