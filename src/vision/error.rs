use thiserror::Error;

#[derive(Debug, Error)]
pub enum VisionError {
    #[error("OpenCV error: {0}")]
    Opencv(String),

    #[error("ONNX runtime error: {0}")]
    Onnx(String),

    #[error("Image decode error: {0}")]
    Decode(String),

    #[error("No detection found")]
    NoDetection,

    #[error("Model not loaded: {0}")]
    ModelNotLoaded(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Vision error: {0}")]
    Other(String),
}

impl From<opencv::Error> for VisionError {
    fn from(e: opencv::Error) -> Self {
        VisionError::Opencv(e.to_string())
    }
}

impl From<ort::Error> for VisionError {
    fn from(e: ort::Error) -> Self {
        VisionError::Onnx(e.to_string())
    }
}
