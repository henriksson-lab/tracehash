//! Sampling policy: decides which calls get recorded to the deep log.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

#[derive(Debug, Clone)]
pub enum SamplePolicy {
    All,
    FirstN(u64),
    FirstNAndLast(u64),
    Probabilistic {
        p: f64,
        first_n_always: u64,
        keep_last: bool,
    },
}

impl SamplePolicy {
    pub fn keeps_last(&self) -> bool {
        matches!(
            self,
            SamplePolicy::FirstNAndLast(_)
                | SamplePolicy::Probabilistic {
                    keep_last: true,
                    ..
                }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sample {
    Record,
    BufferAsLast,
    Skip,
}

pub struct Sampler {
    policy: SamplePolicy,
    rng: StdRng,
}

impl Sampler {
    pub fn new(policy: SamplePolicy, seed: u64) -> Self {
        Self {
            policy,
            rng: StdRng::seed_from_u64(seed),
        }
    }

    pub fn policy(&self) -> &SamplePolicy {
        &self.policy
    }

    pub fn decide(&mut self, seq: u64) -> Sample {
        match &self.policy {
            SamplePolicy::All => Sample::Record,
            SamplePolicy::FirstN(n) => {
                if seq < *n {
                    Sample::Record
                } else {
                    Sample::Skip
                }
            }
            SamplePolicy::FirstNAndLast(n) => {
                if seq < *n {
                    Sample::Record
                } else {
                    Sample::BufferAsLast
                }
            }
            SamplePolicy::Probabilistic {
                p,
                first_n_always,
                keep_last,
            } => {
                if seq < *first_n_always {
                    Sample::Record
                } else if self.rng.gen::<f64>() < *p {
                    Sample::Record
                } else if *keep_last {
                    Sample::BufferAsLast
                } else {
                    Sample::Skip
                }
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SamplePolicyParseError {
    #[error("sample policy cannot be empty")]
    Empty,
    #[error("unknown sample mode '{0}'")]
    UnknownMode(String),
    #[error("malformed sample argument: {0}")]
    Malformed(String),
    #[error("parse int: {0}")]
    BadInt(#[from] std::num::ParseIntError),
    #[error("parse float: {0}")]
    BadFloat(#[from] std::num::ParseFloatError),
}

impl std::str::FromStr for SamplePolicy {
    type Err = SamplePolicyParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(SamplePolicyParseError::Empty);
        }
        let mut parts = s.split(':');
        let mode = parts.next().ok_or(SamplePolicyParseError::Empty)?;
        match mode {
            "all" => Ok(SamplePolicy::All),
            "first" => {
                let n: u64 = parts
                    .next()
                    .ok_or_else(|| SamplePolicyParseError::Malformed(s.into()))?
                    .parse()?;
                Ok(SamplePolicy::FirstN(n))
            }
            "firstlast" => {
                let n: u64 = parts
                    .next()
                    .ok_or_else(|| SamplePolicyParseError::Malformed(s.into()))?
                    .parse()?;
                Ok(SamplePolicy::FirstNAndLast(n))
            }
            "prob" => {
                let p: f64 = parts
                    .next()
                    .ok_or_else(|| SamplePolicyParseError::Malformed(s.into()))?
                    .parse()?;
                let mut first_n_always = 0u64;
                let mut keep_last = true;
                for kv in parts {
                    let (k, v) = kv
                        .split_once('=')
                        .ok_or_else(|| SamplePolicyParseError::Malformed(kv.into()))?;
                    match k {
                        "first" => first_n_always = v.parse()?,
                        "last" => keep_last = v.parse::<u32>()? != 0,
                        _ => return Err(SamplePolicyParseError::Malformed(kv.into())),
                    }
                }
                Ok(SamplePolicy::Probabilistic {
                    p,
                    first_n_always,
                    keep_last,
                })
            }
            other => Err(SamplePolicyParseError::UnknownMode(other.into())),
        }
    }
}

impl std::fmt::Display for SamplePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SamplePolicy::All => write!(f, "all"),
            SamplePolicy::FirstN(n) => write!(f, "first:{}", n),
            SamplePolicy::FirstNAndLast(n) => write!(f, "firstlast:{}", n),
            SamplePolicy::Probabilistic {
                p,
                first_n_always,
                keep_last,
            } => write!(
                f,
                "prob:{}:first={}:last={}",
                p,
                first_n_always,
                if *keep_last { 1 } else { 0 }
            ),
        }
    }
}
