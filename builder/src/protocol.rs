pub trait TouchPortalStringly: Sized {
    fn stringify(&self) -> String;
    fn destringify(s: &str) -> eyre::Result<Self>;
}

impl TouchPortalStringly for String {
    fn stringify(&self) -> String {
        self.clone()
    }

    fn destringify(v: &str) -> eyre::Result<Self> {
        Ok(v.to_string())
    }
}

impl TouchPortalStringly for f64 {
    fn stringify(&self) -> String {
        self.to_string()
    }

    fn destringify(v: &str) -> eyre::Result<Self> {
        Ok(v.parse()?)
    }
}

impl TouchPortalStringly for bool {
    fn stringify(&self) -> String {
        match self {
            true => String::from("On"),
            false => String::from("Off"),
        }
    }

    fn destringify(v: &str) -> eyre::Result<Self> {
        match v {
            "On" => Ok(true),
            "Off" => Ok(false),
            other => eyre::bail!("TouchPortal does not use {other} for switch values"),
        }
    }
}

pub mod serde_tp_stringly {
    use serde::{Serialize, Serializer};

    pub fn serialize<S, T>(t: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: super::TouchPortalStringly,
    {
        T::stringify(t).serialize(serializer)
    }

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: ::serde::Deserializer<'de>,
        T: super::TouchPortalStringly,
    {
        use ::serde::de::Visitor;

        struct V<S>(std::marker::PhantomData<fn() -> S>);

        impl<'de, S> Visitor<'de> for V<S>
        where
            S: super::TouchPortalStringly,
        {
            type Value = S;

            fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                formatter.write_str("a string representing an S")
            }

            fn visit_str<E>(self, v: &str) -> Result<S, E>
            where
                E: ::serde::de::Error,
            {
                S::destringify(v).map_err(::serde::de::Error::custom)
            }
        }

        deserializer.deserialize_str(V::<T>(Default::default()))
    }
}

mod incoming;
pub use incoming::*;

mod outgoing;
pub use outgoing::*;
