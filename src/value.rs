#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    String(String),
    Boolean(bool),
}

impl Value {
    pub fn as_number(&self) -> f64 {
        let number = match self {
            Value::Number(number) => *number,
            Value::Boolean(value) => {
                if *value {
                    1.0
                } else {
                    0.0
                }
            }
            Value::String(value) => {
                let value = value.trim();
                if value.is_empty() {
                    return 0.0;
                }
                if let Some(hex) = value
                    .strip_prefix("0x")
                    .or_else(|| value.strip_prefix("0X"))
                {
                    return u64::from_str_radix(hex, 16)
                        .map(|n| n as f64)
                        .unwrap_or(0.0);
                }
                value.parse().unwrap_or(0.0)
            }
        };
        if number.is_nan() { 0.0 } else { number }
    }

    pub fn compare(&self, other: &Self) -> std::cmp::Ordering {
        fn numeric(value: &Value) -> Option<f64> {
            match value {
                Value::String(text) if text.trim().is_empty() => None,
                Value::String(text) => text.trim().parse::<f64>().ok().filter(|n| !n.is_nan()),
                _ => Some(value.as_number()),
            }
        }
        if let (Some(a), Some(b)) = (numeric(self), numeric(other)) {
            return a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
        }
        self.to_string()
            .to_lowercase()
            .cmp(&other.to_string().to_lowercase())
    }

    #[cfg(feature = "scratch")]
    pub(crate) fn from_json(value: &serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::String(value) => Some(Self::String(value.clone())),
            serde_json::Value::Number(value) => value.as_f64().map(Self::Number),
            serde_json::Value::Bool(value) => Some(Self::Boolean(*value)),
            _ => None,
        }
    }
    pub fn truthy(&self) -> bool {
        match self {
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty() && s != "0" && !s.eq_ignore_ascii_case("false"),
            Value::Boolean(b) => *b,
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Self::String(String::new())
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(value) if *value == f64::INFINITY => f.write_str("Infinity"),
            Self::Number(value) if *value == f64::NEG_INFINITY => f.write_str("-Infinity"),
            Self::Number(value) if *value == 0.0 => f.write_str("0"),
            Self::Number(value) => write!(f, "{value}"),
            Self::String(value) => f.write_str(value),
            Self::Boolean(value) => write!(f, "{value}"),
        }
    }
}
