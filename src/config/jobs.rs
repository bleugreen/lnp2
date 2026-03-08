use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobConfig {
    pub name: String,
    pub boards: Vec<JobBoard>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobBoard {
    pub board_id: String,
    pub origin: BoardOrigin,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BoardOrigin {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub rotation: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_config_roundtrip() {
        let toml = r#"
name = "test_job"

[[boards]]
board_id = "8chsense"
enabled = true

[boards.origin]
x = 50.0
y = 50.0
rotation = 0.0

[[boards]]
board_id = "8chsense"
enabled = true

[boards.origin]
x = 150.0
y = 50.0
rotation = 0.0
"#;
        let job: JobConfig = toml::from_str(toml).unwrap();
        assert_eq!(job.name, "test_job");
        assert_eq!(job.boards.len(), 2);
        assert!((job.boards[0].origin.x - 50.0).abs() < 0.001);
        assert!((job.boards[1].origin.x - 150.0).abs() < 0.001);
    }
}
