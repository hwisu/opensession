//! Convert sea-query values to D1 JsValue bind params.

use worker::wasm_bindgen::JsValue;

/// Convert `sea_query::Values` into a `Vec<JsValue>` suitable for D1 `.bind()`.
pub fn values_to_js(values: &sea_query::Values) -> Vec<JsValue> {
    values
        .0
        .iter()
        .map(|v| match v {
            sea_query::Value::String(Some(s)) => s.as_str().into(),
            sea_query::Value::Int(Some(i)) => (*i as f64).into(),
            sea_query::Value::BigInt(Some(i)) => (*i as f64).into(),
            sea_query::Value::Bool(Some(b)) => (*b).into(),
            _ => JsValue::NULL,
        })
        .collect()
}
