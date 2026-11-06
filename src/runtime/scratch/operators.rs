use crate::{Instruction, Value};
use std::cmp::Ordering;

/// Pure operators are kept separate from scheduling and frontend requests.
pub(super) fn evaluate(instruction: &Instruction, args: &[Value]) -> Option<Value> {
    use Instruction::*;
    let v = |i: usize| args.get(i).cloned().unwrap_or_default();
    let n = |i| v(i).as_number();
    let s = |i| v(i).to_string();
    Some(match instruction {
        Add { .. } => Value::Number(n(0) + n(1)),
        Subtract { .. } => Value::Number(n(0) - n(1)),
        Multiply { .. } => Value::Number(n(0) * n(1)),
        Divide { .. } => Value::Number(n(0) / n(1)),
        Random { .. } => {
            let (low, high) = if n(0) <= n(1) {
                (n(0), n(1))
            } else {
                (n(1), n(0))
            };
            let value = if low == high || !low.is_finite() || !high.is_finite() {
                low
            } else if low.fract() == 0.0 && high.fract() == 0.0 {
                (low + rand::random::<f64>() * (high - low + 1.0))
                    .floor()
                    .min(high)
            } else {
                low + rand::random::<f64>() * (high - low)
            };
            Value::Number(value)
        }
        GreaterThan { .. } => Value::Boolean(v(0).compare(&v(1)) == Ordering::Greater),
        LessThan { .. } => Value::Boolean(v(0).compare(&v(1)) == Ordering::Less),
        Equals { .. } => Value::Boolean(v(0).compare(&v(1)) == Ordering::Equal),
        And { .. } => Value::Boolean(v(0).truthy() && v(1).truthy()),
        Or { .. } => Value::Boolean(v(0).truthy() || v(1).truthy()),
        Not { .. } => Value::Boolean(!v(0).truthy()),
        Join { .. } => Value::String(format!("{}{}", s(0), s(1))),
        LetterOf { .. } => Value::String(if n(0) >= 1.0 {
            s(1).chars()
                .nth((n(0).floor() - 1.0) as usize)
                .map(|c| c.to_string())
                .unwrap_or_default()
        } else {
            String::new()
        }),
        Length { .. } => Value::Number(s(0).chars().count() as f64),
        Contains { .. } => Value::Boolean(s(0).to_lowercase().contains(&s(1).to_lowercase())),
        Mod { .. } => {
            let result = n(0) % n(1);
            Value::Number(if result / n(1) < 0.0 {
                result + n(1)
            } else {
                result
            })
        }
        Round { .. } => Value::Number((n(0) + 0.5).floor()),
        MathOp { operator, .. } => Value::Number(match operator.value.to_string().as_str() {
            "abs" => n(0).abs(),
            "floor" => n(0).floor(),
            "ceiling" => n(0).ceil(),
            "sqrt" => n(0).sqrt(),
            "sin" => n(0).to_radians().sin(),
            "cos" => n(0).to_radians().cos(),
            "tan" => {
                let angle = n(0).rem_euclid(360.0);
                if angle == 90.0 {
                    f64::INFINITY
                } else if angle == 270.0 {
                    f64::NEG_INFINITY
                } else {
                    n(0).to_radians().tan()
                }
            }
            "asin" => n(0).asin().to_degrees(),
            "acos" => n(0).acos().to_degrees(),
            "atan" => n(0).atan().to_degrees(),
            "ln" => n(0).ln(),
            "log" => n(0).log10(),
            "e ^" => n(0).exp(),
            "10 ^" => 10.0_f64.powf(n(0)),
            _ => 0.0,
        }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn operand() -> Box<Instruction> {
        Box::new(Instruction::Literal(Value::default()))
    }

    #[test]
    fn arithmetic_boolean_and_string_operators() {
        let cases = vec![
            (
                Instruction::Add {
                    num1: operand(),
                    num2: operand(),
                },
                vec![Value::String("2".to_string()), Value::Number(3.0)],
                Value::Number(5.0),
            ),
            (
                Instruction::Subtract {
                    num1: operand(),
                    num2: operand(),
                },
                vec![Value::Number(2.0), Value::Number(3.0)],
                Value::Number(-1.0),
            ),
            (
                Instruction::Multiply {
                    num1: operand(),
                    num2: operand(),
                },
                vec![Value::Number(2.0), Value::Number(3.0)],
                Value::Number(6.0),
            ),
            (
                Instruction::Divide {
                    num1: operand(),
                    num2: operand(),
                },
                vec![Value::Number(6.0), Value::Number(3.0)],
                Value::Number(2.0),
            ),
            (
                Instruction::Mod {
                    num1: operand(),
                    num2: operand(),
                },
                vec![Value::Number(-5.0), Value::Number(3.0)],
                Value::Number(1.0),
            ),
            (
                Instruction::Round { num: operand() },
                vec![Value::Number(-1.5)],
                Value::Number(-1.0),
            ),
            (
                Instruction::Equals {
                    operand1: operand(),
                    operand2: operand(),
                },
                vec![
                    Value::String("HELLO".to_string()),
                    Value::String("hello".to_string()),
                ],
                Value::Boolean(true),
            ),
            (
                Instruction::LessThan {
                    operand1: operand(),
                    operand2: operand(),
                },
                vec![Value::Number(2.0), Value::String("10".to_string())],
                Value::Boolean(true),
            ),
            (
                Instruction::GreaterThan {
                    operand1: operand(),
                    operand2: operand(),
                },
                vec![
                    Value::String("z".to_string()),
                    Value::String("a".to_string()),
                ],
                Value::Boolean(true),
            ),
            (
                Instruction::And {
                    operand1: operand(),
                    operand2: operand(),
                },
                vec![Value::Boolean(true), Value::String("false".to_string())],
                Value::Boolean(false),
            ),
            (
                Instruction::Or {
                    operand1: operand(),
                    operand2: operand(),
                },
                vec![Value::Number(0.0), Value::Number(1.0)],
                Value::Boolean(true),
            ),
            (
                Instruction::Not { operand: operand() },
                vec![Value::String("".to_string())],
                Value::Boolean(true),
            ),
            (
                Instruction::Join {
                    string1: operand(),
                    string2: operand(),
                },
                vec![Value::String("score: ".to_string()), Value::Number(3.0)],
                Value::String("score: 3".to_string()),
            ),
            (
                Instruction::Length { string: operand() },
                vec![Value::String("hello".to_string())],
                Value::Number(5.0),
            ),
            (
                Instruction::LetterOf {
                    letter: operand(),
                    string: operand(),
                },
                vec![Value::Number(2.0), Value::String("abc".to_string())],
                Value::String("b".to_string()),
            ),
            (
                Instruction::Contains {
                    string1: operand(),
                    string2: operand(),
                },
                vec![
                    Value::String("Hello!".to_string()),
                    Value::String("ELLO".to_string()),
                ],
                Value::Boolean(true),
            ),
        ];
        for (instruction, values, expected) in cases {
            assert_eq!(
                evaluate(&instruction, &values),
                Some(expected),
                "{instruction:?}"
            );
        }
    }

    #[test]
    fn random_values_stay_in_range_and_math_uses_degrees() {
        let random = Instruction::Random {
            from: operand(),
            to: operand(),
        };
        for _ in 0..100 {
            let value = evaluate(&random, &[Value::Number(4.0), Value::Number(1.0)])
                .unwrap()
                .as_number();
            assert!((1.0..=4.0).contains(&value));
            assert_eq!(value.fract(), 0.0);
        }
        let sin = Instruction::MathOp {
            operator: crate::Field {
                value: Value::String("sin".to_string()),
                id: None,
            },
            num: operand(),
        };
        assert_eq!(
            evaluate(&sin, &[Value::Number(90.0)]),
            Some(Value::Number(1.0))
        );
    }
}
