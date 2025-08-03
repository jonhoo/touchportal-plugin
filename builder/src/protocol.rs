pub trait Unstring: Sized {
    fn unstring(s: &str) -> eyre::Result<Self>;
}

impl Unstring for String {
    fn unstring(v: &str) -> eyre::Result<Self> {
        Ok(v.to_string())
    }
}

impl Unstring for f64 {
    fn unstring(v: &str) -> eyre::Result<Self> {
        Ok(v.parse()?)
    }
}

impl Unstring for bool {
    fn unstring(v: &str) -> eyre::Result<Self> {
        match v {
            "true" | "On" => Ok(true),
            "false" | "Off" => Ok(false),
            other => eyre::bail!("TouchPortal does not use {other} for switch values"),
        }
    }
}

mod incoming;
pub use incoming::*;

mod outgoing;
pub use outgoing::*;
