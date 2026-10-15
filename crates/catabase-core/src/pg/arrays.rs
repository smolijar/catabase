//! Lossless PostgreSQL array literals: unlike a plain JSON array, these retain
//! dimensions/lower bounds and never round numeric or JSON element values.
use fallible_iterator::FallibleIterator;
use postgres_protocol::types::{ArrayDimension, array_from_sql};
use tokio_postgres::types::{FromSql, Kind, Type};

use super::{DateTime, ExactNumeric, NaiveDate, NaiveDateTime, RawJson, Utc, Uuid};

type Error = Box<dyn std::error::Error + Sync + Send>;

pub(super) struct PgArray(pub String);

impl<'a> FromSql<'a> for PgArray {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, Error> {
        let Kind::Array(element) = ty.kind() else {
            return Err("Expected a PostgreSQL array".into());
        };
        let array = array_from_sql(raw)?;
        let dimensions = array.dimensions().collect::<Vec<_>>()?;
        if dimensions.len() > 6 || array.element_type() != element.oid() {
            return Err("Invalid PostgreSQL array dimensions or element type".into());
        }
        let mut values = array.values();
        let mut literal = String::new();
        if dimensions
            .iter()
            .any(|dimension| dimension.lower_bound != 1)
        {
            for dimension in &dimensions {
                let upper = i64::from(dimension.lower_bound) + i64::from(dimension.len) - 1;
                literal.push_str(&format!("[{}:{upper}]", dimension.lower_bound));
            }
            literal.push('=');
        }
        write_dimension(&mut literal, &dimensions, element, &mut values)?;
        // Also checks that the binary payload has been completely consumed.
        if values.next()?.is_some() {
            return Err("Unexpected PostgreSQL array elements".into());
        }
        Ok(Self(literal))
    }

    fn accepts(ty: &Type) -> bool {
        matches!(ty.kind(), Kind::Array(_))
    }
}

fn write_dimension<'a>(
    output: &mut String,
    dimensions: &[ArrayDimension],
    element: &Type,
    values: &mut postgres_protocol::types::ArrayValues<'a>,
) -> Result<(), Error> {
    output.push('{');
    if let Some(dimension) = dimensions.first() {
        for index in 0..dimension.len {
            if index > 0 {
                output.push(',');
            }
            if dimensions.len() > 1 {
                write_dimension(output, &dimensions[1..], element, values)?;
            } else {
                match values.next()?.ok_or("Missing PostgreSQL array element")? {
                    None => output.push_str("NULL"),
                    Some(raw) => {
                        let text = element_text(element, raw)?;
                        output.push('"');
                        for character in text.chars() {
                            if character == '"' || character == '\\' {
                                output.push('\\');
                            }
                            output.push(character);
                        }
                        output.push('"');
                    }
                }
            }
        }
    }
    output.push('}');
    Ok(())
}

