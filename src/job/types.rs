use std::time::{Duration, Instant};

use serde::Serialize;

use crate::config::jobs::JobConfig;
use crate::motion::NozzleId;

/// Runtime state of a running job.
#[derive(Debug)]
pub struct JobState {
    pub status: JobStatus,
    pub config: JobConfig,
    pub boards: Vec<BoardState>,
    pub current_step: Option<String>,
    pub stats: JobStats,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JobStatus {
    Idle,
    Running,
    Paused { reason: PauseReason },
    Complete,
    Error { message: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum PauseReason {
    UserRequested,
    PickFailed {
        placement: String,
        attempts: u32,
    },
    VisionFailed {
        placement: String,
    },
    FeederEmpty {
        feeder_id: String,
    },
}

/// Runtime state of a single board instance within a job.
#[derive(Debug)]
pub struct BoardState {
    pub board_idx: usize,
    pub transform: Option<AffineTransform>,
    pub placements: Vec<PlacementState>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementState {
    Pending,
    InProgress,
    Placed,
    Skipped,
    Failed { reason: String },
}

/// A planned step in the job execution.
#[derive(Debug)]
pub enum JobStep {
    FiducialCheck {
        board_idx: usize,
    },
    ChangeTips(Vec<TipChange>),
    PickBatch(Vec<NozzleAssignment>),
    AlignBatch(Vec<NozzleAssignment>),
    PlaceBatch(Vec<NozzleAssignment>),
}

#[derive(Debug, Clone)]
pub struct TipChange {
    pub nozzle: NozzleId,
    pub from_tip: Option<String>,
    pub to_tip: String,
}

/// Assignment of a nozzle to pick-align-place a specific component.
#[derive(Debug, Clone)]
pub struct NozzleAssignment {
    pub nozzle: NozzleId,
    pub tip_id: String,
    pub feeder_id: String,
    pub part_id: String,
    pub board_idx: usize,
    pub placement_idx: usize,
    /// Set by alignment step.
    pub alignment: Option<AlignmentOffset>,
}

#[derive(Debug, Clone)]
pub struct AlignmentOffset {
    pub dx: f64,
    pub dy: f64,
    pub drot: f64,
}

/// 2D affine transform for board coordinate mapping.
///
/// Maps board-space (x, y) to machine-space (x', y'):
///   x' = a*x + b*y + tx
///   y' = c*x + d*y + ty
#[derive(Debug, Clone)]
pub struct AffineTransform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl AffineTransform {
    /// Identity transform (no transformation).
    pub fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// Create a transform from translation only.
    pub fn translation(tx: f64, ty: f64) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx,
            ty,
        }
    }

    /// Create a transform from translation + rotation (degrees CCW).
    pub fn from_translation_rotation(tx: f64, ty: f64, rotation_deg: f64) -> Self {
        let r = rotation_deg.to_radians();
        let cos = r.cos();
        let sin = r.sin();
        Self {
            a: cos,
            b: -sin,
            c: sin,
            d: cos,
            tx,
            ty,
        }
    }

    /// Transform a point from board-space to machine-space.
    pub fn transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.b * y + self.tx,
            self.c * x + self.d * y + self.ty,
        )
    }

    /// Extract rotation in degrees from the transform matrix.
    pub fn rotation_deg(&self) -> f64 {
        self.c.atan2(self.a).to_degrees()
    }

    /// Transform a rotation angle (add the transform's rotation).
    pub fn transform_rotation(&self, deg: f64) -> f64 {
        deg + self.rotation_deg()
    }
}

/// Accumulated job statistics.
#[derive(Debug, Clone, Serialize)]
pub struct JobStats {
    pub total_placements: usize,
    pub completed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub pick_retries: usize,
    #[serde(skip)]
    pub started_at: Option<Instant>,
    pub elapsed_secs: f64,
}

impl JobStats {
    pub fn new(total_placements: usize) -> Self {
        Self {
            total_placements,
            completed: 0,
            failed: 0,
            skipped: 0,
            pick_retries: 0,
            started_at: None,
            elapsed_secs: 0.0,
        }
    }

    pub fn update_elapsed(&mut self) {
        if let Some(start) = self.started_at {
            self.elapsed_secs = start.elapsed().as_secs_f64();
        }
    }
}

/// Convert machine Z axis position for nozzle engagement.
///
/// The LumenPnP uses a shared Z axis where:
/// - Low Z values (< safe_zone) engage N1
/// - High Z values (> safe_zone) engage N2
/// - Z value for N2 = 63.0 - Z_for_N1
pub fn nozzle_z(nozzle: NozzleId, target_z: f64) -> f64 {
    match nozzle {
        NozzleId::N1 => target_z,
        NozzleId::N2 => 63.0 - target_z,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_affine_identity() {
        let t = AffineTransform::identity();
        let (x, y) = t.transform_point(10.0, 20.0);
        assert!((x - 10.0).abs() < 0.001);
        assert!((y - 20.0).abs() < 0.001);
    }

    #[test]
    fn test_affine_translation() {
        let t = AffineTransform::translation(50.0, 100.0);
        let (x, y) = t.transform_point(10.0, 20.0);
        assert!((x - 60.0).abs() < 0.001);
        assert!((y - 120.0).abs() < 0.001);
    }

    #[test]
    fn test_affine_rotation_90() {
        let t = AffineTransform::from_translation_rotation(0.0, 0.0, 90.0);
        let (x, y) = t.transform_point(10.0, 0.0);
        assert!(x.abs() < 0.001); // should be ~0
        assert!((y - 10.0).abs() < 0.001); // should be 10
    }

    #[test]
    fn test_affine_translation_and_rotation() {
        let t = AffineTransform::from_translation_rotation(100.0, 50.0, 0.0);
        let (x, y) = t.transform_point(10.0, 20.0);
        assert!((x - 110.0).abs() < 0.001);
        assert!((y - 70.0).abs() < 0.001);
    }

    #[test]
    fn test_nozzle_z_n1() {
        assert!((nozzle_z(NozzleId::N1, 5.0) - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_nozzle_z_n2() {
        assert!((nozzle_z(NozzleId::N2, 5.0) - 58.0).abs() < 0.001);
    }

    #[test]
    fn test_rotation_deg_extraction() {
        let t = AffineTransform::from_translation_rotation(0.0, 0.0, 45.0);
        assert!((t.rotation_deg() - 45.0).abs() < 0.01);
    }
}
