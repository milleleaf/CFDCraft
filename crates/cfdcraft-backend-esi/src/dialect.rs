#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    EsiV2512,
}

pub const IMAGE: &str = "opencfd/openfoam-default@sha256:33fb575aa9980d2bc42fd58c75ae698c489293ba30c991380fe3f899c622f319";
pub const IMAGE_ENV: &str = "bash -lc";