fn element_text(ty: &Type, raw: &[u8]) -> Result<String, Error> {
    macro_rules! decode {
        ($type:ty) => {
            <$type>::from_sql(ty, raw)?.to_string()
        };
    }
    Ok(match *ty {
        Type::BOOL => decode!(bool),
        Type::INT2 => decode!(i16),
        Type::INT4 => decode!(i32),
        Type::INT8 => decode!(i64),
        Type::OID => decode!(u32),
        Type::FLOAT4 => decode!(f32),
        Type::FLOAT8 => decode!(f64),
        Type::NUMERIC => ExactNumeric::from_sql(ty, raw)?.0,
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => decode!(String),
        Type::UUID => decode!(Uuid),
        Type::JSON | Type::JSONB => RawJson::from_sql(ty, raw)?.0,
        Type::DATE if raw == i32::MAX.to_be_bytes() => "infinity".into(),
        Type::DATE if raw == i32::MIN.to_be_bytes() => "-infinity".into(),
        Type::DATE => decode!(NaiveDate),
        Type::TIMESTAMP | Type::TIMESTAMPTZ if raw == i64::MAX.to_be_bytes() => "infinity".into(),
        Type::TIMESTAMP | Type::TIMESTAMPTZ if raw == i64::MIN.to_be_bytes() => "-infinity".into(),
        Type::TIMESTAMP => decode!(NaiveDateTime),
        Type::TIMESTAMPTZ => DateTime::<Utc>::from_sql(ty, raw)?.to_rfc3339(),
        Type::BYTEA => {
            let mut text = String::from("\\x");
            for byte in raw {
                use std::fmt::Write;
                write!(text, "{byte:02x}")?;
            }
            text
        }
        _ => match ty.kind() {
            Kind::Enum(_) => std::str::from_utf8(raw)?.to_string(),
            Kind::Domain(base) => return element_text(base, raw),
            _ => return Err(format!("Unsupported array element type: {}", ty.name()).into()),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary(element: &Type, dimensions: &[(i32, i32)], values: &[Option<&[u8]>]) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.extend_from_slice(&(dimensions.len() as i32).to_be_bytes());
        raw.extend_from_slice(&1i32.to_be_bytes());
        raw.extend_from_slice(&element.oid().to_be_bytes());
        for (length, lower) in dimensions {
            raw.extend_from_slice(&length.to_be_bytes());
            raw.extend_from_slice(&lower.to_be_bytes());
        }
        for value in values {
            raw.extend_from_slice(&value.map_or(-1, |value| value.len() as i32).to_be_bytes());
            if let Some(value) = value {
                raw.extend_from_slice(value);
            }
        }
        raw
    }

    #[test]
    fn text_null_empty_and_escaping_are_distinct() {
        let raw = binary(
            &Type::TEXT,
            &[(5, 1)],
            &[
                Some(b""),
                None,
                Some(b"NULL"),
                Some(b"a,{}\"\\"),
                Some("žluťoučký".as_bytes()),
            ],
        );
        assert_eq!(
            PgArray::from_sql(&Type::TEXT_ARRAY, &raw).unwrap().0,
            r#"{"",NULL,"NULL","a,{}\"\\","žluťoučký"}"#
        );
        assert_eq!(
            PgArray::from_sql(&Type::TEXT_ARRAY, &binary(&Type::TEXT, &[], &[]))
                .unwrap()
                .0,
            "{}"
        );
    }

    #[test]
    fn multidimensional_arrays_keep_nonstandard_lower_bounds() {
        let one = 1i32.to_be_bytes();
        let two = 2i32.to_be_bytes();
        let raw = binary(
            &Type::INT4,
            &[(2, -1), (2, 3)],
            &[Some(&one), None, Some(&two), Some(&one)],
        );
        assert_eq!(
            PgArray::from_sql(&Type::INT4_ARRAY, &raw).unwrap().0,
            r#"[-1:0][3:4]={{"1",NULL},{"2","1"}}"#
        );
    }

    #[test]
    fn exact_numeric_and_json_elements_never_pass_through_floats() {
        let bigint = 9_007_199_254_740_993i64.to_be_bytes();
        assert_eq!(
            PgArray::from_sql(
                &Type::INT8_ARRAY,
                &binary(&Type::INT8, &[(1, 1)], &[Some(&bigint)])
            )
            .unwrap()
            .0,
            r#"{"9007199254740993"}"#
        );
        let raw_json = br#"{"n":9007199254740993}"#;
        assert_eq!(
            PgArray::from_sql(
                &Type::JSON_ARRAY,
                &binary(&Type::JSON, &[(1, 1)], &[Some(raw_json)])
            )
            .unwrap()
            .0,
            r#"{"{\"n\":9007199254740993}"}"#
        );
    }

    #[test]
    fn malformed_or_unsupported_arrays_do_not_become_null_values() {
        for raw in [
            vec![],
            binary(&Type::TEXT, &[(2, 1)], &[Some(b"one")]),
            binary(&Type::INT4, &[], &[]),
        ] {
            assert!(PgArray::from_sql(&Type::TEXT_ARRAY, &raw).is_err());
        }
        assert!(element_text(&Type::POINT, &[]).is_err());
    }
}
