use std::{fmt::Display, str::FromStr};

use serde::{de::Error, Deserialize, Serializer};
use serde_cow::CowStr;

pub fn deserialize<'de, D, V>(deserializer: D) -> Result<V, D::Error>
where
    D: serde::Deserializer<'de>,
    <V as FromStr>::Err: Display,
    V: FromStr,
{
    let value_str = <CowStr<'de>>::deserialize(deserializer)?;
    value_str.0.parse().map_err(D::Error::custom)
}

pub fn serialize<S, V>(value: V, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    V: Display,
{
    serializer.collect_str(&value)
}
