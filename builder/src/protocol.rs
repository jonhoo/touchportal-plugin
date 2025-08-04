pub trait TouchPortalToString {
    fn stringify(&self) -> String;
}

impl<T: TouchPortalToString + ?Sized> TouchPortalToString for &T {
    fn stringify(&self) -> String {
        T::stringify(*self)
    }
}

impl TouchPortalToString for String {
    fn stringify(&self) -> String {
        self.clone()
    }
}

impl TouchPortalToString for str {
    fn stringify(&self) -> String {
        self.to_string()
    }
}

impl TouchPortalToString for f64 {
    fn stringify(&self) -> String {
        self.to_string()
    }
}

impl TouchPortalToString for bool {
    fn stringify(&self) -> String {
        match self {
            true => String::from("On"),
            false => String::from("Off"),
        }
    }
}

pub trait TouchPortalFromStr: Sized {
    fn destringify(s: &str) -> eyre::Result<Self>;
}

impl TouchPortalFromStr for String {
    fn destringify(v: &str) -> eyre::Result<Self> {
        Ok(v.to_string())
    }
}

impl TouchPortalFromStr for f64 {
    fn destringify(v: &str) -> eyre::Result<Self> {
        Ok(v.parse()?)
    }
}

impl TouchPortalFromStr for bool {
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
        T: super::TouchPortalToString,
    {
        T::stringify(t).serialize(serializer)
    }

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: ::serde::Deserializer<'de>,
        T: super::TouchPortalFromStr,
    {
        use ::serde::de::Visitor;

        struct V<S>(std::marker::PhantomData<fn() -> S>);

        impl<'de, S> Visitor<'de> for V<S>
        where
            S: super::TouchPortalFromStr,
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
