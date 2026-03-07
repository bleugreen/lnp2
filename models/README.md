# Vision Models

Place ONNX models in this directory. Reference them from `config/machine.toml`:

```toml
[cameras.bottom.vision]
model_path = "models/yolov8n.onnx"
```

## Supported Format

- **YOLOv8** `.onnx` models (standard or OBB variants)
- Input: `[1, 3, 640, 640]` float32, normalized 0–1, RGB
- Output: `[1, (4 + num_classes), 8400]` float32

## Export

Use `scripts/export_model.py` to convert a trained `.pt` model to `.onnx`:

```bash
python scripts/export_model.py --weights path/to/best.pt --output models/custom.onnx
```

## Pre-trained Models

For smoke testing, you can use a YOLOv8n model trained on COCO:

```bash
pip install ultralytics
yolo export model=yolov8n.pt format=onnx imgsz=640
cp yolov8n.onnx models/
```

This won't detect components but verifies the inference pipeline works end-to-end.
