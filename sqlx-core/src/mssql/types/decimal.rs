use rust_decimal::Decimal;

use crate::decode::Decode;
use crate::encode::{Encode, IsNull};
use crate::error::BoxDynError;
use crate::mssql::protocol::type_info::{DataType, TypeInfo};
use crate::mssql::{Mssql, MssqlTypeInfo, MssqlValueRef};
use crate::types::Type;

impl Type<Mssql> for Decimal {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo(TypeInfo {
            scale: u8::MAX,
            ty: DataType::NumericN,
            size: 17,
            collation: None,
            precision: 38,
        })
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(
            ty.0.ty,
            DataType::Numeric
                | DataType::NumericN
                | DataType::Decimal
                | DataType::DecimalN
                | DataType::MoneyN
                | DataType::Money
                | DataType::SmallMoney
        )
    }
}

impl Encode<'_, Mssql> for Decimal {
    fn produces(&self) -> Option<MssqlTypeInfo> {
        let mut info = <Self as Type<Mssql>>::type_info();
        info.0.scale = u8::try_from(self.scale()).unwrap_or(u8::MAX);
        Some(info)
    }

    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> IsNull {
        let sign = if self.is_sign_negative() { 0 } else { 1 };
        buf.push(sign);
        let mantissa = if self.scale() <= u32::from(u8::MAX) {
            self.mantissa().saturating_abs() as u128
        } else {
            0
        };
        buf.extend_from_slice(&mantissa.to_le_bytes());
        IsNull::No
    }
}

impl Decode<'_, Mssql> for Decimal {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let ty = value.type_info.0.ty;
        match ty {
            DataType::Decimal | DataType::DecimalN | DataType::Numeric | DataType::NumericN => {
                let precision = value.type_info.0.precision;
                let scale = value.type_info.0.scale;
                decode_numeric(value.as_bytes()?, precision, scale)
            }
            DataType::MoneyN | DataType::Money | DataType::SmallMoney => {
                // Money is stored as an 8-byte integer representing the amount in ten-thousandths of a currency unit
                let bytes = value.as_bytes()?;
                // println!("bytes: {:?}", bytes);
                if bytes.len() != 8 && bytes.len() != 4 {
                    return Err(
                        err_protocol!("expected 8/4 bytes for Money, got {}", bytes.len()).into(),
                    );
                }
                let amount: i64 = if bytes.len() == 8 {
                    let amount_h = i32::from_le_bytes(bytes[0..4].try_into()?) as i64;
                    let amount_l = u32::from_le_bytes(bytes[4..8].try_into()?) as i64;
                    (amount_h << 32) | amount_l
                } else {
                    i32::from_le_bytes(bytes.try_into()?) as i64
                };
                Ok(Decimal::new(amount, 4))
            }
            _ => Err(err_protocol!("expected numeric type, got {:?}", value.type_info.0).into()),
        }
    }
}

fn decode_numeric(bytes: &[u8], _precision: u8, scale: u8) -> Result<Decimal, BoxDynError> {
    let sign = if bytes[0] == 0 { -1 } else { 1 };
    let rest = &bytes[1..];
    let mut fixed_bytes = [0u8; 16];
    fixed_bytes[0..rest.len()].copy_from_slice(rest);
    let numerator = u128::from_le_bytes(fixed_bytes);
    let small_num = sign * i64::try_from(numerator)?;
    Ok(Decimal::new(small_num, u32::from(scale)))
}
