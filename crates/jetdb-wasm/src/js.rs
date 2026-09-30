//! The JavaScript interface: thin `wasm-bindgen` wrappers over [`crate::Database`].

use js_sys::Reflect;
use wasm_bindgen::prelude::*;

use crate::Database;

/// A Microsoft Access database opened from its bytes.
#[wasm_bindgen(js_name = Database)]
pub struct JsDatabase {
    inner: Database,
}

#[wasm_bindgen(js_class = Database)]
impl JsDatabase {
    /// Opens a database from its bytes, with the password of a
    /// password-protected `.accdb`.
    pub fn open(bytes: Vec<u8>, password: Option<String>) -> Result<JsDatabase, JsError> {
        let inner = Database::open(bytes, password.as_deref()).map_err(to_js_error)?;
        Ok(JsDatabase { inner })
    }

    /// The database engine version: `JET3`, `JET4`, `ACE12`, `ACE14`,
    /// `ACE15`, `ACE16`, or `ACE17`.
    pub fn version(&self) -> String {
        self.inner.version().to_string()
    }

    /// The names of the user tables, sorted. With `{ system: true }`, the
    /// system and hidden tables are included too.
    pub fn tables(&mut self, options: Option<js_sys::Object>) -> Result<Vec<String>, JsError> {
        let include_system = option_bool(options.as_ref(), "system")?;
        self.inner.tables(include_system).map_err(to_js_error)
    }
}

fn to_js_error(e: jetdb::FileError) -> JsError {
    JsError::new(&e.to_string())
}

/// Reads the boolean option `name`, which is `false` when absent.
fn option_bool(options: Option<&js_sys::Object>, name: &str) -> Result<bool, JsError> {
    let Some(options) = options else {
        return Ok(false);
    };
    let value = Reflect::get(options, &JsValue::from_str(name))
        .map_err(|_| JsError::new(&format!("cannot read option {name}")))?;
    if value.is_undefined() || value.is_null() {
        return Ok(false);
    }
    value
        .as_bool()
        .ok_or_else(|| JsError::new(&format!("option {name} must be a boolean")))
}
