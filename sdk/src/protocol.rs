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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_touchportal_to_string_implementations() {
        // Test String implementation
        let s = String::from("test");
        assert_eq!(s.stringify(), "test");

        // Test str implementation
        let s = "test";
        assert_eq!(s.stringify(), "test");

        // Test f64 implementation
        let f = 42.5;
        assert_eq!(f.stringify(), "42.5");

        let f = 0.0;
        assert_eq!(f.stringify(), "0");

        let f = -123.456;
        assert_eq!(f.stringify(), "-123.456");

        // Test bool implementation - TouchPortal uses "On"/"Off"
        let b = true;
        assert_eq!(b.stringify(), "On");

        let b = false;
        assert_eq!(b.stringify(), "Off");
    }

    #[test]
    fn test_touchportal_to_string_reference_types() {
        // Test &T implementation for references
        let s = String::from("reference_test");
        assert_eq!((&s).stringify(), "reference_test");

        let f = &42.0;
        assert_eq!(f.stringify(), "42");

        let b = &true;
        assert_eq!(b.stringify(), "On");
    }

    #[test]
    fn test_touchportal_from_str_implementations() {
        // Test String implementation
        let result = String::destringify("test string");
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(result.unwrap(), "test string");

        // Test f64 implementation - valid numbers
        let result = f64::destringify("42.5");
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(result.unwrap(), 42.5);

        let result = f64::destringify("0");
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(result.unwrap(), 0.0);

        let result = f64::destringify("-123.456");
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(result.unwrap(), -123.456);

        // Test bool implementation - valid values
        let result = bool::destringify("On");
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(result.unwrap(), true);

        let result = bool::destringify("Off");
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(result.unwrap(), false);
    }

    #[test]
    fn test_touchportal_from_str_error_cases() {
        use insta::assert_snapshot;

        // Test f64 implementation - invalid numbers
        let result = f64::destringify("not_a_number");
        assert_snapshot!(result.unwrap_err(), @"invalid float literal");

        let result = f64::destringify("");
        assert_snapshot!(result.unwrap_err(), @"cannot parse float from empty string");

        let result = f64::destringify("42.5.6");
        assert_snapshot!(result.unwrap_err(), @"invalid float literal");

        // Test bool implementation - invalid values
        let invalid_values = ["true", "false", "1", "0", "", "yes", "no"];

        for invalid_value in invalid_values {
            let result = bool::destringify(invalid_value);
            assert!(
                result.is_err(),
                "Expected error for invalid bool value: '{}'",
                invalid_value
            );

            let error_msg = result.unwrap_err().to_string();
            assert_snapshot!(
                format!(
                    "bool_error_for_{}",
                    if invalid_value.is_empty() {
                        "empty"
                    } else {
                        invalid_value
                    }
                ),
                error_msg
            );
        }
    }

    #[test]
    fn test_bool_round_trip_conversion() {
        // Test that bool -> string -> bool conversion is consistent
        let original_true = true;
        let stringified = original_true.stringify();
        let parsed = bool::destringify(&stringified).unwrap();
        assert_eq!(original_true, parsed);

        let original_false = false;
        let stringified = original_false.stringify();
        let parsed = bool::destringify(&stringified).unwrap();
        assert_eq!(original_false, parsed);
    }

    #[test]
    fn test_f64_round_trip_conversion() {
        // Test that f64 -> string -> f64 conversion preserves values
        let test_values = vec![0.0, 42.5, -123.456, 1e6, 1e-6, std::f64::consts::PI];

        for original in test_values {
            let stringified = original.stringify();
            let parsed = f64::destringify(&stringified).unwrap();

            // Use approx_eq for robust floating point comparison
            use float_cmp::approx_eq;
            assert!(
                approx_eq!(f64, original, parsed, ulps = 2),
                "Round trip failed for {}: {} -> {} -> {}",
                original,
                original,
                stringified,
                parsed
            );
        }
    }

    #[test]
    fn test_string_round_trip_conversion() {
        // Test that String -> string -> String conversion preserves content
        let test_strings = vec![
            String::from(""),
            String::from("simple"),
            String::from("with spaces"),
            String::from("with\nnewlines"),
            String::from("with\ttabs"),
            String::from("🎵 unicode"),
            String::from("special chars: !@#$%^&*()"),
        ];

        for original in test_strings {
            let stringified = original.stringify();
            let parsed = String::destringify(&stringified).unwrap();
            assert_eq!(original, parsed);
        }
    }
}
