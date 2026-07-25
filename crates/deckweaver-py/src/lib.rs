//! PyO3 bindings for deckweaver-core.

mod python;

use pyo3::prelude::*;
use pyo3::types::PyModule;

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    pyo3_log::init();
    python::register(m)
}
